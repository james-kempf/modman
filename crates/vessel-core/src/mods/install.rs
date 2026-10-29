// SPDX-License-Identifier: GPL-3.0-or-later
//! Archive -> staging -> `mods` row (legacy `install_archive` L1359-1426,
//! `JQ_UPSERT` L575-588). An archive with `fomod/ModuleConfig.xml` runs the
//! FOMOD installer (legacy L1383-1398) and its choices are recorded in
//! `fomod_choices`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension, Transaction};

use super::{io_err, mark_changed, ModsError};
use crate::archive;
use crate::deploy::staging_root;
use crate::fomod::{
    self, FomodChoices, FomodMode, FomodResolver, FomodRun, ResolvedMode, Selection,
};
use crate::games::GameConfig;
use crate::staging::{
    normalize_layout, sanitize_name, staged_files, LayoutOptions, LayoutOutcome, LayoutProfile,
    StagedFile, VariantChooser,
};
use crate::store::{self, Store, StoreError};

/// What is known about the archive being installed (legacy meta JSON).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallMeta {
    /// Display name; defaults to the archive file name without extension.
    pub name: Option<String>,
    pub version: Option<String>,
    pub nexus_mod_id: Option<u64>,
    pub nexus_file_id: Option<u64>,
    pub md5: Option<String>,
    /// `local` (default), `nexus`, ...
    pub source: Option<String>,
    pub author: Option<String>,
    pub category: Option<String>,
    pub update_policy: Option<String>,
    /// Variant folder picked earlier (reinstall / update).
    pub variant: Option<String>,
    /// Row id of an installed mod this one replaces (newer file, reinstall).
    pub replaces: Option<i64>,
    /// Keep exactly this uid (reinstall).
    pub uid: Option<String>,
    /// FOMOD installer mode (legacy meta `fomodMode`).
    pub fomod_mode: FomodMode,
    /// Recorded choices for `reuse` / `collection` (legacy meta `choices`).
    pub fomod_choices: Option<FomodChoices>,
}

/// Interactive callbacks for [`install_archive`]. Leaving `fomod` unset
/// means non-interactive: `ask`/`auto`/`reuse` fall back to defaults.
#[derive(Default)]
pub struct InstallHooks<'a> {
    pub choose_variant: Option<VariantChooser<'a>>,
    pub fomod: Option<&'a mut dyn FomodResolver>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOutcome {
    pub mod_id: i64,
    pub uid: String,
    pub name: String,
    pub files: usize,
    pub size: u64,
    pub layout: LayoutOutcome,
    /// The archive carries a FOMOD installer.
    pub fomod_found: bool,
    /// The FOMOD installer run, unless skipped (`none`) or absent.
    pub fomod: Option<FomodRun>,
    /// uid of the mod that was replaced, if any.
    pub replaced: Option<String>,
}

/// Folder holding the shallowest `fomod/ModuleConfig.xml` (case-insensitive,
/// up to depth 5; legacy `find_fomod` L1336-1341).
pub fn find_fomod(root: &Path) -> Option<PathBuf> {
    walkdir::WalkDir::new(root)
        .min_depth(2)
        .max_depth(5)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_file()
                && e.file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("moduleconfig.xml")
                && e.path()
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|d| d.to_string_lossy().eq_ignore_ascii_case("fomod"))
        })
        .min_by_key(|e| e.depth())
        .and_then(|e| e.path().parent()?.parent().map(Path::to_path_buf))
}

/// Removes the work folder however the install ends.
struct WorkDir(PathBuf);

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn default_name(archive: &Path) -> String {
    let base = archive
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mod".into());
    let stem = if archive.is_dir() {
        base.as_str()
    } else {
        base.rsplit_once('.').map_or(base.as_str(), |(s, _)| s)
    };
    stem.strip_suffix(".tar").unwrap_or(stem).to_string()
}

fn uid_taken(tx: &Transaction<'_>, game: &str, uid: &str) -> rusqlite::Result<bool> {
    tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM mods WHERE game = ?1 AND uid = ?2)",
        params![game, uid],
        |r| r.get(0),
    )
}

/// Extracts `archive`, normalises its layout for the game's profile, moves the
/// tree into the staging root and records it. A mod with the same uid, or the
/// one named by `meta.replaces`, is replaced in place (keeps its position,
/// enabled state and collection membership); otherwise the mod is appended
/// enabled at the highest priority.
pub fn install_archive(
    store: &mut Store,
    game: &GameConfig,
    data_dir: &Path,
    archive_path: &Path,
    meta: &InstallMeta,
    hooks: InstallHooks<'_>,
) -> Result<InstallOutcome, ModsError> {
    let profile: LayoutProfile = game.profile.parse()?;
    let name = meta
        .name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| default_name(archive_path));
    let root = staging_root(game, data_dir);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    // Inside the staging root so the final move is a rename on one filesystem.
    let work = WorkDir(
        root.join(".tmp")
            .join(format!("install-{}-{nanos}", std::process::id())),
    );
    std::fs::create_dir_all(&work.0).map_err(io_err(&work.0))?;
    let ext = work.0.join("x");
    let out = work.0.join("out");
    archive::extract(archive_path, &ext)?;
    let froot = find_fomod(&ext);
    let fomod_found = froot.is_some();
    let InstallHooks {
        choose_variant,
        fomod: resolver,
    } = hooks;
    let mut fomod_run = None;
    let mut src = ext.clone();
    let mode = fomod::resolve_mode(
        meta.fomod_mode,
        meta.fomod_choices.is_some(),
        resolver.is_some(),
    );
    if let (Some(froot), Some(mode)) = (froot, mode) {
        let how = match (mode, resolver, &meta.fomod_choices) {
            (ResolvedMode::Ask, Some(r), _) => Selection::Ask(r),
            (ResolvedMode::Choices, _, Some(c)) => Selection::Choices(c),
            _ => Selection::Default,
        };
        let deploy_dir = game.folder.as_ref().map(|f| f.join(&game.subdir));
        src = work.0.join("fomod");
        fomod_run = Some(fomod::run(&froot, &src, how, deploy_dir.as_deref())?);
    }

    let mut opts = LayoutOptions::new(profile, &name);
    opts.preset_variant = meta.variant.as_deref().filter(|v| !v.is_empty());
    opts.paks_dir = game.paks_dir.as_deref();
    let ue4ss_dir = crate::games::ue4ss_mods_dir(game, game.folder.as_deref());
    opts.ue4ss_mods_dir = ue4ss_dir.as_deref();
    opts.choose_variant = choose_variant;
    if fomod_run.is_some() {
        // Legacy normalize_layout "$fomod_out" ... 0 0 (L1395).
        opts.strip_wrappers = false;
        opts.detect_variants = false;
    }
    let layout = normalize_layout(&src, &out, &opts)?;
    let files = staged_files(&out)?;
    if files.is_empty() {
        let shown = archive_path.file_name().map_or_else(
            || archive_path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        return Err(ModsError::NoInstallableFiles(shown));
    }
    let size: u64 = files.iter().map(|f| f.size).sum();

    let tx = store.conn_mut().transaction().map_err(StoreError::from)?;
    let replaced: Option<(i64, String, Option<String>)> = match meta.replaces {
        Some(id) => tx
            .query_row(
                "SELECT id, uid, staging_dir FROM mods WHERE game = ?1 AND id = ?2",
                params![game.domain, id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?,
        None => None,
    };
    let uid = match (&meta.uid, meta.nexus_mod_id) {
        (Some(u), _) => u.clone(),
        (None, Some(m)) => format!(
            "{}-{m}-{}",
            sanitize_name(&name),
            meta.nexus_file_id.map_or(String::new(), |f| f.to_string())
        ),
        (None, None) => {
            let base = sanitize_name(&name);
            let mut uid = base.clone();
            let mut i = 2;
            while uid_taken(&tx, &game.domain, &uid)?
                && replaced.as_ref().is_none_or(|(_, u, _)| u != &uid)
            {
                uid = format!("{base}-{i}");
                i += 1;
            }
            uid
        }
    };
    let dest = root.join(&uid);
    if let Some((_, _, Some(old))) = &replaced {
        let old = PathBuf::from(old);
        if old != dest {
            let _ = std::fs::remove_dir_all(&old);
        }
    }
    match std::fs::remove_dir_all(&dest) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io_err(&dest)(e)),
        _ => {}
    }
    std::fs::rename(&out, &dest).map_err(io_err(&dest))?;

    let same: Option<i64> = tx
        .query_row(
            "SELECT id FROM mods WHERE game = ?1 AND uid = ?2",
            params![game.domain, uid],
            |r| r.get(0),
        )
        .optional()?;
    let target = same.or(replaced.as_ref().map(|r| r.0));
    let archive_s = archive_path.to_string_lossy().into_owned();
    let dest_s = dest.to_string_lossy().into_owned();
    let source = meta.source.clone().unwrap_or_else(|| "local".into());
    let i64_of = |v: Option<u64>| v.and_then(|n| i64::try_from(n).ok());
    let mod_id = match target {
        Some(id) => {
            tx.execute(
                "UPDATE mods SET uid = ?1, name = ?2, version = ?3, nexus_mod_id = ?4,
                    nexus_file_id = ?5, md5 = ?6, source = ?7, archive = ?8, author = ?9,
                    category = ?10, update_policy = ?11, root = ?12, variant = ?13,
                    staging_dir = ?14, status = 'installed', file_count = ?15, size = ?16,
                    installed_at = ?17
                 WHERE id = ?18",
                params![
                    uid,
                    name,
                    meta.version,
                    i64_of(meta.nexus_mod_id),
                    i64_of(meta.nexus_file_id),
                    meta.md5,
                    source,
                    archive_s,
                    meta.author,
                    meta.category,
                    meta.update_policy,
                    layout.root.as_str(),
                    layout.variant,
                    dest_s,
                    files.len() as i64,
                    size as i64,
                    store::now(),
                    id
                ],
            )?;
            // The same uid existed and another mod was named as replaced:
            // move its collection membership here, then drop it.
            if let Some((rid, _, _)) = replaced.as_ref().filter(|r| r.0 != id) {
                tx.execute(
                    "INSERT OR IGNORE INTO collection_mods (collection_id, mod_id, position)
                     SELECT collection_id, ?1, position FROM collection_mods WHERE mod_id = ?2",
                    params![id, rid],
                )?;
                tx.execute("DELETE FROM mods WHERE id = ?1", [rid])?;
            }
            id
        }
        None => {
            let prio: i64 = tx.query_row(
                "SELECT COALESCE(MAX(priority) + 1, 0) FROM mods WHERE game = ?1",
                [&game.domain],
                |r| r.get(0),
            )?;
            tx.execute(
                "INSERT INTO mods (game, uid, name, version, nexus_mod_id, nexus_file_id, md5,
                    source, archive, author, category, update_policy, root, variant, staging_dir,
                    enabled, priority, file_count, size, installed_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,1,?16,?17,?18,?19)",
                params![
                    game.domain,
                    uid,
                    name,
                    meta.version,
                    i64_of(meta.nexus_mod_id),
                    i64_of(meta.nexus_file_id),
                    meta.md5,
                    source,
                    archive_s,
                    meta.author,
                    meta.category,
                    meta.update_policy,
                    layout.root.as_str(),
                    layout.variant,
                    dest_s,
                    prio,
                    files.len() as i64,
                    size as i64,
                    store::now()
                ],
            )?;
            tx.last_insert_rowid()
        }
    };
    write_files(&tx, mod_id, &files)?;
    write_choices(&tx, mod_id, fomod_run.as_ref())?;
    tx.commit().map_err(StoreError::from)?;
    mark_changed(store, &game.domain)?;
    Ok(InstallOutcome {
        mod_id,
        uid,
        name,
        files: files.len(),
        size,
        layout,
        fomod_found,
        fomod: fomod_run,
        replaced: replaced.map(|r| r.1),
    })
}

/// Records the choices of a FOMOD run; no run clears them (legacy `.fomod = null`).
fn write_choices(
    tx: &Transaction<'_>,
    mod_id: i64,
    run: Option<&FomodRun>,
) -> rusqlite::Result<()> {
    match run {
        Some(r) => tx.execute(
            "INSERT INTO fomod_choices (mod_id, choices, mode, recorded_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (mod_id) DO UPDATE SET choices = excluded.choices, mode = excluded.mode,
                recorded_at = excluded.recorded_at",
            params![mod_id, r.choices.to_json(), r.mode.as_str(), store::now()],
        )?,
        None => tx.execute("DELETE FROM fomod_choices WHERE mod_id = ?1", [mod_id])?,
    };
    Ok(())
}

fn write_files(tx: &Transaction<'_>, mod_id: i64, files: &[StagedFile]) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM mod_files WHERE mod_id = ?1", [mod_id])?;
    let mut stmt = tx.prepare(
        "INSERT INTO mod_files (mod_id, path, size, sha256, mode) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for f in files {
        stmt.execute(params![mod_id, f.path, f.size as i64, f.sha256, f.mode])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games;
    use crate::mods::{list_mods, set_enabled};

    struct T {
        tmp: tempfile::TempDir,
        store: Store,
        game: GameConfig,
    }

    fn setup(profile: &str) -> T {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let mut game = GameConfig::custom("g");
        game.profile = profile.into();
        game.subdir = "Data".into();
        let folder = tmp.path().join("game");
        std::fs::create_dir_all(&folder).unwrap();
        game.folder = Some(folder);
        games::save_game(store.conn(), &game).unwrap();
        T { tmp, store, game }
    }

    /// A source folder (extract() copies folders like archives).
    fn src(t: &T, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let d = t.tmp.path().join("src").join(name);
        for (p, c) in files {
            let f = d.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, c).unwrap();
        }
        d
    }

    fn install(t: &mut T, p: &Path, meta: InstallMeta) -> Result<InstallOutcome, ModsError> {
        let data = t.tmp.path().join("data");
        install_archive(
            &mut t.store,
            &t.game,
            &data,
            p,
            &meta,
            InstallHooks::default(),
        )
    }

    #[test]
    fn installs_into_staging_and_records_files() {
        let mut t = setup("bethesda");
        let p = src(
            &t,
            "Cool Mod",
            &[("Data/cool.esp", "e"), ("Data/textures/a.dds", "t")],
        );
        let o = install(&mut t, &p, InstallMeta::default()).unwrap();
        assert_eq!(o.uid, "Cool_Mod");
        assert_eq!(o.files, 2);
        let m = &list_mods(t.store.conn(), "g").unwrap()[0];
        let st = m.staging_dir.clone().unwrap();
        assert!(st.join("cool.esp").is_file() && st.join("textures/a.dds").is_file());
        assert!(st.starts_with(t.tmp.path().join("data/g/staging")));
        assert!(m.enabled && m.priority == 0);
        let n: i64 = t
            .store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM mod_files WHERE mod_id = ?1",
                [m.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 2);
        // No leftovers in the work area.
        let tmpdir = t.tmp.path().join("data/g/staging/.tmp");
        assert_eq!(std::fs::read_dir(tmpdir).unwrap().count(), 0);
    }

    #[test]
    fn nexus_uid_and_local_collision_suffix() {
        let mut t = setup("generic");
        let p = src(&t, "x", &[("a.txt", "1")]);
        let meta = InstallMeta {
            name: Some("Sky UI".into()),
            nexus_mod_id: Some(12),
            nexus_file_id: Some(34),
            ..Default::default()
        };
        assert_eq!(install(&mut t, &p, meta).unwrap().uid, "Sky_UI-12-34");
        assert_eq!(
            install(&mut t, &p, InstallMeta::default()).unwrap().uid,
            "x"
        );
        assert_eq!(
            install(&mut t, &p, InstallMeta::default()).unwrap().uid,
            "x-2"
        );
    }

    #[test]
    fn empty_archive_is_rejected_without_a_row() {
        let mut t = setup("bethesda");
        let p = src(&t, "docs", &[("readme.txt", "only docs")]);
        assert!(matches!(
            install(&mut t, &p, InstallMeta::default()),
            Err(ModsError::NoInstallableFiles(_))
        ));
        assert!(list_mods(t.store.conn(), "g").unwrap().is_empty());
    }

    #[test]
    fn replace_keeps_position_and_enabled_state() {
        let mut t = setup("generic");
        let a = src(&t, "a", &[("a.txt", "1")]);
        let b = src(&t, "b", &[("b.txt", "1")]);
        let old = install(
            &mut t,
            &a,
            InstallMeta {
                nexus_mod_id: Some(1),
                nexus_file_id: Some(1),
                name: Some("A".into()),
                ..Default::default()
            },
        )
        .unwrap();
        install(&mut t, &b, InstallMeta::default()).unwrap();
        set_enabled(&mut t.store, "g", &[old.mod_id], false).unwrap();
        let newer = install(
            &mut t,
            &a,
            InstallMeta {
                name: Some("A".into()),
                nexus_mod_id: Some(1),
                nexus_file_id: Some(2),
                replaces: Some(old.mod_id),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(newer.replaced.as_deref(), Some("A-1-1"));
        let mods = list_mods(t.store.conn(), "g").unwrap();
        assert_eq!(mods.len(), 2);
        assert_eq!((mods[0].uid.as_str(), mods[0].enabled), ("A-1-2", false));
        assert!(!t.tmp.path().join("data/g/staging/A-1-1").exists());
    }

    #[test]
    fn detects_fomod_installer() {
        let mut t = setup("generic");
        let p = src(
            &t,
            "f",
            &[
                ("Wrapper/fomod/ModuleConfig.xml", "<x/>"),
                ("Wrapper/a.esp", "e"),
            ],
        );
        let meta = InstallMeta {
            fomod_mode: FomodMode::None,
            ..Default::default()
        };
        assert!(install(&mut t, &p, meta).unwrap().fomod_found);
        assert_eq!(find_fomod(&p).unwrap(), p.join("Wrapper"));
    }

    // ── FOMOD ───────────────────────────────────────────────

    use crate::fomod::tests::{tree, Scripted};
    use crate::fomod::{FomodError, GroupPrompt};

    fn fomod_src(t: &T) -> PathBuf {
        let d = t.tmp.path().join("src/fomodmod");
        // A wrapper folder like real archives ("Mod-1.0/fomod/...").
        tree(&d.join("Wrapper"));
        std::fs::rename(d.join("Wrapper/root"), d.join("Mod-1.0")).unwrap();
        std::fs::remove_dir(d.join("Wrapper")).unwrap();
        d
    }

    fn staged(o: &InstallOutcome, t: &T) -> Vec<String> {
        let st = t.tmp.path().join("data/g/staging").join(&o.uid);
        let mut v: Vec<String> = walkdir::WalkDir::new(&st)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .map(|e| {
                e.path()
                    .strip_prefix(&st)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        v.sort();
        v
    }

    fn row(t: &T, id: i64) -> Option<(String, String)> {
        t.store
            .conn()
            .query_row(
                "SELECT choices, mode FROM fomod_choices WHERE mod_id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .unwrap()
    }

    fn install_with(
        t: &mut T,
        p: &Path,
        meta: InstallMeta,
        r: Option<&mut dyn FomodResolver>,
    ) -> Result<InstallOutcome, ModsError> {
        let data = t.tmp.path().join("data");
        let hooks = InstallHooks {
            choose_variant: None,
            fomod: r,
        };
        install_archive(&mut t.store, &t.game, &data, p, &meta, hooks)
    }

    /// Fails the test if consulted.
    struct NoAsk;
    impl FomodResolver for NoAsk {
        fn choose(&mut self, p: &GroupPrompt<'_>) -> Result<Vec<usize>, FomodError> {
            panic!("asked about {}", p.group.name)
        }
    }

    #[test]
    fn fomod_default_install_stages_plan_and_records_vortex_choices() {
        let mut t = setup("generic");
        let p = fomod_src(&t);
        let o = install_with(&mut t, &p, InstallMeta::default(), None).unwrap();
        let run = o.fomod.as_ref().unwrap();
        assert_eq!(run.mode, ResolvedMode::Default);
        assert_eq!(
            staged(&o, &t),
            [
                "core.esp",
                "docs/readme.txt",
                "large.esp",
                "meshes/body.nif",
                "patches/patch_a.esp",
                "plugins/alpha.esp",
                "shared.txt",
                "textures/a.dds",
                "textures/sub/b.dds"
            ]
        );
        let (json, mode) = row(&t, o.mod_id).unwrap();
        assert_eq!(mode, "default");
        assert!(
            json.starts_with(r#"{"type":"fomod","options":[{"name":"Main""#),
            "{json}"
        );
        assert_eq!(
            crate::mods::recorded_fomod_choices(t.store.conn(), o.mod_id).unwrap(),
            Some(run.choices.clone())
        );
    }

    #[test]
    fn fomod_ask_then_reuse_replays_stored_choices_on_reinstall() {
        let mut t = setup("generic");
        let p = fomod_src(&t);
        // Body: Slim; Extras: Zeta; Patch: none; Readme: none.
        let mut r = Scripted {
            answers: vec![vec![0], vec![1], vec![], vec![]],
            ..Default::default()
        };
        let o = install_with(&mut t, &p, InstallMeta::default(), Some(&mut r)).unwrap();
        assert_eq!(o.fomod.as_ref().unwrap().mode, ResolvedMode::Ask);
        let slim = staged(&o, &t);
        assert!(slim.contains(&"zeta.esp".to_string()), "{slim:?}");
        assert_eq!(row(&t, o.mod_id).unwrap().1, "ask");
        let body = t
            .tmp
            .path()
            .join("data/g/staging")
            .join(&o.uid)
            .join("meshes/body.nif");
        assert_eq!(std::fs::read_to_string(&body).unwrap(), "body/slim.nif");

        // Reinstall (legacy meta: fomodMode reuse + recorded choices).
        let choices = crate::mods::recorded_fomod_choices(t.store.conn(), o.mod_id).unwrap();
        let meta = InstallMeta {
            uid: Some(o.uid.clone()),
            replaces: Some(o.mod_id),
            fomod_mode: FomodMode::Reuse,
            fomod_choices: choices,
            ..Default::default()
        };
        let again = install_with(&mut t, &p, meta, Some(&mut NoAsk)).unwrap();
        assert_eq!(again.mod_id, o.mod_id);
        assert_eq!(again.fomod.as_ref().unwrap().mode, ResolvedMode::Choices);
        assert_eq!(staged(&again, &t), slim);
        assert_eq!(row(&t, o.mod_id).unwrap().1, "choices");

        // Reuse without recorded choices asks when interactive.
        let mut r = Scripted {
            answers: vec![vec![1], vec![], vec![], vec![0], vec![], vec![]],
            ..Default::default()
        };
        let meta = InstallMeta {
            uid: Some(o.uid.clone()),
            replaces: Some(o.mod_id),
            fomod_mode: FomodMode::Reuse,
            ..Default::default()
        };
        let asked = install_with(&mut t, &p, meta, Some(&mut r)).unwrap();
        assert_eq!(asked.fomod.as_ref().unwrap().mode, ResolvedMode::Ask);
        assert_eq!(r.seen.len(), 5, "{:?}", r.seen);
    }

    #[test]
    fn fomod_collection_mode_uses_given_choices_and_never_asks() {
        let mut t = setup("generic");
        let p = fomod_src(&t);
        let choices = crate::fomod::FomodChoices::from_json(
            r#"{"type":"fomod","options":[{"name":"Main","groups":[{"name":"Body","choices":[{"name":"Slim","idx":0}]}]}]}"#,
        );
        let meta = InstallMeta {
            fomod_mode: FomodMode::Collection,
            fomod_choices: choices,
            ..Default::default()
        };
        let o = install_with(&mut t, &p, meta, Some(&mut NoAsk)).unwrap();
        assert_eq!(o.fomod.as_ref().unwrap().mode, ResolvedMode::Choices);
        assert!(!staged(&o, &t).contains(&"large.esp".to_string()));
        // No recorded choices: defaults, still without asking.
        let meta = InstallMeta {
            fomod_mode: FomodMode::Collection,
            ..Default::default()
        };
        let o = install_with(&mut t, &p, meta, Some(&mut NoAsk)).unwrap();
        assert_eq!(o.fomod.as_ref().unwrap().mode, ResolvedMode::Default);
    }

    #[test]
    fn fomod_ask_without_resolver_and_default_mode_use_defaults() {
        let mut t = setup("generic");
        let p = fomod_src(&t);
        for mode in [FomodMode::Ask, FomodMode::Auto, FomodMode::Reuse] {
            let meta = InstallMeta {
                fomod_mode: mode,
                ..Default::default()
            };
            let o = install_with(&mut t, &p, meta, None).unwrap();
            assert_eq!(o.fomod.unwrap().mode, ResolvedMode::Default, "{mode:?}");
        }
        let meta = InstallMeta {
            fomod_mode: FomodMode::Default,
            ..Default::default()
        };
        let o = install_with(&mut t, &p, meta, Some(&mut NoAsk)).unwrap();
        assert_eq!(o.fomod.unwrap().mode, ResolvedMode::Default);
    }

    #[test]
    fn fomod_none_mode_skips_installer_and_clears_choices() {
        let mut t = setup("generic");
        let p = fomod_src(&t);
        let o = install_with(&mut t, &p, InstallMeta::default(), None).unwrap();
        assert!(row(&t, o.mod_id).is_some());
        let meta = InstallMeta {
            uid: Some(o.uid.clone()),
            replaces: Some(o.mod_id),
            fomod_mode: FomodMode::None,
            ..Default::default()
        };
        let plain = install_with(&mut t, &p, meta, None).unwrap();
        assert!(plain.fomod_found && plain.fomod.is_none());
        assert!(row(&t, o.mod_id).is_none());
        // Plain layout keeps the archive tree as is (generic profile).
        assert!(staged(&plain, &t).contains(&"Mod-1.0/body/slim.nif".to_string()));
    }

    #[test]
    fn fomod_failure_aborts_install_without_a_row() {
        let mut t = setup("generic");
        let p = src(
            &t,
            "bad",
            &[
                ("fomod/ModuleConfig.xml", "<config><oops></config>"),
                ("a.esp", "a"),
            ],
        );
        // find_fomod needs depth >= 2: wrap it.
        let w = t.tmp.path().join("src/wrapbad");
        std::fs::create_dir_all(&w).unwrap();
        std::fs::rename(&p, w.join("bad")).unwrap();
        assert!(matches!(
            install_with(&mut t, &w, InstallMeta::default(), None),
            Err(ModsError::Fomod(FomodError::Parse(_)))
        ));
        assert!(list_mods(t.store.conn(), "g").unwrap().is_empty());
    }
}
