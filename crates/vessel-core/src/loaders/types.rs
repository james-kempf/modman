// SPDX-License-Identifier: GPL-3.0-or-later
//! Data types of the loader contract. Everything here is plain data: plugins
//! describe what should happen, and only core turns a plan into processes or
//! file writes. Plans are serialisable because the consent hash covers them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::glob::Glob;

/// What a plan is for. Every op that spawns or writes goes through a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Install,
    Upgrade,
    Repair,
    Uninstall,
    /// Take over a loader that was installed by hand.
    Adopt,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Install => "install",
            Op::Upgrade => "upgrade",
            Op::Repair => "repair",
            Op::Uninstall => "uninstall",
            Op::Adopt => "adopt",
        }
    }

    /// Ops that may plan without an extracted archive.
    pub fn needs_archive(self) -> bool {
        !matches!(self, Op::Uninstall)
    }
}

/// Who owns a game-root path that a loader might want to write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "key")]
pub enum Owner {
    /// Placed by the Vessel deploy engine (a mod file).
    Deployed,
    /// Tracked in `loader_files` of another active loader install.
    Loader(String),
}

/// A game-root relative path that is already taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occupied {
    /// `/`-separated, relative to the game folder.
    pub path: String,
    pub owner: Owner,
}

/// The game a loader op runs against.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameCtx {
    pub game_id: String,
    pub nexus_domain: Option<String>,
    /// Absolute game install folder (the loader root).
    pub game_dir: PathBuf,
    pub steam_appid: Option<u32>,
    /// Paths owned by deployed mods or other loaders, for conflict checks.
    /// Filled by the caller (see `Store::occupied_paths`).
    pub occupied: Vec<Occupied>,
}

/// What core knows about an archive before any loader claims it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveProbe {
    pub file_name: String,
    pub nexus_mod_id: Option<u64>,
    pub nexus_file_id: Option<u64>,
    /// Every entry, `/`-separated and relative to the archive root.
    pub entries: Vec<String>,
}

/// A loader's claim on an archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoaderMatch {
    pub loader_key: String,
    /// Version read from the archive, when the plugin can tell.
    pub version: Option<String>,
}

/// Where an archive came from. Only `Nexus` and `GitHub` downloads made by
/// Vessel itself can be verified; anything the user hands over is `Local`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DownloadOrigin {
    Nexus {
        domain: String,
        mod_id: u64,
        file_id: u64,
    },
    #[serde(rename = "github")]
    GitHub {
        /// `owner/repo`.
        repo: String,
        tag: String,
        asset: String,
    },
    Local {
        path: PathBuf,
    },
}

impl DownloadOrigin {
    /// Value stored in `loader_installs.origin`.
    pub fn as_str(&self) -> &'static str {
        match self {
            DownloadOrigin::Nexus { .. } => "nexus",
            DownloadOrigin::GitHub { .. } => "github",
            DownloadOrigin::Local { .. } => "local",
        }
    }
}

/// Provenance verdict. The layout check is separate and always enforced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Provenance {
    Verified {
        origin: DownloadOrigin,
    },
    /// Allowed only with `unverified_ack` inside the hashed plan.
    Unverified {
        origin: DownloadOrigin,
        reason: String,
    },
}

impl Provenance {
    pub fn is_verified(&self) -> bool {
        matches!(self, Provenance::Verified { .. })
    }
}

/// A Proton install to run Windows programs under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtonEnv {
    /// The `proton` script of the mapped compat tool.
    pub proton: PathBuf,
    /// `steamapps/compatdata/<appid>` (the prefix is `<compat_data>/pfx`).
    pub compat_data: PathBuf,
    /// Steam client root (`STEAM_COMPAT_CLIENT_INSTALL_PATH`).
    pub client_root: PathBuf,
    pub app_id: u32,
    /// Steam Linux Runtime `run` script the tool requires
    /// (`toolmanifest.vdf` `require_tool_appid`); `None` = the tool needs none.
    pub slr: Option<PathBuf>,
}

/// How the game runs on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Runtime {
    Native,
    Proton(ProtonEnv),
    Unknown,
}

impl Runtime {
    /// Value stored in `loader_installs.runtime` / `games.runtime_override`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Runtime::Native => "native",
            Runtime::Proton(_) => "proton",
            Runtime::Unknown => "unknown",
        }
    }
}

/// Kind of program an installer target is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    BashScript,
    BatScript,
    Exe,
}

/// The program to run, relative to the preview scratch dir. Core
/// canonicalises it inside that dir and refuses symlinked components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallerTarget {
    pub rel_path: String,
    pub kind: TargetKind,
}

/// One installer argument. Paths are filled in by core, never by the plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum Arg {
    Lit(String),
    /// The game folder as a host path.
    GamePath,
    /// The game folder as a Wine path (`Z:\...`).
    GamePathWin,
}

/// Where the installer runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Host {
    Native,
    Proton(ProtonEnv),
}

/// A vendor installer run. Only core turns `host` + `target` into argv.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallerSpec {
    pub target: InstallerTarget,
    pub args: Vec<Arg>,
    pub host: Host,
    /// `false` when the vendor installer can exit 0 on failure; success is
    /// then decided by the plugin's post-snapshot check, not the exit code.
    #[serde(default = "yes")]
    pub exit_code_reliable: bool,
    /// The installer leaves temp files behind: core gives it a per-run
    /// `TMPDIR` under the Vessel data dir and cleans that up afterwards.
    #[serde(default)]
    pub private_tmp: bool,
}

fn yes() -> bool {
    true
}

/// `from` -> `to` prefix rewrite applied to payload paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathMap {
    pub from: String,
    pub to: String,
}

/// Unix mode for payload files matching `glob`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeRule {
    pub glob: Glob,
    pub mode: u32,
}

/// Declarative payload: which archive files go where under the game root.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadRecipe {
    /// Nested folder of the archive that holds the payload (glob, must
    /// match exactly one folder). `None` = the archive root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<Glob>,
    /// Payload paths (relative to the selected folder) to leave out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<Glob>,
    /// Prefix rewrites; the first matching rule wins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub map: Vec<PathMap>,
    /// Per-file modes; the first matching rule wins, no match = default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modes: Vec<ModeRule>,
}

/// One payload file, resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedFile {
    /// Absolute path in the extracted archive.
    pub source: PathBuf,
    /// `/`-separated, relative to the game root.
    pub dest: String,
    /// `None` = the deploy default.
    pub mode: Option<u32>,
}

/// Payload files to place under the game root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeployFiles {
    pub recipe: PayloadRecipe,
    pub files: Vec<PlannedFile>,
}

/// Launch changes a loader needs. All parts are optional; core composes
/// them into one Steam launch-options string per game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchChange {
    /// Game-root relative program to run instead of the game exe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exe: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Wine DLL overrides (`winhttp` -> `winhttp=n,b`). Proton only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dll_overrides: Vec<String>,
    /// A literal launch-options string, used as is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam_launch_option: Option<String>,
}

impl LaunchChange {
    pub fn is_empty(&self) -> bool {
        self.exe.is_none()
            && self.env.is_empty()
            && self.dll_overrides.is_empty()
            && self.steam_launch_option.is_none()
    }

    /// The parts that apply to `runtime`: DLL overrides are Wine-only.
    pub fn for_runtime(&self, runtime: &Runtime) -> Self {
        let mut c = self.clone();
        if !matches!(runtime, Runtime::Proton(_)) {
            c.dll_overrides.clear();
        }
        c
    }
}

/// One step of a loader plan, carried out by core in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "step", content = "spec")]
pub enum Step {
    RunInstaller(InstallerSpec),
    DeployFiles(DeployFiles),
    Launch(LaunchChange),
    /// Remove these launch changes from the game's desired launch options.
    RevertLaunch(LaunchChange),
    /// Undo every change recorded in `loader_files` for the install:
    /// delete created files, restore modified/deleted ones from backup.
    RestoreTracked,
    /// Set these modes on game-root files that exist (installers that
    /// `chmod` asynchronously lose that race to the group kill). Applied
    /// before the post-snapshot.
    FixModes(Vec<ModeFix>),
    /// Call [`LoaderPlugin::verify`](super::LoaderPlugin::verify) for
    /// `after`; failure aborts the remaining steps.
    Verify {
        after: Op,
    },
}

/// A mode to set on one game-root file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeFix {
    pub path: String,
    pub mode: u32,
}

/// What an op will do. Hashed into the consent token.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoaderPlan {
    pub op: Option<Op>,
    pub steps: Vec<Step>,
    /// Game-root relative globs the op may change. Changes outside are
    /// flagged unrecoverable.
    pub touch_scope: Vec<Glob>,
    /// Changes outside the game folder, shown to the user as unrecoverable.
    pub external_side_effects: Vec<String>,
    pub expected_version: Option<String>,
    /// Planned without the archive (uninstall): core must extract the
    /// install's cached archive (sha-checked against
    /// `loader_installs.archive_sha256`) into a scratch dir and plan again
    /// with it; only that second plan is consented and run.
    #[serde(default)]
    pub uses_cached_archive: bool,
}

/// State of a loader for one game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum LoaderStatus {
    NotInstalled,
    /// Installed and tracked by Vessel.
    Installed {
        install_id: i64,
        version: Option<String>,
    },
    /// An op is running or was interrupted; recovery is pending.
    Busy {
        install_id: i64,
        status: String,
    },
    /// Files of the loader exist but Vessel did not install it.
    ManualDetected {
        evidence: Vec<String>,
    },
    /// Tracked, but the game folder no longer matches (a Steam update or
    /// file verification undid part of the install).
    Drifted {
        install_id: i64,
        version: Option<String>,
        reason: String,
    },
}

/// The game file a loader replaces, for Steam-update detection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VanillaSpec {
    /// Game-root file the loader replaces.
    pub file: String,
    /// Where the installer keeps (and later restores) the vanilla copy.
    /// After a Steam update these are refreshed from the new file.
    pub copies: Vec<String>,
}

/// What `classify_vanilla` found for the game's `StardewValley`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VanillaState {
    /// Still the file SMAPI left (the install is intact).
    Intact,
    /// Back to the vanilla file recorded before the first install.
    SameVanilla,
    /// A third version: Steam updated the game. Refresh the vanilla backup
    /// from the new file before repair, upgrade or uninstall.
    SteamUpdated,
}

/// Classifies the current `StardewValley` hash against the post-install
/// hash and the pre-first-install vanilla hash.
pub fn classify_vanilla(current: &str, post: &str, vanilla: &str) -> VanillaState {
    if current == post {
        VanillaState::Intact
    } else if current == vanilla {
        VanillaState::SameVanilla
    } else {
        VanillaState::SteamUpdated
    }
}
