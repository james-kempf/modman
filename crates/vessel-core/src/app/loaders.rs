// SPDX-License-Identifier: GPL-3.0-or-later
//! Loader orchestrator: preview -> consent -> phase-tracked run, for every
//! loader op (install, upgrade, repair, uninstall, adopt) and rollback,
//! plus startup recovery. Rust plugins (SMAPI) and declarative
//! `[[loaders]]` go through the same path.
//!
//! - [`preview`] caches the archive under `<data>/loaders/archives/<sha>/`,
//!   extracts it into a 0700 scratch dir, sets exec bits on installer
//!   targets, makes the scratch read-only, hashes it and the plan into a
//!   `plan_hash`, and stores a preview record. Nothing in the game folder
//!   changes and nothing runs.
//! - [`run`] re-checks the token (id, hash, expiry, `unverified_ack`), the
//!   scratch manifest and the cached archive, runs preflight, takes the
//!   deploy lock and then moves the run through `backing_up` (scoped
//!   backup, pre-manifest persisted) -> `spawned` -> `diffing` -> `done`.
//!   Failures roll this run's changes back.
//! - [`recover`] finishes what a crash left (it never starts an installer).
//!
//! Installer runs block the calling thread until the child is gone, which
//! is what the runner's PDEATHSIG contract needs.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::R;
use crate::deploy::confine;
use crate::deploy::place::{hex, sha256_file};
use crate::games::{definitions, steam, GameConfig};
use crate::loaders::launch::compose;
use crate::loaders::records::{
    FileChange, InstallStatus, LoaderFile, LoaderInstall, NewLoaderInstall, NewLoaderRun, RunPhase,
    Strategy,
};
use crate::loaders::runner::{self, ProcessRunner, RealRunner, RunRequest};
use crate::loaders::runtime::{self, GameFacts, PreflightInput, SteamEnv};
use crate::loaders::snapshot::{self, BackupRef, Entry, Manifest};
use crate::loaders::{
    classify_vanilla, ArchiveProbe, DownloadOrigin, GameCtx, Glob, LaunchChange, LoaderError,
    LoaderMatch, LoaderPlan, LoaderPlugin, LoaderRegistry, LoaderStatus, Op, Provenance, Runtime,
    Step, TargetKind, VanillaState,
};
use crate::paths::Paths;
use crate::store::{now, DeployLock, Store, StoreError};

/// Shown with every preview.
pub const PERMISSIONS_NOTE: &str =
    "The installer runs with your user permissions. Vessel limits its \
     environment, working folder and run time, and backs up and tracks the game files it may \
     change, but it is not sandboxed.";
/// How long a preview stays valid.
pub const PREVIEW_TTL: Duration = Duration::from_secs(600);

/// Where the orchestrator keeps its files and how it runs things.
#[derive(Clone)]
pub struct LoaderEnv {
    /// The `data-dir` setting; loader files live in `<data_dir>/loaders`.
    pub data_dir: PathBuf,
    pub lock_path: PathBuf,
    /// The deploy engine's backup store (shared, so its GC sees our roots).
    pub backup_dir: PathBuf,
    pub home: PathBuf,
    pub definitions_dir: Option<PathBuf>,
    pub steam: SteamEnv,
    pub runner: Arc<dyn ProcessRunner>,
    pub proc_root: PathBuf,
    pub fs_root: PathBuf,
    pub timeout: Duration,
    pub preview_ttl: Duration,
    /// Skips runtime detection (an explicit override, or tests).
    pub forced_runtime: Option<Runtime>,
    /// Test-only: return an error once this phase is reached, without
    /// cleaning up, as a crash would.
    #[doc(hidden)]
    pub crash_at: Option<RunPhase>,
}

impl LoaderEnv {
    pub fn new(paths: &Paths, data_dir: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            lock_path: paths.deploy_lock_path(),
            backup_dir: data_dir.join("backups"),
            home: paths.home.clone(),
            definitions_dir: Some(paths.definitions_dir()),
            steam: SteamEnv::discover(&paths.home),
            runner: Arc::new(RealRunner::default()),
            proc_root: PathBuf::from("/proc"),
            fs_root: PathBuf::from("/"),
            timeout: runner::DEFAULT_TIMEOUT,
            preview_ttl: PREVIEW_TTL,
            forced_runtime: None,
            crash_at: None,
        }
    }

    fn dir(&self, sub: &str) -> PathBuf {
        self.data_dir.join("loaders").join(sub)
    }
}

/// What to preview.
#[derive(Debug, Clone)]
pub struct PreviewRequest {
    pub game_id: String,
    pub op: Op,
    /// Loader to act on; required unless an archive names it.
    pub loader_key: Option<String>,
    /// New archive (install, upgrade, adopt; optional for repair, where
    /// the cached archive is used otherwise).
    pub archive: Option<PathBuf>,
    /// Where `archive` came from.
    pub origin: DownloadOrigin,
    pub unverified_ack: bool,
}

/// One process a run will start, as the user should see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    /// Env names and values, as the run sets them (same `TMPDIR` and `PATH`).
    pub env: Vec<(String, String)>,
    pub cmd_line: Option<String>,
}

/// A consentable preview.
#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub preview_id: String,
    pub plan_hash: String,
    pub loader_key: String,
    pub op: Op,
    pub version: Option<String>,
    pub runtime: Runtime,
    pub plan: LoaderPlan,
    pub commands: Vec<PlannedCommand>,
    pub touch_scope: Vec<String>,
    pub external_side_effects: Vec<String>,
    /// `None` when no archive is involved (declarative uninstall).
    pub provenance: Option<Provenance>,
    pub warnings: Vec<String>,
    pub permissions: &'static str,
    pub expires_at: i64,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RunOutcome {
    Succeeded,
    /// The op failed; its changes were rolled back.
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub install_id: i64,
    pub run_id: i64,
    pub outcome: RunOutcome,
    pub status: String,
    pub version: Option<String>,
    /// Folder holding the run's logs (kept on failure too).
    pub log_dir: PathBuf,
    pub warnings: Vec<String>,
    /// Launch options the game now needs (`None` = none).
    pub launch_desired: Option<String>,
}

/// What startup recovery did with one run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RecoveryAction {
    /// Crashed while backing up: the game was not touched; nothing to roll back.
    Discarded,
    /// Crashed after changes started: they are recorded and a rollback
    /// is offered ([`rollback`] or [`decline_rollback`]).
    RollbackOffered { killed: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recovered {
    pub run_id: i64,
    pub install_id: i64,
    pub action: RecoveryAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PreviewRecord {
    id: String,
    game_id: String,
    op: Op,
    loader_key: String,
    plan: LoaderPlan,
    runtime: Runtime,
    install_id: Option<i64>,
    version: Option<String>,
    archive_sha256: Option<String>,
    archive_path: Option<PathBuf>,
    /// Whether the archive is the install's cached one (re-checked
    /// against `loader_installs.archive_sha256`).
    cached_archive: bool,
    origin: DownloadOrigin,
    scratch: Option<PathBuf>,
    scratch_hash: String,
    provenance: Option<Provenance>,
    unverified_ack: bool,
    /// Game folder the plan was built for; the run refuses another one.
    #[serde(default)]
    game_dir: PathBuf,
    /// The caller `PATH` the shown commands were built from; the run
    /// builds its commands from the same value.
    #[serde(default)]
    caller_path: Option<OsString>,
    plan_hash: String,
    created_at: i64,
}

impl PreviewRecord {
    fn compute_hash(&self) -> String {
        let mut h = Sha256::new();
        let plan = serde_json::to_vec(&self.plan).unwrap_or_default();
        let rt = serde_json::to_vec(&self.runtime).unwrap_or_default();
        for part in [
            plan.as_slice(),
            rt.as_slice(),
            self.archive_sha256.as_deref().unwrap_or("").as_bytes(),
            self.scratch_hash.as_bytes(),
            if self.unverified_ack {
                b"ack:1"
            } else {
                b"ack:0"
            },
            self.op.as_str().as_bytes(),
            self.game_id.as_bytes(),
            self.loader_key.as_bytes(),
            self.game_dir.as_os_str().as_encoded_bytes(),
            self.caller_path
                .as_deref()
                .map_or(b"path:none".as_slice(), OsStr::as_encoded_bytes),
        ] {
            h.update((part.len() as u64).to_le_bytes());
            h.update(part);
        }
        hex(&h.finalize())
    }
}

fn msg(s: impl Into<String>) -> super::AppError {
    super::AppError::Msg(s.into())
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> super::AppError + '_ {
    move |e| {
        LoaderError::Io {
            path: path.to_path_buf(),
            source: e,
        }
        .into()
    }
}

fn new_id() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let mut h = Sha256::new();
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    h.update(t.to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    h.update(N.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    hex(&h.finalize())[..20].to_string()
}

fn mkdir_private(p: &Path) -> R<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(p)
        .map_err(io(p))
}

fn write_private(path: &Path, bytes: &[u8]) -> R<()> {
    if let Some(d) = path.parent() {
        mkdir_private(d)?;
    }
    let mut f = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(io(path))?;
    std::io::Write::write_all(&mut f, bytes).map_err(io(path))?;
    f.sync_all().map_err(io(path))
}

/// The game folder and loader context.
fn game_ctx(store: &Store, game: &GameConfig, key: &str) -> R<GameCtx> {
    let dir = game
        .folder
        .clone()
        .ok_or_else(|| msg(format!("Game '{}' has no install folder set.", game.id)))?;
    Ok(GameCtx {
        game_id: game.id.clone(),
        nexus_domain: game.nexus_domain.clone(),
        game_dir: dir,
        steam_appid: game.appid,
        occupied: store.occupied_paths(&game.id, key)?,
    })
}

/// The loaders available for `game`.
pub fn registry_for(env: &LoaderEnv, game: &GameConfig) -> R<LoaderRegistry> {
    let defs = definitions::load_definitions(env.definitions_dir.as_deref())?;
    let def = game
        .definition
        .as_deref()
        .and_then(|d| definitions::find_definition(&defs, d));
    Ok(LoaderRegistry::for_game(def, game.definition.as_deref())?)
}

fn runtime_override(store: &Store, game_id: &str) -> R<Option<String>> {
    Ok(store.conn().query_row(
        "SELECT runtime_override FROM games WHERE id = ?1",
        [game_id],
        |r| r.get(0),
    )?)
}

/// The game's runtime (override, Steam mapping, file sniff).
pub fn game_runtime(env: &LoaderEnv, store: &Store, game: &GameConfig) -> R<Runtime> {
    if let Some(rt) = &env.forced_runtime {
        return Ok(rt.clone());
    }
    let dir = game.folder.as_deref().unwrap_or(Path::new("/nonexistent"));
    let ovr = runtime_override(store, &game.id)?;
    Ok(runtime::detect(
        GameFacts {
            game_dir: dir,
            appid: game.appid,
            runtime_override: ovr.as_deref(),
            native_markers: &[],
        },
        &env.steam,
    )?)
}

fn steam_app(env: &LoaderEnv, game: &GameConfig) -> Option<steam::SteamApp> {
    game.appid
        .and_then(|id| steam::find_app(&env.steam.libraries, id))
}

fn preflight(env: &LoaderEnv, game: &GameConfig, ctx: &GameCtx, rt: &Runtime) -> R<Vec<String>> {
    Ok(runtime::preflight(PreflightInput {
        game_dir: &ctx.game_dir,
        runtime: rt,
        state_flags: steam_app(env, game).and_then(|a| a.state_flags),
        proc_root: &env.proc_root,
        fs_root: &env.fs_root,
    })?)
}

/// Hash of every entry under `dir` (path, type, permission bits, size,
/// content or link target).
fn tree_hash(dir: &Path) -> R<String> {
    let mut h = Sha256::new();
    for e in walkdir::WalkDir::new(dir)
        .min_depth(1)
        .follow_links(false)
        .sort_by_file_name()
    {
        let e = e.map_err(|e| msg(format!("cannot read {}: {e}", dir.display())))?;
        let rel = e.path().strip_prefix(dir).expect("under dir");
        let m = fs::symlink_metadata(e.path()).map_err(io(e.path()))?;
        let body = if m.file_type().is_symlink() {
            format!(
                "link:{}",
                fs::read_link(e.path()).map_err(io(e.path()))?.display()
            )
        } else if m.is_file() {
            sha256_file(e.path()).map_err(io(e.path()))?
        } else {
            "dir".into()
        };
        h.update(
            format!(
                "{}\0{:o}\0{}\0{body}\n",
                rel.display(),
                m.permissions().mode() & 0o7777,
                if m.is_file() { m.len() } else { 0 }
            )
            .as_bytes(),
        );
    }
    Ok(hex(&h.finalize()))
}

fn set_tree_writable(dir: &Path, writable: bool) {
    let walk = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .contents_first(!writable);
    for e in walk.into_iter().flatten() {
        if e.file_type().is_symlink() {
            continue;
        }
        let Ok(m) = e.metadata() else { continue };
        let mode = m.permissions().mode() & 0o777;
        let new = if writable { mode | 0o200 } else { mode & 0o555 };
        let _ = fs::set_permissions(e.path(), fs::Permissions::from_mode(new));
    }
}

const ARCHIVE_CHANGED: &str = "the cached loader archive changed";

/// Copies `src` to `<archives>/<sha>/<name>` (once) and records its origin.
/// The copy is hashed (not `src`), so the sha, the cached bytes and the
/// origin always agree even if `src` changes meanwhile. With `expected`,
/// bytes of any other hash are refused before the origin is attached.
fn cache_archive(
    env: &LoaderEnv,
    src: &Path,
    origin: &DownloadOrigin,
    expected: Option<&str>,
) -> R<(String, PathBuf)> {
    let name = src
        .file_name()
        .map_or_else(|| "archive".into(), |n| n.to_string_lossy().into_owned());
    let root = env.dir("archives");
    mkdir_private(&root)?;
    let tmp = root.join(format!(".{}.tmp", new_id()));
    let changed = || -> super::AppError { LoaderError::Consent(ARCHIVE_CHANGED.into()).into() };
    if let Err(e) = fs::copy(src, &tmp) {
        let _ = fs::remove_file(&tmp);
        // A vanished expected archive has changed too.
        return Err(if expected.is_some() {
            changed()
        } else {
            io(&tmp)(e)
        });
    }
    let sha = match sha256_file(&tmp) {
        Ok(sha) if expected.is_none_or(|e| e == sha) => sha,
        Ok(_) => {
            let _ = fs::remove_file(&tmp);
            return Err(changed());
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            return Err(io(&tmp)(e));
        }
    };
    let dir = root.join(&sha);
    mkdir_private(&dir)?;
    let dest = dir.join(&name);
    if sha256_file(&dest).ok().as_deref() == Some(sha.as_str()) {
        let _ = fs::remove_file(&tmp);
    } else {
        fs::rename(&tmp, &dest).map_err(io(&dest))?;
    }
    let origin_file = dir.join("origin.json");
    if !origin_file.exists() {
        write_private(
            &origin_file,
            &serde_json::to_vec(origin).unwrap_or_default(),
        )?;
    }
    Ok((sha, dest))
}

fn cached_origin(path: &Path) -> DownloadOrigin {
    path.parent()
        .and_then(|d| fs::read(d.join("origin.json")).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| DownloadOrigin::Local {
            path: path.to_path_buf(),
        })
}

fn extract_to_scratch(env: &LoaderEnv, id: &str, archive: &Path) -> R<PathBuf> {
    let scratch = env.dir("scratch").join(id);
    mkdir_private(&scratch)?;
    crate::archive::extract(archive, &scratch)?;
    Ok(scratch)
}

fn probe_of(scratch: &Path, file_name: &str, origin: &DownloadOrigin) -> ArchiveProbe {
    let (mod_id, file_id) = match origin {
        DownloadOrigin::Nexus {
            mod_id, file_id, ..
        } => (Some(*mod_id), Some(*file_id)),
        _ => (None, None),
    };
    ArchiveProbe {
        file_name: file_name.to_string(),
        nexus_mod_id: mod_id,
        nexus_file_id: file_id,
        entries: walkdir::WalkDir::new(scratch)
            .min_depth(1)
            .follow_links(false)
            .into_iter()
            .flatten()
            .filter_map(|e| {
                e.path()
                    .strip_prefix(scratch)
                    .ok()
                    .and_then(Path::to_str)
                    .map(str::to_string)
            })
            .collect(),
    }
}

/// Verified cached archive of an install.
fn cached_archive(install: &LoaderInstall) -> R<(String, PathBuf)> {
    let (Some(sha), Some(path)) = (&install.archive_sha256, &install.archive_path) else {
        return Err(msg("This loader install has no cached archive."));
    };
    let path = PathBuf::from(path);
    if sha256_file(&path).ok().as_deref() != Some(sha.as_str()) {
        return Err(LoaderError::Consent("the cached loader archive changed".into()).into());
    }
    Ok((sha.clone(), path))
}

/// The private `TMPDIR` of a preview's installer steps. Derived from the
/// preview id alone, so the consent dialog shows the path the run uses.
fn installer_tmp(env: &LoaderEnv, preview_id: &str) -> PathBuf {
    env.dir("tmp").join(format!("preview-{preview_id}"))
}

fn run_request<'a>(
    env: &'a LoaderEnv,
    spec: &'a crate::loaders::InstallerSpec,
    scratch: &'a Path,
    game_dir: &'a Path,
    log: &'a Path,
    tmp: Option<&'a Path>,
    caller_path: Option<&'a std::ffi::OsStr>,
) -> RunRequest<'a> {
    RunRequest {
        spec,
        scratch,
        game_dir,
        home: &env.home,
        lang: None,
        caller_path,
        timeout: env.timeout,
        log_path: log,
        cancel: None,
        tmp_dir: tmp,
    }
}

/// Previews `req`. Nothing in the game folder changes.
pub fn preview(env: &LoaderEnv, store: &Store, req: &PreviewRequest) -> R<Preview> {
    preview_expecting(env, store, req, None)
}

/// [`preview`], refusing a `req.archive` whose bytes do not hash to
/// `expected_sha` (a reopened preview's origin belongs to those bytes only).
fn preview_expecting(
    env: &LoaderEnv,
    store: &Store,
    req: &PreviewRequest,
    expected_sha: Option<&str>,
) -> R<Preview> {
    let game = crate::games::require_game(store.conn(), &req.game_id)?;
    let registry = registry_for(env, &game)?;
    let rt = game_runtime(env, store, &game)?;
    if matches!(rt, Runtime::Unknown) {
        return Err(LoaderError::UnknownRuntime(
            "set the game's runtime to native or Proton".into(),
        )
        .into());
    }
    let id = new_id();
    let mut archive: Option<(String, PathBuf)> = None;
    let mut scratch: Option<PathBuf> = None;
    let mut cached = false;
    let mut origin = req.origin.clone();
    let mut matched: Option<LoaderMatch> = None;

    // Which loader, and with which archive.
    let key = if let Some(src) = &req.archive {
        let (sha, path) = cache_archive(env, src, &req.origin, expected_sha)?;
        let s = extract_to_scratch(env, &id, &path)?;
        let file_name = src
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let probe = probe_of(&s, &file_name, &req.origin);
        let ctx0 = game_ctx(store, &game, "")?;
        let Some((plugin, m)) = registry.match_archive(&ctx0, &probe)? else {
            return Err(msg(
                "This archive is not a loader Vessel knows for this game.",
            ));
        };
        if req.loader_key.as_deref().is_some_and(|k| k != plugin.key()) {
            return Err(msg(format!(
                "This archive is '{}', not '{}'.",
                plugin.key(),
                req.loader_key.as_deref().unwrap_or_default()
            )));
        }
        archive = Some((sha, path));
        scratch = Some(s);
        matched = Some(m);
        plugin.key().to_string()
    } else {
        req.loader_key
            .clone()
            .ok_or_else(|| msg("Name the loader or give an archive."))?
    };
    let plugin = registry
        .get(&key)
        .ok_or_else(|| LoaderError::UnknownLoader(key.clone()))?;
    let install = store.active_loader_install(&game.id, &key)?;
    match (req.op, &install) {
        (Op::Install | Op::Adopt, Some(_)) => return Err(LoaderError::AlreadyInstalled(key).into()),
        (Op::Upgrade | Op::Repair | Op::Uninstall, None) => {
            return Err(LoaderError::NotInstalled(key).into())
        }
        _ => {}
    }
    let ctx = game_ctx(store, &game, &key)?;
    let warnings = preflight(env, &game, &ctx, &rt)?;

    let mut plan = match plugin.plan(req.op, &ctx, &rt, scratch.as_deref()) {
        Err(LoaderError::ArchiveRequired { .. }) if req.op == Op::Repair && scratch.is_none() => {
            None
        }
        Ok(p) if p.uses_cached_archive && scratch.is_none() => None,
        other => Some(other?),
    };
    if plan.is_none() {
        let inst = install.as_ref().expect("checked above");
        let (sha, path) = cached_archive(inst)?;
        scratch = Some(extract_to_scratch(env, &id, &path)?);
        origin = cached_origin(&path);
        matched = Some(LoaderMatch {
            loader_key: key.clone(),
            version: inst.version.clone(),
        });
        archive = Some((sha, path));
        cached = true;
        plan = Some(plugin.plan(req.op, &ctx, &rt, scratch.as_deref())?);
    }
    let plan = plan.expect("set");
    let provenance = matched
        .as_ref()
        .map(|m| plugin.provenance(&ctx, m, &origin));

    // Exec bits (zip may drop them), then freeze and hash the scratch.
    let mut commands = Vec::new();
    let caller_path = std::env::var_os("PATH");
    if let Some(s) = &scratch {
        for step in &plan.steps {
            if let Step::RunInstaller(spec) = step {
                if spec.target.kind == TargetKind::Exe {
                    let t = runner::resolve_target(s, &spec.target.rel_path)?;
                    fs::set_permissions(&t, fs::Permissions::from_mode(0o755)).map_err(io(&t))?;
                }
            }
        }
        set_tree_writable(s, false);
        let tmp = installer_tmp(env, &id);
        mkdir_private(&tmp)?;
        let log = tmp.join("unused.log");
        for step in &plan.steps {
            if let Step::RunInstaller(spec) = step {
                let r = run_request(
                    env,
                    spec,
                    s,
                    &ctx.game_dir,
                    &log,
                    Some(&tmp),
                    caller_path.as_deref(),
                );
                let c = runner::build_command(&r)?;
                commands.push(PlannedCommand {
                    program: c.program.display().to_string(),
                    args: c
                        .args
                        .iter()
                        .map(|a| a.to_string_lossy().into_owned())
                        .collect(),
                    cwd: c.cwd.display().to_string(),
                    env: c
                        .env
                        .iter()
                        .map(|(k, v)| (k.clone(), v.to_string_lossy().into_owned()))
                        .collect(),
                    cmd_line: c.cmd_line,
                });
            }
        }
        let _ = fs::remove_dir_all(&tmp);
    }
    let scratch_hash = match &scratch {
        Some(s) => tree_hash(s)?,
        None => String::new(),
    };
    let mut warnings = warnings;
    if let Some(Provenance::Unverified { reason, .. }) = &provenance {
        warnings.push(format!(
            "The archive's source could not be verified ({reason}); confirm to continue anyway."
        ));
    }
    let version = plan
        .expected_version
        .clone()
        .or_else(|| matched.as_ref().and_then(|m| m.version.clone()))
        .or_else(|| install.as_ref().and_then(|i| i.version.clone()));
    let mut rec = PreviewRecord {
        id: id.clone(),
        game_id: game.id.clone(),
        op: req.op,
        loader_key: key.clone(),
        plan: plan.clone(),
        runtime: rt.clone(),
        install_id: install.as_ref().map(|i| i.id),
        version: version.clone(),
        archive_sha256: archive.as_ref().map(|a| a.0.clone()),
        archive_path: archive.as_ref().map(|a| a.1.clone()),
        cached_archive: cached,
        origin,
        scratch,
        scratch_hash,
        provenance: provenance.clone(),
        unverified_ack: req.unverified_ack,
        game_dir: ctx.game_dir.clone(),
        caller_path,
        plan_hash: String::new(),
        created_at: now(),
    };
    rec.plan_hash = rec.compute_hash();
    write_private(
        &env.dir("previews").join(format!("{id}.json")),
        &serde_json::to_vec(&rec).map_err(|e| msg(e.to_string()))?,
    )?;
    Ok(Preview {
        preview_id: id,
        plan_hash: rec.plan_hash.clone(),
        loader_key: key,
        op: req.op,
        version,
        runtime: rt,
        touch_scope: plan.touch_scope.iter().map(|g| g.0.clone()).collect(),
        external_side_effects: plan.external_side_effects.clone(),
        plan,
        commands,
        provenance,
        warnings,
        permissions: PERMISSIONS_NOTE,
        expires_at: rec.created_at + env.preview_ttl.as_secs() as i64,
    })
}

/// Deletes a preview and its scratch dir (declined, expired or used).
pub fn discard_preview(env: &LoaderEnv, preview_id: &str) {
    if preview_id.is_empty() || !preview_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return;
    }
    let scratch = env.dir("scratch").join(preview_id);
    if scratch.exists() {
        set_tree_writable(&scratch, true);
        let _ = fs::remove_dir_all(&scratch);
    }
    let _ = fs::remove_file(env.dir("previews").join(format!("{preview_id}.json")));
}

fn load_record(env: &LoaderEnv, id: &str) -> R<PreviewRecord> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(LoaderError::Consent("unknown preview".into()).into());
    }
    let bytes = fs::read(env.dir("previews").join(format!("{id}.json")))
        .map_err(|_| LoaderError::Consent("unknown preview".into()))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| LoaderError::Consent("unreadable preview".into()).into())
}

fn check_token(env: &LoaderEnv, store: &Store, rec: &PreviewRecord, plan_hash: &str) -> R<()> {
    let consent = |m: &str| Err(LoaderError::Consent(m.into()).into());
    if now() >= rec.created_at + env.preview_ttl.as_secs() as i64 {
        return consent("the preview expired");
    }
    let hash = rec.compute_hash();
    if hash != rec.plan_hash || hash != plan_hash {
        return consent("the plan does not match what was confirmed");
    }
    if matches!(rec.provenance, Some(Provenance::Unverified { .. })) && !rec.unverified_ack {
        return consent("the archive is unverified and was not acknowledged");
    }
    if let Some(s) = &rec.scratch {
        if tree_hash(s)? != rec.scratch_hash {
            return consent("the extracted installer changed after the preview");
        }
    }
    if let (Some(sha), Some(path)) = (&rec.archive_sha256, &rec.archive_path) {
        if sha256_file(path).ok().as_deref() != Some(sha.as_str()) {
            return consent("the cached loader archive changed");
        }
        if rec.cached_archive {
            let inst = rec
                .install_id
                .map(|i| store.loader_install(i))
                .transpose()?
                .flatten();
            if inst.and_then(|i| i.archive_sha256).as_deref() != Some(sha.as_str()) {
                return consent("the cached loader archive changed");
            }
        }
    }
    Ok(())
}

/// Merges a run's changes into an install's existing rows (relative to the
/// state before the first install). The first backup of a path is kept,
/// so an upgrade still restores the original vanilla.
pub fn merge_files(old: &[LoaderFile], new: &[LoaderFile]) -> Vec<LoaderFile> {
    let mut by: BTreeMap<String, LoaderFile> =
        old.iter().map(|f| (f.path.clone(), f.clone())).collect();
    for n in new {
        let Some(o) = by.remove(&n.path) else {
            by.insert(n.path.clone(), n.clone());
            continue;
        };
        let merged = match (o.change, n.change) {
            (FileChange::Created, FileChange::Deleted) => None,
            (FileChange::Created, _) => Some(LoaderFile {
                change: FileChange::Created,
                kind: n.kind,
                post_sha256: n.post_sha256.clone(),
                ..o.clone()
            }),
            (FileChange::Modified | FileChange::Deleted, FileChange::Deleted) => Some(LoaderFile {
                change: FileChange::Deleted,
                post_sha256: None,
                ..o.clone()
            }),
            (_, _) => {
                let back = n.post_sha256.is_some() && n.post_sha256 == o.pre_sha256;
                (!back).then(|| LoaderFile {
                    change: FileChange::Modified,
                    kind: n.kind,
                    post_sha256: n.post_sha256.clone(),
                    ..o.clone()
                })
            }
        };
        if let Some(mut m) = merged {
            m.out_of_scope = o.out_of_scope || n.out_of_scope;
            by.insert(m.path.clone(), m);
        }
    }
    by.into_values().collect()
}

/// Whether a tracked path is back at its pre-install state.
fn at_pre(game_dir: &Path, f: &LoaderFile) -> bool {
    let p = game_dir.join(&f.path);
    let meta = fs::symlink_metadata(&p);
    match f.change {
        FileChange::Created => meta.is_err(),
        FileChange::Modified | FileChange::Deleted => {
            let cur = match meta {
                Ok(m) if m.file_type().is_symlink() => fs::read_link(&p)
                    .ok()
                    .map(|t| hex(&Sha256::digest(t.as_os_str().as_encoded_bytes()))),
                Ok(m) if m.is_file() => sha256_file(&p).ok(),
                _ => None,
            };
            cur.is_some() && cur == f.pre_sha256
        }
    }
}

struct RunDirs {
    dir: PathBuf,
    pre: PathBuf,
    scope: PathBuf,
}

fn run_dirs(env: &LoaderEnv, id: &str) -> RunDirs {
    let dir = env.dir("runs").join(id);
    RunDirs {
        pre: dir.join("pre.json"),
        scope: dir.join("scope.json"),
        dir,
    }
}

/// Takes the deploy lock, retrying for up to a second: a process forked
/// elsewhere in Vessel holds a copy of the lock's file description until
/// it execs, so a just-released lock can look busy for a moment.
fn lock(env: &LoaderEnv) -> R<DeployLock> {
    let mut tries = 0;
    loop {
        match DeployLock::try_acquire(&env.lock_path) {
            Ok(l) => return Ok(l),
            Err(StoreError::LockBusy(_)) if tries < 50 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(StoreError::LockBusy(_)) => {
                return Err(msg(
                    "Another Vessel process is deploying or changing mods; try again when it has finished.",
                ))
            }
            Err(e) => return Err(e.into()),
        }
    }
}

fn crash(env: &LoaderEnv, at: RunPhase) -> R<()> {
    if env.crash_at == Some(at) {
        return Err(msg(format!("injected crash at {}", at.as_str())));
    }
    Ok(())
}

/// Composes and stores the game's desired launch options from every
/// active install. Never touches Steam's files.
fn recompose_launch(
    store: &Store,
    game_id: &str,
    rt: &Runtime,
    game_dir: &Path,
) -> R<Option<String>> {
    let changes: Vec<LaunchChange> = store
        .active_loader_launches(game_id)?
        .iter()
        .filter_map(|j| serde_json::from_str(j).ok())
        .collect();
    let desired = compose(&changes, rt, game_dir).map_err(LoaderError::from)?;
    store
        .set_launch_desired(game_id, desired.as_deref())
        .map_err(LoaderError::from)?;
    Ok(desired)
}

/// Copies one payload file into the game root (temp + rename, no
/// symlinked folders).
fn place_file(root: &Path, dest: &str, source: &Path, mode: Option<u32>) -> R<()> {
    let c = confine::confine(root, dest, true).map_err(|e| LoaderError::UnsafePath {
        path: dest.to_string(),
        message: e.to_string(),
    })?;
    let tmp = crate::deploy::place::tmp_path(&c.path);
    let _ = fs::remove_file(&tmp);
    fs::copy(source, &tmp).map_err(io(source))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(mode.unwrap_or(0o644)))
        .map_err(io(&tmp))?;
    fs::rename(&tmp, &c.path).map_err(io(&root.join(dest)))
}

fn fix_mode(root: &Path, rel: &str, mode: u32) -> R<()> {
    let Ok(c) = confine::confine(root, rel, false) else {
        return Ok(());
    };
    if fs::symlink_metadata(&c.path).is_ok_and(|m| m.is_file()) {
        fs::set_permissions(&c.path, fs::Permissions::from_mode(mode))
            .map_err(io(&root.join(rel)))?;
    }
    Ok(())
}

/// Pre-manifest of the install's first run: the state before the loader
/// existed. Rollbacks use it to remove only folders the loader created.
fn baseline(store: &Store, install_id: i64) -> Option<Manifest> {
    let path: Option<String> = store
        .conn()
        .query_row(
            "SELECT pre_manifest_path FROM loader_runs WHERE install_id = ?1 ORDER BY id LIMIT 1",
            [install_id],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    path.and_then(|p| Manifest::load(Path::new(&p)).ok())
}

/// Mutable state of one run.
struct Exec<'a> {
    env: &'a LoaderEnv,
    rec: &'a PreviewRecord,
    plugin: &'a dyn LoaderPlugin,
    ctx: GameCtx,
    scope: Vec<Glob>,
    run_id: i64,
    install_id: i64,
    dirs: RunDirs,
    pre: Manifest,
    refs: BTreeMap<String, String>,
    exit_code: Option<i64>,
    timed_out: bool,
    launch: Option<Option<LaunchChange>>,
    warnings: Vec<String>,
}

impl Exec<'_> {
    fn game_dir(&self) -> &Path {
        &self.ctx.game_dir
    }

    /// Runs the plan's steps. `Ok(Err(reason))` is an op failure.
    fn steps(&mut self, store: &Store, lock: &DeployLock) -> R<Result<(), String>> {
        let steps = self.rec.plan.steps.clone();
        let last_verify = steps.iter().rposition(|s| matches!(s, Step::Verify { .. }));
        for (i, step) in steps.iter().enumerate() {
            match step {
                Step::RunInstaller(spec) => {
                    let scratch = self
                        .rec
                        .scratch
                        .clone()
                        .ok_or_else(|| msg("installer step without an archive"))?;
                    let tmp = spec
                        .private_tmp
                        .then(|| installer_tmp(self.env, &self.rec.id));
                    if let Some(t) = &tmp {
                        mkdir_private(t)?;
                    }
                    let log = self.dirs.dir.join(format!("step-{}.log", i + 1));
                    let game_dir = self.ctx.game_dir.clone();
                    let req = run_request(
                        self.env,
                        spec,
                        &scratch,
                        &game_dir,
                        &log,
                        tmp.as_deref(),
                        self.rec.caller_path.as_deref(),
                    );
                    // Earlier steps take time (backups, deploys): check the
                    // consented installer once more right before it starts.
                    if tree_hash(&scratch)? != self.rec.scratch_hash {
                        if let Some(t) = &tmp {
                            let _ = fs::remove_dir_all(t);
                        }
                        return Ok(Err(
                            "the extracted installer changed after the preview".into()
                        ));
                    }
                    let run_id = self.run_id;
                    let out = self.env.runner.run(&req, &mut |sp| {
                        store.set_loader_run_process(
                            run_id,
                            sp.pid as i64,
                            sp.pgid as i64,
                            sp.proc_start as i64,
                        )?;
                        if let Some((ws, pfx)) = &sp.wine {
                            store.set_loader_run_wine(
                                run_id,
                                &ws.to_string_lossy(),
                                &pfx.to_string_lossy(),
                            )?;
                        }
                        Ok(())
                    });
                    if let Some(t) = &tmp {
                        let _ = fs::remove_dir_all(t);
                    }
                    let out = match out {
                        Ok(o) => o,
                        Err(e) => return Ok(Err(e.to_string())),
                    };
                    self.exit_code = out.exit_code.map(i64::from);
                    self.timed_out = out.timed_out;
                    if out.exit_verdict() == Some(false) {
                        return Ok(Err(if out.timed_out {
                            "the installer timed out".into()
                        } else {
                            format!("the installer exited with code {:?}", out.exit_code)
                        }));
                    }
                }
                Step::DeployFiles(d) => {
                    for f in &d.files {
                        place_file(self.game_dir(), &f.dest, &f.source, f.mode)?;
                    }
                }
                Step::FixModes(fixes) => {
                    for f in fixes {
                        fix_mode(self.game_dir(), &f.path, f.mode)?;
                    }
                }
                Step::Verify { after } => {
                    let post = snapshot::snapshot(self.game_dir(), &self.scope)?;
                    let rt = &self.rec.runtime;
                    if let Err(e) = self.plugin.verify(*after, &self.ctx, rt, &post) {
                        return Ok(Err(e.to_string()));
                    }
                    // A mid-plan check (adopt: after removing the hand-made
                    // install) starts a new baseline: what follows is
                    // tracked against the verified vanilla state.
                    if Some(i) != last_verify {
                        self.pre = post;
                        self.pre.save(&self.dirs.pre)?;
                        self.refs = snapshot::backup(
                            lock,
                            store,
                            self.run_id,
                            &self.env.backup_dir,
                            &self.ctx.game_dir,
                            &self.pre,
                            &self.scope,
                        )?;
                    }
                }
                Step::Launch(c) => self.launch = Some(Some(c.clone())),
                Step::RevertLaunch(_) => self.launch = Some(None),
                Step::RestoreTracked => {
                    let rows = store.loader_files(self.install_id)?;
                    let base = baseline(store, self.install_id);
                    let r = snapshot::rollback(
                        self.game_dir(),
                        &self.env.backup_dir,
                        &rows,
                        base.as_ref(),
                    )?;
                    self.warn_unrecoverable(&r.unrecoverable);
                }
            }
        }
        Ok(Ok(()))
    }

    fn warn_unrecoverable(&mut self, paths: &[String]) {
        if !paths.is_empty() {
            self.warnings.push(format!(
                "These changes could not be undone: {}",
                paths.join(", ")
            ));
        }
    }
}

/// Before repair, upgrade and uninstall: if Steam replaced the loader's
/// vanilla file with a new version, that new file becomes the vanilla
/// (backup, row and the installer's own copies).
#[allow(clippy::too_many_arguments)]
fn refresh_vanilla(
    env: &LoaderEnv,
    store: &mut Store,
    lock: &DeployLock,
    plugin: &dyn LoaderPlugin,
    install: &LoaderInstall,
    run_id: i64,
    game_dir: &Path,
    warnings: &mut Vec<String>,
) -> R<()> {
    let (Some(spec), Some(vanilla)) = (plugin.vanilla(), install.vanilla_sha256.as_deref()) else {
        return Ok(());
    };
    let file = game_dir.join(&spec.file);
    if !fs::symlink_metadata(&file).is_ok_and(|m| m.is_file()) {
        return Ok(());
    }
    let current = sha256_file(&file).map_err(io(&file))?;
    let mut rows = store.loader_files(install.id)?;
    let Some(row) = rows.iter_mut().find(|r| r.path == spec.file) else {
        return Ok(());
    };
    let post = row.post_sha256.clone().unwrap_or_default();
    match classify_vanilla(&current, &post, vanilla) {
        VanillaState::Intact => return Ok(()),
        VanillaState::SameVanilla => {
            warnings.push(format!("{} is the vanilla file again.", spec.file));
            return Ok(());
        }
        VanillaState::SteamUpdated => {}
    }
    let scope = [Glob::literal(&spec.file)];
    let mut mini = Manifest::default();
    mini.entries.insert(
        spec.file.clone(),
        Entry::File {
            size: 0,
            mode: 0,
            mtime_ns: 0,
            sha256: Some(current.clone()),
        },
    );
    let refs = snapshot::backup(
        lock,
        store,
        run_id,
        &env.backup_dir,
        game_dir,
        &mini,
        &scope,
    )?;
    row.pre_sha256 = Some(current.clone());
    row.backup_ref = refs.get(&spec.file).cloned();
    for copy in &spec.copies {
        if fs::symlink_metadata(game_dir.join(copy)).is_ok_and(|m| m.is_file()) {
            let mode = fs::metadata(&file).map_err(io(&file))?.permissions().mode() & 0o777;
            place_file(game_dir, copy, &file, Some(mode))?;
        }
    }
    store.replace_loader_files(install.id, &rows)?;
    store.set_loader_install_meta(
        install.id,
        install.version.as_deref(),
        &install.runtime,
        install.archive_sha256.as_deref(),
        install.archive_path.as_deref(),
        Some(&current),
        install.game_build.as_deref(),
    )?;
    warnings.push(format!(
        "Steam updated {}; the new file is now the vanilla backup.",
        spec.file
    ));
    Ok(())
}

/// Runs a previewed op. `plan_hash` must be the preview's.
pub fn run(env: &LoaderEnv, store: &mut Store, preview_id: &str, plan_hash: &str) -> R<RunReport> {
    let rec = load_record(env, preview_id)?;
    check_token(env, store, &rec, plan_hash)?;
    let game = crate::games::require_game(store.conn(), &rec.game_id)?;
    let registry = registry_for(env, &game)?;
    let plugin = registry
        .get(&rec.loader_key)
        .ok_or_else(|| LoaderError::UnknownLoader(rec.loader_key.clone()))?;
    let ctx = game_ctx(store, &game, &rec.loader_key)?;
    if ctx.game_dir != rec.game_dir {
        return Err(
            LoaderError::Consent("the game folder changed after the preview".into()).into(),
        );
    }
    let mut warnings = preflight(env, &game, &ctx, &rec.runtime)?;
    let lock = lock(env)?;
    // The token is single use.
    let _ = fs::remove_file(env.dir("previews").join(format!("{preview_id}.json")));

    let has_installer = rec
        .plan
        .steps
        .iter()
        .any(|s| matches!(s, Step::RunInstaller(_)));
    let (install_id, prev_status) = match rec.op {
        Op::Install | Op::Adopt => {
            let (mod_id, file_id) = match &rec.origin {
                DownloadOrigin::Nexus {
                    mod_id, file_id, ..
                } => (Some(*mod_id as i64), Some(*file_id as i64)),
                _ => (None, None),
            };
            let id = store
                .insert_loader_install(&NewLoaderInstall {
                    game_id: game.id.clone(),
                    game_dir: ctx.game_dir.display().to_string(),
                    loader_key: rec.loader_key.clone(),
                    version: rec.version.clone(),
                    runtime: rec.runtime.as_str().into(),
                    strategy: if has_installer {
                        Strategy::Installer
                    } else {
                        Strategy::Deploy
                    },
                    status: InstallStatus::Running,
                    nexus_mod_id: mod_id,
                    nexus_file_id: file_id,
                    origin: rec.origin.as_str().into(),
                    archive_sha256: rec.archive_sha256.clone(),
                    archive_path: rec.archive_path.as_ref().map(|p| p.display().to_string()),
                    vanilla_sha256: None,
                    game_build: None,
                })
                .map_err(|_| LoaderError::AlreadyInstalled(rec.loader_key.clone()))?;
            (id, InstallStatus::Failed)
        }
        _ => {
            let inst = store
                .active_loader_install(&game.id, &rec.loader_key)?
                .ok_or_else(|| LoaderError::NotInstalled(rec.loader_key.clone()))?;
            if Some(inst.id) != rec.install_id {
                return Err(LoaderError::Consent("the install changed".into()).into());
            }
            store.set_loader_install_status(inst.id, InstallStatus::Running)?;
            (inst.id, InstallStatus::Installed)
        }
    };
    let dirs = run_dirs(env, preview_id);
    mkdir_private(&dirs.dir)?;
    let run_id = store.insert_loader_run(&NewLoaderRun {
        install_id,
        op: rec.op,
        consent_hash: rec.plan_hash.clone(),
        pre_manifest_path: Some(dirs.pre.display().to_string()),
        log_path: Some(dirs.dir.display().to_string()),
    })?;
    let scope = rec.plan.touch_scope.clone();
    write_private(&dirs.scope, &serde_json::to_vec(&scope).unwrap_or_default())?;

    // backing_up
    if matches!(rec.op, Op::Repair | Op::Upgrade | Op::Uninstall) {
        let inst = store.loader_install(install_id)?.expect("exists");
        refresh_vanilla(
            env,
            store,
            &lock,
            plugin,
            &inst,
            run_id,
            &ctx.game_dir,
            &mut warnings,
        )?;
    }
    let pre = snapshot::snapshot(&ctx.game_dir, &scope)?;
    pre.save(&dirs.pre)?;
    let refs = snapshot::backup(
        &lock,
        store,
        run_id,
        &env.backup_dir,
        &ctx.game_dir,
        &pre,
        &scope,
    )?;
    crash(env, RunPhase::BackingUp)?;

    // spawned
    store.set_loader_run_phase(run_id, RunPhase::Spawned)?;
    let mut ex = Exec {
        env,
        rec: &rec,
        plugin,
        ctx,
        scope,
        run_id,
        install_id,
        dirs,
        pre,
        refs,
        exit_code: None,
        timed_out: false,
        launch: None,
        warnings,
    };
    let result = ex.steps(store, &lock);
    crash(env, RunPhase::Spawned)?;
    let result = match result {
        Ok(r) => r,
        Err(e) => Err(super::chain(&e)),
    };

    // diffing
    store.set_loader_run_phase(run_id, RunPhase::Diffing)?;
    crash(env, RunPhase::Diffing)?;
    let post = snapshot::snapshot(ex.game_dir(), &ex.scope)?;
    let changes = snapshot::diff(&ex.pre, &post, &ex.refs, &ex.scope);
    let old = store.loader_files(install_id)?;
    let inst = store.loader_install(install_id)?.expect("exists");
    let (outcome, status) = match (rec.op, result) {
        (Op::Uninstall, result) => {
            // Undo whatever the vendor uninstall left (or everything,
            // if it failed), from the backups.
            let merged = merge_files(&old, &changes);
            let pending: Vec<LoaderFile> = merged
                .iter()
                .filter(|f| !at_pre(ex.game_dir(), f))
                .cloned()
                .collect();
            let base = baseline(store, install_id);
            let r = snapshot::rollback(ex.game_dir(), &env.backup_dir, &pending, base.as_ref())?;
            ex.warn_unrecoverable(&r.unrecoverable);
            if let Err(reason) = &result {
                ex.warnings.push(format!(
                    "The vendor uninstall failed ({reason}); Vessel restored the files from its backups."
                ));
            }
            let post = snapshot::snapshot(ex.game_dir(), &ex.scope)?;
            match plugin.verify(Op::Uninstall, &ex.ctx, &rec.runtime, &post) {
                Ok(()) => {
                    store.replace_loader_files(install_id, &[])?;
                    ex.launch = Some(None);
                    (RunOutcome::Succeeded, InstallStatus::Uninstalled)
                }
                Err(e) => {
                    store.replace_loader_files(
                        install_id,
                        &merged
                            .into_iter()
                            .filter(|f| !at_pre(ex.game_dir(), f))
                            .collect::<Vec<_>>(),
                    )?;
                    (
                        RunOutcome::Failed {
                            reason: e.to_string(),
                        },
                        InstallStatus::Failed,
                    )
                }
            }
        }
        (_, Ok(())) => {
            let merged = merge_files(&old, &changes);
            store.replace_loader_files(install_id, &merged)?;
            let vanilla = match (rec.op, plugin.vanilla()) {
                (Op::Install | Op::Adopt, Some(v)) => match ex.pre.entries.get(&v.file) {
                    Some(Entry::File { sha256, .. }) => sha256.clone(),
                    _ => None,
                },
                _ => inst.vanilla_sha256.clone(),
            };
            let build = steam_app(env, &game).and_then(|a| a.buildid);
            store.set_loader_install_meta(
                install_id,
                rec.version.as_deref(),
                rec.runtime.as_str(),
                rec.archive_sha256
                    .as_deref()
                    .or(inst.archive_sha256.as_deref()),
                rec.archive_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .as_deref()
                    .or(inst.archive_path.as_deref()),
                vanilla.as_deref(),
                build.as_deref().or(inst.game_build.as_deref()),
            )?;
            (RunOutcome::Succeeded, InstallStatus::Installed)
        }
        (_, Err(reason)) => {
            let r = snapshot::rollback(ex.game_dir(), &env.backup_dir, &changes, Some(&ex.pre))?;
            ex.warn_unrecoverable(&r.unrecoverable);
            ex.launch = None;
            (RunOutcome::Failed { reason }, prev_status)
        }
    };
    store.set_loader_install_status(install_id, status)?;
    store.finish_loader_run(run_id, ex.exit_code, ex.timed_out)?;
    store.clear_loader_run_backups(run_id)?;
    if let Some(l) = &ex.launch {
        let json = l
            .as_ref()
            .map(|c| serde_json::to_string(c).unwrap_or_default());
        store.set_loader_install_launch(install_id, json.as_deref())?;
    }
    let launch_desired = recompose_launch(store, &game.id, &rec.runtime, ex.game_dir())?;
    drop(lock);
    discard_preview(env, preview_id);
    Ok(RunReport {
        install_id,
        run_id,
        outcome,
        status: status.as_str().into(),
        version: store.loader_install(install_id)?.and_then(|i| i.version),
        log_dir: ex.dirs.dir.clone(),
        warnings: ex.warnings,
        launch_desired,
    })
}

/// Undoes every tracked change of an install (a recovery's offered
/// rollback, or a manual one). Never starts an installer.
pub fn rollback(env: &LoaderEnv, store: &mut Store, install_id: i64) -> R<Vec<String>> {
    let _lock = lock(env)?;
    let inst = store
        .loader_install(install_id)?
        .ok_or_else(|| msg("unknown loader install"))?;
    let rows = store.loader_files(install_id)?;
    let game_dir = PathBuf::from(&inst.game_dir);
    let base = baseline(store, install_id);
    let r = snapshot::rollback(&game_dir, &env.backup_dir, &rows, base.as_ref())?;
    store.replace_loader_files(install_id, &[])?;
    store.set_loader_install_status(install_id, InstallStatus::Uninstalled)?;
    store.set_loader_install_launch(install_id, None)?;
    clear_install_backups(store, install_id)?;
    if let Some(game_id) = &inst.game_id {
        if let Some(game) = crate::games::get_game(store.conn(), game_id)? {
            let rt = game_runtime(env, store, &game).unwrap_or(Runtime::Native);
            recompose_launch(store, game_id, &rt, &game_dir)?;
        }
    }
    Ok(r.unrecoverable)
}

/// Keeps an interrupted install's files as they are (the user declined the
/// offered rollback). The install counts as installed again.
pub fn decline_rollback(env: &LoaderEnv, store: &Store, install_id: i64) -> R<()> {
    let _lock = lock(env)?;
    store.set_loader_install_status(install_id, InstallStatus::Installed)?;
    clear_install_backups(store, install_id)
}

fn clear_install_backups(store: &Store, install_id: i64) -> R<()> {
    let ids: Vec<i64> = {
        let mut stmt = store
            .conn()
            .prepare("SELECT id FROM loader_runs WHERE install_id = ?1")?;
        let rows = stmt.query_map([install_id], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for id in ids {
        store.clear_loader_run_backups(id)?;
    }
    Ok(())
}

/// Startup recovery for every unfinished run, per phase:
/// `backing_up` -> discarded (the game was not touched);
/// `spawned` -> the stored group is killed if it is still the same
/// processes, then as `diffing`; `diffing` -> the diff is finished and
/// recorded, the install is `interrupted` and a rollback is offered.
/// Never starts an installer.
pub fn recover(env: &LoaderEnv, store: &mut Store) -> R<Vec<Recovered>> {
    let runs = store.unfinished_loader_runs()?;
    if runs.is_empty() {
        return Ok(Vec::new());
    }
    let _lock = lock(env)?;
    let mut out = Vec::new();
    for run in runs {
        let Some(inst) = store.loader_install(run.install_id)? else {
            continue;
        };
        let action = match RunPhase::parse(&run.phase) {
            Some(RunPhase::BackingUp) | None => {
                let fresh = matches!(run.op.as_str(), "install" | "adopt")
                    && store.loader_files(inst.id)?.is_empty();
                let status = if fresh {
                    InstallStatus::Failed
                } else {
                    InstallStatus::Installed
                };
                store.set_loader_install_status(inst.id, status)?;
                RecoveryAction::Discarded
            }
            Some(phase @ (RunPhase::Spawned | RunPhase::Diffing)) => {
                let killed = phase == RunPhase::Spawned && runner::kill_stale_run(&run);
                let game_dir = PathBuf::from(&inst.game_dir);
                let dir = run
                    .pre_manifest_path
                    .as_deref()
                    .map(Path::new)
                    .and_then(Path::parent)
                    .map(Path::to_path_buf)
                    .unwrap_or_default();
                let scope: Vec<Glob> = fs::read(dir.join("scope.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default();
                let pre = Manifest::load(&dir.join("pre.json"))?;
                // Map the run's backups back to paths through the pre-manifest.
                let by_sha: BTreeMap<String, String> = store
                    .loader_run_backups(run.id)?
                    .into_iter()
                    .filter_map(|r| BackupRef::parse(&r).map(|b| (b.sha256, r)))
                    .collect();
                let refs: BTreeMap<String, String> = pre
                    .entries
                    .iter()
                    .filter_map(|(p, e)| {
                        let sha = match e {
                            Entry::File { sha256, .. } => sha256.clone(),
                            Entry::Link { sha256, .. } => Some(sha256.clone()),
                            _ => None,
                        }?;
                        by_sha.get(&sha).map(|r| (p.clone(), r.clone()))
                    })
                    .collect();
                let post = snapshot::snapshot(&game_dir, &scope)?;
                let changes = snapshot::diff(&pre, &post, &refs, &scope);
                let merged = merge_files(&store.loader_files(inst.id)?, &changes);
                store.replace_loader_files(inst.id, &merged)?;
                store.set_loader_install_status(inst.id, InstallStatus::Interrupted)?;
                RecoveryAction::RollbackOffered { killed }
            }
            Some(RunPhase::Done) => continue,
        };
        store.finish_loader_run(run.id, None, false)?;
        store.clear_loader_run_backups(run.id)?;
        out.push(Recovered {
            run_id: run.id,
            install_id: inst.id,
            action,
        });
    }
    Ok(out)
}

/// The state of loader `key` for a game.
pub fn status(env: &LoaderEnv, store: &Store, game_id: &str, key: &str) -> R<LoaderStatus> {
    let game = crate::games::require_game(store.conn(), game_id)?;
    let registry = registry_for(env, &game)?;
    let plugin = registry
        .get(key)
        .ok_or_else(|| LoaderError::UnknownLoader(key.into()))?;
    Ok(plugin.detect(&game_ctx(store, &game, key)?, store)?)
}

/// Previews a stored preview's op again (same game, loader, archive and
/// origin) with `unverified_ack`, and drops the old one. The UI uses it to
/// show a pending preview (NXM, collections, install) in full and to
/// re-request it once the user acknowledges an unverified archive (the
/// flag is inside the hashed plan, so it needs a fresh token).
pub fn reopen_preview(
    env: &LoaderEnv,
    store: &Store,
    preview_id: &str,
    unverified_ack: bool,
) -> R<Preview> {
    let rec = load_record(env, preview_id)?;
    let req = PreviewRequest {
        game_id: rec.game_id.clone(),
        op: rec.op,
        loader_key: Some(rec.loader_key.clone()),
        // A cached-archive preview (repair, uninstall) re-derives it.
        archive: rec.archive_path.clone().filter(|_| !rec.cached_archive),
        origin: rec.origin.clone(),
        unverified_ack,
    };
    // The origin (and so the provenance) belongs to the archive that was
    // hashed; a changed file must not inherit it. The check happens on the
    // copy cache_archive hashes, so no swap can slip in between.
    let expected = req.archive.as_ref().and(rec.archive_sha256.as_deref());
    match preview_expecting(env, store, &req, expected) {
        Err(e) if is_archive_changed(&e) => {
            discard_preview(env, preview_id);
            Err(e)
        }
        r => {
            let p = r?;
            discard_preview(env, preview_id);
            Ok(p)
        }
    }
}

fn is_archive_changed(e: &super::AppError) -> bool {
    matches!(
        e,
        super::AppError::Core(crate::Error::Loader(LoaderError::Consent(m)))
            if m == ARCHIVE_CHANGED
    )
}

/// The newest run of an install (for its log).
pub fn latest_run(store: &Store, install_id: i64) -> R<Option<i64>> {
    use rusqlite::OptionalExtension;
    Ok(store
        .conn()
        .query_row(
            "SELECT id FROM loader_runs WHERE install_id = ?1 ORDER BY id DESC LIMIT 1",
            [install_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// Sets (`native`, `proton`) or clears the game's runtime override.
pub fn set_runtime_override(store: &Store, game_id: &str, runtime: Option<&str>) -> R<()> {
    if let Some(v) = runtime {
        if !matches!(v, "native" | "proton") {
            return Err(msg(format!("Unknown runtime '{v}': use native or proton.")));
        }
    }
    match store.conn().execute(
        "UPDATE games SET runtime_override = ?1 WHERE id = ?2",
        rusqlite::params![runtime, game_id],
    )? {
        0 => Err(msg(format!("Unknown game '{game_id}'."))),
        _ => Ok(()),
    }
}

/// A loader Vessel manages for a game, as the UI lists it.
#[derive(Debug, Clone, Serialize)]
pub struct LoaderView {
    pub key: String,
    pub name: String,
    /// The loader's Nexus page on the game's domain (install, upgrade and
    /// adopt download from it).
    pub nexus_mod_id: Option<u64>,
    pub status: LoaderStatus,
    /// `native`, `proton` or `unknown` (detection failed or is ambiguous).
    pub runtime: String,
    pub runtime_override: Option<String>,
    pub launch: crate::loaders::launch::LaunchState,
    pub steam_running: bool,
    /// Newest run of the active install (its log).
    pub last_run: Option<i64>,
}

/// Display name and Nexus mod id of loader `key` (definition first, then
/// the built-in SMAPI identity).
fn loader_identity(def: Option<&definitions::GameDefinition>, key: &str) -> (String, Option<u64>) {
    match def.and_then(|d| d.loaders.iter().find(|l| l.key == key)) {
        Some(l) => (l.name.clone(), l.nexus_mod_id),
        None if key == crate::loaders::smapi::KEY => (
            "SMAPI".to_string(),
            Some(crate::loaders::smapi::NEXUS_MOD_ID),
        ),
        None => (key.to_string(), None),
    }
}

/// A loader with an active install record (installed, running or
/// interrupted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledLoader {
    pub key: String,
    pub name: String,
    pub version: Option<String>,
    /// The install's own id, else the loader's known Nexus id (adopted and
    /// local installs store none).
    pub nexus_mod_id: Option<u64>,
    pub install_id: i64,
    pub status: String,
}

/// The loaders of `game` with an active install record. Reads records
/// only (no detection), so it is cheap enough for list views.
pub fn installed_loaders(
    definitions_dir: Option<&Path>,
    store: &Store,
    game: &GameConfig,
) -> R<Vec<InstalledLoader>> {
    let defs = definitions::load_definitions(definitions_dir)?;
    let def = game
        .definition
        .as_deref()
        .and_then(|d| definitions::find_definition(&defs, d));
    let registry = LoaderRegistry::for_game(def, game.definition.as_deref())?;
    let mut out = Vec::new();
    for key in registry.keys() {
        let Some(inst) = store.active_loader_install(&game.id, key)? else {
            continue;
        };
        let (name, known_id) = loader_identity(def, key);
        out.push(InstalledLoader {
            key: key.to_string(),
            name,
            version: inst.version,
            nexus_mod_id: inst
                .nexus_mod_id
                .and_then(|i| u64::try_from(i).ok())
                .or(known_id),
            install_id: inst.id,
            status: inst.status,
        });
    }
    Ok(out)
}

/// Every loader of `game` with its state.
pub fn views(env: &LoaderEnv, store: &Store, game: &GameConfig) -> R<Vec<LoaderView>> {
    let defs = definitions::load_definitions(env.definitions_dir.as_deref())?;
    let def = game
        .definition
        .as_deref()
        .and_then(|d| definitions::find_definition(&defs, d));
    let registry = LoaderRegistry::for_game(def, game.definition.as_deref())?;
    let runtime = game_runtime(env, store, game)
        .map_or_else(|_| "unknown".into(), |r| r.as_str().to_string());
    let runtime_override = runtime_override(store, &game.id)?;
    let launch = store.launch_state(&game.id).map_err(LoaderError::from)?;
    let steam_running = super::shortcut::steam_running();
    let mut out = Vec::new();
    for key in registry.keys() {
        let (name, nexus_mod_id) = loader_identity(def, key);
        let status = status(env, store, &game.id, key)?;
        let install = match &status {
            LoaderStatus::Installed { install_id, .. }
            | LoaderStatus::Busy { install_id, .. }
            | LoaderStatus::Drifted { install_id, .. } => Some(*install_id),
            _ => None,
        };
        out.push(LoaderView {
            key: key.to_string(),
            name,
            nexus_mod_id,
            status,
            runtime: runtime.clone(),
            runtime_override: runtime_override.clone(),
            launch: launch.clone(),
            steam_running,
            last_run: install.map(|i| latest_run(store, i)).transpose()?.flatten(),
        });
    }
    Ok(out)
}

/// A mod row that is really a loader archive staged as a mod (how SMAPI's
/// installer used to end up under `Mods/`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoaderJunk {
    pub mod_id: i64,
    pub uid: String,
    pub name: String,
    pub loader_key: String,
}

/// Mod rows of `game` whose staged files match a loader's archive
/// signature. A staged path may sit below extra folders, so every suffix
/// of each path is offered to the loaders.
pub fn find_loader_junk(cx: &super::Cx<'_>, game: &GameConfig) -> R<Vec<LoaderJunk>> {
    let defs = definitions::load_definitions(Some(&cx.paths.definitions_dir()))?;
    let def = game
        .definition
        .as_deref()
        .and_then(|d| definitions::find_definition(&defs, d));
    let registry = LoaderRegistry::for_game(def, game.definition.as_deref())?;
    junk_in(cx.store, &registry, game)
}

fn junk_in(store: &Store, registry: &LoaderRegistry, game: &GameConfig) -> R<Vec<LoaderJunk>> {
    let ctx = GameCtx {
        game_id: game.id.clone(),
        nexus_domain: game.nexus_domain.clone(),
        game_dir: game.folder.clone().unwrap_or_default(),
        steam_appid: game.appid,
        occupied: Vec::new(),
    };
    let mut out = Vec::new();
    for m in crate::mods::list_mods(store.conn(), &game.id)? {
        let paths: Vec<String> = {
            let mut stmt = store
                .conn()
                .prepare("SELECT path FROM mod_files WHERE mod_id = ?1")?;
            let rows = stmt.query_map([m.id], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let mut entries = Vec::new();
        for p in &paths {
            let parts: Vec<&str> = p.split('/').collect();
            for i in 0..parts.len() {
                entries.push(parts[i..].join("/"));
            }
        }
        let probe = ArchiveProbe {
            file_name: m.name.clone(),
            nexus_mod_id: m.nexus_mod_id,
            nexus_file_id: m.nexus_file_id,
            entries,
        };
        let key = match registry.match_archive(&ctx, &probe) {
            Ok(Some((p, _))) => p.key().to_string(),
            Err(LoaderError::AmbiguousMatch(a, _)) => a,
            _ => continue,
        };
        out.push(LoaderJunk {
            mod_id: m.id,
            uid: m.uid.clone(),
            name: m.name.clone(),
            loader_key: key,
        });
    }
    Ok(out)
}

/// Removes loader junk rows through the normal mod-removal path: purge the
/// deployment (so their files leave `Mods/`), delete the rows and staged
/// folders, then redeploy the remaining mods. Only rows
/// [`find_loader_junk`] reports are touched. Returns how many were removed.
pub fn remove_loader_junk(cx: &mut super::Cx<'_>, game: &GameConfig, mod_ids: &[i64]) -> R<usize> {
    let junk: Vec<i64> = find_loader_junk(cx, game)?
        .into_iter()
        .map(|j| j.mod_id)
        .filter(|id| mod_ids.contains(id))
        .collect();
    if junk.is_empty() {
        return Ok(0);
    }
    let cfg = cx.config()?;
    let was_active = crate::games::deployment_active(cx.store.conn(), &game.id)?;
    if was_active {
        let env = crate::deploy::DeployEnv::new(cx.paths, &cfg.data_dir, &game.id);
        let r = crate::deploy::purge(cx.store, game, &env)?;
        for m in super::deploy::purge_msgs(&r, &env.overwrite_dir) {
            cx.ui.say(m);
        }
    }
    let lock = super::staging_lock(cx.paths)?;
    let r = crate::mods::remove_mods(cx.store, &game.id, &junk, None)?;
    drop(lock);
    if was_active {
        super::deploy::run_deploy(cx, game, None, false)?;
    }
    cx.ui.success(&format!(
        "Removed {} leftover loader archive(s) that were installed as mods.",
        r.mods
    ));
    Ok(r.mods)
}

#[cfg(test)]
mod pipeline_tests;
#[cfg(test)]
mod tests;
