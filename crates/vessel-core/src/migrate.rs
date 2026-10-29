// SPDX-License-Identifier: GPL-3.0-or-later
//! First-launch migration of pre-rebrand state (names from [`crate::legacy`])
//! into the Vessel locations.
//!
//! Runs before logging and `Store::open`, holding `vessel-migration.lock`
//! and the old `deploy.lock` for the whole run, and refuses while a legacy
//! process is alive (old deploy lock held or its D-Bus name owned). Progress
//! is recorded per step in `<config>/migration.json` ([`marker`]); a rerun
//! resumes at the first unfinished step. Small state (config, DB, log) is
//! copied and verified; bulk data and per-library staging are renamed, with a
//! verified `*.migrating` copy when the rename crosses filesystems. Stored
//! absolute paths are rewritten in one transaction, then symlink deployments
//! are retargeted. Old dirs are removed only once every step is done.
//!
//! Conflict rule: an existing Vessel dir with no marker means nothing is
//! touched and the conflict is reported.
//!
//! Extension point: [`INTEGRATION_STEPS`] run after the core steps and before
//! the final cleanup; see [`steps`].

mod db;
mod fsops;
mod integrations;
pub mod marker;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::autoupdate::Systemctl;
use crate::deploy::STAGING_DIR;
use crate::legacy::{
    LEGACY_APP_DIR, LEGACY_DBUS_NAME, LEGACY_DB_FILE, LEGACY_ENV_PREFIX, LEGACY_LOG_FILE,
    LEGACY_STAGING_DIR, LEGACY_UNIT,
};
use crate::paths::Paths;
use crate::store::{DeployLock, StoreError};

pub use integrations::{retry_legacy_key, LegacyKeyRetry};
pub use marker::{LibraryState, Marker, Sources, StepState};

/// Marker file in the new config dir.
pub const MARKER_FILE: &str = "migration.json";
/// Migration lock, next to the new data dir.
pub const LOCK_FILE: &str = "vessel-migration.lock";

/// Errors that stop the migration. The old data is intact whenever one is
/// returned.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MigrateError {
    #[error("Close {app} before starting Vessel ({0})", app = LEGACY_APP_DIR)]
    LegacyRunning(String),
    #[error("another Vessel process is migrating data (lock {0} is held)")]
    Busy(PathBuf),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("database error during migration: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("copy of {0} did not verify; the original is kept")]
    Verify(PathBuf),
    #[error("migration: {0}")]
    Marker(String),
    #[error("injected crash: {0}")]
    InjectedCrash(String),
    /// A non-fatal integration step failed; it is retried next start.
    #[error("{0}")]
    Integration(String),
}

impl MigrateError {
    pub(crate) fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// The legacy environment variables present at startup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvSnapshot {
    vars: Vec<(String, String)>,
}

impl EnvSnapshot {
    /// Every set variable with the legacy prefix.
    pub fn capture() -> Self {
        Self::from_vars(std::env::vars_os().map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        }))
    }

    pub fn from_vars(vars: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut vars: Vec<_> = vars
            .into_iter()
            .filter(|(k, _)| k.starts_with(LEGACY_ENV_PREFIX))
            .collect();
        vars.sort();
        Self { vars }
    }

    /// One line per set legacy variable. Only the variables the legacy
    /// app read have a Vessel replacement; `DATA_DIR`/`CONFIG_DIR` were
    /// read only by the legacy bash tool and are never migration sources.
    fn warnings(&self) -> Vec<String> {
        self.vars
            .iter()
            .map(|(k, _)| {
                let rest = &k[LEGACY_ENV_PREFIX.len()..];
                if RENAMED_ENV.contains(&rest) {
                    format!("{k} is deprecated and ignored; use VESSEL_{rest} instead")
                } else if matches!(rest, "DATA_DIR" | "CONFIG_DIR") {
                    format!("{k} is used only by the legacy bash tool; ignored by Vessel")
                } else {
                    format!("{k} is not used by Vessel and is ignored")
                }
            })
            .collect()
    }
}

/// Legacy variables (after the prefix) the legacy app read, each renamed
/// to `VESSEL_<same>`.
const RENAMED_ENV: &[&str] = &[
    "DEBUG",
    "NO_COLOR",
    "LOOT_URL",
    "NEXUS_URL",
    "SYSTEMCTL",
    "SANDBOX_ROOT",
    "ART_PROBE_APPID",
    "ART_PROBE_KIND",
    "ART_PROBE_ROOT",
];

/// What the migration did, buffered until logging is up (R12).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MigrationReport {
    /// Steps ran in this process.
    pub ran: bool,
    /// The marker records every step done.
    pub finished: bool,
    /// Deprecations and problems the user must see.
    pub warnings: Vec<String>,
    /// Conflicts that kept old data in place.
    pub conflicts: Vec<String>,
    pub offline_libraries: Vec<PathBuf>,
    /// Symlink deployments not retargeted because the user changed them.
    pub skipped_links: Vec<PathBuf>,
    /// Informational lines (log only).
    pub notes: Vec<String>,
}

impl MigrationReport {
    /// Lines for the user (stderr / UI): warnings and conflicts.
    pub fn user_messages(&self) -> Vec<String> {
        self.warnings
            .iter()
            .chain(&self.conflicts)
            .cloned()
            .collect()
    }

    /// Logs the informational lines. Call only after logging init (the log
    /// lives in the migrated state dir).
    pub fn log_notes(&self) {
        for n in &self.notes {
            tracing::info!("migration: {n}");
        }
        if self.ran {
            tracing::info!(finished = self.finished, "migration ran");
        }
    }

    /// Logs everything, user messages as warnings. Call only after logging
    /// init.
    pub fn emit(&self) {
        self.log_notes();
        for w in self.user_messages() {
            tracing::warn!("migration: {w}");
        }
    }
}

/// Session-bus name ownership probe (R6). Injectable for tests.
pub trait BusProbe {
    fn has_owner(&self, name: &str) -> bool;
}

/// The real session bus; no bus or no answer within 3 s counts as "not owned".
#[derive(Debug, Clone, Copy, Default)]
pub struct SessionBus;

impl BusProbe for SessionBus {
    fn has_owner(&self, name: &str) -> bool {
        let name = name.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("migrate-dbus".into())
            .spawn(move || {
                let r = (|| -> zbus::Result<bool> {
                    let conn = zbus::blocking::Connection::session()?;
                    let proxy = zbus::blocking::fdo::DBusProxy::new(&conn)?;
                    let bus = zbus::names::BusName::try_from(name.as_str())?;
                    Ok(proxy.name_has_owner(bus)?)
                })();
                let _ = tx.send(r);
            });
        spawned.is_ok() && matches!(rx.recv_timeout(Duration::from_secs(3)), Ok(Ok(true)))
    }
}

/// Where an injected crash happens relative to the marker write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashPoint {
    /// Step work done, marker not yet updated (step reruns).
    BeforeRecord,
    AfterRecord,
}

/// Test seams. All off in production.
#[derive(Debug, Clone, Default)]
pub struct Faults {
    pub crash: Option<(&'static str, CrashPoint)>,
    /// Treat every rename of bulk data/staging as EXDEV.
    pub force_exdev: bool,
    /// Stop a fallback copy after this many files.
    pub partial_copy_after: Option<usize>,
    /// Fail inside the DB rewrite transaction, before commit.
    pub fail_db_rewrite: bool,
}

/// Opens the keyring store (Secret Service in production, a mock in tests).
pub type KeyringHook =
    Arc<dyn Fn() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> + Send + Sync>;

/// The installed Vessel programs the integrations point at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Programs {
    /// The `vessel` CLI (timer `ExecStart`, nxm handler `Exec`).
    pub cli: Option<PathBuf>,
    /// The `vessel-desktop` GUI (Steam shortcut).
    pub gui: Option<PathBuf>,
}

impl Programs {
    /// From this process: the GUI finds the CLI next to it or on `PATH`,
    /// and the CLI finds the GUI the same way. Unknown stays `None`; the
    /// step that needs it then retries on a later start.
    pub fn current() -> Self {
        let Ok(exe) = std::env::current_exe() else {
            return Self::default();
        };
        let gui_name = crate::ipc::GUI_ARGV0;
        if exe.file_name().is_some_and(|n| n == gui_name) {
            return Self {
                cli: crate::app::actions::cli_binary(&exe).ok(),
                gui: Some(exe),
            };
        }
        let gui = exe
            .parent()
            .map(|d| d.join(gui_name))
            .filter(|p| p.is_file())
            .or_else(|| crate::app::which(gui_name));
        Self {
            cli: Some(exe),
            gui,
        }
    }
}

/// Injected collaborators.
pub struct Hooks<'a> {
    pub bus: &'a dyn BusProbe,
    pub systemctl: &'a dyn Systemctl,
    pub keyring: KeyringHook,
    pub programs: Programs,
    pub faults: Faults,
}

/// Running migration state handed to every step.
pub struct Migration<'a> {
    pub paths: &'a Paths,
    pub hooks: &'a Hooks<'a>,
    pub marker: Marker,
    pub report: MigrationReport,
    marker_path: PathBuf,
}

impl Migration<'_> {
    pub fn sources(&self) -> &Sources {
        &self.marker.sources
    }

    pub fn save_marker(&self) -> Result<(), MigrateError> {
        self.marker.save(&self.marker_path)
    }
}

pub type StepFn = fn(&mut Migration<'_>) -> Result<StepState, MigrateError>;

/// One recorded step. A non-fatal step's error is reported, the step stays
/// pending (retried next run) and the run continues; cleanup then waits.
pub struct Step {
    pub name: &'static str,
    pub fatal: bool,
    pub run: StepFn,
}

/// Steps 3-9 of the plan (1-2 are detection and locking in [`run_with`]).
pub const CORE_STEPS: &[Step] = &[
    Step {
        name: "stop_timer",
        fatal: false,
        run: stop_timer,
    },
    Step {
        name: "wal_checkpoint",
        fatal: true,
        run: wal_checkpoint,
    },
    Step {
        name: "copy_state",
        fatal: true,
        run: copy_state,
    },
    Step {
        name: "move_bulk",
        fatal: true,
        run: move_bulk,
    },
    Step {
        name: "move_staging",
        fatal: true,
        run: move_staging,
    },
    Step {
        name: "rewrite_db",
        fatal: true,
        run: rewrite_db,
    },
    Step {
        name: "retarget_symlinks",
        fatal: true,
        run: retarget_symlinks,
    },
];

/// External integrations (keyring, systemd units, nxm handler, Steam
/// shortcut). Each is non-fatal: a failure is reported and retried on the
/// next start, and the old dirs are kept until it succeeds.
pub const INTEGRATION_STEPS: &[Step] = &[
    Step {
        name: "keyring",
        fatal: false,
        run: integrations::keyring,
    },
    Step {
        name: "systemd_units",
        fatal: false,
        run: integrations::systemd_units,
    },
    Step {
        name: "nxm_handler",
        fatal: false,
        run: integrations::nxm_handler,
    },
    Step {
        name: "steam_shortcut",
        fatal: false,
        run: integrations::steam_shortcut,
    },
];

/// Final step 10: removes the old dirs. Runs only when all others are done.
pub const CLEANUP: Step = Step {
    name: "cleanup",
    fatal: true,
    run: cleanup,
};

/// Steps re-run while a library is offline or in conflict.
const LIBRARY_STEPS: [&str; 3] = ["move_staging", "rewrite_db", "retarget_symlinks"];

/// The full ordered step list.
pub fn steps() -> Vec<&'static Step> {
    CORE_STEPS
        .iter()
        .chain(INTEGRATION_STEPS)
        .chain(std::iter::once(&CLEANUP))
        .collect()
}

/// Production entry point: the session bus and `systemctl --user` (or the
/// `VESSEL_SYSTEMCTL` stand-in).
pub fn run(paths: &Paths, env: &EnvSnapshot) -> Result<MigrationReport, MigrateError> {
    let ctl = crate::app::actions::systemctl();
    let hooks = Hooks {
        bus: &SessionBus,
        systemctl: &ctl,
        keyring: Arc::new(|| crate::nexus::keyring_opener()()),
        programs: Programs::current(),
        faults: Faults::default(),
    };
    run_with(paths, env, &hooks)
}

/// `vessel-migration.lock`, next to the Vessel data dir.
pub(crate) fn lock_path(paths: &Paths) -> PathBuf {
    paths
        .data_dir
        .parent()
        .unwrap_or(&paths.home)
        .join(LOCK_FILE)
}

/// `flock` on `vessel-migration.lock`, released explicitly on drop.
///
/// Closing the fd alone does not release an `flock` while a child forked by
/// another thread (between `fork` and its `O_CLOEXEC` `exec`) still shares
/// the open file description, so an immediate re-acquire could fail.
/// `LOCK_UN` releases it for every fd sharing the description.
pub(crate) struct MigrationLock(std::fs::File);

impl MigrationLock {
    /// Takes the lock without blocking; `LockBusy` if another holder exists.
    pub(crate) fn try_acquire(path: &Path) -> Result<Self, StoreError> {
        let io_err = |source| StoreError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io_err)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .map_err(io_err)?;
        match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => Ok(Self(file)),
            Err(e) if e == rustix::io::Errno::WOULDBLOCK => {
                Err(StoreError::LockBusy(path.to_path_buf()))
            }
            Err(e) => Err(io_err(e.into())),
        }
    }
}

impl Drop for MigrationLock {
    fn drop(&mut self) {
        let _ = rustix::fs::flock(&self.0, rustix::fs::FlockOperation::Unlock);
    }
}

/// Removes the migration lock file on drop while armed.
struct LockFileCleanup<'p> {
    path: &'p Path,
    armed: bool,
}

impl Drop for LockFileCleanup<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(self.path);
        }
    }
}

/// Legacy location next to a Vessel XDG dir (`.../vessel` -> `.../<legacy>`).
fn sibling(dir: &Path) -> PathBuf {
    dir.with_file_name(LEGACY_APP_DIR)
}

fn present_sources(s: &Sources) -> bool {
    [&s.config, &s.data, &s.state].iter().any(|p| p.exists())
}

fn nested(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

pub fn run_with(
    paths: &Paths,
    env: &EnvSnapshot,
    hooks: &Hooks<'_>,
) -> Result<MigrationReport, MigrateError> {
    let mut report = MigrationReport {
        warnings: env.warnings(),
        ..Default::default()
    };
    // 1) Detect: always the XDG siblings (legacy env vars are not sources).
    let detected = Sources {
        config: sibling(&paths.config_dir),
        data: sibling(&paths.data_dir),
        state: sibling(&paths.state_dir),
    };
    let marker_path = paths.config_dir.join(MARKER_FILE);
    let done = |m: &Option<Marker>| {
        m.as_ref()
            .is_some_and(|m| m.finished_at.is_some() && !m.has_waiting_libraries())
    };
    let marker = Marker::load(&marker_path)?;
    if done(&marker) {
        report.finished = true;
        return Ok(report);
    }
    if marker.is_none() && !present_sources(&detected) {
        return Ok(report); // fresh install: create nothing
    }
    let sources = marker
        .as_ref()
        .map_or_else(|| detected.clone(), |m| m.sources.clone());
    for (old, new) in [
        (&sources.config, &paths.config_dir),
        (&sources.data, &paths.data_dir),
        (&sources.state, &paths.state_dir),
    ] {
        if nested(old, new) {
            return Err(MigrateError::Marker(format!(
                "legacy dir {} and {} overlap; move one of them first",
                old.display(),
                new.display()
            )));
        }
    }

    // 2) Locks and liveness.
    let lock_path = lock_path(paths);
    let created_lock = !lock_path.exists();
    let _lock = MigrationLock::try_acquire(&lock_path).map_err(|e| match e {
        StoreError::LockBusy(p) => MigrateError::Busy(p),
        StoreError::Io { path, source } => MigrateError::Io { path, source },
        e => MigrateError::Marker(e.to_string()),
    })?;
    // Dropped before `_lock`: an abort or conflict leaves no lock file
    // behind when this run created it.
    let mut lock_file = LockFileCleanup {
        path: &lock_path,
        armed: created_lock,
    };
    let marker = Marker::load(&marker_path)?; // re-read under the lock
    if done(&marker) {
        report.finished = true;
        return Ok(report);
    }
    if marker.is_none() {
        let existing: Vec<_> = [&paths.config_dir, &paths.data_dir, &paths.state_dir]
            .into_iter()
            .filter(|p| p.exists())
            .collect();
        if !existing.is_empty() {
            report.conflicts.push(format!(
                "Vessel data already exists in {}; legacy data in {}, {} and {} was left untouched. \
                 Move one side away and restart to migrate.",
                existing
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                sources.config.display(),
                sources.data.display(),
                sources.state.display()
            ));
            return Ok(report);
        }
    }
    if hooks.bus.has_owner(LEGACY_DBUS_NAME) {
        return Err(MigrateError::LegacyRunning(format!(
            "its window is open: {LEGACY_DBUS_NAME}"
        )));
    }
    let _old_lock = if sources.data.is_dir() {
        Some(
            DeployLock::try_acquire(&sources.data.join("deploy.lock")).map_err(|e| match e {
                StoreError::LockBusy(p) => {
                    MigrateError::LegacyRunning(format!("it holds {}", p.display()))
                }
                e => MigrateError::Marker(e.to_string()),
            })?,
        )
    } else {
        None
    };

    lock_file.armed = false;
    let mut marker = marker.unwrap_or_else(|| Marker::new(sources));
    if marker.has_waiting_libraries() {
        for s in LIBRARY_STEPS {
            marker.steps.insert(s.into(), StepState::Pending);
        }
        marker.finished_at = None;
    }
    let mut m = Migration {
        paths,
        hooks,
        marker,
        report,
        marker_path,
    };
    m.save_marker()?;
    m.report.ran = true;

    let all = steps();
    for step in &all {
        if m.marker.step(step.name) == StepState::Done {
            continue;
        }
        if step.name == CLEANUP.name
            && all
                .iter()
                .any(|s| s.name != CLEANUP.name && m.marker.step(s.name) != StepState::Done)
        {
            m.report
                .notes
                .push("old data kept until every step is done; retrying next start".into());
            break;
        }
        let state = match (step.run)(&mut m) {
            Ok(state) => state,
            Err(e @ MigrateError::InjectedCrash(_)) => return Err(e),
            Err(e) if !step.fatal => {
                m.report.warnings.push(format!("{}: {e}", step.name));
                StepState::Pending
            }
            Err(e) => return Err(e),
        };
        let crash = |p| hooks.faults.crash == Some((step.name, p));
        if crash(CrashPoint::BeforeRecord) {
            return Err(MigrateError::InjectedCrash(step.name.into()));
        }
        m.marker.steps.insert(step.name.into(), state);
        m.save_marker()?;
        if crash(CrashPoint::AfterRecord) {
            return Err(MigrateError::InjectedCrash(step.name.into()));
        }
    }
    if all.iter().all(|s| m.marker.step(s.name) == StepState::Done) {
        m.marker.finished_at = Some(crate::store::now());
        m.save_marker()?;
        m.report.finished = !m.marker.has_waiting_libraries();
    }
    Ok(m.report)
}

// ---- steps ----

/// 3) Stop and disable the old timer (unit files stay for `systemd_units`).
///
/// Whether it was enabled is recorded first, and saved before `disable`
/// erases it, so the Vessel timer can match.
fn stop_timer(m: &mut Migration<'_>) -> Result<StepState, MigrateError> {
    let timer = format!("{LEGACY_UNIT}.timer");
    let dir = crate::autoupdate::systemd_user_dir(m.paths);
    if dir.join(&timer).exists() {
        if m.marker.legacy_timer_enabled.is_none() {
            let enabled = matches!(
                m.hooks.systemctl.run(&["is-enabled", &timer]),
                Ok(s) if s.starts_with("enabled")
            ) || dir.join("timers.target.wants").join(&timer).exists();
            m.marker.legacy_timer_enabled = Some(enabled);
            m.save_marker()?;
        }
        if let Err(e) = m.hooks.systemctl.run(&["disable", "--now", &timer]) {
            m.report
                .warnings
                .push(format!("could not stop {timer}: {e}"));
        }
    }
    // A run already started by the timer must not write the old DB while
    // it is checkpointed and copied.
    let service = format!("{LEGACY_UNIT}.service");
    if dir.join(&service).exists() {
        if let Err(e) = m.hooks.systemctl.run(&["stop", &service]) {
            m.report
                .warnings
                .push(format!("could not stop {service}: {e}"));
        }
    }
    Ok(StepState::Done)
}

/// 4) Fold the old WAL into the DB file.
fn wal_checkpoint(m: &mut Migration<'_>) -> Result<StepState, MigrateError> {
    let db = m.sources().data.join(LEGACY_DB_FILE);
    if db.is_file() {
        db::checkpoint(&db)?;
    }
    Ok(StepState::Done)
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Copies the tree `src` into `dst` file by file (verified), renaming a
/// top-level file per `rename`, and records each source hash.
fn copy_dir(
    m: &mut Migration<'_>,
    src: &Path,
    dst: &Path,
    rename: Option<(&str, &str)>,
) -> Result<(), MigrateError> {
    if !src.is_dir() {
        return Ok(());
    }
    for (rel, entry) in fsops::manifest(src)? {
        let from = src.join(&rel);
        let rel = match rename {
            Some((a, b)) if rel == Path::new(a) => PathBuf::from(b),
            _ => rel,
        };
        let to = dst.join(&rel);
        match entry {
            fsops::Entry::Dir => {
                std::fs::create_dir_all(&to).map_err(|e| MigrateError::io(&to, e))?;
            }
            fsops::Entry::Link(target) => {
                fsops::remove_all(&to)?;
                std::os::unix::fs::symlink(target, &to).map_err(|e| MigrateError::io(&to, e))?;
            }
            fsops::Entry::File { .. } => {
                let sha = fsops::copy_file_verified(&from, &to)?;
                m.marker.copied.insert(from.to_string_lossy().into(), sha);
            }
        }
    }
    Ok(())
}

/// 5) Copy config dir, DB and log; DB -> vessel.db, log -> vessel.log.
fn copy_state(m: &mut Migration<'_>) -> Result<StepState, MigrateError> {
    let s = m.sources().clone();
    let paths = m.paths;
    copy_dir(m, &s.config, &paths.config_dir, None)?;

    let new_db = paths.db_path();
    // Any vessel.db here is a partial copy of ours (the marker exists and
    // this step is not done); a stale -wal must never pair with a new copy.
    for sfx in ["", "-wal", "-shm"] {
        fsops::remove_all(&with_suffix(&new_db, sfx))?;
    }
    let old_db = s.data.join(LEGACY_DB_FILE);
    if old_db.is_file() {
        for sfx in ["", "-wal"] {
            let from = with_suffix(&old_db, sfx);
            if from.metadata().is_ok_and(|md| md.len() > 0) {
                let sha = fsops::copy_file_verified(&from, &with_suffix(&new_db, sfx))?;
                m.marker.copied.insert(from.to_string_lossy().into(), sha);
            }
        }
    }

    let log_name = paths.log_path();
    let log_name = log_name.file_name().and_then(|n| n.to_str()).unwrap_or("");
    copy_dir(
        m,
        &s.state,
        &paths.state_dir,
        Some((LEGACY_LOG_FILE, log_name)),
    )?;
    Ok(StepState::Done)
}

/// Names in the old data dir that are copied (DB) or dropped (lock), not moved.
fn kept_in_old_data(name: &str) -> bool {
    name == "deploy.lock"
        || name.ends_with(fsops::MIGRATING_SUFFIX)
        || ["", "-wal", "-shm"]
            .iter()
            .any(|s| name == format!("{LEGACY_DB_FILE}{s}"))
}

/// 6) Move downloads, backups and per-game dirs entry by entry.
fn move_bulk(m: &mut Migration<'_>) -> Result<StepState, MigrateError> {
    let src = m.sources().data.clone();
    if !src.is_dir() {
        return Ok(StepState::Done);
    }
    let dst = m.paths.data_dir.clone();
    std::fs::create_dir_all(&dst).map_err(|e| MigrateError::io(&dst, e))?;
    let mut names: Vec<_> = std::fs::read_dir(&src)
        .map_err(|e| MigrateError::io(&src, e))?
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .filter(|n| !kept_in_old_data(&n.to_string_lossy()))
        .collect();
    names.sort();
    let mut state = StepState::Done;
    for n in names {
        let (from, to) = (src.join(&n), dst.join(&n));
        match fsops::move_entry(&from, &to, &m.hooks.faults)? {
            fsops::Moved::Conflict => {
                m.report.conflicts.push(format!(
                    "{} and {} differ; both kept",
                    from.display(),
                    to.display()
                ));
                state = StepState::Conflict;
            }
            fsops::Moved::Copied => m
                .report
                .notes
                .push(format!("copied {} across filesystems", from.display())),
            _ => {}
        }
    }
    Ok(state)
}

/// 7) Rename `<lib>/<legacy staging>` to `<lib>/.vessel/staging` (R4).
fn move_staging(m: &mut Migration<'_>) -> Result<StepState, MigrateError> {
    for lib in db::libraries(&m.paths.db_path())? {
        let prev = m.marker.libraries.get(&lib).copied();
        if prev == Some(LibraryState::Done) {
            continue;
        }
        let (old, new) = (lib.join(LEGACY_STAGING_DIR), lib.join(STAGING_DIR));
        if !lib.is_dir() {
            m.marker
                .libraries
                .insert(lib.clone(), LibraryState::Offline);
            m.report.offline_libraries.push(lib);
            continue;
        }
        if prev != Some(LibraryState::Pending) && fsops::present(&old) && fsops::present(&new) {
            m.marker
                .libraries
                .insert(lib.clone(), LibraryState::Conflict);
            m.report.conflicts.push(format!(
                "{} already exists; {} was left in place",
                new.display(),
                old.display()
            ));
            continue;
        }
        m.marker
            .libraries
            .insert(lib.clone(), LibraryState::Pending);
        m.save_marker()?;
        let state = match fsops::move_entry(&old, &new, &m.hooks.faults)? {
            fsops::Moved::Conflict => {
                m.report.conflicts.push(format!(
                    "{} and {} differ; both kept",
                    old.display(),
                    new.display()
                ));
                LibraryState::Conflict
            }
            moved => {
                if moved == fsops::Moved::Copied {
                    m.report.notes.push(format!(
                        "staging in {} was copied across filesystems; hardlink deployments \
                         no longer share storage with it",
                        lib.display()
                    ));
                }
                if let Some(parent) = old.parent() {
                    let _ = std::fs::remove_dir(parent); // only if empty
                }
                LibraryState::Done
            }
        };
        m.marker.libraries.insert(lib, state);
    }
    Ok(StepState::Done)
}

fn prefix_map(m: &Migration<'_>) -> db::PrefixMap {
    let mut map = db::PrefixMap::default();
    map.push(&m.sources().data, &m.paths.data_dir);
    for (lib, s) in &m.marker.libraries {
        if *s == LibraryState::Done {
            map.push(&lib.join(LEGACY_STAGING_DIR), &lib.join(STAGING_DIR));
        }
    }
    map
}

/// 8) Rewrite stored paths in one transaction.
fn rewrite_db(m: &mut Migration<'_>) -> Result<StepState, MigrateError> {
    let dbp = m.paths.db_path();
    if dbp.is_file() {
        let n = db::rewrite(&dbp, &prefix_map(m), &m.hooks.faults)?;
        m.report.notes.push(format!("rewrote {n} stored paths"));
    }
    Ok(StepState::Done)
}

/// 9) Retarget symlink deployments at the moved staging (R7).
fn retarget_symlinks(m: &mut Migration<'_>) -> Result<StepState, MigrateError> {
    let r = db::retarget(&m.paths.db_path(), &prefix_map(m))?;
    for l in &r.skipped {
        m.report.warnings.push(format!(
            "deployed link {} was changed outside Vessel and was not updated",
            l.display()
        ));
    }
    m.report.skipped_links.extend(r.skipped);
    if r.retargeted > 0 {
        m.report
            .notes
            .push(format!("retargeted {} symlink deployments", r.retargeted));
    }
    Ok(StepState::Done)
}

/// True when every file in `dir` was copied and is unchanged since.
fn unchanged_since_copy(m: &Migration<'_>, dir: &Path) -> Result<bool, MigrateError> {
    for (rel, e) in fsops::manifest(dir)? {
        if let fsops::Entry::File { sha256, .. } = e {
            let key = dir.join(rel).to_string_lossy().into_owned();
            if m.marker.copied.get(&key) != Some(&sha256) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// 10) Remove the old dirs, each only if unchanged since its verified copy.
fn cleanup(m: &mut Migration<'_>) -> Result<StepState, MigrateError> {
    let s = m.sources().clone();
    for dir in [&s.config, &s.state] {
        if !dir.exists() {
            continue;
        }
        if unchanged_since_copy(m, dir)? {
            fsops::remove_all(dir)?;
        } else {
            m.report.warnings.push(format!(
                "{} changed after it was copied and was kept; remove it by hand",
                dir.display()
            ));
        }
    }
    if s.data.is_dir() {
        let db = s.data.join(LEGACY_DB_FILE);
        let copied = |p: &Path| -> Result<bool, MigrateError> {
            Ok(match m.marker.copied.get(&*p.to_string_lossy()) {
                Some(h) => p.is_file() && fsops::sha256_file(p)? == *h,
                None => p.metadata().map_or(true, |md| md.len() == 0),
            })
        };
        let wal = with_suffix(&db, "-wal");
        if copied(&db)? && copied(&wal)? {
            for p in [
                &db,
                &wal,
                &with_suffix(&db, "-shm"),
                &s.data.join("deploy.lock"),
            ] {
                fsops::remove_all(p)?;
            }
            if std::fs::remove_dir(&s.data).is_err() {
                m.report
                    .warnings
                    .push(format!("{} is not empty and was kept", s.data.display()));
            }
        } else {
            m.report.warnings.push(format!(
                "{} changed after it was copied and was kept",
                db.display()
            ));
        }
    }
    Ok(StepState::Done)
}
