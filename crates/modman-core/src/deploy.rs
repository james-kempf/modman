// SPDX-License-Identifier: GPL-3.0-or-later
//! M2: deployment engine (tech stack §4.1; legacy `cmd_deploy`/`undeploy`/
//! `cmd_verify`, L1574-1991).
//!
//! - Staging lives per Steam library (`<library>/.modman/staging/<domain>`) so
//!   hardlinks stay on one filesystem; see [`staging_root`].
//! - The plan has one winner per path (case-insensitive), decided by mod
//!   priority; later wins. Conflicts are listed by [`Plan::conflicts`].
//! - Files are placed with the chain hardlink -> reflink -> symlink -> copy.
//!   Hardlink and reflink are skipped when staging and target differ in
//!   `st_dev`. The method used is recorded per file in `deployments`.
//! - Replaced game files go to a content-addressed backup store, written and
//!   fsynced before the game file is touched.
//! - Every deploy and purge runs under a journal in SQLite. An interrupted
//!   deploy is rolled back and an interrupted purge is resumed by
//!   [`recover`], which [`deploy`] and [`purge`] call first.
//! - Deploy and purge refuse while the game runs ([`GameProbe`]) or while
//!   another process holds the deploy lock.

mod backup;
mod confine;
mod engine;
mod place;
mod plan;
mod probe;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use engine::{deploy, purge, recover, status, verify};
pub use plan::{build_plan, conflicts, Conflict, ModRef, Plan, PlanItem, SkippedMod};
pub use probe::{GameProbe, NeverRunning, ProcProbe};

/// Checks a game-relative folder setting (`subdir`, `paks_dir`): relative,
/// no `.`/`..` components, no NUL. `.` alone and a leading `./` (legacy
/// spelling) are accepted.
pub fn check_rel_dir(value: &str) -> Result<(), &'static str> {
    let v = value.trim_start_matches("./").trim_end_matches('/');
    if v.is_empty() || v == "." {
        return Ok(());
    }
    confine::components(v).map(drop)
}

use crate::games::{GameConfig, GamesError, Target};
use crate::paths::Paths;
use crate::store::StoreError;

/// Errors from deployment.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DeployError {
    #[error("{0} is running; close the game before deploying or purging")]
    GameRunning(String),
    #[error("another modman process is deploying or purging (lock {0} is held)")]
    LockBusy(PathBuf),
    #[error("no {what} set for {game}")]
    NoTargetRoot { game: String, what: &'static str },
    #[error("target folder {0} does not exist")]
    TargetRootMissing(PathBuf),
    #[error("invalid deploy method '{0}' (expected auto|hardlink|reflink|symlink|copy)")]
    InvalidMethod(String),
    #[error("cannot deploy {rel}: {reason}")]
    PathConflict { rel: String, reason: String },
    #[error("could not place {path} with any method: {source}")]
    Place {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Test fault injection: the process "crashed" and left the journal open.
    #[error("simulated crash (fault injection)")]
    InjectedCrash,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Games(#[from] GamesError),
}

impl From<rusqlite::Error> for DeployError {
    fn from(e: rusqlite::Error) -> Self {
        DeployError::Store(StoreError::Sqlite(e))
    }
}

pub(crate) fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> DeployError + '_ {
    move |source| DeployError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// How a file was placed in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Method {
    Hardlink,
    Reflink,
    Symlink,
    Copy,
}

impl Method {
    /// The automatic fallback chain, in order.
    pub const CHAIN: [Method; 4] = [
        Method::Hardlink,
        Method::Reflink,
        Method::Symlink,
        Method::Copy,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Method::Hardlink => "hardlink",
            Method::Reflink => "reflink",
            Method::Symlink => "symlink",
            Method::Copy => "copy",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::CHAIN.into_iter().find(|m| m.as_str() == s)
    }

    /// Needs source and target on the same filesystem.
    pub fn same_device_only(self) -> bool {
        matches!(self, Method::Hardlink | Method::Reflink)
    }
}

/// Requested deploy method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MethodChoice {
    /// hardlink -> reflink -> symlink -> copy.
    #[default]
    Auto,
    /// This method, falling back to copy if it fails for a file.
    Only(Method),
}

impl MethodChoice {
    pub fn parse(s: &str) -> Result<Self, DeployError> {
        match s {
            "auto" | "" => Ok(MethodChoice::Auto),
            m => Method::parse(m)
                .map(MethodChoice::Only)
                .ok_or_else(|| DeployError::InvalidMethod(s.to_string())),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            MethodChoice::Auto => "auto",
            MethodChoice::Only(m) => m.as_str(),
        }
    }

    /// Methods to try for each file, in order.
    pub fn chain(self) -> Vec<Method> {
        match self {
            MethodChoice::Auto => Method::CHAIN.to_vec(),
            MethodChoice::Only(Method::Copy) => vec![Method::Copy],
            MethodChoice::Only(m) => vec![m, Method::Copy],
        }
    }
}

/// `--method` override, then the game's method, then the global
/// `deploy-method` setting (legacy `resolve_method`, L1574-1584).
pub fn resolve_method(
    override_: Option<&str>,
    game: &GameConfig,
    global: &str,
) -> Result<MethodChoice, DeployError> {
    let s = override_
        .or(game.method.as_deref().filter(|m| !m.is_empty()))
        .unwrap_or(global);
    MethodChoice::parse(s)
}

/// Steam library holding `folder` (`<lib>/steamapps/common/<game>`).
fn library_of(folder: &Path) -> Option<PathBuf> {
    let common = folder.parent()?;
    let steamapps = common.parent()?;
    (common.file_name()? == "common" && steamapps.file_name()? == "steamapps")
        .then(|| steamapps.parent().map(Path::to_path_buf))
        .flatten()
}

/// Where staged mod trees of `game` live: `<library>/.modman/staging/<domain>`
/// for a Steam game (so hardlinks work on SD-card libraries), else
/// `<data_dir>/<domain>/staging`.
pub fn staging_root(game: &GameConfig, data_dir: &Path) -> PathBuf {
    game.library
        .clone()
        .or_else(|| game.folder.as_deref().and_then(library_of))
        .map_or_else(
            || data_dir.join(&game.domain).join("staging"),
            |lib| lib.join(".modman/staging").join(&game.domain),
        )
}

/// Root folder that `target` paths are relative to.
pub fn target_root(game: &GameConfig, target: Target) -> Result<PathBuf, DeployError> {
    let (root, what) = match target {
        Target::GameDir => (game.folder.clone(), "game folder"),
        Target::PrefixDir => (game.prefix.clone(), "Proton prefix"),
    };
    let root = root.ok_or_else(|| DeployError::NoTargetRoot {
        game: game.domain.clone(),
        what,
    })?;
    if !root.is_dir() {
        return Err(DeployError::TargetRootMissing(root));
    }
    Ok(root)
}

/// Test-only fault injection.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct Faults {
    /// "Crash" (return [`DeployError::InjectedCrash`] without cleanup) once
    /// this many files have been placed (deploy) or undone (purge).
    pub crash_after_files: Option<usize>,
    /// Files per journal batch (default 256).
    pub batch_size: Option<usize>,
    /// Pretend `method` fails for the target path when this returns true.
    #[allow(clippy::type_complexity)]
    pub deny: Option<Arc<dyn Fn(Method, &Path) -> bool + Send + Sync>>,
}

impl std::fmt::Debug for Faults {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Faults")
            .field("crash_after_files", &self.crash_after_files)
            .field("batch_size", &self.batch_size)
            .field("deny", &self.deny.is_some())
            .finish()
    }
}

/// Where the engine keeps its own files, and how it detects the game.
#[derive(Clone)]
pub struct DeployEnv {
    pub lock_path: PathBuf,
    /// Content-addressed backups: `<backup_dir>/<sha[..2]>/<sha>`.
    pub backup_dir: PathBuf,
    /// Files the game changed after deploy end up here on purge.
    pub overwrite_dir: PathBuf,
    pub probe: Arc<dyn GameProbe>,
    #[doc(hidden)]
    pub faults: Faults,
}

impl DeployEnv {
    /// Standard locations: lock in the XDG data dir, backups and overwrite
    /// under the `data-dir` setting.
    pub fn new(paths: &Paths, data_dir: &Path, domain: &str) -> Self {
        Self {
            lock_path: paths.deploy_lock_path(),
            backup_dir: data_dir.join("backups"),
            overwrite_dir: data_dir.join(domain).join("overwrite"),
            probe: Arc::new(ProcProbe::default()),
            faults: Faults::default(),
        }
    }
}

/// Progress event (files done of total) for deploy and purge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Default)]
pub struct DeployOptions {
    pub method: MethodChoice,
    /// Report what would happen; touch nothing (no lock, journal or files).
    pub dry_run: bool,
    pub progress: Option<std::sync::mpsc::Sender<Progress>>,
}

/// Result of [`deploy`].
#[derive(Debug, Clone, Default)]
pub struct DeployReport {
    pub dry_run: bool,
    pub method: MethodChoice,
    /// Mods contributing to the plan.
    pub mods: usize,
    pub skipped: Vec<SkippedMod>,
    pub conflicts: Vec<Conflict>,
    /// Files placed (or that would be, on a dry run).
    pub placed: usize,
    /// Original game files backed up (or that would be).
    pub backed_up: usize,
    /// Folders created (or that would be).
    pub new_dirs: usize,
    pub by_method: BTreeMap<Method, usize>,
    /// The previous deployment removed before this one.
    pub purged: Option<PurgeReport>,
    pub recovered: Option<RecoveryReport>,
}

/// Result of [`purge`] (legacy summary: removed, restored, missing, changed,
/// kept).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PurgeReport {
    pub nothing_deployed: bool,
    /// Deployed files deleted.
    pub removed: usize,
    /// Original game files put back.
    pub restored: usize,
    /// Deployed files that were already gone.
    pub missing: usize,
    /// Deployed files changed since deploy (kept or moved to overwrite).
    pub changed: usize,
    /// Backups not restored because the path is occupied.
    pub kept: usize,
    pub recovered: Option<Box<RecoveryReport>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// An interrupted deploy was undone.
    RolledBack,
    /// An interrupted purge was finished.
    Resumed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    pub journal_id: i64,
    pub op: String,
    pub outcome: RecoveryOutcome,
    pub undo: PurgeReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VerifyKind {
    Missing,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyIssue {
    pub target: Target,
    pub rel_path: String,
    pub method: Method,
    pub kind: VerifyKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerifyReport {
    pub intact: usize,
    pub issues: Vec<VerifyIssue>,
    /// A deploy or purge was interrupted and not yet recovered.
    pub interrupted: bool,
}

/// Deployment state of one game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeployStatus {
    pub active: bool,
    pub files: usize,
    pub by_method: BTreeMap<Method, usize>,
    pub backups_held: usize,
    pub dirs: usize,
    pub last_deployed_at: Option<i64>,
    pub interrupted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_names_roundtrip_and_match_config_values() {
        for m in Method::CHAIN {
            assert_eq!(Method::parse(m.as_str()), Some(m));
            assert!(crate::config::DEPLOY_METHODS.contains(&m.as_str()));
        }
        assert_eq!(MethodChoice::parse("auto").unwrap(), MethodChoice::Auto);
    }

    #[test]
    fn chains_end_in_copy() {
        assert_eq!(MethodChoice::Auto.chain(), Method::CHAIN.to_vec());
        assert_eq!(
            MethodChoice::Only(Method::Symlink).chain(),
            [Method::Symlink, Method::Copy]
        );
        assert_eq!(MethodChoice::Only(Method::Copy).chain(), [Method::Copy]);
    }

    #[test]
    fn library_is_derived_only_from_steamapps_common() {
        assert_eq!(
            library_of(Path::new("/l/steamapps/common/G")),
            Some(PathBuf::from("/l"))
        );
        assert_eq!(library_of(Path::new("/games/G")), None);
    }
}
