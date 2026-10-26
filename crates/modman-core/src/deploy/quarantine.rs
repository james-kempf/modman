// SPDX-License-Identifier: GPL-3.0-or-later
//! Poisoned-backup cleanup. A held backup is "poisoned" when it holds a
//! mod's bytes for that mod's own path: purge would "restore" the mod file
//! instead of the game's original. This happens when a deploy backs up a
//! file an earlier, untracked deploy placed.
//!
//! - [`poisoned_backups`] lists them (read-only), classified `confirmed`
//!   (the path lies in a mod-only folder the game never ships, the UE4SS
//!   mods folder or the paks folder; or the journal shows modman placed
//!   it, with no purge in between, before the backup was taken) or
//!   `ambiguous` (anything else, e.g. a vanilla-identical file a purge
//!   restored before the backup, or a file UE4SS itself ships).
//! - [`quarantine_poisoned`] moves them, never deletes: backup blobs, their
//!   rows and untracked game files go to
//!   `<data-dir>/<game>/quarantine/<id>/` with a `manifest.json`.
//! - [`restore_quarantine`] puts everything back.
//!
//! Both changing calls take `confirmed`; the front end asks the user first.
//! The core never asks (a GUI `Ui` may auto-answer).

use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::backup::{Blob, BlobStore};
use super::confine::{components, confine};
use super::engine::{confine_err, guard, interrupted, release_blob};
use super::place::sha256_file;
use super::plan::base_prefix;
use super::{io_err, target_root, DeployEnv, DeployError};
use crate::games::{GameConfig, Target};
use crate::store::{now, Store};

const MANIFEST: &str = "manifest.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoisonClass {
    /// A mod-only folder, or modman placed this path (not undone by a
    /// purge) before the backup was taken.
    Confirmed,
    /// No earlier deploy record, or a file UE4SS itself ships.
    Ambiguous,
}

/// A held backup holding the bytes a mod ships for the same path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PoisonedBackup {
    pub backup_id: i64,
    pub target: Target,
    /// Relative to the target root (real spelling).
    pub path: String,
    pub sha256: String,
    pub class: PoisonClass,
    pub reason: String,
    /// Mods (uids) shipping these bytes at this path.
    pub mods: Vec<String>,
    /// The path is part of the active deployment.
    pub deployed: bool,
    /// ...placed by a mod that is enabled. Its game file stays; only the
    /// backup goes, so a later purge removes the file instead of
    /// "restoring" it.
    pub deployed_by_enabled_mod: bool,
}

/// What a quarantine entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineOrigin {
    /// A backup blob and its `backups` row (`blobs/<sha>`).
    Backup,
    /// An untracked file from the game folder (`game/<target>/<path>`).
    GameFile,
}

/// The `backups` row of a quarantined backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupRecord {
    pub id: i64,
    pub size: i64,
    pub mode: u32,
    pub mtime_ns: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantineItem {
    pub path: String,
    pub target: Target,
    pub sha256: String,
    pub origin: QuarantineOrigin,
    pub class: PoisonClass,
    pub reason: String,
    /// Set for [`QuarantineOrigin::Backup`].
    pub backup: Option<BackupRecord>,
    /// Deployments that referred to the backup.
    #[serde(default)]
    pub deployment_ids: Vec<i64>,
}

/// `<quarantine>/<id>/manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantineManifest {
    pub id: String,
    pub game: String,
    pub created_at: i64,
    pub restored_at: Option<i64>,
    pub items: Vec<QuarantineItem>,
}

/// Result of [`quarantine_poisoned`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct QuarantineReport {
    /// `None` when nothing was quarantined.
    pub id: Option<String>,
    pub dir: Option<PathBuf>,
    pub backups: usize,
    pub game_files: usize,
    /// Ambiguous entries left alone (`include_ambiguous` = false).
    pub skipped_ambiguous: usize,
}

/// Result of [`restore_quarantine`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RestoreReport {
    pub id: String,
    pub backups: usize,
    pub game_files: usize,
}

fn quarantine_root(env: &DeployEnv) -> PathBuf {
    env.game_data_dir().join("quarantine")
}

/// Why a path is part of UE4SS itself rather than of a Lua mod.
fn ue4ss_reason(rel: &str) -> Option<&'static str> {
    let lower = rel.to_lowercase();
    let mut comps = lower.split('/');
    let name = lower.rsplit('/').next().unwrap_or_default();
    if comps.any(|c| c == "keybinds" || c == "bpmodloadermod") {
        Some("a UE4SS built-in mod")
    } else if name == "mods.txt" || name == "mods.json" {
        Some("the UE4SS mod list")
    } else {
        None
    }
}

struct Provider {
    mod_id: i64,
    uid: String,
    sha: String,
}

/// Held backups of `game` whose bytes a mod ships for the same path.
/// Read-only.
pub fn poisoned_backups(
    conn: &Connection,
    game: &GameConfig,
) -> Result<Vec<PoisonedBackup>, DeployError> {
    let target = game.target.as_str();
    let base = base_prefix(&game.subdir);

    // Every mod's files (enabled or not) at their deploy path.
    let mut providers: HashMap<String, Vec<Provider>> = HashMap::new();
    let mut ue4ss_mods = BTreeSet::new();
    let mut stmt = conn.prepare(
        "SELECT m.id, m.uid, m.root, f.path, f.sha256 FROM mods m
         JOIN mod_files f ON f.mod_id = m.id WHERE m.game = ?1 ORDER BY m.priority, m.id",
    )?;
    let rows = stmt.query_map([&game.domain], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    for row in rows {
        let (mod_id, uid, root, path, sha) = row?;
        if path.rsplit('/').next().unwrap_or_default().to_lowercase() == "ue4ss.dll" {
            ue4ss_mods.insert(mod_id);
        }
        let prefix = if root == "game" { "" } else { base.as_str() };
        providers
            .entry(format!("{prefix}{path}").to_lowercase())
            .or_default()
            .push(Provider { mod_id, uid, sha });
    }

    // Mod-only folders (target-root relative, lower-cased, with a trailing
    // `/`): the game never ships files there, so any backup is mod bytes.
    let mod_only: Vec<String> = if game.target == Target::GameDir {
        crate::games::ue4ss_mods_dir(game, game.folder.as_deref())
            .into_iter()
            .chain(game.paks_dir.clone())
            .map(|d| d.trim_matches('/').to_string())
            .filter(|d| !d.is_empty())
            .map(|d| format!("{base}{d}/").to_lowercase())
            .collect()
    } else {
        Vec::new()
    };

    // Journals that undid every placement before them: committed purges
    // and rollbacks.
    let undone: Vec<i64> = conn
        .prepare(
            "SELECT id FROM deploy_journal WHERE game = ?1
             AND ((op = 'purge' AND state = 'committed') OR state = 'rolled_back')",
        )?
        .query_map([&game.domain], |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    // Deploy journal: where each backup was taken, and every placement.
    let mut taken_in: HashMap<i64, i64> = HashMap::new();
    let mut placed: HashMap<String, Vec<i64>> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT s.journal_id, s.rel_path, s.done, j.state,
                json_extract(s.detail, '$.backup_id')
         FROM deploy_journal_steps s JOIN deploy_journal j ON j.id = s.journal_id
         WHERE j.game = ?1 AND s.action = 'place' AND s.target = ?2",
    )?;
    let rows = stmt.query_map([&game.domain, target], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, bool>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<i64>>(4)?,
        ))
    })?;
    for row in rows {
        let (journal, rel, done, state, backup) = row?;
        if let Some(b) = backup {
            taken_in.insert(b, journal);
        }
        if done && state != "rolled_back" {
            placed.entry(rel.to_lowercase()).or_default().push(journal);
        }
    }

    // Active deployment: path -> placed by an enabled mod.
    let mut stmt = conn.prepare(
        "SELECT d.rel_path, COALESCE(m.enabled, 0) FROM deployments d
         LEFT JOIN mods m ON m.id = d.mod_id WHERE d.game = ?1 AND d.target = ?2",
    )?;
    let deployed: HashMap<String, bool> = stmt
        .query_map([&game.domain, target], |r| {
            Ok((r.get::<_, String>(0)?.to_lowercase(), r.get(1)?))
        })?
        .collect::<Result<_, _>>()?;

    let mut stmt = conn.prepare(
        "SELECT id, rel_path, sha256 FROM backups
         WHERE game = ?1 AND target = ?2 AND restored_at IS NULL ORDER BY rel_path, id",
    )?;
    let held = stmt
        .query_map([&game.domain, target], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut out = Vec::new();
    for (id, rel, sha) in held {
        let key = rel.to_lowercase();
        let shipping: Vec<&Provider> = providers
            .get(&key)
            .into_iter()
            .flatten()
            .filter(|p| p.sha == sha)
            .collect();
        if shipping.is_empty() {
            continue;
        }
        let bundled = ue4ss_reason(&rel).or_else(|| {
            shipping
                .iter()
                .any(|p| ue4ss_mods.contains(&p.mod_id))
                .then_some("shipped by the UE4SS mod itself")
        });
        let backup_journal = taken_in.get(&id).copied().unwrap_or(i64::MAX);
        // A placement still in effect when the backup was taken: a purge
        // in between restored the original, which may be vanilla bytes
        // identical to the mod's.
        let prior = placed.get(&key).and_then(|js| {
            js.iter()
                .copied()
                .filter(|j| {
                    *j < backup_journal && !undone.iter().any(|u| j < u && *u < backup_journal)
                })
                .max()
        });
        let in_mod_only = mod_only.iter().any(|d| key.starts_with(d.as_str()));
        let (class, reason) = match (bundled, prior) {
            (Some(why), _) => (
                PoisonClass::Ambiguous,
                format!("UE4SS file ({why}); it may belong to a UE4SS install outside modman"),
            ),
            (None, Some(j)) => (
                PoisonClass::Confirmed,
                format!("modman placed this file before the backup was taken (deploy #{j})"),
            ),
            (None, None) if in_mod_only => (
                PoisonClass::Confirmed,
                "in a mod-only folder (UE4SS mods or paks) the game never ships".to_string(),
            ),
            (None, None) => (
                PoisonClass::Ambiguous,
                "no earlier modman deploy of this path still in effect when the backup was taken \
                 (a purge restores the original); the mod may ship the game's own bytes"
                    .to_string(),
            ),
        };
        let mut mods: Vec<String> = shipping.iter().map(|p| p.uid.clone()).collect();
        mods.dedup();
        out.push(PoisonedBackup {
            backup_id: id,
            target: game.target,
            path: rel,
            sha256: sha,
            class,
            reason,
            mods,
            deployed: deployed.contains_key(&key),
            deployed_by_enabled_mod: deployed.get(&key).copied().unwrap_or(false),
        });
    }
    Ok(out)
}

fn refuse_interrupted(conn: &Connection, game: &GameConfig) -> Result<(), DeployError> {
    if interrupted(conn, &game.domain)? {
        return Err(DeployError::Quarantine(format!(
            "a deploy or purge of {} was interrupted; run 'modman deploy' or 'modman purge' to recover first",
            game.domain
        )));
    }
    Ok(())
}

fn fsync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

/// Copies `src` to `dest` durably (content only).
fn copy_durable(src: &Path, dest: &Path) -> io::Result<()> {
    let dir = dest.parent().expect("destination has a parent");
    fs::create_dir_all(dir)?;
    fs::copy(src, dest)?;
    File::open(dest)?.sync_all()?;
    fsync_dir(dir)
}

/// Moves a file or symlink, across file systems too (content, mode and
/// mtime are kept).
fn move_file(src: &Path, dest: &Path) -> io::Result<()> {
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    match fs::rename(src, dest) {
        Err(e) if e.raw_os_error() == Some(rustix::io::Errno::XDEV.raw_os_error()) => {
            let meta = fs::symlink_metadata(src)?;
            if meta.is_symlink() {
                std::os::unix::fs::symlink(fs::read_link(src)?, dest)?;
            } else {
                fs::copy(src, dest)?;
                let f = File::options().write(true).open(dest)?;
                f.set_modified(meta.modified()?)?;
                f.sync_all()?;
            }
            fs::remove_file(src)
        }
        r => r,
    }?;
    if let Some(dir) = dest.parent() {
        fsync_dir(dir)?;
    }
    Ok(())
}

fn write_manifest(dir: &Path, m: &QuarantineManifest) -> Result<(), DeployError> {
    let path = dir.join(MANIFEST);
    let tmp = dir.join(".manifest.json.tmp");
    let data = serde_json::to_vec_pretty(m).expect("manifest serializes");
    (|| {
        let mut f = File::create(&tmp)?;
        io::Write::write_all(&mut f, &data)?;
        f.sync_all()?;
        fs::rename(&tmp, &path)?;
        fsync_dir(dir)
    })()
    .map_err(io_err(&path))
}

fn read_manifest(dir: &Path) -> Result<QuarantineManifest, DeployError> {
    let path = dir.join(MANIFEST);
    let data = fs::read(&path).map_err(io_err(&path))?;
    serde_json::from_slice(&data)
        .map_err(|e| DeployError::Quarantine(format!("{}: {e}", path.display())))
}

/// Where a game file of `item` is kept inside the quarantine folder.
fn game_copy(qdir: &Path, item: &QuarantineItem) -> Result<PathBuf, DeployError> {
    let comps = components(&item.path).map_err(|m| DeployError::PathConflict {
        rel: item.path.clone(),
        reason: m.to_string(),
    })?;
    Ok(comps
        .iter()
        .fold(qdir.join("game").join(item.target.as_str()), |p, c| {
            p.join(c)
        }))
}

/// A new quarantine folder named after the current time.
fn new_quarantine_dir(root: &Path) -> Result<(String, PathBuf), DeployError> {
    fs::create_dir_all(root).map_err(io_err(root))?;
    let stamp = now();
    for n in 0.. {
        let id = if n == 0 {
            stamp.to_string()
        } else {
            format!("{stamp}-{n}")
        };
        let dir = root.join(&id);
        match fs::create_dir(&dir) {
            Ok(()) => return Ok((id, dir)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_err(&dir)(e)),
        }
    }
    unreachable!("an unused quarantine id exists")
}

/// The untracked game file at `e.path`, when it holds the poisoned bytes
/// (a different file there is left alone). Escaping paths are refused.
fn movable_game_file(game: &GameConfig, e: &PoisonedBackup) -> Result<bool, DeployError> {
    let root = target_root(game, e.target)?;
    match confine(&root, &e.path, false) {
        Ok(c) => Ok(fs::symlink_metadata(&c.path).is_ok_and(|m| !m.is_dir())
            && sha256_file(&c.path).is_ok_and(|s| s == e.sha256)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(confine_err(&root, &e.path, err)),
    }
}

/// Moves the poisoned backups of `game` into a new quarantine: confirmed
/// entries, plus ambiguous ones with `include_ambiguous`. Untracked game
/// files holding the poisoned bytes go too; files of the active deployment
/// stay. Refuses without `confirmed`, while the game runs, or while another
/// deploy holds the lock. Every game-folder path is checked before anything
/// changes.
pub fn quarantine_poisoned(
    store: &mut Store,
    game: &GameConfig,
    env: &DeployEnv,
    include_ambiguous: bool,
    confirmed: bool,
) -> Result<QuarantineReport, DeployError> {
    if !confirmed {
        return Err(DeployError::NotConfirmed);
    }
    let _lock = guard(game, env)?;
    refuse_interrupted(store.conn(), game)?;
    let (picked, left): (Vec<_>, Vec<_>) = poisoned_backups(store.conn(), game)?
        .into_iter()
        .partition(|e| include_ambiguous || e.class == PoisonClass::Confirmed);
    let mut report = QuarantineReport {
        skipped_ambiguous: left.len(),
        ..Default::default()
    };
    if picked.is_empty() {
        return Ok(report);
    }
    // Read-only preflight: which game files move (deployed ones never do).
    let mut moves = Vec::with_capacity(picked.len());
    for e in &picked {
        moves.push(!e.deployed && movable_game_file(game, e)?);
    }

    let (id, qdir) = new_quarantine_dir(&quarantine_root(env))?;
    let mut items = Vec::new();
    let mut blobs = Vec::new();
    for (e, mv) in picked.iter().zip(&moves) {
        let (store_path, rec): (String, BackupRecord) = store.conn().query_row(
            "SELECT store_path, size, mode, mtime_ns, created_at FROM backups WHERE id = ?1",
            [e.backup_id],
            |r| {
                Ok((
                    r.get(0)?,
                    BackupRecord {
                        id: e.backup_id,
                        size: r.get(1)?,
                        mode: r.get(2)?,
                        mtime_ns: r.get(3)?,
                        created_at: r.get(4)?,
                    },
                ))
            },
        )?;
        let deployment_ids = {
            let mut stmt = store
                .conn()
                .prepare("SELECT id FROM deployments WHERE backup_id = ?1 ORDER BY id")?;
            let ids = stmt.query_map([e.backup_id], |r| r.get(0))?;
            ids.collect::<Result<Vec<i64>, _>>()?
        };
        let blob = PathBuf::from(&store_path);
        let dest = qdir.join("blobs").join(&e.sha256);
        if !dest.exists() {
            copy_durable(&blob, &dest).map_err(io_err(&blob))?;
        }
        blobs.push(Blob {
            sha256: e.sha256.clone(),
            path: blob,
            size: rec.size,
            mode: rec.mode,
            mtime_ns: rec.mtime_ns.unwrap_or(0),
        });
        let item = |origin, backup, deployment_ids| QuarantineItem {
            path: e.path.clone(),
            target: e.target,
            sha256: e.sha256.clone(),
            origin,
            class: e.class,
            reason: e.reason.clone(),
            backup,
            deployment_ids,
        };
        items.push(item(QuarantineOrigin::Backup, Some(rec), deployment_ids));
        if *mv {
            items.push(item(QuarantineOrigin::GameFile, None, Vec::new()));
        }
    }
    let manifest = QuarantineManifest {
        id: id.clone(),
        game: game.domain.clone(),
        created_at: now(),
        restored_at: None,
        items,
    };
    // The manifest is durable before anything leaves its place, so a crash
    // from here on is undone by restore_quarantine.
    write_manifest(&qdir, &manifest)?;

    for it in manifest
        .items
        .iter()
        .filter(|i| i.origin == QuarantineOrigin::GameFile)
    {
        let root = target_root(game, it.target)?;
        let c = confine(&root, &it.path, false).map_err(|e| confine_err(&root, &it.path, e))?;
        let dest = game_copy(&qdir, it)?;
        move_file(&c.path, &dest).map_err(io_err(&root.join(&it.path)))?;
        report.game_files += 1;
    }
    let tx = store.conn_mut().transaction()?;
    for e in &picked {
        tx.execute(
            "UPDATE deployments SET backup_id = NULL WHERE backup_id = ?1",
            [e.backup_id],
        )?;
        tx.execute("DELETE FROM backups WHERE id = ?1", [e.backup_id])?;
    }
    tx.commit()?;
    // The store copy goes only when no other held backup shares the blob;
    // the quarantine keeps its own copy.
    for b in &blobs {
        release_blob(store.conn(), b)?;
    }
    report.backups = picked.len();
    report.id = Some(id);
    report.dir = Some(qdir);
    tracing::info!(id = ?report.id, backups = report.backups, game_files = report.game_files, "quarantined poisoned backups");
    Ok(report)
}

/// Quarantines of `game`, oldest first.
pub fn list_quarantines(env: &DeployEnv) -> Result<Vec<QuarantineManifest>, DeployError> {
    let root = quarantine_root(env);
    let Ok(dirs) = fs::read_dir(&root) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for d in dirs.flatten() {
        if d.path().join(MANIFEST).is_file() {
            out.push(read_manifest(&d.path())?);
        }
    }
    out.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
    Ok(out)
}

fn check_id(id: &str) -> Result<(), DeployError> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(DeployError::Quarantine(format!(
            "invalid quarantine id '{id}'"
        )));
    }
    Ok(())
}

/// Held backup at the item's path: (id, sha256).
fn held_backup(
    conn: &Connection,
    domain: &str,
    it: &QuarantineItem,
) -> Result<Option<(i64, String)>, DeployError> {
    Ok(conn
        .query_row(
            "SELECT id, sha256 FROM backups
             WHERE game = ?1 AND target = ?2 AND rel_path = ?3 AND restored_at IS NULL",
            params![domain, it.target.as_str(), it.path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// Puts quarantine `id` of `game` back: blobs into the backup store, the
/// `backups` rows (and their links from still-present deployments) and the
/// game files. Checks every path first and changes nothing on a conflict.
/// Safe to re-run after a crash.
pub fn restore_quarantine(
    store: &mut Store,
    game: &GameConfig,
    env: &DeployEnv,
    id: &str,
    confirmed: bool,
) -> Result<RestoreReport, DeployError> {
    if !confirmed {
        return Err(DeployError::NotConfirmed);
    }
    check_id(id)?;
    let qdir = quarantine_root(env).join(id);
    if !qdir.join(MANIFEST).is_file() {
        return Err(DeployError::Quarantine(format!(
            "no quarantine '{id}' for {}",
            game.domain
        )));
    }
    let mut manifest = read_manifest(&qdir)?;
    if manifest.game != game.domain {
        return Err(DeployError::Quarantine(format!(
            "quarantine '{id}' belongs to {}, not {}",
            manifest.game, game.domain
        )));
    }
    if manifest.restored_at.is_some() {
        return Err(DeployError::Quarantine(format!(
            "quarantine '{id}' was already restored"
        )));
    }
    let _lock = guard(game, env)?;
    refuse_interrupted(store.conn(), game)?;
    let blobs = BlobStore {
        root: &env.backup_dir,
    };
    let conflict = |it: &QuarantineItem, why: &str| {
        DeployError::Quarantine(format!(
            "cannot restore quarantine '{id}': {} {why}; nothing was changed",
            it.path
        ))
    };

    // Preflight (read-only).
    for it in &manifest.items {
        let copy = game_copy(&qdir, it)?;
        match it.origin {
            QuarantineOrigin::Backup => {
                if it.backup.is_none() {
                    return Err(conflict(it, "has no backup record in the manifest"));
                }
                if held_backup(store.conn(), &game.domain, it)?.is_some_and(|(_, s)| s != it.sha256)
                {
                    return Err(conflict(it, "has a different backup held now"));
                }
                if !qdir.join("blobs").join(&it.sha256).is_file()
                    && !blobs.blob_path(&it.sha256).is_file()
                {
                    return Err(conflict(it, "is missing its backup blob"));
                }
            }
            QuarantineOrigin::GameFile => {
                if fs::symlink_metadata(&copy).is_err() {
                    continue; // never moved (interrupted quarantine)
                }
                let root = target_root(game, it.target)?;
                match confine(&root, &it.path, false) {
                    Ok(c) => match fs::symlink_metadata(&c.path) {
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(e) => return Err(io_err(&root.join(&it.path))(e)),
                        Ok(m)
                            if !m.is_dir()
                                && sha256_file(&c.path).is_ok_and(|s| s == it.sha256) => {}
                        Ok(_) => return Err(conflict(it, "is occupied in the game folder")),
                    },
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(confine_err(&root, &it.path, e)),
                }
            }
        }
    }

    let mut report = RestoreReport {
        id: id.to_string(),
        ..Default::default()
    };
    for it in &manifest.items {
        match it.origin {
            QuarantineOrigin::Backup => {
                let dest = blobs.blob_path(&it.sha256);
                if !dest.is_file() {
                    let src = qdir.join("blobs").join(&it.sha256);
                    let b = blobs.store(&src).map_err(io_err(&src))?;
                    if b.sha256 != it.sha256 {
                        return Err(DeployError::Quarantine(format!(
                            "quarantined blob {} is damaged (sha256 {})",
                            src.display(),
                            b.sha256
                        )));
                    }
                }
            }
            QuarantineOrigin::GameFile => {
                let copy = game_copy(&qdir, it)?;
                if fs::symlink_metadata(&copy).is_err() {
                    continue;
                }
                let root = target_root(game, it.target)?;
                let c =
                    confine(&root, &it.path, true).map_err(|e| confine_err(&root, &it.path, e))?;
                if fs::symlink_metadata(&c.path).is_err() {
                    move_file(&copy, &c.path).map_err(io_err(&root.join(&it.path)))?;
                }
                report.game_files += 1;
            }
        }
    }
    let tx = store.conn_mut().transaction()?;
    for it in &manifest.items {
        let Some(rec) = it
            .backup
            .as_ref()
            .filter(|_| it.origin == QuarantineOrigin::Backup)
        else {
            continue;
        };
        let backup_id = match held_backup(&tx, &game.domain, it)? {
            Some((existing, _)) => existing,
            None => {
                // Keep the old id unless a newer row took it.
                let free: bool = tx.query_row(
                    "SELECT NOT EXISTS (SELECT 1 FROM backups WHERE id = ?1)",
                    [rec.id],
                    |r| r.get(0),
                )?;
                tx.execute(
                    "INSERT INTO backups (id, game, target, rel_path, sha256, store_path, size, mode, mtime_ns, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        free.then_some(rec.id),
                        game.domain,
                        it.target.as_str(),
                        it.path,
                        it.sha256,
                        blobs.blob_path(&it.sha256).to_string_lossy(),
                        rec.size,
                        rec.mode,
                        rec.mtime_ns,
                        rec.created_at
                    ],
                )?;
                tx.last_insert_rowid()
            }
        };
        for d in &it.deployment_ids {
            tx.execute(
                "UPDATE deployments SET backup_id = ?2 WHERE id = ?1 AND game = ?3 AND backup_id IS NULL",
                params![d, backup_id, game.domain],
            )?;
        }
        report.backups += 1;
    }
    tx.commit()?;
    manifest.restored_at = Some(now());
    write_manifest(&qdir, &manifest)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ue4ss_files_are_recognised_case_insensitively() {
        for p in [
            "Pal/Binaries/Win64/ue4ss/Mods/Keybinds/Scripts/main.lua",
            "Pal/Binaries/Win64/Mods/BPModLoaderMod/enabled.txt",
            "Pal/Binaries/Win64/ue4ss/Mods/MODS.TXT",
            "Pal/Binaries/Win64/Mods/mods.json",
        ] {
            assert!(ue4ss_reason(p).is_some(), "{p}");
        }
        assert!(ue4ss_reason("Pal/Binaries/Win64/ue4ss/Mods/MyMod/Scripts/main.lua").is_none());
    }

    #[test]
    fn quarantine_ids_are_unique_and_validated() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, _) = new_quarantine_dir(tmp.path()).unwrap();
        let (b, _) = new_quarantine_dir(tmp.path()).unwrap();
        assert_ne!(a, b);
        check_id(&a).unwrap();
        check_id(&b).unwrap();
        for bad in ["", "..", "a/b", "../x"] {
            assert!(check_id(bad).is_err(), "{bad:?}");
        }
    }
}
