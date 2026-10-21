// SPDX-License-Identifier: GPL-3.0-or-later
//! The collection driver (legacy `collection_install` L2328-2555,
//! `collection_update`, `collection_remove`, `collection_info`): fetching,
//! selection, downloads and installs on top of [`crate::collections`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::deploy as dep;
use super::nexus::{self as nx, FileRequest, Nexus};
use super::{chain, fail, human_size, staging_lock, AppError, Cx, Msg, R};
use crate::collections::{
    self, CollectionRecord, CollectionRef, Finish, Manifest, ManifestMod, OptionalMode, Revision,
};
use crate::deploy::DeployReport;
use crate::download::{self, DownloadStatus, FetchRequest, Fetcher};
use crate::fomod::FomodMode;
use crate::games::GameConfig;
use crate::mods;

pub const URL_HELP: &str = "  Supported formats:
    nxm://{game}/collections/{slug}[/revisions/{rev}]
    https://www.nexusmods.com/games/{game}/collections/{slug}
    https://www.nexusmods.com/{game}/collections/{slug}
    {slug}   (for the active game)";

/// Options of an install, download or update.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallOpts {
    /// Collection URL, nxm:// link or slug.
    pub reference: String,
    /// all, none or ask (default ask; none when the UI cannot ask).
    pub optional: Option<String>,
    /// collection (default), ask or default.
    pub fomod: Option<String>,
    pub revision: Option<u64>,
    pub deploy: bool,
    pub download_only: bool,
}

pub fn parse(reference: &str) -> R<CollectionRef> {
    collections::parse_ref(reference).ok_or_else(|| {
        AppError::Msg(format!(
            "Cannot parse collection reference: {reference}\n{URL_HELP}"
        ))
    })
}

/// A collection mod that could not be installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    pub name: String,
    pub reason: String,
    /// Nexus file page, when known.
    pub url: Option<String>,
}

/// What an install did.
pub struct Installed {
    pub game: GameConfig,
    /// Collection mods in their new load order.
    pub ordered: Vec<i64>,
    pub manifest: Manifest,
}

/// Result of [`install`], [`update`] and [`install_link`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct CollectionResult {
    pub game: Option<String>,
    pub slug: String,
    pub revision: Option<u64>,
    /// Cancelled at the confirmation, or `--download-only`.
    pub cancelled: bool,
    pub download_only: bool,
    pub installed: usize,
    pub present: usize,
    pub failed: Vec<Failure>,
    /// Mods dropped by an update and uninstalled.
    pub removed: usize,
    pub up_to_date: bool,
    pub deployed: Option<DeployReport>,
}

fn optional_picks(cx: &Cx<'_>, m: &Manifest, mode: OptionalMode) -> Vec<usize> {
    let opt = collections::optional_indexes(m);
    if opt.is_empty() {
        return Vec::new();
    }
    let skip = |cx: &Cx<'_>| {
        cx.ui.info(&format!(
            "Skipping {} optional mods (use --optional all to include them).",
            opt.len()
        ));
        Vec::new()
    };
    match mode {
        OptionalMode::All => (0..opt.len()).collect(),
        OptionalMode::None => skip(cx),
        OptionalMode::Ask => match cx.ui.choose_optional(m, &opt) {
            Some(p) => p,
            None => skip(cx),
        },
    }
}

/// `browse` / `manual` sources: only when the UI can ask (legacy
/// L2637-2651). The chosen file is copied next to the downloads.
fn manual(cx: &Cx<'_>, mm: &ManifestMod, dl: &Path) -> R<PathBuf> {
    let Some(p) = cx.ui.manual_download(mm) else {
        fail!("manual download required");
    };
    let p = super::expand_home(&p.to_string_lossy(), &cx.paths.home);
    if p.as_os_str().is_empty() || !p.exists() {
        fail!("manual download required");
    }
    if p.is_dir() {
        return Ok(p);
    }
    let dest = dl.join(
        p.file_name()
            .ok_or_else(|| AppError::Msg("not a file".into()))?,
    );
    if dest != p {
        std::fs::create_dir_all(dl)?;
        std::fs::copy(&p, &dest)?;
    }
    Ok(dest)
}

/// Gets the archive of one collection mod (legacy L2452-2463, L2576-2635).
fn acquire(
    cx: &Cx<'_>,
    nexus: &Nexus,
    game: &str,
    dir: &Path,
    dl: &Path,
    mm: &ManifestMod,
) -> R<PathBuf> {
    match mm.source_type.as_str() {
        "nexus" => {
            let (Some(m), Some(f)) = (mm.mod_id, mm.file_id) else {
                fail!("the collection names no Nexus mod/file id");
            };
            let (path, md5) = nx::download_file(
                cx,
                nexus,
                FileRequest {
                    game,
                    mod_id: m,
                    file_id: f,
                    info: None,
                    nxm: None,
                    row: None,
                },
            )
            .map_err(|e| AppError::Msg(format!("download failed: {}", chain(&e))))?;
            if let (Some(want), Some(got)) = (&mm.md5, &md5) {
                if !want.eq_ignore_ascii_case(got) {
                    fail!("download failed: checksum mismatch (expected {want}, got {got})");
                }
            }
            Ok(path)
        }
        "bundle" => collections::find_bundled(dir, mm)
            .ok_or_else(|| AppError::Msg("bundled file missing from collection archive".into())),
        "direct" => {
            let url = mm
                .url()
                .ok_or_else(|| AppError::Msg("direct download failed: no URL".into()))?;
            let name = collections::direct_file_name(url, &mm.name);
            cx.ui.say(Msg::line("  ").d(format!("↓ {name} (direct)")));
            let dest = dl.join(&name);
            let f = nx::fetch_with_progress(
                nexus,
                cx.ui,
                &name,
                FetchRequest {
                    url,
                    dest: &dest,
                    md5: mm.md5.as_deref(),
                    sha256: None,
                    events: None,
                },
            )?
            .map_err(|e| AppError::Msg(format!("direct download failed: {e}")))?;
            Ok(f.path)
        }
        "browse" | "manual" => manual(cx, mm, dl),
        other => fail!("unsupported source type '{other}'"),
    }
}

fn failures(game: &str, m: &Manifest, failed: &[(usize, String)]) -> Vec<Failure> {
    failed
        .iter()
        .map(|(i, why)| {
            let mm = &m.mods[*i];
            Failure {
                name: mm.name.clone(),
                reason: why.clone(),
                url: mm.mod_id.map(|id| {
                    format!(
                        "https://www.nexusmods.com/{game}/mods/{id}?tab=files&file_id={}",
                        mm.file_id.map_or(String::new(), |f| f.to_string())
                    )
                }),
            }
        })
        .collect()
}

fn print_failures(cx: &Cx<'_>, failed: &[Failure]) {
    if failed.is_empty() {
        return;
    }
    cx.ui.blank();
    for f in failed {
        cx.ui
            .say(Msg::err_line(format!("    ✗ {} ", f.name)).d(format!("— {}", f.reason)));
        if let Some(u) = &f.url {
            cx.ui.say(Msg::err_line("      ").d(u));
        }
    }
}

/// Fetches, selects, downloads and installs a collection (without the final
/// deploy). `None` when cancelled or with `download_only`; `out` collects
/// the counts either way.
pub fn install_collection(
    cx: &mut Cx<'_>,
    opts: &InstallOpts,
    out: &mut CollectionResult,
) -> R<Option<Installed>> {
    let ui = cx.ui;
    let r = parse(&opts.reference)?;
    out.slug = r.slug.clone();
    let optional: OptionalMode = opts.optional.as_deref().unwrap_or("ask").parse()?;
    let fomod_mode: FomodMode = opts.fomod.as_deref().unwrap_or("collection").parse()?;
    let nexus = nx::require(cx.paths, cx.store)?;
    let game = cx.game_for(r.game.as_deref())?;
    out.game = Some(game.domain.clone());
    let d = game.domain.clone();
    let cfg = cx.config()?;
    let premium = nx::is_premium(cx.store, &nexus).unwrap_or(false);
    ui.heading(&format!("Collection — {}", r.slug));
    ui.say(Msg::info("Querying collection ").b(&r.slug).p(format!(
            " ({d}, revision {})...",
            opts.revision
                .or(r.revision)
                .map_or("latest".into(), |n| n.to_string())
        )));
    let rev = nexus.run(collections::fetch_revision(
        &nexus.client,
        &d,
        &r.slug,
        opts.revision.or(r.revision),
        cfg.adult,
    ))??;
    out.revision = Some(rev.number);
    let dir = collections::revision_dir(&cfg.data_dir, &d, &r.slug, rev.number);
    let fetcher = Fetcher::new()?;
    let fm = nexus.run(collections::fetch_manifest(
        &nexus.client,
        &fetcher,
        &rev,
        &dir,
    ))??;
    if fm.from_api {
        ui.warn(&format!(
            "Collection archive unavailable ({}) — using the API mod list (no FOMOD choices or load-order rules).",
            fm.archive_error.as_deref().unwrap_or("unknown reason")
        ));
    }
    let m = fm.manifest;
    let n_opt = collections::optional_indexes(&m).len();
    ui.blank();
    ui.say(
        Msg::info("Collection : ")
            .b(&rev.name)
            .p(" by ")
            .b(&rev.author),
    );
    let mut line = Msg::info("Revision   : ")
        .b(rev.number.to_string())
        .p(format!("  [{}]", rev.status));
    if let Some(l) = rev.latest.filter(|l| *l != rev.number) {
        line = line.y(format!("  (latest is {l})"));
    }
    ui.say(line);
    ui.say(
        Msg::info("Mods       : ")
            .b(m.mods.len().to_string())
            .p(format!(
                "  ({} required, {n_opt} optional)",
                m.mods.len() - n_opt
            )),
    );
    ui.say(Msg::info("Total size : ").b(human_size(rev.total_size)));
    if let Some(i) = m.install_instructions() {
        ui.say(
            Msg::line("\n")
                .b("Curator's instructions")
                .p(" ")
                .d("(written for Vortex — modman handles staging/deploy itself)"),
        );
        for line in i.lines() {
            ui.line(&format!("    │ {line}"));
        }
    }
    ui.blank();

    let picks = optional_picks(cx, &m, optional);
    let selected = collections::select(&m, &picks);
    let installed = mods::list_mods(cx.store.conn(), &d)?;
    let plan = collections::plan(&m, &selected, &installed);
    let n_dl = plan
        .todo
        .iter()
        .filter(|&&i| m.mods[i].source_type == "nexus")
        .count();
    ui.info(&format!("Already installed : {}", plan.have.len()));
    ui.info(&format!("To install        : {}", plan.todo.len()));
    ui.info(&format!(
        "To download       : {n_dl}  ({})",
        human_size(plan.download_bytes)
    ));
    let dl = mods::downloads_dir(&cfg.data_dir, &d);
    if let Some(free) = collections::free_bytes(&dl) {
        if plan.download_bytes.saturating_mul(2) > free {
            ui.warn(&format!(
                "Low disk space: ~{} needed (archives + extracted files), {} free.",
                human_size(plan.download_bytes.saturating_mul(2)),
                human_size(free)
            ));
        }
    }
    if n_dl > 0 && !premium {
        ui.warn(&format!(
            "Free account: Nexus requires a click on \"Slow download\" for each of the {n_dl} files."
        ));
    }
    if plan.todo.is_empty() && plan.have.is_empty() {
        fail!("Nothing selected.");
    }
    ui.blank();
    if !ui.confirm("Proceed?", true) {
        ui.info("Cancelled.");
        out.cancelled = true;
        return Ok(None);
    }

    let mut failed: Vec<(usize, String)> = Vec::new();
    let mut archives: BTreeMap<usize, PathBuf> = BTreeMap::new();
    if !plan.todo.is_empty() {
        ui.heading("Downloading");
    }
    for (k, &i) in plan.todo.iter().enumerate() {
        let mm = &m.mods[i];
        ui.say(
            Msg::line(format!("  [{}/{}] {}  ", k + 1, plan.todo.len(), mm.name))
                .d(human_size(mm.file_size)),
        );
        match acquire(cx, &nexus, &d, &dir, &dl, mm) {
            Ok(p) => {
                archives.insert(i, p);
            }
            Err(e) => failed.push((i, chain(&e))),
        }
    }
    if opts.download_only {
        ui.blank();
        ui.success(&format!(
            "Downloaded {} archive(s) into {}",
            archives.len(),
            dl.display()
        ));
        out.failed = failures(&d, &m, &failed);
        print_failures(cx, &out.failed);
        ui.info(&format!(
            "Install with: modman collection install nxm://{d}/collections/{}/revisions/{}",
            r.slug, rev.number
        ));
        out.download_only = true;
        return Ok(None);
    }

    let mut ids: BTreeMap<usize, i64> = plan.have.clone();
    if !archives.is_empty() {
        ui.heading("Installing");
    }
    let total = plan.todo.len();
    for (k, &i) in plan.todo.iter().enumerate() {
        let Some(path) = archives.get(&i) else {
            continue;
        };
        let mm = &m.mods[i];
        ui.say(
            Msg::line(format!("  [{}/{total}] ", k + 1))
                .b(&mm.name)
                .p(" ")
                .d(mm.version.as_deref().unwrap_or("")),
        );
        let meta = mm.install_meta(fomod_mode);
        let lock = staging_lock(cx.paths)?;
        let res = super::install::install_archive(
            cx,
            &game,
            &cfg.data_dir,
            path,
            &meta,
            fomod_mode == FomodMode::Ask,
        );
        drop(lock);
        match res {
            Ok(o) => {
                ids.insert(i, o.mod_id);
            }
            Err(e) => failed.push((i, format!("install failed: {}", chain(&e)))),
        }
    }

    failed.sort_by_key(|f| f.0);
    let failed_names: Vec<String> = failed
        .iter()
        .map(|(i, _)| m.mods[*i].name.clone())
        .collect();
    let lock = staging_lock(cx.paths)?;
    let fin = collections::finish(
        cx.store,
        &Finish {
            game: &d,
            revision: &rev,
            manifest: &m,
            dir: &dir,
            selected: &selected,
            ids: &ids,
            failed: &failed_names,
        },
    )?;
    drop(lock);
    if fin.in_cycle > 0 {
        ui.warn(&format!(
            "{} mods are part of a rule cycle; keeping collection order for them",
            fin.in_cycle
        ));
    }
    if !fin.ordered.is_empty() {
        ui.info(&format!(
            "Applied the collection's load order ({} mods, {} rules).",
            fin.ordered.len(),
            fin.rules
        ));
    }
    let ok = ids.len() - plan.have.len();
    out.installed = ok;
    out.present = plan.have.len();
    out.failed = failures(&d, &m, &failed);
    ui.blank();
    ui.heading("Collection Summary");
    ui.line(&format!(
        "  Collection : {}  (rev {})",
        rev.name, rev.number
    ));
    ui.line(&format!("  Installed  : {ok}"));
    ui.line(&format!(
        "  Present    : {}  (already installed)",
        plan.have.len()
    ));
    ui.line(&format!("  Failed     : {}", failed.len()));
    print_failures(cx, &out.failed);
    ui.blank();
    if !failed.is_empty() {
        ui.warn("Re-run the same command to retry — finished mods are skipped.");
    }
    let members = mods::collection_mod_ids(cx.store.conn(), &d, &r.slug)?;
    super::updates::requirements_after_collection(cx, &game, &members);
    Ok(Some(Installed {
        game,
        ordered: fin.ordered,
        manifest: m,
    }))
}

/// `collection install|download`, deploying afterwards per `--deploy` /
/// `auto-deploy`.
pub fn install(cx: &mut Cx<'_>, opts: &InstallOpts) -> R<CollectionResult> {
    let mut out = CollectionResult::default();
    if let Some(done) = install_collection(cx, opts, &mut out)? {
        out.deployed = dep::deploy_after_collection(cx, &done.game, opts.deploy)?;
    }
    Ok(out)
}

/// An nxm:// collection link (queued as `row`): installs the pinned
/// revision and closes the row.
pub fn install_link(
    cx: &mut Cx<'_>,
    game: &str,
    slug: &str,
    revision: Option<u64>,
    row: i64,
) -> R<CollectionResult> {
    download::set_status(cx.store, row, DownloadStatus::Downloading)?;
    let opts = InstallOpts {
        reference: match revision {
            Some(n) => format!("nxm://{game}/collections/{slug}/revisions/{n}"),
            None => format!("nxm://{game}/collections/{slug}"),
        },
        ..Default::default()
    };
    let mut out = CollectionResult::default();
    match install_collection(cx, &opts, &mut out) {
        Ok(Some(done)) => {
            download::set_status(cx.store, row, DownloadStatus::Done)?;
            out.deployed = dep::deploy_after_collection(cx, &done.game, false)?;
            Ok(out)
        }
        Ok(None) => {
            download::set_status(cx.store, row, DownloadStatus::Cancelled)?;
            Ok(out)
        }
        Err(e) => {
            let _ = download::mark_failed(cx.store, row, &chain(&e));
            Err(e)
        }
    }
}

/// `collection info`: the revision on Nexus and the installed record.
#[derive(Debug, Clone, Serialize)]
pub struct CollectionInfo {
    pub game: String,
    pub slug: String,
    pub revision: RevisionInfo,
    pub installed: Option<CollectionRecord>,
    /// A newer revision than the installed one exists.
    pub update_available: Option<u64>,
}

/// Serializable part of a [`Revision`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RevisionInfo {
    pub name: String,
    pub author: String,
    pub game_name: String,
    pub number: u64,
    pub status: String,
    pub latest: Option<u64>,
    pub mod_count: u64,
    pub optional_count: u64,
    pub total_size: u64,
    pub summary: String,
    pub url: String,
}

fn revision_info(rev: &Revision, game: &str, slug: &str) -> RevisionInfo {
    RevisionInfo {
        name: rev.name.clone(),
        author: rev.author.clone(),
        game_name: rev.game_name.clone(),
        number: rev.number,
        status: rev.status.clone(),
        latest: rev.latest,
        mod_count: rev.mod_count,
        optional_count: rev.optional_count as u64,
        total_size: rev.total_size,
        summary: rev.summary.clone(),
        url: format!("https://www.nexusmods.com/games/{game}/collections/{slug}"),
    }
}

/// `collection info <ref>`.
pub fn info(cx: &Cx<'_>, reference: &str) -> R<CollectionInfo> {
    let r = parse(reference)?;
    let nexus = nx::require(cx.paths, cx.store)?;
    let game = cx.game_for(r.game.as_deref())?;
    let cfg = cx.config()?;
    let rev = nexus.run(collections::fetch_revision(
        &nexus.client,
        &game.domain,
        &r.slug,
        r.revision,
        cfg.adult,
    ))??;
    let installed = collections::get(cx.store.conn(), &game.domain, &r.slug)?;
    let update_available = installed
        .as_ref()
        .and_then(|rec| rev.latest.filter(|l| *l != rec.revision));
    Ok(CollectionInfo {
        revision: revision_info(&rev, &game.domain, &r.slug),
        game: game.domain,
        slug: r.slug,
        installed,
        update_available,
    })
}

/// `collection update`: installs the latest revision and removes the mods
/// it dropped (after a confirmation).
pub fn update(cx: &mut Cx<'_>, opts: &InstallOpts) -> R<CollectionResult> {
    let r = parse(&opts.reference)?;
    let game = cx.game_for(r.game.as_deref())?;
    let d = game.domain.clone();
    let rec = collections::get(cx.store.conn(), &d, &r.slug)?.ok_or_else(|| {
        AppError::Msg(format!(
            "Collection '{}' is not installed for {}.",
            r.slug, game.name
        ))
    })?;
    let nexus = nx::require(cx.paths, cx.store)?;
    let cfg = cx.config()?;
    // Legacy ambiguity A6 forced viewAdultContent:false here; the setting applies.
    let latest = nexus
        .run(collections::fetch_revision(
            &nexus.client,
            &d,
            &r.slug,
            None,
            cfg.adult,
        ))??
        .number;
    let mut out = CollectionResult {
        game: Some(d.clone()),
        slug: r.slug.clone(),
        revision: Some(latest),
        ..Default::default()
    };
    if latest == rec.revision && rec.failed.is_empty() {
        cx.ui
            .success(&format!("{} is up to date (revision {latest}).", r.slug));
        out.up_to_date = true;
        return Ok(out);
    }
    cx.ui.info(&format!(
        "Updating {}: revision {} → {latest}",
        r.slug, rec.revision
    ));
    let o = InstallOpts {
        reference: format!("nxm://{d}/collections/{}", r.slug),
        revision: Some(latest),
        ..opts.clone()
    };
    let Some(done) = install_collection(cx, &o, &mut out)? else {
        return Ok(out);
    };
    let dropped = collections::dropped_members(
        cx.store.conn(),
        &d,
        &r.slug,
        &rec.mods,
        &done.ordered,
        &done.manifest,
    )?;
    let ids = |v: &[mods::ModRecord]| v.iter().map(|m| m.id).collect::<Vec<_>>();
    collections::detach(cx.store, &d, &r.slug, &ids(&dropped.shared))?;
    if !dropped.exclusive.is_empty() {
        cx.ui.blank();
        cx.ui.warn(&format!(
            "{} mod(s) are no longer part of {}:",
            dropped.exclusive.len(),
            r.slug
        ));
        for m in &dropped.exclusive {
            cx.ui.say(Msg::line("    ").y("−").p(format!(" {}", m.uid)));
        }
        let gone = ids(&dropped.exclusive);
        if cx.ui.confirm("Uninstall them?", true) {
            let lock = staging_lock(cx.paths)?;
            mods::remove_mods(cx.store, &d, &gone, None)?;
            drop(lock);
            cx.ui.success(&format!(
                "Removed {} dropped mod(s).",
                dropped.exclusive.len()
            ));
            out.removed = gone.len();
        } else {
            collections::detach(cx.store, &d, &r.slug, &gone)?;
        }
    }
    out.deployed = dep::deploy_after_collection(cx, &done.game, opts.deploy)?;
    Ok(out)
}

/// Result of [`remove`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct RemoveResult {
    pub removed: bool,
    pub uninstalled: usize,
    pub after: super::install::AfterChangeView,
}

/// `collection remove [--keep-mods]` after a confirmation.
pub fn remove(cx: &mut Cx<'_>, reference: &str, keep_mods: bool) -> R<RemoveResult> {
    let r = parse(reference)?;
    let game = cx.game_for(r.game.as_deref())?;
    let d = game.domain.clone();
    if collections::get(cx.store.conn(), &d, &r.slug)?.is_none() {
        fail!("Collection '{}' is not installed.", r.slug);
    }
    let (ex, sh) = collections::exclusive_members(cx.store.conn(), &d, &r.slug)?;
    let ui = cx.ui;
    ui.heading(&format!("Remove collection — {}", r.slug));
    if keep_mods {
        ui.info("Mods stay installed (only the collection record is removed).");
    } else {
        ui.info(&format!(
            "{} mod(s) will be uninstalled; {} shared with other collections stay.",
            ex.len(),
            sh.len()
        ));
    }
    if !ui.confirm(&format!("Remove collection {}?", r.slug), true) {
        ui.info("Cancelled.");
        return Ok(RemoveResult::default());
    }
    let lock = staging_lock(cx.paths)?;
    let res = collections::remove(cx.store, &d, &r.slug, keep_mods)?;
    drop(lock);
    if !res.uninstalled.is_empty() {
        ui.info(&format!("Uninstalled {} mod(s).", res.uninstalled.len()));
    }
    ui.success(&format!("Collection {} removed.", r.slug));
    let after = dep::after_change(cx, &game)?;
    Ok(RemoveResult {
        removed: true,
        uninstalled: res.uninstalled.len(),
        after: after.into(),
    })
}
