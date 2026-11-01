// SPDX-License-Identifier: GPL-3.0-or-later
//! Mod loaders and script extenders (SMAPI, BepInEx, SKSE, ...): archives
//! that install into the game root or change how the game launches, instead
//! of being deployed as ordinary mods.
//!
//! One contract, [`LoaderPlugin`], serves both Rust plugins (vendor
//! installers) and declarative `[[loaders]]` tables in the game TOML
//! ([`declarative::DeclarativeLoader`]). Plugins only describe work as a
//! [`LoaderPlan`]; core owns consent, spawning, snapshots and rollback.

mod codes;
pub mod declarative;
pub mod glob;
pub mod housekeeping;
pub mod launch;
pub mod records;
pub mod registry;
pub mod runner;
pub mod runtime;
pub mod smapi;
pub mod snapshot;
pub mod types;

use std::path::{Path, PathBuf};

pub use declarative::{DeclarativeLoader, LoaderDef};
pub use glob::Glob;
pub use registry::LoaderRegistry;
pub use types::*;

use crate::store::{Store, StoreError};

/// Errors from loader planning and bookkeeping.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoaderError {
    #[error("'{path}' is already owned by {}", owner_text(.owner))]
    Conflict { path: String, owner: Owner },
    #[error("{op} needs the extracted loader archive")]
    ArchiveRequired { op: &'static str },
    #[error("loader '{key}': {message}")]
    Payload { key: String, message: String },
    #[error("unsafe path '{path}': {message}")]
    UnsafePath { path: String, message: String },
    #[error("argument '{0}' contains a character cmd.exe would interpret")]
    UnsafeForCmd(String),
    #[error("loader '{0}' is already installed outside Vessel")]
    ManualInstallDetected(String),
    #[error("archive matches more than one loader ({0} and {1})")]
    AmbiguousMatch(String, String),
    #[error("loader '{0}' is registered twice")]
    DuplicateKey(String),
    #[error("unknown loader '{0}'")]
    UnknownLoader(String),
    #[error(
        "no Proton compatibility tool is mapped for app {appid} and there is no global default"
    )]
    NoCompatTool { appid: u32 },
    #[error("compatibility tool '{0}' was not found in any Steam library")]
    CompatToolMissing(String),
    #[error("refusing Proton at {path}: {reason}")]
    ProtonUntrusted { path: PathBuf, reason: String },
    #[error("Proton needs Steam Linux Runtime (app {appid}); install it from Steam first")]
    SlrMissing { appid: u32 },
    #[error(
        "running Windows installers through Proton is not supported while Vessel runs as a Flatpak"
    )]
    FlatpakUnsupported,
    #[error("running Windows installers through Flatpak Steam's Proton is not supported")]
    FlatpakSteamUnsupported,
    #[error("the game is running (pid {pid}); close it first")]
    GameRunning { pid: u32 },
    #[error("Steam reports the game is not fully installed or needs an update (StateFlags {flags}); update it in Steam first")]
    NotFullyInstalled { flags: u32 },
    #[error("the game's Proton prefix {0} does not exist; start the game once from Steam")]
    PrefixMissing(PathBuf),
    #[error("cannot tell whether the game runs natively or through Proton: {0}")]
    UnknownRuntime(String),
    #[error("{kind} installers cannot run on the {host} host")]
    HostMismatch {
        kind: &'static str,
        host: &'static str,
    },
    #[error("cannot start {program}: {source}")]
    Spawn {
        program: PathBuf,
        source: std::io::Error,
    },
    #[error("not a valid {loader} archive: {reason}")]
    LayoutMismatch {
        loader: &'static str,
        reason: String,
    },
    #[error("{op} did not complete: {reason}")]
    VerifyFailed {
        op: &'static str,
        reason: String,
        /// The fix is to verify the game files in Steam (a game file is
        /// not vanilla); the UI offers that action.
        steam_hint: bool,
    },
    #[error("consent check failed: {0}; preview the operation again")]
    Consent(String),
    #[error("loader '{0}' is already installed; upgrade or repair it instead")]
    AlreadyInstalled(String),
    #[error("loader '{0}' is not installed")]
    NotInstalled(String),
    #[error(transparent)]
    Launch(#[from] launch::LaunchError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

fn owner_text(owner: &Owner) -> String {
    match owner {
        Owner::Deployed => "a deployed mod".into(),
        Owner::Loader(k) => format!("loader '{k}'"),
    }
}

impl From<rusqlite::Error> for LoaderError {
    fn from(e: rusqlite::Error) -> Self {
        LoaderError::Store(e.into())
    }
}

pub(crate) fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> LoaderError + '_ {
    move |source| LoaderError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// A loader Vessel knows how to install, detect and remove.
pub trait LoaderPlugin: Send + Sync {
    /// Stable key, equal to the requirements tool key (`smapi`, ...).
    fn key(&self) -> &str;

    /// Claims an archive, or `None` when it is not this loader's.
    fn matches(&self, game: &GameCtx, probe: &ArchiveProbe) -> Option<LoaderMatch>;

    /// Provenance verdict for a matched archive.
    fn provenance(&self, game: &GameCtx, m: &LoaderMatch, origin: &DownloadOrigin) -> Provenance;

    /// What `op` would do. `extracted` is the extracted archive; it may be
    /// `None` only for [`Op::Uninstall`].
    fn plan(
        &self,
        op: Op,
        game: &GameCtx,
        runtime: &Runtime,
        extracted: Option<&Path>,
    ) -> Result<LoaderPlan, LoaderError>;

    /// Current state for `game`.
    fn detect(&self, game: &GameCtx, store: &Store) -> Result<LoaderStatus, LoaderError>;

    /// The vanilla game file this loader replaces, if any. Core hashes it
    /// before repair, upgrade and uninstall to spot a Steam update.
    fn vanilla(&self) -> Option<VanillaSpec> {
        None
    }

    /// Post-run success check for `op` against the game folder (`post` is
    /// its snapshot). Decides success when the installer's exit code is
    /// not reliable; run for every [`Step::Verify`]. Must not spawn.
    fn verify(
        &self,
        _op: Op,
        _game: &GameCtx,
        _runtime: &Runtime,
        _post: &snapshot::Manifest,
    ) -> Result<(), LoaderError> {
        Ok(())
    }
}
