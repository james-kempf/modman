// SPDX-License-Identifier: GPL-3.0-or-later
//! `mods restage`: rebuilds staged mod trees from their kept archives, so a
//! changed staging layout (UE4SS per-mod folders) applies to mods installed
//! before it.
//!
//! Data-safe by construction:
//! - poisoned backups are checked first: confirmed ones block (a purge
//!   would put mod files back as "originals"), ambiguous ones warn;
//! - each mod is reinstalled through the normal install pipeline into a
//!   scratch staging root, so its live tree and row stay untouched until
//!   the new tree is complete;
//! - files the user edited in staging carry over to the matching path of
//!   the new tree, or are copied to a backup folder when nothing matches;
//! - the old tree is swapped out by rename and only deleted once the new
//!   tree and its `mod_files` rows are in place. Enabled state, priority,
//!   collection membership and FOMOD choices are kept: the row is updated,
//!   never replaced.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::Serialize;

use super::{fail, staging_lock, AppError, Cx, Msg, R};
use crate::deploy::{self, staging_root, PoisonClass};
use crate::fomod::FomodMode;
use crate::games::{self, GameConfig};
use crate::mods::{self, InstallHooks, InstallMeta, ModRecord};
use crate::staging::{staged_files, StagedFile};
use crate::store::{Store, StoreError};

/// What to restage.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestageRequest {
    /// Mods (`mods.id`) to restage. Empty: the affected ones, or every mod
    /// with `all`.
    pub ids: Vec<i64>,
    pub all: bool,
    /// Go ahead although confirmed poisoned backups exist, as long as none
    /// is at a path a selected mod provides.
    pub ignore_poisoned: bool,
}

/// A staged file the user edited, moved into the new tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CarriedFile {
    /// Path in the old tree.
    pub from: String,
    /// Path in the new tree.
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestagedMod {
    pub id: i64,
    pub uid: String,
    pub files: usize,
    pub carried: Vec<CarriedFile>,
    /// Edited files with no place in the new tree (copied to
    /// [`RestageReport::backup_dir`]`/<uid>/`).
    pub backed_up: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestageFailure {
    pub uid: String,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RestageReport {
    pub restaged: Vec<RestagedMod>,
    /// uids of mods whose archive is gone; they were left as they are.
    pub no_archive: Vec<String>,
    /// Mods that could not be restaged; their old tree is intact.
    pub failed: Vec<RestageFailure>,
    /// Holds edited files that did not fit the new layout.
    pub backup_dir: Option<PathBuf>,
    pub poisoned_confirmed: usize,
    pub poisoned_ambiguous: usize,
}

/// Restages the mods `req` selects. Fails with nothing changed when
/// confirmed poisoned backups exist and `req.ignore_poisoned` is unset,
/// or (even with it) when a selected mod provides one of their paths.
pub fn restage(cx: &mut Cx<'_>, game: &GameConfig, req: &RestageRequest) -> R<RestageReport> {
    restage_with(cx, game, req, &|_| Ok(()))
}

/// [`restage`] with a hook that runs once per mod after its new tree is
/// complete and before the swap (tests inject failures there).
fn restage_with(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    req: &RestageRequest,
    before_swap: &dyn Fn(&ModRecord) -> R<()>,
) -> R<RestageReport> {
    let mut report = RestageReport::default();
    let poisoned = deploy::poisoned_backups(cx.store.conn(), game)?;
    let confirmed = poisoned
        .iter()
        .filter(|p| p.class == PoisonClass::Confirmed)
        .count();
    report.poisoned_confirmed = confirmed;
    report.poisoned_ambiguous = poisoned.len() - confirmed;
    if confirmed > 0 && !req.ignore_poisoned {
        fail!(
            "Restage blocked: {confirmed} held backup(s) of {} are mod files modman placed, not game originals, \
             so a purge would put mod files back. Review and quarantine them first: \
             modman deploy clean-backups (then --apply), or pass --ignore-poisoned.",
            game.name
        );
    }
    let selected = select(cx.store.conn(), game, req)?;
    if confirmed > 0 {
        // Restage moves the selected mods' files to new paths, after which
        // no mod provides the old paths and clean-backups can no longer
        // match the backups held there.
        let uids: HashSet<&str> = selected.iter().map(|m| m.uid.as_str()).collect();
        let overlap = poisoned
            .iter()
            .filter(|p| p.class == PoisonClass::Confirmed)
            .filter(|p| p.mods.iter().any(|u| uids.contains(u.as_str())))
            .count();
        if overlap > 0 {
            fail!(
                "Restage refused: {overlap} confirmed poisoned backup(s) of {} are at paths the selected mods \
                 provide now; after restaging, clean-backups could no longer match them. \
                 Run modman deploy clean-backups --apply first (--ignore-poisoned does not cover this).",
                game.name
            );
        }
        cx.ui.warn(&format!(
            "Ignoring {confirmed} confirmed poisoned backup(s) (--ignore-poisoned). Skipping the cleanup \
             can leave backups at old paths that no mod matches any more; run modman deploy clean-backups --apply \
             to quarantine them."
        ));
    }
    if report.poisoned_ambiguous > 0 {
        cx.ui.say(
            Msg::warn(format!(
                "{} held backup(s) may contain mod files instead of game originals. Review them with: ",
                report.poisoned_ambiguous
            ))
            .b("modman deploy clean-backups"),
        );
    }

    if selected.is_empty() {
        cx.ui.info(&format!(
            "Nothing to restage for {}: no mod is staged in an outdated layout (pass --all or mod ids to restage anyway).",
            game.name
        ));
        return Ok(report);
    }
    let data_dir = cx.config()?.data_dir;
    let _lock = staging_lock(cx.paths)?;
    let backup_root = data_dir
        .join(&game.domain)
        .join("restage-backup")
        .join(stamp().to_string());
    for rec in &selected {
        let Some(archive) = rec.archive.clone().filter(|a| a.exists()) else {
            cx.ui.say(
                Msg::warn("No archive kept for ")
                    .b(rec.uid.clone())
                    .p("; left as it is (reinstall it to restage)."),
            );
            report.no_archive.push(rec.uid.clone());
            continue;
        };
        cx.ui.info(&format!("Restaging {}...", rec.uid));
        let backup = backup_root.join(&rec.uid);
        match restage_one(
            cx.store,
            game,
            &data_dir,
            rec,
            &archive,
            &backup,
            before_swap,
        ) {
            Ok(m) => {
                for c in &m.carried {
                    cx.ui.say(
                        Msg::line("  kept your edit: ")
                            .b(c.from.clone())
                            .p(" → ")
                            .b(c.to.clone()),
                    );
                }
                if !m.backed_up.is_empty() {
                    cx.ui.say(Msg::warn(format!(
                        "{} edited file(s) of {} have no place in the new layout; copied to {}",
                        m.backed_up.len(),
                        rec.uid,
                        backup.display()
                    )));
                    for f in &m.backed_up {
                        cx.ui.say(Msg::line("  ").b(f.clone()));
                    }
                    report.backup_dir = Some(backup_root.clone());
                }
                report.restaged.push(m);
            }
            Err(e) => {
                let error = super::chain(&e);
                cx.ui.error(&format!(
                    "Restaging {} failed (its staged files are unchanged): {error}",
                    rec.uid
                ));
                report.failed.push(RestageFailure {
                    uid: rec.uid.clone(),
                    error,
                });
            }
        }
    }
    if !report.restaged.is_empty() {
        cx.ui
            .success(&format!("Restaged {} mod(s).", report.restaged.len()));
        cx.ui
            .say(Msg::info("Apply the new layout to the game folder with: ").b("modman deploy"));
    }
    Ok(report)
}

fn stamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// A UE4SS mod entry point (`main.lua`, `main.dll`), any case. The old
/// layout stripped the mod folder and `Scripts/` too, staging a bare
/// `main.lua`, so the file name alone decides.
fn is_ue4ss_marker(lower: &str) -> bool {
    let name = lower.rsplit('/').next().unwrap_or(lower);
    name == "main.lua" || name == "main.dll"
}

fn select(conn: &Connection, game: &GameConfig, req: &RestageRequest) -> R<Vec<ModRecord>> {
    let all = mods::list_mods(conn, &game.domain)?;
    if !req.ids.is_empty() {
        return req
            .ids
            .iter()
            .map(|id| {
                all.iter().find(|m| m.id == *id).cloned().ok_or_else(|| {
                    AppError::Msg(format!(
                        "No mod with id {id} is installed for {}.",
                        game.name
                    ))
                })
            })
            .collect();
    }
    if req.all {
        return Ok(all);
    }
    // Affected: a UE4SS entry point staged outside the resolved mods folder.
    let Some(dir) = games::ue4ss_mods_dir(game, game.folder.as_deref()) else {
        return Ok(Vec::new());
    };
    let prefix = format!("{}/", dir.to_lowercase());
    let mut stmt = conn.prepare("SELECT path FROM mod_files WHERE mod_id = ?1")?;
    let mut out = Vec::new();
    for m in all {
        let paths = stmt
            .query_map([m.id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if paths.iter().any(|p| {
            let l = p.to_lowercase();
            is_ue4ss_marker(&l) && !l.starts_with(&prefix)
        }) {
            out.push(m);
        }
    }
    Ok(out)
}

/// Scratch folder, removed however the restage ends unless `keep` is set
/// (it then holds the only copy of an old tree a failed rollback left).
struct WorkDir {
    path: PathBuf,
    keep: bool,
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn io_at(path: &Path) -> impl FnOnce(std::io::Error) -> AppError + '_ {
    move |e| AppError::Msg(format!("{}: {e}", path.display()))
}

fn restage_one(
    store: &mut Store,
    game: &GameConfig,
    data_dir: &Path,
    rec: &ModRecord,
    archive: &Path,
    backup: &Path,
    before_swap: &dyn Fn(&ModRecord) -> R<()>,
) -> R<RestagedMod> {
    let old_dir = rec
        .staging_dir
        .clone()
        .unwrap_or_else(|| staging_root(game, data_dir).join(&rec.uid));
    let Some(parent) = old_dir.parent() else {
        fail!("Bad staging folder: {}", old_dir.display());
    };
    // Next to the old tree, so both swaps are renames on one filesystem.
    let mut work = WorkDir {
        path: parent
            .join(".tmp")
            .join(format!("restage-{}-{}", std::process::id(), stamp())),
        keep: false,
    };
    std::fs::create_dir_all(&work.path).map_err(io_at(&work.path))?;

    // The installer writes into a scratch store and staging root; the live
    // row and tree are not touched until the swap.
    let mut scratch_game = game.clone();
    scratch_game.library = Some(work.path.join("lib"));
    let mut scratch = Store::open_in_memory()?;
    games::save_game(scratch.conn(), &scratch_game)?;
    let choices = mods::recorded_fomod_choices(store.conn(), rec.id)?;
    // A FOMOD run always records its choices, so none recorded means no
    // installer ran (none in the archive, or skipped): keep it that way.
    let fomod_mode = if choices.is_some() {
        FomodMode::Reuse
    } else {
        FomodMode::None
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
        variant: rec.variant.clone(),
        replaces: None,
        uid: Some(rec.uid.clone()),
        fomod_mode,
        fomod_choices: choices,
    };
    let out = mods::install_archive(
        &mut scratch,
        &scratch_game,
        &work.path.join("data"),
        archive,
        &meta,
        InstallHooks::default(),
    )?;
    let new_dir = staging_root(&scratch_game, &work.path).join(&out.uid);
    let new_files = staged_files(&new_dir)?;
    let (carried, backed_up) =
        carry_over(store.conn(), rec.id, &old_dir, &new_dir, &new_files, backup)?;
    before_swap(rec)?;

    let aside = work.path.join("old");
    let had_old = std::fs::symlink_metadata(&old_dir).is_ok();
    if had_old {
        std::fs::rename(&old_dir, &aside).map_err(io_at(&old_dir))?;
    }
    let rollback = |work: &mut WorkDir, new_back: bool| {
        let back = !new_back || std::fs::rename(&old_dir, &new_dir).is_ok();
        if had_old && !(back && std::fs::rename(&aside, &old_dir).is_ok()) {
            work.keep = true;
            return format!(
                " The old staged files could not be put back; they are kept in {}.",
                aside.display()
            );
        }
        String::new()
    };
    if let Err(e) = std::fs::rename(&new_dir, &old_dir) {
        let note = rollback(&mut work, false);
        fail!("{}: {e}.{note}", old_dir.display());
    }
    if let Err(e) = write_rows(store, &scratch, rec.id, out.mod_id) {
        let note = rollback(&mut work, true);
        fail!("{}{note}", super::chain(&e));
    }
    mods::mark_changed(store, &game.domain)?;
    Ok(RestagedMod {
        id: rec.id,
        uid: rec.uid.clone(),
        files: out.files,
        carried,
        backed_up,
    })
}

/// Copies files the user edited in `old_dir` (sha differs from the one the
/// archive produced, or not from the archive at all) into `new_dir` at the
/// path with the longest matching tail, or into `backup` when no single
/// path matches.
fn carry_over(
    conn: &Connection,
    mod_id: i64,
    old_dir: &Path,
    new_dir: &Path,
    new_files: &[StagedFile],
    backup: &Path,
) -> R<(Vec<CarriedFile>, Vec<String>)> {
    let mut carried = Vec::new();
    let mut backed_up = Vec::new();
    if !old_dir.is_dir() {
        return Ok((carried, backed_up));
    }
    let recorded: HashMap<String, String> = conn
        .prepare("SELECT path, sha256 FROM mod_files WHERE mod_id = ?1")?
        .query_map([mod_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut used = HashSet::new();
    for f in staged_files(old_dir)? {
        if recorded.get(&f.path) == Some(&f.sha256) {
            continue;
        }
        let src = old_dir.join(&f.path);
        let to = best_match(&f.path, new_files).filter(|t| used.insert(t.to_string()));
        let dst = match to {
            Some(t) => new_dir.join(t),
            None => backup.join(&f.path),
        };
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p).map_err(io_at(p))?;
        }
        std::fs::copy(&src, &dst).map_err(io_at(&src))?;
        match to {
            Some(t) => carried.push(CarriedFile {
                from: f.path,
                to: t.to_string(),
            }),
            None => backed_up.push(f.path),
        }
    }
    Ok((carried, backed_up))
}

/// The one new path sharing the longest tail of path segments with `old`
/// (case-insensitive, at least the file name); `None` when tied or absent.
fn best_match<'a>(old: &str, new_files: &'a [StagedFile]) -> Option<&'a str> {
    let segs = |p: &str| p.split('/').map(str::to_lowercase).collect::<Vec<_>>();
    let o = segs(old);
    let mut best = 0;
    let mut hits: Vec<&str> = Vec::new();
    for n in new_files {
        let k = o
            .iter()
            .rev()
            .zip(segs(&n.path).iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        if k == 0 || k < best {
            continue;
        }
        if k > best {
            best = k;
            hits.clear();
        }
        hits.push(&n.path);
    }
    match hits.as_slice() {
        [one] => Some(one),
        _ => None,
    }
}

/// Points the live row of `id` at the new tree: layout columns and
/// `mod_files` from the scratch install `sid`. Enabled state, priority,
/// collections and FOMOD choices stay as they are.
fn write_rows(store: &mut Store, scratch: &Store, id: i64, sid: i64) -> R<()> {
    let (root, variant, count, size): (String, Option<String>, i64, i64) =
        scratch.conn().query_row(
            "SELECT root, variant, file_count, size FROM mods WHERE id = ?1",
            [sid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
    type FileRow = (String, i64, String, Option<i64>);
    let files: Vec<FileRow> = scratch
        .conn()
        .prepare("SELECT path, size, sha256, mode FROM mod_files WHERE mod_id = ?1")?
        .query_map([sid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let tx = store.conn_mut().transaction().map_err(StoreError::from)?;
    tx.execute(
        "UPDATE mods SET root = ?1, variant = ?2, file_count = ?3, size = ?4 WHERE id = ?5",
        params![root, variant, count, size, id],
    )?;
    tx.execute("DELETE FROM mod_files WHERE mod_id = ?1", [id])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO mod_files (mod_id, path, size, sha256, mode) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for (path, size, sha, mode) in &files {
            stmt.execute(params![id, path, size, sha, mode])?;
        }
    }
    tx.commit().map_err(StoreError::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::*;
    use crate::app::testing::Recorder;
    use crate::deploy::{DeployEnv, DeployOptions, DeployReport, Faults, NeverRunning};
    use crate::fomod::FomodResolver;
    use crate::paths::Paths;

    const UE4SS_MODS: &str = "Pal/Binaries/Win64/ue4ss/Mods";
    const PROBE: &str = "Pal/Binaries/Win64/ue4ss/UE4SS.dll";

    /// Temp HOME, data dir and game folder only.
    struct Fx {
        tmp: tempfile::TempDir,
        paths: Paths,
        store: Store,
        game: GameConfig,
        ui: Recorder,
    }

    fn write(path: &Path, data: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }

    impl Fx {
        fn with(mut game: GameConfig, present: &[&str]) -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let folder = tmp.path().join("game");
            write(&folder.join("game.exe"), "exe");
            for p in present {
                write(&folder.join(p), "probe");
            }
            game.folder = Some(folder);
            let store = Store::open(&tmp.path().join("modman.db")).unwrap();
            games::save_game(store.conn(), &game).unwrap();
            let paths = Paths::for_home(&tmp.path().join("home"));
            Fx {
                tmp,
                paths,
                store,
                game,
                ui: Recorder::default(),
            }
        }

        fn custom() -> Self {
            Self::with(GameConfig::custom("g"), &[])
        }

        fn palworld() -> Self {
            let defs = games::builtin_definitions();
            let def = games::find_definition(&defs, "palworld").unwrap();
            Self::with(GameConfig::from_definition(def), &[PROBE])
        }

        fn game_dir(&self) -> PathBuf {
            self.game.folder.clone().unwrap()
        }

        fn data_dir(&mut self) -> PathBuf {
            Cx::new(&self.paths, &mut self.store, &self.ui)
                .config()
                .unwrap()
                .data_dir
        }

        fn src(&self, name: &str, files: &[(&str, &str)]) -> PathBuf {
            let d = self.tmp.path().join("src").join(name);
            for (p, c) in files {
                write(&d.join(p), c);
            }
            d
        }

        /// Installs `src` as `game` sees it (an older layout when `game`
        /// lacks the UE4SS block).
        fn install_as(
            &mut self,
            game: &GameConfig,
            name: &str,
            src: &Path,
            fomod: Option<&mut dyn FomodResolver>,
        ) -> ModRecord {
            let data = self.data_dir();
            let meta = InstallMeta {
                name: Some(name.into()),
                ..Default::default()
            };
            let hooks = InstallHooks {
                choose_variant: None,
                fomod,
            };
            let o = mods::install_archive(&mut self.store, game, &data, src, &meta, hooks).unwrap();
            self.rec(o.mod_id)
        }

        fn install(&mut self, name: &str, files: &[(&str, &str)]) -> ModRecord {
            let src = self.src(name, files);
            let game = self.game.clone();
            self.install_as(&game, name, &src, None)
        }

        fn rec(&self, id: i64) -> ModRecord {
            mods::get_mod(self.store.conn(), &self.game.domain, id)
                .unwrap()
                .unwrap()
        }

        fn restage_hooked(
            &mut self,
            req: RestageRequest,
            hook: &dyn Fn(&ModRecord) -> R<()>,
        ) -> R<RestageReport> {
            let game = self.game.clone();
            let mut cx = Cx::new(&self.paths, &mut self.store, &self.ui);
            restage_with(&mut cx, &game, &req, hook)
        }

        fn restage(&mut self, req: RestageRequest) -> R<RestageReport> {
            self.restage_hooked(req, &|_| Ok(()))
        }

        fn env(&mut self) -> DeployEnv {
            let data = self.data_dir();
            DeployEnv {
                lock_path: data.join("deploy.lock"),
                backup_dir: data.join("backups"),
                overwrite_dir: data.join(&self.game.domain).join("overwrite"),
                probe: Arc::new(NeverRunning),
                faults: Faults::default(),
            }
        }

        fn deploy(&mut self) -> DeployReport {
            let env = self.env();
            deploy::deploy(&mut self.store, &self.game, &env, &DeployOptions::default()).unwrap()
        }

        /// Staged tree of `id`: relative path -> content.
        fn staged(&self, id: i64) -> BTreeMap<String, String> {
            let dir = self.rec(id).staging_dir.unwrap();
            staged_files(&dir)
                .unwrap()
                .into_iter()
                .map(|f| {
                    let c = std::fs::read_to_string(dir.join(&f.path)).unwrap();
                    (f.path, c)
                })
                .collect()
        }

        fn rows(&self, id: i64) -> Vec<(String, String)> {
            self.store
                .conn()
                .prepare("SELECT path, sha256 FROM mod_files WHERE mod_id = ?1 ORDER BY path")
                .unwrap()
                .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        }

        /// The live Palworld state: the game folder already holds the
        /// mod's bytes at `rel` (in a mod-only folder) before modman's first
        /// deploy, which backs them up as the "original".
        fn poison_confirmed(&mut self, rel: &str, bytes: &str) {
            let dir = rel.rsplit_once('/').map_or("", |(d, _)| d);
            self.game.paks_dir = Some(dir.to_string());
            games::save_game(self.store.conn(), &self.game).unwrap();
            write(&self.game_dir().join(rel), bytes);
            assert_eq!(self.deploy().backed_up, 1, "poisoned fixture");
        }

        fn texts(&self) -> String {
            self.ui.texts().join("\n")
        }
    }

    fn all() -> RestageRequest {
        RestageRequest {
            all: true,
            ..Default::default()
        }
    }

    /// Catches: restaging while a backup holds a mod file (a later purge
    /// would "restore" mod bytes as the game original), and
    /// --ignore-poisoned moving a mod away from the path of its poisoned
    /// backup, which clean-backups could then no longer match.
    #[test]
    fn confirmed_poisoned_backup_blocks_and_changes_nothing() {
        let mut fx = Fx::custom();
        let m = fx.install("m", &[("Data/a.txt", "mod-a")]);
        let n = fx.install("n", &[("Other/b.txt", "mod-b")]);
        fx.poison_confirmed("Data/a.txt", "mod-a");
        write(&m.staging_dir.clone().unwrap().join("Data/a.txt"), "edited");
        let (tree, rows) = (fx.staged(m.id), fx.rows(m.id));
        let err = fx.restage(all()).unwrap_err().to_string();
        assert!(err.contains("modman deploy clean-backups"), "{err}");
        assert!(err.contains("--ignore-poisoned"), "{err}");
        assert_eq!(
            (fx.staged(m.id), fx.rows(m.id)),
            (tree.clone(), rows.clone())
        );
        assert_eq!(fx.rec(m.id).installed_at, m.installed_at);

        let ignore = |ids: Vec<i64>| RestageRequest {
            ignore_poisoned: true,
            ids,
            ..Default::default()
        };
        let err = fx.restage(ignore(vec![m.id])).unwrap_err().to_string();
        assert!(err.contains("clean-backups --apply first"), "{err}");
        assert_eq!(
            (fx.staged(m.id), fx.rows(m.id)),
            (tree.clone(), rows.clone())
        );

        let r = fx.restage(ignore(vec![n.id])).unwrap();
        assert_eq!((r.restaged.len(), r.poisoned_confirmed), (1, 1));
        assert!(fx.texts().contains("old paths"), "{}", fx.texts());
        assert_eq!((fx.staged(m.id), fx.rows(m.id)), (tree, rows));
    }

    /// Catches: ambiguous entries (no journal record) blocking restage, or
    /// passing silently without pointing at clean-backups.
    #[test]
    fn ambiguous_poisoned_backups_warn_but_do_not_block() {
        let mut fx = Fx::custom();
        write(&fx.game_dir().join("Data/same.txt"), "same");
        fx.install("m", &[("Data/same.txt", "same")]);
        fx.deploy();
        let r = fx.restage(all()).unwrap();
        assert_eq!((r.poisoned_confirmed, r.poisoned_ambiguous), (0, 1));
        assert_eq!(r.restaged.len(), 1);
        assert!(fx.texts().contains("modman deploy clean-backups"));
    }

    /// Catches: mods staged before the UE4SS layout keeping the root-level
    /// tree, user edits lost, enabled state or order reset, and the next
    /// deploy leaving the old root-level files in the game folder.
    #[test]
    fn old_root_layout_restages_under_ue4ss_mods_and_deploy_drops_old_files() {
        let mut fx = Fx::palworld();
        let mut old_game = fx.game.clone();
        old_game.definition = None;
        let a_src = fx.src(
            "Mod A",
            &[
                ("ModA/enabled.txt", ""),
                ("ModA/Scripts/main.lua", "a-main"),
                ("ModA/Scripts/config.lua", "a-config"),
            ],
        );
        let b_src = fx.src(
            "Mod B",
            &[
                ("ModB/enabled.txt", ""),
                ("ModB/Scripts/main.lua", "b-main"),
            ],
        );
        let a = fx.install_as(&old_game, "Mod A", &a_src, None);
        let b = fx.install_as(&old_game, "Mod B", &b_src, None);
        mods::set_enabled(&mut fx.store, &fx.game.domain, &[b.id], false).unwrap();
        let old_a = fx.staged(a.id);
        assert!(
            old_a.keys().all(|p| !p.starts_with(UE4SS_MODS)),
            "fixture must start in the old layout: {old_a:?}"
        );
        let old_config = old_a
            .keys()
            .find(|p| p.ends_with("config.lua"))
            .unwrap()
            .clone();
        fx.deploy();
        let old_deployed: Vec<PathBuf> = old_a.keys().map(|p| fx.game_dir().join(p)).collect();
        assert!(old_deployed.iter().all(|p| p.exists()));

        // User edits in staging: one with a place in the new tree, one not.
        let a_dir = a.staging_dir.clone().unwrap();
        write(&a_dir.join(&old_config), "my-config");
        write(&a_dir.join("notes.ini"), "mine");

        let r = fx.restage(RestageRequest::default()).unwrap();
        let uids: Vec<&str> = r.restaged.iter().map(|m| m.uid.as_str()).collect();
        assert_eq!(uids, [a.uid.as_str(), b.uid.as_str()], "affected mods");
        let new_a = fx.staged(a.id);
        let cfg = format!("{UE4SS_MODS}/ModA/Scripts/config.lua");
        assert_eq!(new_a.get(&cfg).map(String::as_str), Some("my-config"));
        assert_eq!(
            new_a
                .get(&format!("{UE4SS_MODS}/ModA/Scripts/main.lua"))
                .map(String::as_str),
            Some("a-main")
        );
        assert!(new_a.contains_key(&format!("{UE4SS_MODS}/ModA/enabled.txt")));
        assert!(new_a.keys().all(|p| p.starts_with(UE4SS_MODS)), "{new_a:?}");
        assert_eq!(
            r.restaged[0].carried,
            [CarriedFile {
                from: old_config.clone(),
                to: cfg.clone()
            }]
        );
        assert_eq!(r.restaged[0].backed_up, ["notes.ini"]);
        let backup = r.backup_dir.clone().unwrap().join(&a.uid).join("notes.ini");
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "mine");
        // Rows follow the new tree; state and order are kept.
        let rows = fx.rows(a.id);
        assert!(
            rows.iter().all(|(p, _)| p.starts_with(UE4SS_MODS)),
            "{rows:?}"
        );
        let (a2, b2) = (fx.rec(a.id), fx.rec(b.id));
        assert_eq!((a2.enabled, a2.priority), (true, a.priority));
        assert_eq!((b2.enabled, b2.priority), (false, b.priority));
        assert_eq!(a2.staging_dir, a.staging_dir);

        fx.deploy();
        for p in &old_deployed {
            assert!(!p.exists(), "old root-level file left: {}", p.display());
        }
        let deployed = fx.game_dir().join(&cfg);
        assert_eq!(std::fs::read_to_string(deployed).unwrap(), "my-config");
        // Nothing left in the old layout.
        let again = fx.restage(RestageRequest::default()).unwrap();
        assert!(again.restaged.is_empty());
    }

    /// Catches: a failure after the new tree is built leaving a half-swapped
    /// or deleted staging folder, or rows pointing at missing files.
    #[test]
    fn failure_before_swap_keeps_old_tree_and_rows() {
        let mut fx = Fx::custom();
        let m = fx.install("m", &[("Data/a.txt", "a"), ("Data/b.txt", "b")]);
        let dir = m.staging_dir.clone().unwrap();
        write(&dir.join("Data/a.txt"), "edited");
        let (tree, rows) = (fx.staged(m.id), fx.rows(m.id));
        let r = fx
            .restage_hooked(all(), &|_| Err(AppError::Msg("injected".into())))
            .unwrap();
        assert!(r.restaged.is_empty());
        assert_eq!(r.failed.len(), 1);
        assert!(r.failed[0].error.contains("injected"));
        assert_eq!((fx.staged(m.id), fx.rows(m.id)), (tree, rows));
        let tmp = dir.parent().unwrap().join(".tmp");
        assert_eq!(std::fs::read_dir(tmp).unwrap().count(), 0, "scratch left");
    }

    /// Catches: a mod whose archive is gone failing the run (or being
    /// emptied) instead of being reported and left as it is.
    #[test]
    fn missing_archive_is_reported_not_failed() {
        let mut fx = Fx::custom();
        let gone = fx.install("gone", &[("Data/g.txt", "g")]);
        let kept = fx.install("kept", &[("Data/k.txt", "k")]);
        std::fs::remove_dir_all(gone.archive.clone().unwrap()).unwrap();
        let tree = fx.staged(gone.id);
        let r = fx.restage(all()).unwrap();
        assert_eq!(r.no_archive, std::slice::from_ref(&gone.uid));
        assert!(r.failed.is_empty());
        assert_eq!(r.restaged.len(), 1);
        assert_eq!(r.restaged[0].uid, kept.uid);
        assert_eq!(fx.staged(gone.id), tree);
    }

    /// Catches: restage re-running a FOMOD installer with defaults instead
    /// of the recorded (non-default) choices.
    #[test]
    fn fomod_choices_are_replayed() {
        use crate::fomod::tests::{tree, Scripted};
        let mut fx = Fx::custom();
        let src = fx.tmp.path().join("src/fomodmod");
        tree(&src.join("Wrapper"));
        std::fs::rename(src.join("Wrapper/root"), src.join("Mod-1.0")).unwrap();
        std::fs::remove_dir(src.join("Wrapper")).unwrap();
        // Body: Slim; Extras: Zeta; Patch: none; Readme: none.
        let mut r = Scripted {
            answers: vec![vec![0], vec![1], vec![], vec![]],
            ..Default::default()
        };
        let game = fx.game.clone();
        let m = fx.install_as(&game, "F", &src, Some(&mut r));
        let before = fx.staged(m.id);
        assert_eq!(before["meshes/body.nif"], "body/slim.nif");
        let choices = mods::fomod_choices(fx.store.conn(), m.id).unwrap();
        let rep = fx.restage(all()).unwrap();
        assert_eq!(rep.restaged.len(), 1);
        assert_eq!(fx.staged(m.id), before);
        assert_eq!(mods::fomod_choices(fx.store.conn(), m.id).unwrap(), choices);
    }

    #[test]
    fn nothing_affected_without_ue4ss_or_after_restage() {
        let mut fx = Fx::custom();
        fx.install("lua", &[("Mod/Scripts/main.lua", "x")]);
        let r = fx.restage(RestageRequest::default()).unwrap();
        assert!(r.restaged.is_empty() && r.failed.is_empty());
        assert!(fx.texts().contains("Nothing to restage"));
        let err = fx
            .restage(RestageRequest {
                ids: vec![999],
                ..Default::default()
            })
            .unwrap_err();
        assert!(err.to_string().contains("999"));
    }

    /// Catches: an edit carried to the wrong file when several new paths
    /// match equally well.
    #[test]
    fn ambiguous_match_goes_to_backup() {
        let f = |p: &str| StagedFile {
            path: p.into(),
            size: 0,
            sha256: String::new(),
            mode: 0,
        };
        let new = [
            f("X/A/config.lua"),
            f("X/B/config.lua"),
            f("X/A/Scripts/main.lua"),
        ];
        assert_eq!(best_match("config.lua", &new), None);
        assert_eq!(best_match("A/config.lua", &new), Some("X/A/config.lua"));
        assert_eq!(
            best_match("scripts/MAIN.lua", &new),
            Some("X/A/Scripts/main.lua")
        );
        assert_eq!(best_match("other.txt", &new), None);
    }
}
