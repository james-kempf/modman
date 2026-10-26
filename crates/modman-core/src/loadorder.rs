// SPDX-License-Identifier: GPL-3.0-or-later
//! M6: plugins.txt, LOOT sorting and collection plugin rules (tech stack
//! §4.3; legacy `plugins_update` / `plugins_remove_managed`, L2046-2120).
//!
//! - [`update_plugins`] runs after every successful deploy: it drops the
//!   entries modman added last time, then appends the plugins of the enabled
//!   mods that are not listed yet, sorted by LOOT (libloot) with the
//!   collection `pluginRules` as user metadata. Lines the user had are kept
//!   verbatim and in order.
//! - [`remove_managed`] runs on purge and removes only modman's entries.
//! - Bookkeeping lives in the per-game data folder (`plugins.managed`,
//!   `plugins.txt.orig`, `cache/loot/`), as in legacy.
//! - The masterlist is fetched only on request ([`fetch_masterlist`]); deploy
//!   uses the cached copy, so deploy never touches the network. The LOOT
//!   masterlist is community data downloaded at runtime and not bundled; its
//!   licence was not verified (spikes.md §1).
//! - UE4SS `mods.txt` (`[[shared_files]]`) is merged on deploy and
//!   restored on purge (`ue4ss_mods_txt`); mods' copies are never placed.
//! - [`conflict_data`] gives the GUI per-file winners/losers and the plugin
//!   view (provider, overrides, load index, active, managed).

mod conflicts;
mod loot;
mod plugins_txt;
mod rules;
mod ue4ss_mods_txt;

use std::path::{Path, PathBuf};

pub use conflicts::{conflict_data, ConflictData, FileConflict, ModInfo, PluginEntry};
pub use loot::{
    fetch_masterlist, loot_game_type, sort_with_loot, MasterlistSource, DEFAULT_LOOT_BASE_URL,
    LOOT_URL_ENV,
};
pub use plugins_txt::{
    is_plugin_name, plugins_applies, read_plugins_txt, remove_managed, update_plugins, PluginLine,
    PluginStage, PluginsFiles, PluginsReport, SkipReason, SortMethod,
};
pub use rules::apply_rules;
pub(crate) use ue4ss_mods_txt::{
    apply as merge_shared_files, revert as revert_shared_files, shared_paths,
};

use crate::store::StoreError;

/// Errors from this module.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoadOrderError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("refusing to use {0}: it is a symbolic link, or a folder on its path is")]
    Symlink(PathBuf),
    #[error("LOOT: {0}")]
    Loot(String),
    #[error("LOOT does not support game '{0}'")]
    UnsupportedGame(String),
    #[error("masterlist download from {url} failed: {message}")]
    Masterlist { url: String, message: String },
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Deploy(Box<crate::deploy::DeployError>),
}

impl From<rusqlite::Error> for LoadOrderError {
    fn from(e: rusqlite::Error) -> Self {
        LoadOrderError::Store(StoreError::Sqlite(e))
    }
}

impl From<crate::deploy::DeployError> for LoadOrderError {
    fn from(e: crate::deploy::DeployError) -> Self {
        LoadOrderError::Deploy(Box::new(e))
    }
}

pub(crate) fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> LoadOrderError + '_ {
    move |source| LoadOrderError::Io {
        path: path.to_path_buf(),
        source,
    }
}
