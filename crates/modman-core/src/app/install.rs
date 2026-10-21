// SPDX-License-Identifier: GPL-3.0-or-later
//! The install pipeline around [`crate::mods::install_archive`]: install
//! targets (legacy `parse_mod_target` L2829-2846), Nexus installs
//! (`install_nexus_mod` L3239-3288), local archives (`install_local`
//! L3290-3318), `download` (L3365-3392) and `reinstall` (L3635-3665).

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::deploy::{self as dep, AfterChange};
use super::nexus::{self as nx, FileRequest, Nexus};
use super::{fail, human_size, staging_lock, AppError, Cx, Msg, R};
use crate::download::{self, NxmLink};
use crate::fomod::{FomodMode, FomodResolver, ResolvedMode};
use crate::games::{self, GameConfig};
use crate::mods::{self, InstallHooks, InstallMeta, InstallOutcome, ModRecord};
use crate::nexus::{FileInfo, NxmKey};

/// What an install target names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Local(PathBuf),
    Collection(String),
    Nexus {
        game: Option<String>,
        mod_id: u64,
        file_id: Option<u64>,
        key: Option<NxmKey>,
    },
}

fn digits(s: &str) -> Option<u64> {
    (!s.is_empty() && s.len() <= 19 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// Classifies `t`; `None` when it is none of the known forms. nxm:// links
/// go through [`NxmLink::parse`] and fail when malformed.
pub fn parse_target(t: &str) -> R<Option<Target>> {
    if Path::new(t).exists() {
        return Ok(Some(Target::Local(PathBuf::from(t))));
    }
    if t.contains("collections/") {
        return Ok(Some(Target::Collection(t.to_string())));
    }
    if t.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("nxm://")) {
        return match NxmLink::parse(t)? {
            NxmLink::Mod(m) => Ok(Some(Target::Nexus {
                game: Some(m.game),
                mod_id: m.mod_id,
                file_id: Some(m.file_id),
                key: m.key,
            })),
            NxmLink::Collection { .. } => Ok(Some(Target::Collection(t.to_string()))),
        };
    }
    let lower = t.to_ascii_lowercase();
    if let Some(i) = lower.find("nexusmods.com/") {
        let rest = &lower[i + "nexusmods.com/".len()..];
        let rest = rest.strip_prefix("games/").unwrap_or(rest);
        if let Some((g, rest)) = rest.split_once('/') {
            if let Some(after) = rest.strip_prefix("mods/") {
                let id: String = after.chars().take_while(char::is_ascii_digit).collect();
                let file_id = lower
                    .split(['?', '&'])
                    .find_map(|p| p.strip_prefix("file_id="))
                    .and_then(|v| {
                        digits(
                            &v.chars()
                                .take_while(char::is_ascii_digit)
                                .collect::<String>(),
                        )
                    });
                if let (Some(mod_id), false) = (digits(&id), g.is_empty()) {
                    return Ok(Some(Target::Nexus {
                        game: Some(games::normalize_domain(g)?),
                        mod_id,
                        file_id,
                        key: None,
                    }));
                }
            }
        }
    }
    if let Some((g, m)) = t.split_once(':') {
        if games::is_valid_domain(g) {
            if let Some(mod_id) = digits(m) {
                return Ok(Some(Target::Nexus {
                    game: Some(g.to_string()),
                    mod_id,
                    file_id: None,
                    key: None,
                }));
            }
        }
    }
    Ok(digits(t).map(|mod_id| Target::Nexus {
        game: None,
        mod_id,
        file_id: None,
        key: None,
    }))
}

/// Runs the core install pipeline with the UI's prompts and reports
/// FOMOD/layout warnings. `ask_explicit` = `--fomod ask` / `reinstall --ask`.
pub fn install_archive(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    data_dir: &Path,
    archive: &Path,
    meta: &InstallMeta,
    ask_explicit: bool,
) -> R<InstallOutcome> {
    let ui = cx.ui;
    let prompts = ui.install_prompts(ask_explicit);
    let chooser = |c: &[String]| ui.choose_variant(c);
    let name = meta.name.clone().unwrap_or_default();
    let mut resolver = if prompts.fomod {
        ui.fomod_resolver(&name)
    } else {
        None
    };
    let hooks = InstallHooks {
        choose_variant: prompts
            .variant
            .then_some(&chooser as &dyn Fn(&[String]) -> Option<usize>),
        fomod: resolver.as_deref_mut().map(|r| r as &mut dyn FomodResolver),
    };
    let o = mods::install_archive(cx.store, game, data_dir, archive, meta, hooks)?;
    drop(resolver);
    if let Some(run) = &o.fomod {
        match run.mode {
            ResolvedMode::Ask => {}
            ResolvedMode::Default => ui.info("FOMOD installer: default choices"),
            ResolvedMode::Choices => ui.info("FOMOD installer: recorded choices"),
        }
        for m in &run.report.missing {
            ui.warn(&format!("FOMOD: '{m}' not found in the archive; skipped"));
        }
        for r in &run.report.refused {
            ui.warn(&format!("FOMOD: refusing unsafe destination '{r}'"));
        }
    }
    for w in &o.layout.warnings {
        ui.warn(w);
    }
    Ok(o)
}

/// `--fomod` value, else the `fomod` config key (legacy ambiguity A2: the
/// key was never read; here it sets the default for `install`).
pub fn install_mode(cx: &Cx<'_>, flag: Option<&str>) -> R<FomodMode> {
    match flag {
        Some(m) => Ok(m.parse()?),
        None => Ok(cx.config()?.fomod.parse().unwrap_or_default()),
    }
}

/// Downloadable files in legacy `choose_file` order (MAIN, UPDATE,
/// OPTIONAL, MISCELLANEOUS, newest first) and the index of the default: the
/// author's primary file, else the first (legacy L2849-2859).
pub fn file_rows(files: &[FileInfo]) -> (Vec<&FileInfo>, usize) {
    const CATS: [&str; 4] = ["MAIN", "UPDATE", "OPTIONAL", "MISCELLANEOUS"];
    let cat = |f: &FileInfo| {
        CATS.iter()
            .position(|c| Some(*c) == f.category_name.as_deref())
    };
    let mut rows: Vec<&FileInfo> = files.iter().filter(|f| cat(f).is_some()).collect();
    rows.sort_by_key(|f| (cat(f), std::cmp::Reverse(f.uploaded_timestamp.unwrap_or(0))));
    let def = rows.iter().position(|f| f.is_primary).unwrap_or(0);
    (rows, def)
}

/// Legacy `choose_file`: the UI picks, defaulting to the primary file.
pub fn choose_file(cx: &Cx<'_>, files: &[FileInfo]) -> R<u64> {
    let (rows, def) = file_rows(files);
    if rows.is_empty() {
        fail!("This mod has no downloadable files.");
    }
    if rows.len() == 1 {
        return Ok(rows[def].file_id);
    }
    let i = cx.ui.choose_file(&rows, def);
    Ok(rows.get(i).unwrap_or(&rows[def]).file_id)
}

/// A Nexus file to install.
#[derive(Debug, Clone, Default)]
pub struct NexusInstall {
    pub mod_id: u64,
    pub file_id: Option<u64>,
    pub key: Option<NxmKey>,
    pub name: Option<String>,
    /// downloads row of an nxm link.
    pub row: Option<i64>,
    /// `--fomod`.
    pub fomod: Option<String>,
}

/// Installs a Nexus mod file. `None` when the user declined a reinstall.
pub fn install_nexus(
    cx: &mut Cx<'_>,
    nexus: &Nexus,
    game: &GameConfig,
    req: NexusInstall,
) -> R<Option<InstallOutcome>> {
    let ui = cx.ui;
    let d = &game.domain;
    let mod_id = req.mod_id;
    let info = nexus.run(nexus.client.mod_info(d, mod_id))??;
    let mname = info.name.clone().unwrap_or_else(|| format!("mod {mod_id}"));
    ui.say(
        Msg::line("")
            .b(&mname)
            .p(format!(" by {} ", info.author.as_deref().unwrap_or("?")))
            .d(format!(
                "(v{}) — https://www.nexusmods.com/{d}/mods/{mod_id}",
                info.version.as_deref().unwrap_or("?")
            )),
    );
    if let Some(s) = info.status.as_deref().filter(|s| *s != "published") {
        ui.warn(&format!("Mod status on Nexus: {s}"));
    }
    let files = nexus.run(nexus.client.mod_files(d, mod_id))??;
    let file_id = match req.file_id {
        Some(f) => f,
        None => choose_file(cx, &files.files)?,
    };
    let file = files
        .files
        .iter()
        .find(|f| f.file_id == file_id)
        .ok_or_else(|| AppError::Msg(format!("File {file_id} does not belong to mod {mod_id}.")))?
        .clone();
    ui.say(Msg::line(format!("  File: {} ", file.name)).d(format!(
        "({}, v{}, {})",
        file.category_name.as_deref().unwrap_or("?"),
        file.version.as_deref().unwrap_or("?"),
        human_size(file.size_in_bytes.unwrap_or(0))
    )));
    let installed = mods::list_mods(cx.store.conn(), d)?;
    let mut replaces: Option<&ModRecord> = None;
    if let Some(same) = installed
        .iter()
        .find(|m| m.nexus_mod_id == Some(mod_id) && m.nexus_file_id == Some(file_id))
    {
        if !ui.confirm(
            &format!("Already installed ({}). Reinstall?", same.uid),
            false,
        ) {
            ui.info("Nothing to do.");
            if let Some(r) = req.row {
                download::set_status(cx.store, r, download::DownloadStatus::Cancelled)?;
            }
            return Ok(None);
        }
        replaces = Some(same);
    } else if let Some(older) = installed.iter().find(|m| m.nexus_mod_id == Some(mod_id)) {
        if ui.confirm(
            &format!("Replace the installed file {} with this one?", older.uid),
            true,
        ) {
            replaces = Some(older);
        }
    }
    let (archive, md5) = nx::download_file(
        cx,
        nexus,
        FileRequest {
            game: d,
            mod_id,
            file_id,
            info: Some(&file),
            nxm: req.key.clone(),
            row: req.row,
        },
    )?;
    let name = match req.name.as_deref() {
        Some(n) => n.to_string(),
        None if file.category_name.as_deref() != Some("MAIN") && file.name != mname => {
            format!("{mname} - {}", file.name)
        }
        None => mname.clone(),
    };
    ui.info(&format!("Installing {name}..."));
    let fomod = req.fomod.as_deref();
    // Replacing an older file reuses its FOMOD choices (legacy L3267-3271).
    let (fomod_mode, fomod_choices) = match replaces {
        Some(r) => (
            match fomod {
                Some(m) => m.parse()?,
                None => FomodMode::Reuse,
            },
            mods::recorded_fomod_choices(cx.store.conn(), r.id)?,
        ),
        None => (install_mode(cx, fomod)?, None),
    };
    let meta = InstallMeta {
        name: Some(name.clone()),
        version: file.version.clone(),
        nexus_mod_id: Some(mod_id),
        nexus_file_id: Some(file_id),
        md5,
        source: Some("nexus".into()),
        author: info.author.clone(),
        variant: replaces.and_then(|r| r.variant.clone()),
        replaces: replaces.map(|r| r.id),
        update_policy: replaces.and_then(|r| r.update_policy.clone()),
        fomod_mode,
        fomod_choices,
        ..Default::default()
    };
    let data_dir = cx.config()?.data_dir;
    let lock = staging_lock(cx.paths)?;
    let o = install_archive(cx, game, &data_dir, &archive, &meta, fomod == Some("ask"))?;
    drop(lock);
    ui.say(
        Msg::success("Installed ")
            .b(&name)
            .p(format!(" → {}", o.uid)),
    );
    super::updates::requirements_after_install(cx, game, o.mod_id);
    Ok(Some(o))
}

/// Identifies a local archive on Nexus by its md5 when logged in, so
/// updates can be tracked (legacy L3298-3305). Lookup failures are ignored.
fn identify(cx: &Cx<'_>, game: &GameConfig, meta: &mut InstallMeta) {
    let Some(md5) = meta.md5.clone() else { return };
    let Ok(Some(nexus)) = nx::connect(cx.paths, cx.store, false) else {
        return;
    };
    if !nexus.client.is_authenticated() {
        return;
    }
    let hit = match nexus.run(nexus.client.md5_search(&game.domain, &md5)) {
        Ok(Ok(hits)) => hits.into_iter().next(),
        Ok(Err(e)) => {
            tracing::debug!("md5 lookup failed: {e}");
            None
        }
        Err(_) => None,
    };
    let Some(h) = hit else { return };
    if meta.name.is_none() {
        meta.name = h.mod_name.clone();
    }
    meta.nexus_mod_id = Some(h.mod_id);
    meta.nexus_file_id = Some(h.file_id);
    meta.version = h.version.clone();
    meta.source = Some("nexus".into());
    cx.ui.info(&format!(
        "Identified on Nexus: {} (mod {}, file {})",
        meta.name.as_deref().unwrap_or("?"),
        h.mod_id,
        h.file_id
    ));
}

/// Installs a local archive or folder (copied next to the Nexus downloads).
pub fn install_local(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    path: &Path,
    name: Option<&str>,
    fomod: Option<&str>,
) -> R<InstallOutcome> {
    let path = path
        .canonicalize()
        .map_err(|e| AppError::Msg(format!("{}: {e}", path.display())))?;
    let base = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cfg = cx.config()?;
    let dl = mods::downloads_dir(&cfg.data_dir, &game.domain);
    std::fs::create_dir_all(&dl)?;
    let dest = dl.join(&base);
    // Keep a copy next to Nexus downloads, like legacy.
    let archive = if path.is_file() && dest != path {
        std::fs::copy(&path, &dest).map_err(|e| {
            AppError::Msg(format!(
                "cannot copy {} to {}: {e}",
                path.display(),
                dest.display()
            ))
        })?;
        dest
    } else {
        path.clone()
    };
    let mut meta = InstallMeta {
        name: name.map(str::to_string),
        source: Some("local".into()),
        fomod_mode: install_mode(cx, fomod)?,
        ..Default::default()
    };
    if archive.is_file() {
        meta.md5 = Some(download::md5_file(&archive)?);
        identify(cx, game, &mut meta);
    }
    cx.ui.info(&format!("Installing from {base}..."));
    let lock = staging_lock(cx.paths)?;
    let o = install_archive(
        cx,
        game,
        &cfg.data_dir,
        &archive,
        &meta,
        fomod == Some("ask"),
    )?;
    drop(lock);
    cx.ui.say(
        Msg::success("Installed ")
            .b(&o.name)
            .p(format!(" → {}", o.uid)),
    );
    if meta.nexus_mod_id.is_some() {
        super::updates::requirements_after_install(cx, game, o.mod_id);
    }
    Ok(o)
}

/// Options of [`install`] (legacy `cmd_install` L3323-3360).
#[derive(Debug, Clone, Default)]
pub struct InstallRequest {
    /// Mod id, Nexus URL, `<game>:<id>`, nxm:// link, archive or collection.
    pub target: String,
    pub file: Option<u64>,
    /// ask, default, none or reuse.
    pub fomod: Option<String>,
    pub name: Option<String>,
    pub deploy: bool,
    /// Collections only.
    pub optional: Option<String>,
    pub revision: Option<u64>,
    pub download_only: bool,
}

/// Result of [`install`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct InstallResult {
    /// Game the target was installed into.
    pub game: Option<String>,
    /// The installed mod (`None`: declined, collection, or download only).
    pub installed: Option<InstalledMod>,
    /// A collection was installed (or downloaded).
    pub collection: Option<super::collection::CollectionResult>,
    pub after: AfterChangeView,
}

/// Serializable part of an [`InstallOutcome`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledMod {
    pub mod_id: i64,
    pub uid: String,
    pub name: String,
    pub files: usize,
    pub size: u64,
    pub fomod_found: bool,
    pub replaced: Option<String>,
    pub warnings: Vec<String>,
}

impl From<&InstallOutcome> for InstalledMod {
    fn from(o: &InstallOutcome) -> Self {
        let mut warnings = o.layout.warnings.clone();
        if let Some(run) = &o.fomod {
            warnings.extend(
                run.report
                    .missing
                    .iter()
                    .map(|m| format!("FOMOD: '{m}' not found in the archive; skipped")),
            );
            warnings.extend(
                run.report
                    .refused
                    .iter()
                    .map(|r| format!("FOMOD: refusing unsafe destination '{r}'")),
            );
        }
        Self {
            mod_id: o.mod_id,
            uid: o.uid.clone(),
            name: o.name.clone(),
            files: o.files,
            size: o.size,
            fomod_found: o.fomod_found,
            replaced: o.replaced.clone(),
            warnings,
        }
    }
}

/// Serializable [`AfterChange`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct AfterChangeView {
    pub deployed: Option<crate::deploy::DeployReport>,
    pub needs_deploy: bool,
}

impl From<AfterChange> for AfterChangeView {
    fn from(a: AfterChange) -> Self {
        Self {
            deployed: a.deployed,
            needs_deploy: a.needs_deploy,
        }
    }
}

/// `install <target>`: Nexus mods, local archives and collections.
pub fn install(cx: &mut Cx<'_>, req: InstallRequest) -> R<InstallResult> {
    let target = parse_target(&req.target)?.ok_or_else(|| {
        AppError::Msg(format!(
            "Don't know how to install '{}'. Give a mod id, a Nexus URL, an nxm:// link, an archive or a collection URL.",
            req.target
        ))
    })?;
    let (game, installed) = match target {
        Target::Local(p) => {
            let game = cx.need_game()?;
            let o = install_local(cx, &game, &p, req.name.as_deref(), req.fomod.as_deref())?;
            (game, Some(InstalledMod::from(&o)))
        }
        Target::Collection(reference) => {
            // `install --fomod` values that collections accept pass through.
            let fomod = req
                .fomod
                .filter(|f| matches!(f.as_str(), "ask" | "default"));
            let r = super::collection::install(
                cx,
                &super::collection::InstallOpts {
                    reference,
                    optional: req.optional,
                    fomod,
                    revision: req.revision,
                    deploy: req.deploy,
                    download_only: req.download_only,
                },
            )?;
            return Ok(InstallResult {
                game: r.game.clone(),
                collection: Some(r),
                ..Default::default()
            });
        }
        Target::Nexus {
            game,
            mod_id,
            file_id,
            key,
        } => {
            let nexus = nx::require(cx.paths, cx.store)?;
            let game = cx.game_for(game.as_deref())?;
            let file_id = req.file.or(file_id);
            let Some(o) = install_nexus(
                cx,
                &nexus,
                &game,
                NexusInstall {
                    mod_id,
                    file_id,
                    key,
                    name: req.name.clone(),
                    row: None,
                    fomod: req.fomod.clone(),
                },
            )?
            else {
                return Ok(InstallResult {
                    game: Some(game.domain),
                    ..Default::default()
                });
            };
            (game, Some(InstalledMod::from(&o)))
        }
    };
    let after = if req.deploy {
        cx.ui.blank();
        AfterChange {
            deployed: Some(dep::run_deploy(cx, &game, None, false)?),
            needs_deploy: false,
        }
    } else {
        dep::after_change(cx, &game)?
    };
    Ok(InstallResult {
        game: Some(game.domain),
        installed,
        collection: None,
        after: after.into(),
    })
}

/// `download <target>`: fetches a Nexus file without installing it.
/// Collection targets are handled by the caller.
pub fn download_target(
    cx: &mut Cx<'_>,
    game: Option<&str>,
    mod_id: u64,
    file_id: Option<u64>,
    key: Option<NxmKey>,
) -> R<PathBuf> {
    let nexus = nx::require(cx.paths, cx.store)?;
    let game = cx.game_for(game)?;
    let files = nexus.run(nexus.client.mod_files(&game.domain, mod_id))??;
    let file_id = match file_id {
        Some(f) => f,
        None => choose_file(cx, &files.files)?,
    };
    let info = files.files.iter().find(|f| f.file_id == file_id);
    let (path, _) = nx::download_file(
        cx,
        &nexus,
        FileRequest {
            game: &game.domain,
            mod_id,
            file_id,
            info,
            nxm: key,
            row: None,
        },
    )?;
    cx.ui.success(&format!("Saved → {}", path.display()));
    Ok(path)
}

/// `reinstall <mod>`: re-extracts the archive (downloading it again when it
/// is gone). `fomod` = `--fomod`, `ask` = `--ask` (legacy L3636-3641).
pub fn reinstall(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    reference: &str,
    fomod: Option<&str>,
    ask: bool,
) -> R<Reinstalled> {
    let fomod_mode = match (fomod, ask) {
        (Some(m), _) => m.parse()?,
        (None, true) => FomodMode::Ask,
        (None, false) => FomodMode::Reuse,
    };
    let rec = mods::resolve(cx.store.conn(), &game.domain, reference)?;
    let archive = match &rec.archive {
        Some(a) if a.exists() => a.clone(),
        other => {
            let (Some(m), Some(f)) = (rec.nexus_mod_id, rec.nexus_file_id) else {
                fail!(
                    "Archive missing: {}",
                    other
                        .as_ref()
                        .map_or("(none)".into(), |p| p.display().to_string())
                );
            };
            let nexus = nx::require(cx.paths, cx.store)?;
            nx::download_file(
                cx,
                &nexus,
                FileRequest {
                    game: &game.domain,
                    mod_id: m,
                    file_id: f,
                    info: None,
                    nxm: None,
                    row: None,
                },
            )?
            .0
        }
    };
    let meta = InstallMeta {
        name: Some(rec.name.clone()),
        version: rec.version.clone(),
        nexus_mod_id: rec.nexus_mod_id,
        nexus_file_id: rec.nexus_file_id,
        md5: rec.md5.clone(),
        source: Some(rec.source.clone()),
        author: rec.author.clone(),
        category: rec.category.clone(),
        update_policy: rec.update_policy.clone(),
        // --ask re-picks the variant too (legacy L3660).
        variant: if ask { None } else { rec.variant.clone() },
        replaces: Some(rec.id),
        uid: Some(rec.uid.clone()),
        fomod_mode,
        fomod_choices: mods::recorded_fomod_choices(cx.store.conn(), rec.id)?,
    };
    cx.ui.info(&format!("Reinstalling {}...", rec.uid));
    let data_dir = cx.config()?.data_dir;
    let lock = staging_lock(cx.paths)?;
    let o = install_archive(
        cx,
        game,
        &data_dir,
        &archive,
        &meta,
        fomod_mode == FomodMode::Ask,
    )?;
    drop(lock);
    cx.ui.success(&format!("Reinstalled {}", rec.uid));
    let after = dep::after_change(cx, game)?;
    Ok(Reinstalled {
        installed: InstalledMod::from(&o),
        after: after.into(),
    })
}

/// Result of [`reinstall`].
#[derive(Debug, Clone, Serialize)]
pub struct Reinstalled {
    pub installed: InstalledMod,
    pub after: AfterChangeView,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nexus(game: Option<&str>, m: u64, f: Option<u64>) -> Option<Target> {
        Some(Target::Nexus {
            game: game.map(str::to_string),
            mod_id: m,
            file_id: f,
            key: None,
        })
    }

    fn file(id: u64, cat: &str, ts: i64, primary: bool) -> FileInfo {
        FileInfo {
            file_id: id,
            name: format!("f{id}"),
            version: None,
            category_name: Some(cat.into()),
            file_name: None,
            size_in_bytes: None,
            uploaded_timestamp: Some(ts),
            is_primary: primary,
        }
    }

    /// Catches: the `fomod` config key ignored when `--fomod` is absent
    /// (legacy A2), or overriding an explicit flag.
    #[test]
    fn fomod_config_key_sets_install_mode_unless_flag_given() {
        use crate::app::testing::Recorder;
        use crate::config::{self, ConfigKey};
        use crate::paths::Paths;
        use crate::store::Store;
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::for_home(home.path());
        let mut store = Store::open_in_memory().unwrap();
        let ui = Recorder::default();
        let mode = |store: &mut Store, flag: Option<&str>| {
            install_mode(&Cx::new(&paths, store, &ui), flag).unwrap()
        };
        assert_eq!(mode(&mut store, None), FomodMode::Auto);
        for (value, want) in [("ask", FomodMode::Ask), ("default", FomodMode::Default)] {
            config::set(&store, &paths, ConfigKey::Fomod, value).unwrap();
            assert_eq!(mode(&mut store, None), want);
        }
        assert_eq!(mode(&mut store, Some("none")), FomodMode::None);
        assert!(install_mode(&Cx::new(&paths, &mut store, &ui), Some("bogus")).is_err());
    }

    #[test]
    fn file_rows_default_to_the_primary_file() {
        let files = [
            file(1, "OPTIONAL", 5, false),
            file(2, "MAIN", 1, false),
            file(3, "MAIN", 2, false),
            file(4, "OLD_VERSION", 9, true),
        ];
        let (rows, def) = file_rows(&files);
        let ids: Vec<u64> = rows.iter().map(|f| f.file_id).collect();
        assert_eq!((ids, def), (vec![3, 2, 1], 0));
        let files = [file(1, "MAIN", 2, false), file(2, "OPTIONAL", 1, true)];
        assert_eq!(file_rows(&files).1, 1);
    }

    #[test]
    fn targets_parse_like_legacy() {
        let p = |t: &str| parse_target(t).unwrap();
        assert_eq!(p("12604"), nexus(None, 12604, None));
        assert_eq!(
            p("skyrimspecialedition:266"),
            nexus(Some("skyrimspecialedition"), 266, None)
        );
        assert_eq!(
            p("https://www.nexusmods.com/skyrimspecialedition/mods/266?tab=files&file_id=99"),
            nexus(Some("skyrimspecialedition"), 266, Some(99))
        );
        assert_eq!(
            p("https://www.nexusmods.com/games/stardewvalley/mods/5"),
            nexus(Some("stardewvalley"), 5, None)
        );
        let Some(Target::Nexus { key, file_id, .. }) = p(
            "nxm://skyrimspecialedition/mods/266/files/1000?key=abc&expires=9999999999&user_id=2",
        ) else {
            panic!()
        };
        assert_eq!(file_id, Some(1000));
        assert_eq!(key.unwrap().key, "abc");
        assert_eq!(
            p("nxm://skyrimspecialedition/collections/abc123"),
            Some(Target::Collection(
                "nxm://skyrimspecialedition/collections/abc123".into()
            ))
        );
        assert!(matches!(
            p("https://next.nexusmods.com/skyrimspecialedition/collections/x"),
            Some(Target::Collection(_))
        ));
        assert_eq!(p("not a target"), None);
        assert_eq!(p("Bad:12"), None);
        assert!(parse_target("nxm://evil.example/mods/1/files/2").is_err());
    }
}
