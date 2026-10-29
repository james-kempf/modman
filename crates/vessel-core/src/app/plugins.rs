// SPDX-License-Identifier: GPL-3.0-or-later
//! Bethesda plugins: plugins.txt view, LOOT masterlist download and the
//! re-sort of the plugins vessel added.

use std::path::PathBuf;

use super::{fail, Cx, R};
use crate::deploy::DeployEnv;
use crate::games::GameConfig;
use crate::loadorder::{
    self, MasterlistSource, PluginLine, PluginStage, PluginsFiles, DEFAULT_LOOT_BASE_URL,
    LOOT_URL_ENV,
};
use crate::store::DeployLock;

/// LOOT base URL, honouring the [`LOOT_URL_ENV`] test hook.
pub fn loot_base() -> String {
    std::env::var(LOOT_URL_ENV)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_LOOT_BASE_URL.to_string())
}

/// plugins.txt files and the deploy environment of a game that has one.
pub fn files_for(cx: &Cx<'_>, game: &GameConfig) -> R<(PluginsFiles, DeployEnv)> {
    if !loadorder::plugins_applies(game) {
        fail!(
            "{} has no plugins.txt (only bethesda-profile games with a plugins format).",
            game.name
        );
    }
    let cfg = cx.config()?;
    let env = DeployEnv::new(cx.paths, &cfg.data_dir, &game.domain);
    Ok((PluginsFiles::new(env.game_data_dir()), env))
}

/// plugins.txt entries; `None` when the file does not exist yet.
pub fn list(cx: &Cx<'_>, game: &GameConfig) -> R<Option<Vec<PluginLine>>> {
    let (files, _) = files_for(cx, game)?;
    Ok(loadorder::read_plugins_txt(game, &files)?)
}

/// Downloads the LOOT masterlist into the cache; returns its path.
pub fn masterlist(cx: &Cx<'_>, game: &GameConfig) -> R<PathBuf> {
    let (files, _) = files_for(cx, game)?;
    let src = MasterlistSource::for_game(game, &loot_base())?;
    let path = crate::blocking::block_on(loadorder::fetch_masterlist(&src, &files.game_data))??;
    cx.ui
        .success(&format!("LOOT masterlist saved to {}", path.display()));
    Ok(path)
}

/// Re-sorts the plugins vessel added (LOOT + collection rules), optionally
/// fetching the masterlist first.
pub fn sort(cx: &Cx<'_>, game: &GameConfig, update_masterlist: bool) -> R<PluginStage> {
    let (files, env) = files_for(cx, game)?;
    if update_masterlist {
        masterlist(cx, game)?;
    }
    // plugins.txt is shared state with deploy/purge.
    let _lock = DeployLock::try_acquire(&env.lock_path)?;
    let stage = loadorder::update_plugins(cx.store.conn(), game, &files)?;
    for m in super::deploy::plugin_stage_msgs(Some(&stage)) {
        cx.ui.say(m);
    }
    if let PluginStage::Updated(r) = &stage {
        if r.added.is_empty() && r.removed.is_empty() {
            cx.ui.info("No mod plugins to sort.");
        }
    }
    Ok(stage)
}
