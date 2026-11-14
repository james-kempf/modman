// SPDX-License-Identifier: GPL-3.0-or-later
//! Undo of an uninstall: a session store of [`RestoreSnapshot`]s keyed by
//! opaque random tokens, filled by [`super::mods::uninstall`] when the
//! caller opts in (the GUI does, the CLI never), and [`restore`], which
//! reinstalls each mod from its archive with the same uid and writes the
//! snapshot back. Snapshots never leave the backend; front ends only see
//! tokens.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;

use super::deploy::after_change;
use super::install::{install_archive, AfterChangeView, Installed};
use super::{staging_lock, ConfirmAnswer, ConfirmSpec, Cx, Msg, Ui, R};
use crate::deploy::staging_root;
use crate::fomod::{FomodChoices, FomodMode, FomodResolver};
use crate::games::GameConfig;
use crate::mods::restore::{self as snap, RestoreSnapshot};
use crate::mods::{self, InstallMeta, ModRecord};
use crate::nexus::FileInfo;

/// Why an uninstalled mod cannot be restored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreReason {
    /// Installed from a folder (or with no archive): nothing to reinstall from.
    FolderInstall,
    /// The archive was deleted with the mod (or was already gone).
    ArchiveDeleted,
    /// The archive is outside the game's downloads folder.
    OutsideDownloads,
}

/// One uninstalled mod in [`super::mods::UninstallResult::restore`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreEntry {
    /// Opaque; pass it to [`restore`].
    pub token: String,
    pub uid: String,
    pub name: String,
    /// `false` whenever `reason` is set.
    pub restorable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<RestoreReason>,
}

/// In-memory snapshots per game, for the life of the process.
#[derive(Debug, Default)]
pub struct RestoreStore {
    games: Mutex<HashMap<String, HashMap<String, RestoreSnapshot>>>,
}

impl RestoreStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, HashMap<String, RestoreSnapshot>>> {
        // A panic while holding the lock leaves plain data; keep using it.
        self.games.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn put(&self, game: &str, token: String, s: RestoreSnapshot) {
        self.lock()
            .entry(game.to_string())
            .or_default()
            .insert(token, s);
    }

    fn get(&self, game: &str, token: &str) -> Option<RestoreSnapshot> {
        self.lock().get(game)?.get(token).cloned()
    }

    fn consume(&self, game: &str, token: &str) {
        if let Some(m) = self.lock().get_mut(game) {
            m.remove(token);
        }
    }

    #[cfg(test)]
    pub(crate) fn with_snapshot(
        &self,
        game: &str,
        token: &str,
        f: impl FnOnce(&mut RestoreSnapshot),
    ) {
        if let Some(s) = self.lock().get_mut(game).and_then(|m| m.get_mut(token)) {
            f(s);
        }
    }
}

/// 128 random bits as hex: no path, uid or name in it.
fn new_token() -> R<String> {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Snapshots of `targets`, taken before their rows are deleted.
pub(crate) fn capture(cx: &Cx<'_>, targets: &[&ModRecord]) -> R<Vec<RestoreSnapshot>> {
    targets
        .iter()
        .map(|m| Ok(snap::capture(cx.store.conn(), m)?))
        .collect()
}

/// Why a captured mod cannot be restored after the uninstall deleted
/// `deleted` archives; `None` = restorable.
fn reason(
    m: &ModRecord,
    deleted: &[std::path::PathBuf],
    downloads: &Path,
) -> Option<RestoreReason> {
    let Some(a) = m.archive.as_ref().filter(|a| !a.is_dir()) else {
        return Some(RestoreReason::FolderInstall);
    };
    if deleted.contains(a) || !a.is_file() {
        return Some(RestoreReason::ArchiveDeleted);
    }
    if !a.starts_with(downloads) {
        return Some(RestoreReason::OutsideDownloads);
    }
    None
}

/// Stores the restorable snapshots and lists every uninstalled mod.
pub(crate) fn keep(
    store: &RestoreStore,
    game: &str,
    snaps: Vec<RestoreSnapshot>,
    deleted: &[std::path::PathBuf],
    downloads: &Path,
) -> R<Vec<RestoreEntry>> {
    let mut out = Vec::new();
    for s in snaps {
        let token = new_token()?;
        let reason = reason(&s.record, deleted, downloads);
        out.push(RestoreEntry {
            token: token.clone(),
            uid: s.record.uid.clone(),
            name: s.record.name.clone(),
            restorable: reason.is_none(),
            reason,
        });
        if reason.is_none() {
            store.put(game, token, s);
        }
    }
    Ok(out)
}

/// Kind of a failed restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreErrorKind {
    /// The archive is gone.
    ArchiveMissing,
    /// The uid, name or install folder is taken by another mod.
    Conflict,
    /// Unknown, already used or expired token.
    Expired,
    /// The reinstall or the write-back failed; nothing was kept.
    InstallFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreError {
    pub kind: RestoreErrorKind,
    pub message: String,
}

/// Result of one token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoredMod {
    pub token: String,
    /// `None` for an expired token.
    pub uid: Option<String>,
    pub name: Option<String>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RestoreError>,
    pub warnings: Vec<String>,
}

/// Result of [`restore`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct RestoreResult {
    /// Expired tokens first, then the rest lowest position first.
    pub results: Vec<RestoredMod>,
    pub restored: usize,
    pub after: AfterChangeView,
}

/// Answers every question with "no" / cancel and remembers it was asked,
/// so a restore never proceeds on a default (never [`super::Headless`] or
/// `default_yes`). Messages go to the wrapped front end.
pub(crate) struct RejectingUi<'a> {
    inner: &'a dyn Ui,
    asked: RefCell<Vec<&'static str>>,
}

impl<'a> RejectingUi<'a> {
    pub(crate) fn new(inner: &'a dyn Ui) -> Self {
        Self {
            inner,
            asked: RefCell::default(),
        }
    }

    fn note(&self, q: &'static str) {
        self.asked.borrow_mut().push(q);
    }

    /// The questions asked so far; empty when none.
    pub(crate) fn asked(&self) -> Vec<&'static str> {
        self.asked.borrow().clone()
    }
}

impl Ui for RejectingUi<'_> {
    fn say(&self, msg: Msg) {
        self.inner.say(msg);
    }
    fn confirm(&self, _q: &str, _default_yes: bool) -> bool {
        self.note("confirm");
        false
    }
    fn confirm_with(&self, _spec: &ConfirmSpec) -> ConfirmAnswer {
        self.note("confirm");
        ConfirmAnswer::default()
    }
    fn ask(&self, _q: &str, _default: &str) -> Option<String> {
        self.note("ask");
        None
    }
    fn choose_file(&self, _files: &[&FileInfo], default: usize) -> usize {
        self.note("choose_file");
        default
    }
    fn choose_optional(
        &self,
        _m: &crate::collections::Manifest,
        _optional: &[usize],
    ) -> Option<Vec<usize>> {
        self.note("choose_optional");
        None
    }
    fn manual_download(&self, _m: &crate::collections::ManifestMod) -> Option<std::path::PathBuf> {
        self.note("manual_download");
        None
    }
    /// Routes a variant pick here, where it is rejected. The FOMOD
    /// resolver stays off: a restore replays recorded choices or skips it.
    fn install_prompts(&self, _ask_explicit: bool) -> super::InstallPrompts {
        super::InstallPrompts {
            variant: true,
            fomod: false,
        }
    }
    fn choose_variant(&self, _choices: &[String]) -> Option<usize> {
        self.note("choose_variant");
        None
    }
    fn fomod_resolver(&self, _mod_name: &str) -> Option<Box<dyn FomodResolver + '_>> {
        self.note("fomod");
        None
    }
}

fn failed(
    token: &str,
    s: Option<&RestoreSnapshot>,
    kind: RestoreErrorKind,
    message: String,
) -> RestoredMod {
    RestoredMod {
        token: token.to_string(),
        uid: s.map(|s| s.record.uid.clone()),
        name: s.map(|s| s.record.name.clone()),
        ok: false,
        error: Some(RestoreError { kind, message }),
        warnings: Vec::new(),
    }
}

/// Puts the mods of `tokens` back (lowest snapshot position first), each on
/// its own: a failed mod leaves nothing behind and the rest go on. A token
/// is used up when its mod is back.
pub fn restore(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    store: &RestoreStore,
    tokens: &[String],
) -> R<RestoreResult> {
    let mut out = RestoreResult::default();
    let mut todo = Vec::new();
    for t in tokens {
        match store.get(&game.id, t) {
            Some(s) if !todo.iter().any(|(x, _): &(String, _)| x == t) => todo.push((t.clone(), s)),
            Some(_) => {}
            None => out.results.push(failed(
                t,
                None,
                RestoreErrorKind::Expired,
                "This undo is no longer available.".into(),
            )),
        }
    }
    todo.sort_by_key(|(_, s)| s.record.position);
    let data_dir = cx.config()?.data_dir;
    let root = staging_root(game, &data_dir);
    let lock = staging_lock(cx.paths)?;
    for (token, s) in todo {
        let r = restore_one(cx, game, &data_dir, &root, &token, &s);
        if r.ok {
            store.consume(&game.id, &token);
            out.restored += 1;
            cx.ui.success(&format!("Restored {}", s.record.name));
        } else if let Some(e) = &r.error {
            cx.ui.error(&format!(
                "Could not restore {}: {}",
                s.record.name, e.message
            ));
        }
        for w in &r.warnings {
            cx.ui.warn(w);
        }
        out.results.push(r);
    }
    drop(lock);
    if out.restored > 0 {
        out.after = after_change(cx, game)?.into();
    }
    Ok(out)
}

fn restore_one(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    data_dir: &Path,
    root: &Path,
    token: &str,
    s: &RestoreSnapshot,
) -> RestoredMod {
    use RestoreErrorKind as K;
    let fail = |kind, msg: String| failed(token, Some(s), kind, msg);
    let m = &s.record;
    let Some(archive) = m.archive.as_ref().filter(|a| a.is_file()) else {
        return fail(K::ArchiveMissing, "its archive is gone".into());
    };
    let dest = snap::dest_for(root, &m.uid);
    match snap::conflict(cx.store.conn(), &game.id, s, &dest) {
        Ok(Some(why)) => return fail(K::Conflict, why),
        Ok(None) => {}
        Err(e) => return fail(K::InstallFailed, e.to_string()),
    }
    let choices = s.fomod_choices().and_then(|j| FomodChoices::from_json(&j));
    let meta = InstallMeta {
        name: Some(m.name.clone()),
        version: m.version.clone(),
        nexus_mod_id: m.nexus_mod_id,
        nexus_file_id: m.nexus_file_id,
        md5: m.md5.clone(),
        source: Some(m.source.clone()),
        author: m.author.clone(),
        category: m.category.clone(),
        update_policy: m.update_policy.clone(),
        variant: m.variant.clone(),
        replaces: None,
        uid: Some(m.uid.clone()),
        // No recorded choices: the FOMOD installer did not run last time.
        fomod_mode: if choices.is_some() {
            FomodMode::Collection
        } else {
            FomodMode::None
        },
        fomod_choices: choices,
    };
    let rej = RejectingUi::new(cx.ui);
    let installed = {
        let mut inner = Cx::new(cx.paths, &mut *cx.store, &rej);
        inner.game = cx.game;
        install_archive(&mut inner, game, data_dir, archive, &meta, false, None)
    };
    let o = match installed {
        Ok(Installed::Mod(o)) => o,
        Ok(Installed::Loader(_)) => {
            return fail(K::InstallFailed, "the archive is a mod loader".into())
        }
        Err(e) => {
            // The install moves the files in before its row commits; the
            // conflict check proved `dest` was free, so anything there now
            // is this attempt's leftover.
            let _ = std::fs::remove_dir_all(&dest);
            return fail(K::InstallFailed, super::chain(&e));
        }
    };
    let asked = rej.asked();
    let written = if asked.is_empty() {
        snap::apply(cx.store, &game.id, o.mod_id, s).map_err(|e| (K::InstallFailed, e.to_string()))
    } else {
        // A confirm guards an overwrite; any other question means the
        // reinstall would not be the same mod.
        let kind = if asked.contains(&"confirm") {
            K::Conflict
        } else {
            K::InstallFailed
        };
        Err((
            kind,
            format!("the reinstall asked a question ({})", asked.join(", ")),
        ))
    };
    match written {
        Ok(warnings) => RestoredMod {
            token: token.to_string(),
            uid: Some(m.uid.clone()),
            name: Some(m.name.clone()),
            ok: true,
            error: None,
            warnings,
        },
        Err((kind, msg)) => {
            let msg = match mods::restore::rollback(cx.store, &game.id, o.mod_id) {
                Ok(()) => msg,
                Err(e) => format!("{msg}; cleanup failed: {e}"),
            };
            fail(kind, msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::io::Write;
    use std::path::PathBuf;

    use rusqlite::params;

    use crate::app::mods::{uninstall, UninstallResult};
    use crate::app::{ConfirmAnswer, ConfirmSpec, Headless};
    use crate::fomod::tests::Scripted;
    use crate::mods::restore::RowValues;
    use crate::mods::{InstallHooks, InstallMeta};
    use crate::paths::Paths;
    use crate::store::Store;

    struct T {
        tmp: tempfile::TempDir,
        paths: Paths,
        store: Store,
        game: GameConfig,
        data: PathBuf,
        dl: PathBuf,
        sessions: RestoreStore,
    }

    fn setup() -> T {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::for_home(&tmp.path().join("home"));
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        let store = Store::open_in_memory().unwrap();
        let mut game = GameConfig::custom("g");
        game.profile = "generic".into();
        let folder = tmp.path().join("game");
        std::fs::create_dir_all(&folder).unwrap();
        game.folder = Some(folder);
        crate::games::save_game(store.conn(), &game).unwrap();
        let game = crate::games::get_game(store.conn(), "g").unwrap().unwrap();
        let data = crate::config::Config::load(&store, &paths)
            .unwrap()
            .data_dir;
        let dl = mods::downloads_dir(&data, "g");
        std::fs::create_dir_all(&dl).unwrap();
        T {
            tmp,
            paths,
            store,
            game,
            data,
            dl,
            sessions: RestoreStore::default(),
        }
    }

    /// Zips every file under `dir` into `out`.
    fn zip_tree(dir: &Path, out: &Path) {
        let mut w = zip::ZipWriter::new(std::fs::File::create(out).unwrap());
        for e in walkdir::WalkDir::new(dir).sort_by_file_name() {
            let e = e.unwrap();
            if e.file_type().is_file() {
                let rel = e.path().strip_prefix(dir).unwrap().to_string_lossy();
                w.start_file(rel, zip::write::SimpleFileOptions::default())
                    .unwrap();
                w.write_all(&std::fs::read(e.path()).unwrap()).unwrap();
            }
        }
        w.finish().unwrap();
    }

    /// A zip in downloads (or `at`) holding `files`.
    fn archive(t: &T, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let src = t.tmp.path().join("src").join(name);
        for (p, c) in files {
            let f = src.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, c).unwrap();
        }
        let out = t.dl.join(format!("{name}.zip"));
        zip_tree(&src, &out);
        out
    }

    fn install(t: &mut T, arch: &Path, name: &str, r: Option<&mut dyn FomodResolver>) -> i64 {
        let meta = InstallMeta {
            name: Some(name.into()),
            ..Default::default()
        };
        let hooks = InstallHooks {
            fomod: r,
            ..Default::default()
        };
        mods::install_archive(&mut t.store, &t.game, &t.data, arch, &meta, hooks)
            .unwrap()
            .mod_id
    }

    fn simple(t: &mut T, name: &str) -> i64 {
        let a = archive(t, name, &[(&format!("{name}.txt"), name)]);
        install(t, &a, name, None)
    }

    fn uninstall_gui(t: &mut T, refs: &[&str], ui: &dyn Ui) -> UninstallResult {
        let refs: Vec<String> = refs.iter().map(|s| s.to_string()).collect();
        let mut cx = Cx::new(&t.paths, &mut t.store, ui);
        uninstall(&mut cx, &t.game, &refs, false, Some(&t.sessions)).unwrap()
    }

    fn restore_all(t: &mut T, tokens: &[String], ui: &dyn Ui) -> RestoreResult {
        let mut cx = Cx::new(&t.paths, &mut t.store, ui);
        restore(&mut cx, &t.game, &t.sessions, tokens).unwrap()
    }

    fn tokens(r: &UninstallResult) -> Vec<String> {
        r.restore.iter().map(|e| e.token.clone()).collect()
    }

    fn uids(t: &T) -> Vec<String> {
        mods::list_mods(t.store.conn(), "g")
            .unwrap()
            .into_iter()
            .map(|m| m.uid)
            .collect()
    }

    /// The mods row, each child table's rows and the position.
    type Stored = (RowValues, Vec<(&'static str, Vec<RowValues>)>, usize);

    /// Everything stored for `uid` with row ids removed: the mods row and
    /// each child table's rows.
    fn stored(t: &T, uid: &str) -> Stored {
        let m = mods::list_mods(t.store.conn(), "g")
            .unwrap()
            .into_iter()
            .find(|m| m.uid == uid)
            .unwrap();
        let s = snap::capture(t.store.conn(), &m).unwrap();
        let strip = |r: &RowValues, c: &str| -> RowValues {
            r.iter().filter(|(n, _)| n != c).cloned().collect()
        };
        let children = s
            .children
            .iter()
            .map(|(tb, rows)| (*tb, rows.iter().map(|r| strip(r, "mod_id")).collect()))
            .collect();
        (strip(&s.row, "id"), children, m.position)
    }

    fn deployed(t: &T) -> Vec<(String, Option<String>, Option<String>)> {
        let mut st = t
            .store
            .conn()
            .prepare(
                "SELECT d.rel_path, d.sha256, m.uid FROM deployments d
                 LEFT JOIN mods m ON m.id = d.mod_id ORDER BY d.rel_path",
            )
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn deploy(t: &mut T) {
        let mut cx = Cx::new(&t.paths, &mut t.store, &Headless);
        crate::app::deploy::run_deploy(&mut cx, &t.game, None, false).unwrap();
    }

    fn kind(r: &RestoredMod) -> Option<RestoreErrorKind> {
        r.error.as_ref().map(|e| e.kind)
    }

    // Fails when a restore loses anything the uninstall deleted: a column
    // of the mods row, the FOMOD choices, the collection membership, the
    // uid, enabled state or position, or the deployed files.
    #[test]
    fn round_trip_restores_store_contents_and_deployed_files() {
        let mut t = setup();
        simple(&mut t, "Alpha");
        let src = t.tmp.path().join("src/fomodmod");
        crate::fomod::tests::tree(&src);
        let fa = t.dl.join("Fomod Mod.zip");
        zip_tree(&src.join("root"), &fa);
        // Non-default picks, so a replay with defaults would differ.
        let mut r = Scripted {
            answers: vec![vec![0], vec![1], vec![], vec![]],
            ..Default::default()
        };
        let id = install(&mut t, &fa, "Fomod Mod", Some(&mut r));
        simple(&mut t, "Gamma");
        simple(&mut t, "Delta");
        t.store
            .conn()
            .execute_batch(&format!(
                "INSERT INTO collections (id, game, slug, revision, installed_at)
                     VALUES (9, 'g', 'col', 1, 0);
                 INSERT INTO collection_mods VALUES (9, {id}, 4);
                 UPDATE mods SET enabled = 1, author = 'Someone', category = 'UI',
                     nexus_mod_id = 77, update_policy = 'exact', installed_at = 1234
                     WHERE id = {id};"
            ))
            .unwrap();
        let uid = mods::get_mod(t.store.conn(), "g", id).unwrap().unwrap().uid;
        deploy(&mut t);
        let before = stored(&t, &uid);
        assert_eq!(before.2, 2, "non-edge position");
        assert!(
            before.1.iter().all(|(_, rows)| !rows.is_empty()),
            "every child table has rows"
        );
        let files = deployed(&t);

        let u = uninstall_gui(&mut t, &[&uid], &Headless);
        assert_eq!(u.restore.len(), 1);
        assert!(u.restore[0].restorable && u.restore[0].reason.is_none());
        assert!(!uids(&t).contains(&uid));

        let r = restore_all(&mut t, &tokens(&u), &Headless);
        assert_eq!(r.restored, 1, "{:?}", r.results);
        assert!(r.results[0].ok && r.results[0].warnings.is_empty());
        assert_eq!(stored(&t, &uid), before);
        deploy(&mut t);
        assert_eq!(deployed(&t), files);
        // The token is used up.
        let again = restore_all(&mut t, &tokens(&u), &Headless);
        assert_eq!(kind(&again.results[0]), Some(RestoreErrorKind::Expired));
    }

    // Fails when bulk restore applies tokens in request order instead of
    // lowest position first (later mods land at shifted positions).
    #[test]
    fn bulk_restore_puts_mods_back_at_their_positions() {
        let mut t = setup();
        for n in ["A", "B", "C", "D", "E", "F", "G"] {
            simple(&mut t, n);
        }
        t.store
            .conn()
            .execute("UPDATE mods SET enabled = 0 WHERE uid = 'D'", [])
            .unwrap();
        let before = uids(&t);
        let u = uninstall_gui(&mut t, &["F", "B", "D"], &Headless);
        assert_eq!(uids(&t), ["A", "C", "E", "G"]);
        // Reversed request order on purpose.
        let mut tk = tokens(&u);
        tk.reverse();
        let r = restore_all(&mut t, &tk, &Headless);
        assert_eq!(r.restored, 3, "{:?}", r.results);
        assert_eq!(uids(&t), before);
        let names: Vec<_> = r.results.iter().map(|x| x.uid.clone().unwrap()).collect();
        assert_eq!(names, ["B", "D", "F"]);
        let d = mods::list_mods(t.store.conn(), "g").unwrap();
        assert!(!d[3].enabled && d.iter().filter(|m| !m.enabled).count() == 1);
        let prios: Vec<i64> = d.iter().map(|m| m.priority).collect();
        assert_eq!(prios, [0, 1, 2, 3, 4, 5, 6]);
    }

    // Fails when a stale position is used as is (out of range) instead of
    // clamped to 1..=len+1.
    #[test]
    fn restore_clamps_the_position() {
        let mut t = setup();
        for n in ["A", "B", "C", "D"] {
            simple(&mut t, n);
        }
        let u = uninstall_gui(&mut t, &["D"], &Headless);
        uninstall_gui(&mut t, &["B", "C"], &Headless);
        let r = restore_all(&mut t, &tokens(&u), &Headless);
        assert!(r.results[0].ok, "{:?}", r.results);
        assert_eq!(uids(&t), ["A", "D"]);
        let d = mods::list_mods(t.store.conn(), "g").unwrap();
        assert_eq!(d[1].priority, 1);
    }

    fn snapshot_of_other(t: &T, uid: &str) -> (Stored, Vec<String>) {
        let st = stored(t, uid);
        let m = mods::list_mods(t.store.conn(), "g")
            .unwrap()
            .into_iter()
            .find(|m| m.uid == uid)
            .unwrap();
        let dir = m.staging_dir.unwrap();
        let mut files: Vec<String> = walkdir::WalkDir::new(&dir)
            .into_iter()
            .flatten()
            .map(|e| e.path().display().to_string())
            .collect();
        files.sort();
        (st, files)
    }

    // Fails when a restore overwrites or moves a newer mod that took the
    // same name, uid or install folder, or reports it as anything but a
    // conflict.
    #[test]
    fn restore_never_touches_a_newer_mod_and_reports_conflict() {
        let mut t = setup();
        simple(&mut t, "Alpha");
        simple(&mut t, "Beta");
        let u = uninstall_gui(&mut t, &["Alpha"], &Headless);
        let tk = tokens(&u);

        // Same name, other uid.
        let a = archive(&t, "Other", &[("o.txt", "other")]);
        let meta = InstallMeta {
            name: Some("ALPHA".into()),
            uid: Some("alpha-new".into()),
            ..Default::default()
        };
        mods::install_archive(
            &mut t.store,
            &t.game,
            &t.data,
            &a,
            &meta,
            InstallHooks::default(),
        )
        .unwrap();
        let other = snapshot_of_other(&t, "alpha-new");
        let r = restore_all(&mut t, &tk, &Headless);
        assert_eq!(kind(&r.results[0]), Some(RestoreErrorKind::Conflict));
        assert_eq!(snapshot_of_other(&t, "alpha-new"), other);
        assert_eq!(uids(&t), ["Beta", "alpha-new"]);

        // Same uid, other name.
        t.store
            .conn()
            .execute(
                "UPDATE mods SET name = 'Renamed', uid = 'Alpha' WHERE uid = 'alpha-new'",
                [],
            )
            .unwrap();
        let r = restore_all(&mut t, &tk, &Headless);
        assert_eq!(kind(&r.results[0]), Some(RestoreErrorKind::Conflict));
        assert!(r.results[0]
            .error
            .as_ref()
            .unwrap()
            .message
            .contains("Alpha"));

        // Same install folder, other uid and name.
        let old_dir = t.data.join("g/staging/Alpha");
        t.store
            .conn()
            .execute(
                "UPDATE mods SET uid = 'x', name = 'X', staging_dir = ?1 WHERE uid = 'Alpha'",
                params![old_dir.to_string_lossy()],
            )
            .unwrap();
        let r = restore_all(&mut t, &tk, &Headless);
        assert_eq!(kind(&r.results[0]), Some(RestoreErrorKind::Conflict));

        // An unowned folder where it would stage.
        t.store
            .conn()
            .execute("DELETE FROM mods WHERE uid = 'x'", [])
            .unwrap();
        std::fs::create_dir_all(old_dir.join("keep")).unwrap();
        let r = restore_all(&mut t, &tk, &Headless);
        assert_eq!(kind(&r.results[0]), Some(RestoreErrorKind::Conflict));
        assert!(old_dir.join("keep").is_dir());
        assert_eq!(uids(&t), ["Beta"]);
    }

    /// Says yes to everything and records each question: a restore that
    /// fell back to the front end's (or a default) answer would proceed.
    #[derive(Default)]
    struct YesMan {
        asked: Cell<usize>,
    }

    impl Ui for YesMan {
        fn say(&self, _msg: Msg) {}
        fn confirm(&self, _q: &str, _d: bool) -> bool {
            self.asked.set(self.asked.get() + 1);
            true
        }
        fn confirm_with(&self, _s: &ConfirmSpec) -> ConfirmAnswer {
            self.asked.set(self.asked.get() + 1);
            ConfirmAnswer {
                yes: true,
                option_checked: false,
            }
        }
        fn install_prompts(&self, _e: bool) -> crate::app::InstallPrompts {
            crate::app::InstallPrompts {
                variant: true,
                fomod: true,
            }
        }
        fn choose_variant(&self, c: &[String]) -> Option<usize> {
            self.asked.set(self.asked.get() + 1);
            Some(c.len() - 1)
        }
    }

    // Fails when a restore that needs an answer takes one from the real
    // front end or a default (Headless/default_yes) instead of failing.
    #[test]
    fn restore_rejects_every_question_and_fails_typed() {
        let mut t = setup();
        // Variant folders are detected for this layout profile.
        t.game.profile = "bethesda".into();
        let a = archive(
            &t,
            "Variants",
            &[
                ("Mod/1k/textures/a.dds", "1k"),
                ("Mod/2k/textures/a.dds", "2k"),
            ],
        );
        let meta = InstallMeta {
            name: Some("Variants".into()),
            variant: Some("2k".into()),
            ..Default::default()
        };
        mods::install_archive(
            &mut t.store,
            &t.game,
            &t.data,
            &a,
            &meta,
            InstallHooks::default(),
        )
        .unwrap();
        let ui = YesMan::default();
        let u = uninstall_gui(&mut t, &["Variants"], &ui);
        assert_eq!(ui.asked.get(), 1, "the uninstall confirm");
        // The recorded variant is gone, so the reinstall must ask.
        t.sessions.with_snapshot("g", &u.restore[0].token, |s| {
            s.record.variant = Some("4k".into())
        });
        let r = restore_all(&mut t, &tokens(&u), &ui);
        assert_eq!(ui.asked.get(), 1, "restore asked the front end");
        assert_eq!(kind(&r.results[0]), Some(RestoreErrorKind::InstallFailed));
        let msg = &r.results[0].error.as_ref().unwrap().message;
        assert!(msg.contains("asked a question (choose_variant)"), "{msg}");
        assert!(uids(&t).is_empty());
        assert!(!t.data.join("g/staging/Variants").exists());

        let rej = RejectingUi::new(&ui);
        assert!(!rej.confirm("q", true));
        assert!(
            !rej.confirm_with(&ConfirmSpec {
                default_yes: true,
                ..Default::default()
            })
            .yes
        );
        assert_eq!(rej.choose_variant(&["a".into()]), None);
        assert_eq!(rej.asked(), ["confirm", "confirm", "choose_variant"]);
        assert_eq!(ui.asked.get(), 1);
    }

    // Fails when a failure after the reinstall committed leaves the row,
    // child rows or staged files behind, or stops the rest of a bulk call.
    #[test]
    fn a_failed_write_back_leaves_nothing_and_others_still_restore() {
        let mut t = setup();
        for n in ["A", "B", "C"] {
            simple(&mut t, n);
        }
        let b = mods::list_mods(t.store.conn(), "g").unwrap()[1].id;
        t.store
            .conn()
            .execute_batch(&format!(
                "INSERT INTO collections (id, game, slug, revision, installed_at)
                     VALUES (5, 'g', 'c', 1, 0);
                 INSERT INTO collection_mods VALUES (5, {b}, 1);
                 INSERT INTO fomod_choices VALUES ({b}, '{{}}', 'ask', 1);"
            ))
            .unwrap();
        let u = uninstall_gui(&mut t, &["A", "B", "C"], &Headless);
        // Injected failure in the collection_mods write of B.
        t.store
            .conn()
            .execute_batch(
                "CREATE TEMP TRIGGER boom BEFORE INSERT ON collection_mods
                 WHEN (SELECT uid FROM mods WHERE id = NEW.mod_id) = 'B'
                 BEGIN SELECT RAISE(ABORT, 'injected'); END;",
            )
            .unwrap();
        let r = restore_all(&mut t, &tokens(&u), &Headless);
        let ok: Vec<_> = r
            .results
            .iter()
            .map(|x| (x.uid.clone().unwrap(), x.ok))
            .collect();
        assert_eq!(
            ok,
            [("A".into(), true), ("B".into(), false), ("C".into(), true)]
        );
        assert_eq!(kind(&r.results[1]), Some(RestoreErrorKind::InstallFailed));
        assert_eq!(uids(&t), ["A", "C"]);
        let count = |sql: &str| -> i64 { t.store.conn().query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT COUNT(*) FROM mods WHERE uid = 'B'"), 0);
        for tb in snap::CHILD_TABLES {
            assert_eq!(
                count(&format!(
                    "SELECT COUNT(*) FROM {tb} WHERE mod_id NOT IN (SELECT id FROM mods)"
                )),
                0,
                "{tb}"
            );
        }
        assert!(!t.data.join("g/staging/B").exists());
        let tmp = t.data.join("g/staging/.tmp");
        assert_eq!(std::fs::read_dir(tmp).map_or(0, |d| d.count()), 0);
        // B's token is still usable once the cause is gone.
        t.store.conn().execute_batch("DROP TRIGGER boom;").unwrap();
        let again = restore_all(&mut t, &[u.restore[1].token.clone()], &Headless);
        assert!(again.results[0].ok, "{:?}", again.results);
        assert_eq!(uids(&t), ["A", "B", "C"]);
    }

    // Fails when a vanished archive aborts the whole bulk restore, or when
    // an unknown token is not reported as expired.
    #[test]
    fn missing_archive_and_unknown_tokens_are_typed() {
        let mut t = setup();
        for n in ["A", "B"] {
            simple(&mut t, n);
        }
        let u = uninstall_gui(&mut t, &["A", "B"], &Headless);
        std::fs::remove_file(t.dl.join("A.zip")).unwrap();
        let mut tk = tokens(&u);
        tk.push("deadbeef".into());
        let r = restore_all(&mut t, &tk, &Headless);
        assert_eq!(kind(&r.results[0]), Some(RestoreErrorKind::Expired));
        assert_eq!(r.results[0].uid, None);
        assert_eq!(kind(&r.results[1]), Some(RestoreErrorKind::ArchiveMissing));
        assert!(r.results[2].ok);
        assert_eq!(uids(&t), ["B"]);
        // Tokens are per game.
        let mut cx = Cx::new(&t.paths, &mut t.store, &Headless);
        let mut other = t.game.clone();
        other.id = "h".into();
        let r = restore(&mut cx, &other, &t.sessions, &[u.restore[0].token.clone()]).unwrap();
        assert_eq!(kind(&r.results[0]), Some(RestoreErrorKind::Expired));
    }

    // Fails when a token is derived from the mod (path, uid, name) or
    // repeats, which would let a front end guess or forge one.
    #[test]
    fn tokens_are_opaque_and_unique() {
        let mut t = setup();
        for n in ["Alpha", "Beta"] {
            simple(&mut t, n);
        }
        let u = uninstall_gui(&mut t, &["Alpha", "Beta"], &Headless);
        let mut seen = std::collections::HashSet::new();
        for e in &u.restore {
            assert_eq!(e.token.len(), 32);
            assert!(e.token.bytes().all(|b| b.is_ascii_hexdigit()));
            let low = e.token.to_lowercase();
            assert!(!low.contains(&e.uid.to_lowercase()) && !low.contains("/"));
            assert!(seen.insert(e.token.clone()));
        }
        for _ in 0..100 {
            assert!(seen.insert(new_token().unwrap()));
        }
    }

    /// Answers the uninstall confirm with "Delete archive too" ticked.
    struct Tick;
    impl Ui for Tick {
        fn say(&self, _msg: Msg) {}
        fn confirm_with(&self, _s: &ConfirmSpec) -> ConfirmAnswer {
            ConfirmAnswer {
                yes: true,
                option_checked: true,
            }
        }
    }

    // Fails when restorable/reason is wrong for any of the four cases, or
    // when a non-restorable mod still gets a usable token.
    #[test]
    fn restorable_and_reason_cover_all_cases() {
        let mut t = setup();
        simple(&mut t, "Kept");
        let folder = t.tmp.path().join("src/Folder");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("f.txt"), "f").unwrap();
        install(&mut t, &folder, "Folder", None);
        let outside = t.tmp.path().join("Outside.zip");
        let src = t.tmp.path().join("src/out");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("o.txt"), "o").unwrap();
        zip_tree(&src, &outside);
        install(&mut t, &outside, "Outside", None);
        let u = uninstall_gui(&mut t, &["Kept", "Folder", "Outside"], &Headless);
        let got: Vec<_> = u
            .restore
            .iter()
            .map(|e| (e.name.as_str(), e.restorable, e.reason))
            .collect();
        assert_eq!(
            got,
            [
                ("Kept", true, None),
                ("Folder", false, Some(RestoreReason::FolderInstall)),
                ("Outside", false, Some(RestoreReason::OutsideDownloads)),
            ]
        );
        simple(&mut t, "Deleted");
        let d = uninstall_gui(&mut t, &["Deleted"], &Tick);
        assert_eq!(d.archives.len(), 1);
        assert_eq!(
            (d.restore[0].restorable, d.restore[0].reason),
            (false, Some(RestoreReason::ArchiveDeleted))
        );
        let r = restore_all(
            &mut t,
            &[u.restore[1].token.clone(), d.restore[0].token.clone()],
            &Headless,
        );
        assert!(r
            .results
            .iter()
            .all(|x| kind(x) == Some(RestoreErrorKind::Expired)));
        let json = serde_json::to_value(&u.restore).unwrap();
        assert_eq!(json[0].get("reason"), None);
        assert_eq!(json[1]["reason"], "folder_install");
        assert_eq!(json[2]["reason"], "outside_downloads");
    }

    // Fails when an uninstall without capture changes the JSON shape
    // (a `restore` key appears) or stores snapshots.
    #[test]
    fn without_capture_the_result_serializes_as_before() {
        let mut t = setup();
        simple(&mut t, "A");
        let mut cx = Cx::new(&t.paths, &mut t.store, &Headless);
        let r = uninstall(&mut cx, &t.game, &["A".into()], false, None).unwrap();
        assert!(r.restore.is_empty());
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"done":true,"mods":1,"archives":[],"after":{"deployed":null,"needs_deploy":false}}"#
        );
        assert!(t.sessions.lock().is_empty());
    }
}
