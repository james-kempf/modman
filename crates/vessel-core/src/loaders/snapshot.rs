// SPDX-License-Identifier: GPL-3.0-or-later
//! Before/after snapshots of the game folder around a loader op, the backup
//! of in-scope originals, the diff that becomes `loader_files`, and the
//! rollback that undoes it.
//!
//! - Everything is lstat-based: symlinks are recorded as links (their
//!   target) and never followed.
//! - Every entry is stat'ed; only in-scope files are hashed.
//! - Backups go to the deploy backup store (copies, never hardlinks) and
//!   need the deploy lock, because the store's GC removes temp files.
//! - `backup_ref` is `<sha256>:<st_mode octal>:<mtime_ns>`, so a restore
//!   needs no manifest; the GC keys on the sha prefix.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::glob::Glob;
use super::records::{FileChange, FileKind, LoaderFile};
use super::types::Owner;
use super::{io_err, LoaderError};
use crate::deploy::backup::{Blob, BlobStore};
use crate::deploy::confine;
use crate::deploy::place::{hash_copy, hex, mtime_ns};
use crate::store::{DeployLock, Store};

/// One lstat'ed entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Entry {
    File {
        size: u64,
        mode: u32,
        mtime_ns: i64,
        /// Only for in-scope files.
        sha256: Option<String>,
    },
    Link {
        target: String,
        /// Hash of the raw target bytes (what the backup store keeps).
        sha256: String,
    },
    Dir {
        mode: u32,
    },
    /// FIFO, socket or device: recorded by stat only, never opened.
    Other {
        mode: u32,
    },
}

/// The game folder at one point in time, keyed by `/`-separated path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub entries: BTreeMap<String, Entry>,
}

fn sha_bytes(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

/// Hash of `path` if it is a regular file: opened without following a
/// link or blocking on a FIFO, then checked with fstat. `None` otherwise.
fn hash_regular(path: &Path) -> io::Result<Option<String>> {
    let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
    let f = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits() as i32)
        .open(path)
    {
        Ok(f) => f,
        // ELOOP: the entry became a symlink after the lstat.
        Err(e) if e.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) => {
            return Ok(None)
        }
        Err(e) => return Err(e),
    };
    if !f.metadata()?.is_file() {
        return Ok(None);
    }
    hash_copy(f, None).map(Some)
}

fn in_scope(scope: &[Glob], rel: &str) -> bool {
    scope.iter().any(|g| g.matches(rel))
}

/// Walks `root` without following links. Files matching `scope` are hashed.
pub fn snapshot(root: &Path, scope: &[Glob]) -> Result<Manifest, LoaderError> {
    let mut entries = BTreeMap::new();
    for e in walkdir::WalkDir::new(root).min_depth(1).follow_links(false) {
        let e = e.map_err(|e| {
            let p = e
                .path()
                .map_or_else(|| root.to_path_buf(), Path::to_path_buf);
            io_err(&p)(e.into())
        })?;
        let rel = e
            .path()
            .strip_prefix(root)
            .expect("walk stays under root")
            .to_string_lossy()
            .into_owned();
        let m = fs::symlink_metadata(e.path()).map_err(io_err(e.path()))?;
        let entry = if m.file_type().is_symlink() {
            let t = fs::read_link(e.path()).map_err(io_err(e.path()))?;
            Entry::Link {
                target: t.to_string_lossy().into_owned(),
                sha256: sha_bytes(t.as_os_str().as_bytes()),
            }
        } else if m.is_dir() {
            Entry::Dir { mode: m.mode() }
        } else if !m.is_file() {
            Entry::Other { mode: m.mode() }
        } else {
            let sha256 = if in_scope(scope, &rel) {
                hash_regular(e.path()).map_err(io_err(e.path()))?
            } else {
                None
            };
            Entry::File {
                size: m.size(),
                mode: m.mode(),
                mtime_ns: mtime_ns(&m),
                sha256,
            }
        };
        entries.insert(rel, entry);
    }
    Ok(Manifest { entries })
}

impl Manifest {
    /// Writes the manifest (0600) durably, for crash recovery.
    pub fn save(&self, path: &Path) -> Result<(), LoaderError> {
        if let Some(d) = path.parent() {
            fs::create_dir_all(d).map_err(io_err(d))?;
        }
        let json = serde_json::to_vec(self).map_err(|e| io_err(path)(io::Error::other(e)))?;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(io_err(path))?;
        io::Write::write_all(&mut f, &json).map_err(io_err(path))?;
        f.sync_all().map_err(io_err(path))
    }

    pub fn load(path: &Path) -> Result<Self, LoaderError> {
        let bytes = fs::read(path).map_err(io_err(path))?;
        serde_json::from_slice(&bytes).map_err(|e| io_err(path)(io::Error::other(e)))
    }
}

/// Refuses an op whose scope covers a file or link the deploy engine
/// placed: the loader would overwrite it and a later purge would delete
/// the loader's version.
pub fn check_deployed(store: &Store, game_id: &str, scope: &[Glob]) -> Result<(), LoaderError> {
    for o in store.occupied_paths(game_id, "")? {
        if o.owner == Owner::Deployed && in_scope(scope, &o.path) {
            return Err(LoaderError::Conflict {
                path: o.path,
                owner: Owner::Deployed,
            });
        }
    }
    Ok(())
}

/// `backup_ref` of a stored original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupRef {
    pub sha256: String,
    /// Full `st_mode` (type bits tell file from link).
    pub mode: u32,
    pub mtime_ns: i64,
}

impl BackupRef {
    pub fn encode(&self) -> String {
        format!("{}:{:o}:{}", self.sha256, self.mode, self.mtime_ns)
    }

    pub fn parse(s: &str) -> Option<Self> {
        let mut it = s.split(':');
        let sha = it.next().filter(|h| h.len() == 64)?;
        let mode = u32::from_str_radix(it.next()?, 8).ok()?;
        let mtime_ns = it.next()?.parse().ok()?;
        Some(Self {
            sha256: sha.to_string(),
            mode,
            mtime_ns,
        })
    }
}

/// Backs up every in-scope regular file and link of `pre` into
/// `backup_dir`, recording each ref in `loader_run_backups` for `run_id` as
/// soon as its blob is durable, so the GC keeps it even if Vessel dies
/// before the run's `loader_files` exist. `_lock` proves the deploy lock is
/// held (the GC deletes temp files). Returns `path -> backup_ref`.
pub fn backup(
    _lock: &DeployLock,
    store: &Store,
    run_id: i64,
    backup_dir: &Path,
    root: &Path,
    pre: &Manifest,
    scope: &[Glob],
) -> Result<BTreeMap<String, String>, LoaderError> {
    let blobs = BlobStore { root: backup_dir };
    let mut out = BTreeMap::new();
    for (rel, e) in &pre.entries {
        let wanted = matches!(e, Entry::File { .. } | Entry::Link { .. });
        if !wanted || !in_scope(scope, rel) {
            continue;
        }
        let c = confined(root, rel, false)?;
        let b = blobs.store(&c.path).map_err(io_err(&root.join(rel)))?;
        let r = BackupRef {
            sha256: b.sha256,
            mode: b.mode,
            mtime_ns: b.mtime_ns,
        }
        .encode();
        store.add_loader_run_backup(run_id, &r)?;
        out.insert(rel.clone(), r);
    }
    Ok(out)
}

fn confined(root: &Path, rel: &str, create: bool) -> Result<confine::Confined, LoaderError> {
    confine::confine(root, rel, create).map_err(|e| LoaderError::UnsafePath {
        path: rel.to_string(),
        message: e.to_string(),
    })
}

fn kind_sha(e: &Entry) -> Option<(FileKind, Option<String>)> {
    match e {
        Entry::File { sha256, .. } => Some((FileKind::File, sha256.clone())),
        Entry::Link { sha256, .. } => Some((FileKind::Link, Some(sha256.clone()))),
        // Not restorable: reported as an out-of-scope (unrecoverable) file.
        Entry::Other { .. } => Some((FileKind::File, None)),
        Entry::Dir { .. } => None,
    }
}

fn is_other(m: &Manifest, path: &str) -> bool {
    matches!(m.entries.get(path), Some(Entry::Other { .. }))
}

fn changed(a: &Entry, b: &Entry) -> bool {
    match (a, b) {
        (
            Entry::File {
                size: s1,
                mode: m1,
                mtime_ns: t1,
                sha256: h1,
            },
            Entry::File {
                size: s2,
                mode: m2,
                mtime_ns: t2,
                sha256: h2,
            },
        ) => match (h1, h2) {
            // Hashed (in scope): content and permissions decide.
            (Some(h1), Some(h2)) => h1 != h2 || m1 != m2,
            _ => s1 != s2 || m1 != m2 || t1 != t2,
        },
        (Entry::Link { sha256: a, .. }, Entry::Link { sha256: b, .. }) => a != b,
        _ => true,
    }
}

/// Changes from `pre` to `post`, as `loader_files` rows. Folders are not
/// rows. Changes outside `scope` are flagged `out_of_scope` (unrecoverable:
/// they have no backup).
pub fn diff(
    pre: &Manifest,
    post: &Manifest,
    backups: &BTreeMap<String, String>,
    scope: &[Glob],
) -> Vec<LoaderFile> {
    let mut out = Vec::new();
    let paths: std::collections::BTreeSet<&String> =
        pre.entries.keys().chain(post.entries.keys()).collect();
    for path in paths {
        let before = pre.entries.get(path).and_then(kind_sha);
        let after = post.entries.get(path).and_then(kind_sha);
        let (change, kind) = match (&before, &after) {
            (None, None) => continue,
            (None, Some((k, _))) => (FileChange::Created, *k),
            (Some((k, _)), None) => (FileChange::Deleted, *k),
            (Some(_), Some((k, _))) => {
                if !changed(&pre.entries[path], &post.entries[path]) {
                    continue;
                }
                (FileChange::Modified, *k)
            }
        };
        out.push(LoaderFile {
            path: path.clone(),
            change,
            kind,
            pre_sha256: before.and_then(|(_, h)| h),
            post_sha256: after.and_then(|(_, h)| h),
            backup_ref: backups.get(path).cloned(),
            out_of_scope: !in_scope(scope, path) || is_other(pre, path) || is_other(post, path),
        });
    }
    out
}

/// What a rollback did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RollbackReport {
    pub removed: Vec<String>,
    pub restored: Vec<String>,
    /// Changes that cannot be undone (out of scope, or no backup).
    pub unrecoverable: Vec<String>,
}

/// Undoes `files` under `root`: created entries are removed, modified and
/// deleted ones restored from their backup. With `pre`, folders the op
/// created are removed when empty. Writes never follow a symlinked folder.
pub fn rollback(
    root: &Path,
    backup_dir: &Path,
    files: &[LoaderFile],
    pre: Option<&Manifest>,
) -> Result<RollbackReport, LoaderError> {
    let blobs = BlobStore { root: backup_dir };
    let mut report = RollbackReport::default();
    let mut ordered: Vec<&LoaderFile> = files.iter().collect();
    // Deepest first, so a file goes before its folder.
    ordered.sort_by_key(|f| std::cmp::Reverse(f.path.matches('/').count()));
    for f in ordered {
        if f.out_of_scope {
            report.unrecoverable.push(f.path.clone());
            continue;
        }
        match f.change {
            FileChange::Created => {
                let c = confined(root, &f.path, false)?;
                match fs::remove_file(&c.path) {
                    Ok(()) => report.removed.push(f.path.clone()),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(io_err(&root.join(&f.path))(e)),
                }
            }
            FileChange::Modified | FileChange::Deleted => {
                let Some(r) = f.backup_ref.as_deref().and_then(BackupRef::parse) else {
                    report.unrecoverable.push(f.path.clone());
                    continue;
                };
                let path = blobs.blob_path(&r.sha256);
                let size = fs::metadata(&path).map_err(io_err(&path))?.len() as i64;
                let blob = Blob {
                    sha256: r.sha256,
                    path,
                    size,
                    mode: r.mode,
                    mtime_ns: r.mtime_ns,
                };
                let c = confined(root, &f.path, true)?;
                // A folder now in the way of a restored file.
                if fs::symlink_metadata(&c.path).is_ok_and(|m| m.is_dir()) {
                    report.unrecoverable.push(f.path.clone());
                    continue;
                }
                blobs
                    .restore(&blob, &c.path)
                    .map_err(io_err(&root.join(&f.path)))?;
                report.restored.push(f.path.clone());
            }
        }
    }
    if let Some(pre) = pre {
        let mut dirs: Vec<PathBuf> = walkdir::WalkDir::new(root)
            .min_depth(1)
            .follow_links(false)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_dir())
            .filter(|e| {
                let rel = e.path().strip_prefix(root).expect("under root");
                !pre.entries.contains_key(rel.to_string_lossy().as_ref())
            })
            .map(walkdir::DirEntry::into_path)
            .collect();
        dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
        for d in dirs {
            // Only empty folders go; fails harmlessly otherwise.
            let _ = fs::remove_dir(d);
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Fx {
        _tmp: tempfile::TempDir,
        game: PathBuf,
        backups: PathBuf,
        lock: DeployLock,
        store: Store,
        run: i64,
    }

    fn fx() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("game");
        fs::create_dir_all(game.join("Mods")).unwrap();
        fs::write(game.join("StardewValley"), "#!/bin/bash\nvanilla\n").unwrap();
        fs::write(game.join("steam_appid.txt"), "413150").unwrap();
        fs::write(game.join("Stardew Valley.dll"), "game").unwrap();
        let lock = DeployLock::try_acquire(&tmp.path().join("deploy.lock")).unwrap();
        let store = Store::open_in_memory().unwrap();
        store
            .conn()
            .execute_batch(
                "INSERT INTO games (id, name, added_at) VALUES ('stardewvalley', 'SDV', 0);
                 INSERT INTO loader_installs (id, game_id, game_dir, loader_key, runtime, strategy,
                     status, origin, installed_at, updated_at)
                   VALUES (1, 'stardewvalley', '/g', 'smapi', 'native', 'installer', 'running',
                     'local', 0, 0);
                 INSERT INTO loader_runs (id, install_id, op, consent_hash, phase, started_at)
                   VALUES (1, 1, 'install', 'h', 'backing_up', 0);",
            )
            .unwrap();
        Fx {
            backups: tmp.path().join("backups"),
            game,
            lock,
            store,
            run: 1,
            _tmp: tmp,
        }
    }

    fn scope() -> Vec<Glob> {
        [
            "StardewValley",
            "StardewValley-original",
            "StardewModdingAPI*",
            "smapi-internal/**",
            "steam_appid.txt",
            "link",
        ]
        .map(Glob::new)
        .to_vec()
    }

    fn by_path(files: &[LoaderFile]) -> BTreeMap<&str, &LoaderFile> {
        files.iter().map(|f| (f.path.as_str(), f)).collect()
    }

    /// Catches: links followed during the snapshot (outside files hashed or
    /// walked), and link changes missed.
    #[test]
    fn symlinks_are_recorded_as_links_and_not_followed() {
        let f = fx();
        let outside = f.game.parent().unwrap().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, f.game.join("link")).unwrap();
        let m = snapshot(&f.game, &scope()).unwrap();
        assert!(matches!(m.entries["link"], Entry::Link { .. }));
        assert!(!m.entries.contains_key("link/secret"));
        fs::remove_file(f.game.join("link")).unwrap();
        std::os::unix::fs::symlink("/elsewhere", f.game.join("link")).unwrap();
        let post = snapshot(&f.game, &scope()).unwrap();
        let d = diff(&m, &post, &BTreeMap::new(), &scope());
        assert_eq!(
            (d.len(), d[0].kind, d[0].change),
            (1, FileKind::Link, FileChange::Modified)
        );
    }

    /// Catches: a hardlinked backup (the installer rewriting the game file
    /// in place would change the "backup" too).
    #[test]
    fn backups_are_copies_not_hardlinks() {
        let f = fx();
        let pre = snapshot(&f.game, &scope()).unwrap();
        let refs = backup(
            &f.lock,
            &f.store,
            f.run,
            &f.backups,
            &f.game,
            &pre,
            &scope(),
        )
        .unwrap();
        let r = BackupRef::parse(&refs["StardewValley"]).unwrap();
        let blob = BlobStore { root: &f.backups }.blob_path(&r.sha256);
        let (a, b) = (
            fs::metadata(f.game.join("StardewValley")).unwrap(),
            fs::metadata(blob).unwrap(),
        );
        assert_ne!((a.dev(), a.ino()), (b.dev(), b.ino()));
        assert!(
            !refs.contains_key("Stardew Valley.dll"),
            "out of scope is not backed up"
        );
    }

    /// Catches: misclassified changes, rollback leaving installer files or
    /// losing originals, and out-of-scope changes reported as undone.
    #[test]
    fn diff_classifies_and_rollback_restores() {
        let f = fx();
        fs::set_permissions(
            f.game.join("StardewValley"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let pre = snapshot(&f.game, &scope()).unwrap();
        let refs = backup(
            &f.lock,
            &f.store,
            f.run,
            &f.backups,
            &f.game,
            &pre,
            &scope(),
        )
        .unwrap();
        // What the SMAPI installer does, roughly.
        fs::rename(
            f.game.join("StardewValley"),
            f.game.join("StardewValley-original"),
        )
        .unwrap();
        fs::write(f.game.join("StardewValley"), "#!/bin/bash\nsmapi\n").unwrap();
        fs::create_dir_all(f.game.join("smapi-internal/i18n")).unwrap();
        fs::write(f.game.join("smapi-internal/i18n/de.json"), "{}").unwrap();
        fs::write(f.game.join("StardewModdingAPI.dll"), "smapi").unwrap();
        fs::remove_file(f.game.join("steam_appid.txt")).unwrap();
        fs::write(f.game.join("Stardew Valley.dll"), "patched").unwrap();
        let post = snapshot(&f.game, &scope()).unwrap();
        let d = diff(&pre, &post, &refs, &scope());
        let m = by_path(&d);
        assert_eq!(m["StardewValley"].change, FileChange::Modified);
        assert!(m["StardewValley"].backup_ref.is_some());
        assert_eq!(m["StardewValley-original"].change, FileChange::Created);
        assert_eq!(m["StardewModdingAPI.dll"].change, FileChange::Created);
        assert_eq!(m["smapi-internal/i18n/de.json"].change, FileChange::Created);
        assert_eq!(m["steam_appid.txt"].change, FileChange::Deleted);
        assert!(m["Stardew Valley.dll"].out_of_scope);
        assert!(!m.contains_key("smapi-internal"), "folders are not rows");

        let r = rollback(&f.game, &f.backups, &d, Some(&pre)).unwrap();
        assert_eq!(r.unrecoverable, ["Stardew Valley.dll"]);
        assert_eq!(
            fs::read_to_string(f.game.join("StardewValley")).unwrap(),
            "#!/bin/bash\nvanilla\n"
        );
        assert_eq!(
            fs::metadata(f.game.join("StardewValley")).unwrap().mode() & 0o777,
            0o755
        );
        assert_eq!(
            fs::read_to_string(f.game.join("steam_appid.txt")).unwrap(),
            "413150"
        );
        for gone in [
            "StardewValley-original",
            "StardewModdingAPI.dll",
            "smapi-internal",
        ] {
            assert!(!f.game.join(gone).exists(), "{gone}");
        }
        assert!(
            f.game.join("Mods").is_dir(),
            "pre-existing empty folder kept"
        );
        let after = snapshot(&f.game, &scope()).unwrap();
        let left: Vec<String> = diff(&pre, &after, &refs, &scope())
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(left, ["Stardew Valley.dll"]);
    }

    /// Catches: backups not recorded as GC roots before the run spawns (F4).
    #[test]
    fn backups_are_recorded_for_the_run() {
        let f = fx();
        let pre = snapshot(&f.game, &scope()).unwrap();
        let refs = backup(
            &f.lock,
            &f.store,
            f.run,
            &f.backups,
            &f.game,
            &pre,
            &scope(),
        )
        .unwrap();
        let mut want: Vec<String> = refs.values().cloned().collect();
        want.sort();
        assert_eq!(f.store.loader_backup_refs().unwrap(), want);
        f.store.clear_loader_run_backups(f.run).unwrap();
        assert!(f.store.loader_backup_refs().unwrap().is_empty());
    }

    /// Catches: an in-scope FIFO (or device) being opened, which blocks the
    /// snapshot or backup forever (F3).
    #[test]
    fn in_scope_fifo_never_blocks() {
        let f = fx();
        let fifo = f.game.join("StardewModdingAPI.fifo");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o644),
            0,
        )
        .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (game, backups) = (f.game.clone(), f.backups.clone());
        std::thread::spawn(move || {
            let pre = snapshot(&game, &scope()).unwrap();
            let lock_dir = tempfile::tempdir().unwrap();
            let lock = DeployLock::try_acquire(&lock_dir.path().join("l")).unwrap();
            let store = Store::open_in_memory().unwrap();
            store
                .conn()
                .execute_batch(
                    "INSERT INTO loader_installs (id, game_dir, loader_key, runtime, strategy,
                         status, origin, installed_at, updated_at)
                       VALUES (1, '/g', 'smapi', 'native', 'installer', 'running', 'local', 0, 0);
                     INSERT INTO loader_runs (id, install_id, op, consent_hash, phase, started_at)
                       VALUES (1, 1, 'install', 'h', 'backing_up', 0);",
                )
                .unwrap();
            let refs = backup(&lock, &store, 1, &backups, &game, &pre, &scope()).unwrap();
            tx.send((pre, refs)).unwrap();
        });
        let (pre, refs) = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("snapshot/backup blocked on a FIFO");
        assert!(matches!(
            pre.entries["StardewModdingAPI.fifo"],
            Entry::Other { .. }
        ));
        assert!(!refs.contains_key("StardewModdingAPI.fifo"));
        // A FIFO the installer creates is reported, as unrecoverable.
        let mut before = pre.clone();
        before.entries.remove("StardewModdingAPI.fifo");
        let d = diff(&before, &pre, &refs, &scope());
        assert!(d.len() == 1 && d[0].out_of_scope, "{d:?}");
    }

    /// Catches: a chmod-only change (an installer's async `chmod 755`) going
    /// unnoticed.
    #[test]
    fn mode_change_is_a_modification() {
        let f = fx();
        let pre = snapshot(&f.game, &scope()).unwrap();
        fs::set_permissions(
            f.game.join("StardewValley"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let post = snapshot(&f.game, &scope()).unwrap();
        assert_eq!(diff(&pre, &post, &BTreeMap::new(), &scope()).len(), 1);
    }

    /// Catches: rollback writing through a symlinked folder the installer
    /// planted.
    #[test]
    fn rollback_refuses_symlinked_parents() {
        let f = fx();
        let outside = f.game.parent().unwrap().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("x"), "keep").unwrap();
        std::os::unix::fs::symlink(&outside, f.game.join("smapi-internal")).unwrap();
        let rows = [LoaderFile {
            path: "smapi-internal/x".into(),
            change: FileChange::Created,
            kind: FileKind::File,
            pre_sha256: None,
            post_sha256: None,
            backup_ref: None,
            out_of_scope: false,
        }];
        assert!(rollback(&f.game, &f.backups, &rows, None).is_err());
        assert!(outside.join("x").exists());
    }

    /// Catches: in-scope files placed by the deploy engine not blocking the op.
    #[test]
    fn deployed_file_in_scope_is_a_conflict() {
        let s = Store::open_in_memory().unwrap();
        s.conn()
            .execute_batch(
                "INSERT INTO games (id, name, added_at) VALUES ('stardewvalley', 'SDV', 0);
                 INSERT INTO deployments (game, target, rel_path, method, source, deployed_at)
                   VALUES ('stardewvalley', 'game_dir', 'smapi-internal/config.json', 'symlink', '/s', 0),
                          ('stardewvalley', 'game_dir', 'Mods/A/a.dll', 'hardlink', '/s', 0);",
            )
            .unwrap();
        let err = check_deployed(&s, "stardewvalley", &scope()).unwrap_err();
        assert!(
            matches!(
                err,
                LoaderError::Conflict {
                    owner: Owner::Deployed,
                    ..
                }
            ),
            "{err:?}"
        );
        assert!(check_deployed(&s, "stardewvalley", &[Glob::new("StardewValley")]).is_ok());
    }

    #[test]
    fn manifest_and_backup_ref_round_trip() {
        let f = fx();
        let m = snapshot(&f.game, &scope()).unwrap();
        let p = f.game.parent().unwrap().join("m/pre.json");
        m.save(&p).unwrap();
        assert_eq!(Manifest::load(&p).unwrap(), m);
        assert_eq!(fs::metadata(&p).unwrap().mode() & 0o777, 0o600);
        let r = BackupRef {
            sha256: "a".repeat(64),
            mode: 0o100755,
            mtime_ns: -5,
        };
        assert_eq!(BackupRef::parse(&r.encode()), Some(r));
        assert_eq!(BackupRef::parse("short:1:2"), None);
    }
}
