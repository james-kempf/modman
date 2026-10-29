// SPDX-License-Identifier: GPL-3.0-or-later
//! Journaled deploy, purge, recovery, verify and status.
//!
//! Deploy works in batches. For each batch: (A) originals about to be
//! replaced are copied into the backup store and fsynced; (B) one SQLite
//! transaction records the backups and a `place` journal step per file
//! (`done = 0`); (C) files are placed (temp file + atomic rename); (D) one
//! transaction records the `deployments` rows and marks the steps done.
//! Folders to create are recorded in `deployment_dirs` before they are made.
//! After a crash, [`recover`] undoes unfinished steps and every recorded
//! placement (rollback). An interrupted purge is simply run again (resume):
//! each entry is removed from `deployments` only after its file was handled.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use super::backup::{Blob, BlobStore};
use super::confine::{confine, is_escape, Confined};
use super::place::{is_ours, mtime_ns, place, tmp_path, Placed};
use super::plan::{build_plan, CaseResolver, Plan};
use super::provenance::{placed_by_vessel, untracked_placements};
use super::{
    io_err, target_root, DeployEnv, DeployError, DeployOptions, DeployReport, DeployStatus, Method,
    Progress, PurgeReport, RecoveryOutcome, RecoveryReport, VerifyIssue, VerifyKind, VerifyReport,
};
use crate::games::{self, GameConfig, Target};
use crate::store::{now, DeployLock, Store, StoreError};

const DEFAULT_BATCH: usize = 256;

/// Refuses while the game runs, then takes the deploy lock.
pub(super) fn guard(game: &GameConfig, env: &DeployEnv) -> Result<DeployLock, DeployError> {
    if env.probe.is_running(game) {
        return Err(DeployError::GameRunning(game.name.clone()));
    }
    DeployLock::try_acquire(&env.lock_path).map_err(|e| match e {
        StoreError::LockBusy(p) => DeployError::LockBusy(p),
        e => e.into(),
    })
}

fn target_of(s: &str) -> Target {
    Target::parse(s).unwrap_or_default()
}

/// [`confine`] with errors mapped for `rel` under `root`: a symlinked or
/// blocked folder is a [`DeployError::PathConflict`].
fn confined(root: &Path, rel: &str, create_parents: bool) -> Result<Confined, DeployError> {
    confine(root, rel, create_parents).map_err(|e| confine_err(root, rel, e))
}

pub(super) fn confine_err(root: &Path, rel: &str, e: io::Error) -> DeployError {
    if is_escape(&e) {
        DeployError::PathConflict {
            rel: rel.to_string(),
            reason: format!("refusing to write through a symbolic link or non-folder ({e})"),
        }
    } else {
        io_err(&root.join(rel))(e)
    }
}

/// Deploys the enabled mods of `game`. Recovers an interrupted run first and
/// removes an existing deployment (redeploy = purge + deploy). On failure the
/// partial deploy is rolled back before the error is returned.
pub fn deploy(
    store: &mut Store,
    game: &GameConfig,
    env: &DeployEnv,
    opts: &DeployOptions,
) -> Result<DeployReport, DeployError> {
    let root = target_root(game, game.target)?;
    let mut report = DeployReport {
        dry_run: opts.dry_run,
        method: opts.method,
        ..Default::default()
    };
    if opts.dry_run {
        let plan = build_plan(store.conn(), game)?;
        fill_plan_info(&mut report, &plan);
        dry_run(store.conn(), game, &root, &plan, &mut report)?;
        return Ok(report);
    }
    let _lock = guard(game, env)?;
    report.recovered = recover_locked(store, game, env)?;
    let plan = build_plan(store.conn(), game)?;
    fill_plan_info(&mut report, &plan);
    if games::deployment_active(store.conn(), &game.domain)? {
        report.purged = Some(purge_locked(store, game, env)?);
    }
    if plan.items.is_empty() {
        report.plugins = Some(plugin_stage(store, game, env, true));
        return Ok(report);
    }
    let pre = preflight(&root, &plan)?;
    match run_deploy(store, game, env, opts, &root, &plan, &pre, &mut report) {
        Ok(()) => {
            report.plugins = Some(plugin_stage(store, game, env, true));
            Ok(report)
        }
        Err(DeployError::InjectedCrash) => Err(DeployError::InjectedCrash),
        Err(e) => {
            if let Err(re) = recover_locked(store, game, env) {
                tracing::error!(error = %re, "rollback after failed deploy failed");
            }
            Err(e)
        }
    }
}

fn fill_plan_info(report: &mut DeployReport, plan: &Plan) {
    report.mods = plan.mods.len();
    report.skipped = plan.skipped.clone();
    report.conflicts = plan.conflicts();
}

struct PreItem {
    plan_idx: usize,
    /// Real spelling relative to the target root.
    rel: String,
    original: bool,
}

struct Pre {
    items: Vec<PreItem>,
    new_dirs: Vec<String>,
}

/// Resolves every plan path against the target (read-only). Path conflicts
/// (a file where a folder is needed or vice versa) fail before any change.
fn preflight(root: &Path, plan: &Plan) -> Result<Pre, DeployError> {
    let mut r = CaseResolver::new(root);
    let items = plan
        .items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let (rel, original) = r.resolve(&it.rel_path)?;
            Ok(PreItem {
                plan_idx: i,
                rel,
                original,
            })
        })
        .collect::<Result<_, DeployError>>()?;
    Ok(Pre {
        items,
        new_dirs: r.new_dirs,
    })
}

fn dry_run(
    conn: &Connection,
    game: &GameConfig,
    root: &Path,
    plan: &Plan,
    report: &mut DeployReport,
) -> Result<(), DeployError> {
    let pre = preflight(root, plan)?;
    // Files of the current deployment are not originals, unless they replaced
    // one (which a redeploy backs up again after restoring it).
    let mut stmt = conn.prepare(
        "SELECT rel_path, backup_id IS NOT NULL FROM deployments WHERE game = ?1 AND target = ?2",
    )?;
    let deployed: HashMap<String, bool> = stmt
        .query_map(params![game.domain, plan.target.as_str()], |r| {
            Ok((r.get::<_, String>(0)?.to_lowercase(), r.get(1)?))
        })?
        .collect::<Result<_, _>>()?;
    // A real deploy purges an active deployment first, which ends the
    // provenance of every earlier placement.
    let ours = if games::deployment_active(conn, &game.domain)? {
        Default::default()
    } else {
        untracked_placements(conn, &game.domain, plan.target.as_str())?
    };
    report.placed = pre.items.len();
    report.new_dirs = pre.new_dirs.len();
    report.backed_up = pre
        .items
        .iter()
        .filter(|p| p.original && *deployed.get(&p.rel.to_lowercase()).unwrap_or(&true))
        .filter(|p| !placed_by_vessel(&ours, &p.rel, &root.join(&p.rel)))
        .count();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_deploy(
    store: &mut Store,
    game: &GameConfig,
    env: &DeployEnv,
    opts: &DeployOptions,
    root: &Path,
    plan: &Plan,
    pre: &Pre,
    report: &mut DeployReport,
) -> Result<(), DeployError> {
    let target = plan.target.as_str();
    let chain = opts.method.chain();
    let blobs = BlobStore {
        root: &env.backup_dir,
    };
    let crash = |n: usize| env.faults.crash_after_files == Some(n);
    // Runs after the purge of the previous deployment, so only placements
    // whose tracking was lost remain (see `untracked_placements`).
    let ours = untracked_placements(store.conn(), &game.domain, target)?;

    let journal = {
        let tx = store.conn_mut().transaction()?;
        tx.execute(
            "INSERT INTO deploy_journal (game, op, method, started_at) VALUES (?1, 'deploy', ?2, ?3)",
            params![game.domain, opts.method.as_str(), now()],
        )?;
        let id = tx.last_insert_rowid();
        for d in &pre.new_dirs {
            tx.execute(
                "INSERT OR IGNORE INTO deployment_dirs (game, target, rel_path) VALUES (?1, ?2, ?3)",
                params![game.domain, target, d],
            )?;
        }
        tx.commit()?;
        id
    };
    for d in &pre.new_dirs {
        let c = confined(root, d, false)?;
        match fs::create_dir(&c.path) {
            Err(e) if e.kind() != io::ErrorKind::AlreadyExists => {
                return Err(io_err(&root.join(d))(e))
            }
            _ => {}
        }
    }
    report.new_dirs = pre.new_dirs.len();

    let total = pre.items.len();
    let mut placed = 0usize;
    let mut seq = 0i64;
    for chunk in pre
        .items
        .chunks(env.faults.batch_size.unwrap_or(DEFAULT_BATCH).max(1))
    {
        // (A) Backups are durable before any game file is replaced. A file
        // the journal says vessel placed is not an original: backing it up
        // would make purge "restore" mod bytes (a poisoned backup).
        let mut chunk_blobs = Vec::with_capacity(chunk.len());
        for p in chunk {
            chunk_blobs.push(if p.original {
                let c = confined(root, &p.rel, false)?;
                if placed_by_vessel(&ours, &p.rel, &c.path) {
                    tracing::warn!(path = %p.rel, "not backing up a file Vessel placed earlier (tracking was lost)");
                    None
                } else {
                    Some(blobs.store(&c.path).map_err(io_err(&root.join(&p.rel)))?)
                }
            } else {
                None
            });
        }
        // (B) Journal the batch.
        let mut steps = Vec::with_capacity(chunk.len());
        {
            let tx = store.conn_mut().transaction()?;
            for (p, b) in chunk.iter().zip(&chunk_blobs) {
                let backup_id = match b {
                    Some(b) => {
                        tx.execute(
                            "INSERT INTO backups (game, target, rel_path, sha256, store_path, size, mode, mtime_ns, created_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                            params![
                                game.domain,
                                target,
                                p.rel,
                                b.sha256,
                                b.path.to_string_lossy(),
                                b.size,
                                b.mode,
                                b.mtime_ns,
                                now()
                            ],
                        )?;
                        Some(tx.last_insert_rowid())
                    }
                    None => None,
                };
                let item = &plan.items[p.plan_idx];
                let mod_id = plan.mods[item.winner].id;
                let sha: Option<String> = tx
                    .query_row(
                        "SELECT sha256 FROM mod_files WHERE mod_id = ?1 AND path = ?2",
                        params![mod_id, item.staged_path],
                        |r| r.get(0),
                    )
                    .optional()?;
                seq += 1;
                // `sha256` is what was placed: provenance checks compare it.
                let detail = serde_json::json!({
                    "source": item.source.to_string_lossy(),
                    "mod_id": mod_id,
                    "backup_id": backup_id,
                    "sha256": sha,
                });
                tx.execute(
                    "INSERT INTO deploy_journal_steps (journal_id, seq, action, target, rel_path, detail)
                     VALUES (?1, ?2, 'place', ?3, ?4, ?5)",
                    params![journal, seq, target, p.rel, detail.to_string()],
                )?;
                steps.push((tx.last_insert_rowid(), backup_id, sha));
            }
            tx.commit()?;
        }
        // (C) Place.
        let mut rows = Vec::with_capacity(chunk.len());
        for p in chunk {
            if crash(placed) {
                return Err(DeployError::InjectedCrash);
            }
            let item = &plan.items[p.plan_idx];
            let abs = root.join(&p.rel);
            // Re-checked here: a folder swapped for a symlink since preflight
            // fails the deploy (and rolls it back) instead of escaping.
            let c = confined(root, &p.rel, false)?;
            let method =
                place(&item.source, &c.path, &abs, &chain, &env.faults).map_err(|source| {
                    DeployError::Place {
                        path: abs.clone(),
                        source,
                    }
                })?;
            let meta = fs::symlink_metadata(&c.path).map_err(io_err(&abs))?;
            rows.push((method, meta));
            placed += 1;
            *report.by_method.entry(method).or_default() += 1;
            if let Some(tx) = &opts.progress {
                let _ = tx.send(Progress {
                    done: placed,
                    total,
                });
            }
        }
        // (D) Record.
        let tx = store.conn_mut().transaction()?;
        for ((p, (method, meta)), (step, backup_id, sha)) in chunk.iter().zip(&rows).zip(&steps) {
            let item = &plan.items[p.plan_idx];
            let m = &plan.mods[item.winner];
            tx.execute(
                "INSERT INTO deployments (game, target, rel_path, mod_id, method, source, dev, inode, size, mtime_ns, sha256, backup_id, deployed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    game.domain,
                    target,
                    p.rel,
                    m.id,
                    method.as_str(),
                    item.source.to_string_lossy(),
                    meta.dev() as i64,
                    meta.ino() as i64,
                    meta.size() as i64,
                    mtime_ns(meta),
                    sha,
                    backup_id,
                    now()
                ],
            )?;
            tx.execute(
                "UPDATE deploy_journal_steps SET done = 1 WHERE id = ?1",
                [step],
            )?;
        }
        tx.commit()?;
        report.backed_up += steps.iter().filter(|(_, b, _)| b.is_some()).count();
    }
    // Shared files (UE4SS mods.txt) are merged inside the journal: a crash
    // before the commit below rolls them back with the placed files. The
    // crash point after the last placement (`crash_after_files` = all
    // files) therefore falls between the mods.txt write and the commit.
    let placed_rels: Vec<&str> = pre.items.iter().map(|p| p.rel.as_str()).collect();
    let merged = crate::loadorder::merge_shared_files(
        &env.game_data_dir(),
        game,
        root,
        &placed_rels,
        &plan.shared,
    )?;
    for m in merged {
        seq += 1;
        let detail = serde_json::to_string(&m).expect("merge record serializes");
        store.conn().execute(
            "INSERT INTO deploy_journal_steps (journal_id, seq, action, target, rel_path, detail, done)
             VALUES (?1, ?2, 'merge', ?3, ?4, ?5, 1)",
            params![journal, seq, target, m.rel_path, detail],
        )?;
    }
    if crash(placed) {
        return Err(DeployError::InjectedCrash);
    }
    store.conn().execute(
        "UPDATE deploy_journal SET state = 'committed', finished_at = ?2 WHERE id = ?1",
        params![journal, now()],
    )?;
    report.placed = placed;
    Ok(())
}

/// Removes the deployment of `game` and restores the original files
/// (legacy `cmd_purge`/`undeploy`, L1666-1736). Recovers an interrupted run
/// first.
pub fn purge(
    store: &mut Store,
    game: &GameConfig,
    env: &DeployEnv,
) -> Result<PurgeReport, DeployError> {
    let _lock = guard(game, env)?;
    let recovered = recover_locked(store, game, env)?;
    let mut report = purge_locked(store, game, env)?;
    if recovered.is_some() && report.nothing_deployed {
        report.nothing_deployed = false;
    }
    report.recovered = recovered.map(Box::new);
    report.plugins = Some(plugin_stage(store, game, env, false));
    Ok(report)
}

/// plugins.txt stage, run under the deploy lock after the journal committed
/// (legacy `plugins_update` L1883 / `plugins_remove_managed` L1734). A
/// failure here never undoes the deploy or purge; it is reported instead.
fn plugin_stage(
    store: &Store,
    game: &GameConfig,
    env: &DeployEnv,
    deployed: bool,
) -> crate::loadorder::PluginStage {
    use crate::loadorder::{remove_managed, update_plugins, PluginStage, PluginsFiles};
    let files = PluginsFiles::new(env.game_data_dir());
    let r = if deployed {
        update_plugins(store.conn(), game, &files)
    } else {
        remove_managed(game, &files)
    };
    r.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "plugins.txt stage failed");
        PluginStage::Failed(e.to_string())
    })
}

fn purge_locked(
    store: &mut Store,
    game: &GameConfig,
    env: &DeployEnv,
) -> Result<PurgeReport, DeployError> {
    let mut report = PurgeReport::default();
    if !games::deployment_active(store.conn(), &game.domain)? {
        report.nothing_deployed = true;
        return Ok(report);
    }
    store.conn().execute(
        "INSERT INTO deploy_journal (game, op, started_at) VALUES (?1, 'purge', ?2)",
        params![game.domain, now()],
    )?;
    let journal = store.conn().last_insert_rowid();
    undo_all(store, game, env, &mut report, env.faults.crash_after_files)?;
    store.conn().execute(
        "UPDATE deploy_journal SET state = 'committed', finished_at = ?2 WHERE id = ?1",
        params![journal, now()],
    )?;
    Ok(report)
}

/// Rolls back an interrupted deploy or finishes an interrupted purge of
/// `game`. `None` when nothing was interrupted. Takes the deploy lock and
/// refuses while the game runs.
pub fn recover(
    store: &mut Store,
    game: &GameConfig,
    env: &DeployEnv,
) -> Result<Option<RecoveryReport>, DeployError> {
    let _lock = guard(game, env)?;
    recover_locked(store, game, env)
}

fn recover_locked(
    store: &mut Store,
    game: &GameConfig,
    env: &DeployEnv,
) -> Result<Option<RecoveryReport>, DeployError> {
    let journals: Vec<(i64, String)> = {
        let mut stmt = store.conn().prepare(
            "SELECT id, op FROM deploy_journal WHERE game = ?1 AND state = 'started' ORDER BY id",
        )?;
        let rows = stmt.query_map([&game.domain], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut last = None;
    for (id, op) in journals {
        let mut undo = PurgeReport::default();
        let outcome = if op == "purge" {
            RecoveryOutcome::Resumed
        } else {
            undo_unfinished_steps(store, game, env, id, &mut undo)?;
            RecoveryOutcome::RolledBack
        };
        undo_all(store, game, env, &mut undo, None)?;
        let state = match outcome {
            RecoveryOutcome::Resumed => "committed",
            RecoveryOutcome::RolledBack => "rolled_back",
        };
        store.conn().execute(
            "UPDATE deploy_journal SET state = ?2, finished_at = ?3, error = 'interrupted; recovered' WHERE id = ?1",
            params![id, state, now()],
        )?;
        tracing::warn!(journal = id, op = %op, ?outcome, "recovered interrupted {op}");
        last = Some(RecoveryReport {
            journal_id: id,
            op,
            outcome,
            undo,
        });
    }
    if last.is_some() {
        sweep_orphan_blobs(store.conn(), &env.backup_dir)?;
    }
    Ok(last)
}

/// Deletes blobs no held backup (of any game) refers to, and leftover
/// `.incoming-*` temp files. A crash between storing a backup (step A) and
/// journaling it (step B) leaves such orphans. Runs under the deploy lock,
/// which is global, so no other deploy is between A and B.
fn sweep_orphan_blobs(conn: &Connection, backup_dir: &Path) -> Result<(), DeployError> {
    let Ok(top) = fs::read_dir(backup_dir) else {
        return Ok(());
    };
    let mut held = conn.prepare(
        "SELECT EXISTS (SELECT 1 FROM backups WHERE sha256 = ?1 AND restored_at IS NULL)",
    )?;
    for shard in top.flatten() {
        let name = shard.file_name();
        let name = name.to_string_lossy();
        let path = shard.path();
        if name.starts_with(".incoming-") {
            let _ = fs::remove_file(&path);
            continue;
        }
        let is_shard = name.len() == 2 && name.bytes().all(|b| b.is_ascii_hexdigit());
        if !is_shard || !shard.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        for blob in fs::read_dir(&path).map_err(io_err(&path))?.flatten() {
            let sha = blob.file_name().to_string_lossy().into_owned();
            if sha.len() != 64 || !sha.starts_with(name.as_ref()) {
                continue;
            }
            if !held.query_row([&sha], |r| r.get::<_, bool>(0))? {
                tracing::info!(blob = %sha, "removing orphaned backup blob");
                let _ = fs::remove_file(blob.path());
            }
        }
    }
    Ok(())
}

/// Undoes `place` steps that never got a `deployments` row (crash between
/// placing a file and recording it), newest first.
fn undo_unfinished_steps(
    store: &mut Store,
    game: &GameConfig,
    env: &DeployEnv,
    journal: i64,
    report: &mut PurgeReport,
) -> Result<(), DeployError> {
    let steps: Vec<(String, String, Option<String>)> = {
        let mut stmt = store.conn().prepare(
            "SELECT target, rel_path, detail FROM deploy_journal_steps
             WHERE journal_id = ?1 AND action = 'place' AND done = 0 ORDER BY seq DESC",
        )?;
        let rows = stmt.query_map([journal], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    for (target, rel, detail) in steps {
        let detail: serde_json::Value = detail
            .as_deref()
            .and_then(|d| serde_json::from_str(d).ok())
            .unwrap_or_default();
        let source = PathBuf::from(detail["source"].as_str().unwrap_or_default());
        let backup = match detail["backup_id"].as_i64() {
            Some(id) => load_backup(store.conn(), id)?,
            None => None,
        };
        let entry = Entry {
            target: target_of(&target),
            rel,
            deployment_id: None,
            method: None,
            source,
            inode: None,
            size: None,
            mtime_ns: None,
            backup,
        };
        undo_entry(store, game, env, &entry, report)?;
    }
    Ok(())
}

struct Entry {
    target: Target,
    rel: String,
    deployment_id: Option<i64>,
    method: Option<Method>,
    source: PathBuf,
    inode: Option<i64>,
    size: Option<i64>,
    mtime_ns: Option<i64>,
    backup: Option<(i64, Blob)>,
}

fn load_backup(conn: &Connection, id: i64) -> Result<Option<(i64, Blob)>, DeployError> {
    Ok(conn
        .query_row(
            "SELECT sha256, store_path, size, mode, mtime_ns FROM backups
             WHERE id = ?1 AND restored_at IS NULL",
            [id],
            |r| {
                Ok((
                    id,
                    Blob {
                        sha256: r.get(0)?,
                        path: PathBuf::from(r.get::<_, String>(1)?),
                        size: r.get(2)?,
                        mode: r.get(3)?,
                        mtime_ns: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    },
                ))
            },
        )
        .optional()?)
}

/// Moves a changed file out of the game into `dest` (legacy `overwrite/`).
fn move_out(src: &Path, dest: &Path) -> io::Result<()> {
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    if fs::symlink_metadata(dest).is_ok() {
        fs::remove_file(dest)?;
    }
    match fs::rename(src, dest) {
        Err(e) if e.raw_os_error() == Some(rustix::io::Errno::XDEV.raw_os_error()) => {
            fs::copy(src, dest)?;
            fs::remove_file(src)
        }
        r => r,
    }
}

/// Deletes a blob once no held backup refers to it.
pub(super) fn release_blob(conn: &Connection, blob: &Blob) -> Result<(), DeployError> {
    let held: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM backups WHERE sha256 = ?1 AND restored_at IS NULL)",
        [&blob.sha256],
        |r| r.get(0),
    )?;
    if !held {
        match fs::remove_file(&blob.path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(io_err(&blob.path)(e)),
            _ => {}
        }
    }
    Ok(())
}

/// Undoes one placement (legacy undeploy step 1 + restore, L1679-1724):
/// - target already holds the original (purge crashed after restoring): keep;
/// - target is still our placement: delete it;
/// - recorded placement replaced by the game while backed up: keep the game's
///   version, retire the backup to `overwrite/.stale-backups/`;
/// - otherwise the changed file moves to `overwrite/`.
///
/// Then a held backup is restored if the path is free. The database is
/// updated only after the file system, so re-running is safe.
fn undo_entry(
    store: &mut Store,
    game: &GameConfig,
    env: &DeployEnv,
    e: &Entry,
    report: &mut PurgeReport,
) -> Result<(), DeployError> {
    let root = target_root(game, e.target)?;
    let shown = root.join(&e.rel);
    let blobs = BlobStore {
        root: &env.backup_dir,
    };
    let recorded = e.deployment_id.is_some();
    // A missing parent folder means the file is gone too.
    let c = match confine(&root, &e.rel, false) {
        Ok(c) => Some(c),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) if is_escape(&err) => {
            // A folder on the path became a symlink (or file) after deploy:
            // nothing of ours can be there, and nothing is written through
            // it. A held backup stays held (and the deployment active).
            tracing::warn!(path = %shown.display(), error = %err, "leaving path alone: a folder on it is no longer a real folder");
            if e.backup.is_some() {
                report.kept += 1;
            } else if recorded {
                report.missing += 1;
            }
            if let Some(id) = e.deployment_id {
                store
                    .conn()
                    .execute("DELETE FROM deployments WHERE id = ?1", [id])?;
            }
            return Ok(());
        }
        Err(err) => return Err(confine_err(&root, &e.rel, err)),
    };
    let abs = c.as_ref().map(|c| c.path.as_path());
    if let Some(abs) = abs {
        let _ = fs::remove_file(tmp_path(abs));
    }
    let mut restore = e.backup.as_ref();
    match abs.map(fs::symlink_metadata) {
        None => {
            if recorded {
                report.missing += 1;
            }
        }
        Some(Err(err)) if err.kind() == io::ErrorKind::NotFound => {
            if recorded {
                report.missing += 1;
            }
        }
        Some(Err(err)) => return Err(io_err(&shown)(err)),
        Some(Ok(meta)) => {
            let abs = abs.expect("metadata came from a path");
            let placed = Placed {
                method: e.method,
                source: &e.source,
                inode: e.inode,
                size: e.size,
                mtime_ns: e.mtime_ns,
            };
            // Ours is checked first: a placement byte-identical to the
            // original (but with the mod's mode, or hardlinked to staging)
            // must still be replaced by the real original.
            if is_ours(&placed, abs, &meta) {
                fs::remove_file(abs).map_err(io_err(&shown))?;
                report.removed += 1;
            } else if e
                .backup
                .as_ref()
                .is_some_and(|(_, b)| blobs.matches(b, abs, &meta))
            {
                restore = None;
                if recorded {
                    report.restored += 1;
                }
            } else if let (Some((_, b)), true) = (&e.backup, recorded) {
                let dest = env.overwrite_dir.join(".stale-backups").join(&e.rel);
                blobs.copy_out(b, &dest).map_err(io_err(&dest))?;
                restore = None;
                report.changed += 1;
            } else {
                move_out(abs, &env.overwrite_dir.join(&e.rel)).map_err(io_err(&shown))?;
                report.changed += 1;
            }
        }
    }
    if let Some((_, b)) = restore {
        let c = confined(&root, &e.rel, true)?;
        blobs.restore(b, &c.path).map_err(io_err(&shown))?;
        report.restored += 1;
    }
    let tx = store.conn_mut().transaction()?;
    if let Some(id) = e.deployment_id {
        tx.execute("DELETE FROM deployments WHERE id = ?1", [id])?;
    }
    if let Some((id, _)) = &e.backup {
        tx.execute(
            "UPDATE backups SET restored_at = ?2 WHERE id = ?1",
            params![id, now()],
        )?;
    }
    tx.commit()?;
    if let Some((_, b)) = &e.backup {
        release_blob(store.conn(), b)?;
    }
    Ok(())
}

/// Undoes every recorded placement (newest first), restores remaining held
/// backups whose path is free, and removes created folders deepest first.
fn undo_all(
    store: &mut Store,
    game: &GameConfig,
    env: &DeployEnv,
    report: &mut PurgeReport,
    crash_after: Option<usize>,
) -> Result<(), DeployError> {
    type Row = (
        i64,
        String,
        String,
        String,
        String,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    );
    let rows: Vec<Row> = {
        let mut stmt = store.conn().prepare(
            "SELECT id, target, rel_path, method, source, inode, size, mtime_ns, backup_id
             FROM deployments WHERE game = ?1 ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([&game.domain], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        })?;
        rows.collect::<Result<_, _>>()?
    };
    for (n, (id, target, rel, method, source, inode, size, mtime, backup_id)) in
        rows.into_iter().enumerate()
    {
        if crash_after == Some(n) {
            return Err(DeployError::InjectedCrash);
        }
        let backup = match backup_id {
            Some(b) => load_backup(store.conn(), b)?,
            None => None,
        };
        let entry = Entry {
            target: target_of(&target),
            rel,
            deployment_id: Some(id),
            method: Method::parse(&method),
            source: PathBuf::from(source),
            inode,
            size,
            mtime_ns: mtime,
            backup,
        };
        undo_entry(store, game, env, &entry, report)?;
    }

    // Held backups with no deployment row left (legacy backup.list pass).
    let held: Vec<(i64, String, String)> = {
        let mut stmt = store.conn().prepare(
            "SELECT id, target, rel_path FROM backups WHERE game = ?1 AND restored_at IS NULL ORDER BY id",
        )?;
        let rows = stmt.query_map([&game.domain], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let blobs = BlobStore {
        root: &env.backup_dir,
    };
    for (id, target, rel) in held {
        let Some((_, b)) = load_backup(store.conn(), id)? else {
            continue;
        };
        let root = target_root(game, target_of(&target))?;
        let shown = root.join(&rel);
        let c = match confine(&root, &rel, true) {
            Ok(c) => c,
            Err(err) if is_escape(&err) => {
                report.kept += 1;
                continue;
            }
            Err(err) => return Err(confine_err(&root, &rel, err)),
        };
        match fs::symlink_metadata(&c.path) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                blobs.restore(&b, &c.path).map_err(io_err(&shown))?;
                report.restored += 1;
            }
            Err(err) => return Err(io_err(&shown)(err)),
            Ok(meta) if blobs.matches(&b, &c.path, &meta) => {}
            Ok(_) => {
                report.kept += 1;
                continue;
            }
        }
        store.conn().execute(
            "UPDATE backups SET restored_at = ?2 WHERE id = ?1",
            params![id, now()],
        )?;
        release_blob(store.conn(), &b)?;
    }

    // Shared files (UE4SS mods.txt) before the folders: one vessel created
    // is removed, so its folder can go too.
    if let Ok(root) = target_root(game, game.target) {
        crate::loadorder::revert_shared_files(&env.game_data_dir(), &root)?;
    }

    // Created folders, deepest first; non-empty ones stay (legacy L1727-1732).
    let dirs: Vec<(String, String)> = {
        let mut stmt = store.conn().prepare(
            "SELECT target, rel_path FROM deployment_dirs WHERE game = ?1 ORDER BY rel_path DESC",
        )?;
        let rows = stmt.query_map([&game.domain], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    for (target, rel) in dirs {
        if let Ok(root) = target_root(game, target_of(&target)) {
            let removed = confine(&root, &rel, false).and_then(|c| fs::remove_dir(&c.path));
            if let Err(e) = removed {
                tracing::debug!(dir = %rel, error = %e, "kept folder");
            }
        }
    }
    store.conn().execute(
        "DELETE FROM deployment_dirs WHERE game = ?1",
        [&game.domain],
    )?;
    Ok(())
}

pub(super) fn interrupted(conn: &Connection, domain: &str) -> Result<bool, DeployError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM deploy_journal WHERE game = ?1 AND state = 'started')",
        [domain],
        |r| r.get(0),
    )?)
}

/// Checks every deployed file (legacy `cmd_verify`, L1949-1991): hardlinks by
/// inode, size and mtime; copies and reflinks by size and mtime; symlinks by
/// link target, which must still resolve. Read-only; takes no lock.
pub fn verify(conn: &Connection, game: &GameConfig) -> Result<VerifyReport, DeployError> {
    let mut report = VerifyReport {
        interrupted: interrupted(conn, &game.domain)?,
        ..Default::default()
    };
    let mut stmt = conn.prepare(
        "SELECT target, rel_path, method, source, inode, size, mtime_ns FROM deployments
         WHERE game = ?1 ORDER BY rel_path",
    )?;
    let rows = stmt.query_map([&game.domain], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, Option<i64>>(5)?,
            r.get::<_, Option<i64>>(6)?,
        ))
    })?;
    for row in rows {
        let (target, rel, method, source, inode, size, mtime) = row?;
        let target = target_of(&target);
        let method = Method::parse(&method).unwrap_or(Method::Copy);
        let abs = target_root(game, target)?.join(&rel);
        let kind = match fs::symlink_metadata(&abs) {
            Err(_) => Some(VerifyKind::Missing),
            Ok(meta) => match method {
                Method::Symlink => {
                    if !meta.is_symlink()
                        || fs::read_link(&abs).ok().as_deref() != Some(Path::new(&source))
                    {
                        Some(VerifyKind::Modified)
                    } else if !abs.exists() {
                        Some(VerifyKind::Missing)
                    } else {
                        None
                    }
                }
                _ => {
                    let same = meta.is_file()
                        && size.is_none_or(|s| meta.size() as i64 == s)
                        && mtime.is_none_or(|t| mtime_ns(&meta) == t)
                        && (method != Method::Hardlink
                            || inode.is_none_or(|i| meta.ino() as i64 == i));
                    (!same).then_some(VerifyKind::Modified)
                }
            },
        };
        match kind {
            None => report.intact += 1,
            Some(kind) => report.issues.push(VerifyIssue {
                target,
                rel_path: rel,
                method,
                kind,
            }),
        }
    }
    Ok(report)
}

/// Deployment state of a game (for `status`).
pub fn status(conn: &Connection, domain: &str) -> Result<DeployStatus, DeployError> {
    let mut s = DeployStatus {
        active: games::deployment_active(conn, domain)?,
        interrupted: interrupted(conn, domain)?,
        ..Default::default()
    };
    let mut stmt =
        conn.prepare("SELECT method, COUNT(*) FROM deployments WHERE game = ?1 GROUP BY method")?;
    for row in stmt.query_map([domain], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })? {
        let (m, n) = row?;
        if let Some(m) = Method::parse(&m) {
            s.by_method.insert(m, n as usize);
        }
        s.files += n as usize;
    }
    let one = |sql: &str| -> Result<i64, DeployError> {
        Ok(conn.query_row(sql, [domain], |r| r.get(0))?)
    };
    s.backups_held =
        one("SELECT COUNT(*) FROM backups WHERE game = ?1 AND restored_at IS NULL")? as usize;
    s.dirs = one("SELECT COUNT(*) FROM deployment_dirs WHERE game = ?1")? as usize;
    s.last_deployed_at = conn.query_row(
        "SELECT MAX(deployed_at) FROM deployments WHERE game = ?1",
        [domain],
        |r| r.get(0),
    )?;
    s.enabled_mods = one("SELECT COUNT(*) FROM mods WHERE game = ?1 AND enabled = 1")? as usize;
    s.deployed_mods = one(
        "SELECT COUNT(*) FROM mods m WHERE m.game = ?1 AND m.enabled = 1 \
         AND EXISTS (SELECT 1 FROM deployments d WHERE d.game = m.game AND d.mod_id = m.id)",
    )? as usize;
    Ok(s)
}
