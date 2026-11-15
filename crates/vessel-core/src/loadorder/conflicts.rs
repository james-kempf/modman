// SPDX-License-Identifier: GPL-3.0-or-later
//! Conflict data for the GUI conflict view: per-file winners and losers from
//! the deploy plan, and the plugin view (who provides each plugin, who it
//! overrides, where it sits in plugins.txt).

use std::collections::HashMap;

use rusqlite::Connection;
use serde::Serialize;

use super::plugins_txt::{is_plugin_name, read_plugins_txt, PluginsFiles};
use super::LoadOrderError;
use crate::deploy::{build_plan, ModRef};
use crate::games::GameConfig;

/// A mod as shown in the conflict view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModInfo {
    pub id: i64,
    pub uid: String,
    pub name: String,
    pub priority: i64,
}

impl From<&ModRef> for ModInfo {
    fn from(m: &ModRef) -> Self {
        Self {
            id: m.id,
            uid: m.uid.clone(),
            name: m.name.clone(),
            priority: m.priority,
        }
    }
}

/// A target path provided by more than one enabled mod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileConflict {
    /// Relative to the deploy target root.
    pub rel_path: String,
    pub winner: ModInfo,
    /// Overridden mods, lowest priority first.
    pub losers: Vec<ModInfo>,
    /// The path is a plugin (`.esm/.esl/.esp`) in the deploy folder.
    pub plugin: bool,
}

/// One plugin, from enabled mods and/or plugins.txt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginEntry {
    pub name: String,
    /// Mod whose copy is deployed; `None` for plugins not from a mod (game
    /// or user files).
    pub provider: Option<ModInfo>,
    /// Other mods shipping the same plugin, lowest priority first.
    pub overridden: Vec<ModInfo>,
    /// 0-based position among plugins.txt entries.
    pub load_index: Option<usize>,
    pub active: bool,
    /// Added to plugins.txt by vessel.
    pub managed: bool,
}

/// Everything the conflict view needs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ConflictData {
    pub files: Vec<FileConflict>,
    /// plugins.txt order first, then mod plugins not listed (priority order).
    pub plugins: Vec<PluginEntry>,
}

/// Name of the plugin at `rel` when it sits directly in the deploy folder.
fn plugin_at(rel: &str, base: &str) -> Option<String> {
    let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
    (dir.eq_ignore_ascii_case(base) && is_plugin_name(name)).then(|| name.to_string())
}

/// Builds the conflict data of `game` from its enabled mods (read-only).
pub fn conflict_data(
    conn: &Connection,
    game: &GameConfig,
    files: &PluginsFiles,
) -> Result<ConflictData, LoadOrderError> {
    let plan = build_plan(conn, game)?;
    let base = game
        .subdir
        .trim_start_matches("./")
        .trim_matches('/')
        .trim_start_matches('.')
        .to_string();
    let mut out = ConflictData::default();
    let mut from_mods: Vec<PluginEntry> = Vec::new();
    for it in &plan.items {
        let plugin = plugin_at(&it.rel_path, &base);
        let winner = ModInfo::from(&plan.mods[it.winner]);
        let losers: Vec<ModInfo> = it
            .providers
            .iter()
            .filter(|p| **p != it.winner)
            .map(|p| ModInfo::from(&plan.mods[*p]))
            .collect();
        if !losers.is_empty() {
            out.files.push(FileConflict {
                rel_path: it.rel_path.clone(),
                winner: winner.clone(),
                losers: losers.clone(),
                plugin: plugin.is_some(),
            });
        }
        if let Some(name) = plugin {
            from_mods.push(PluginEntry {
                name,
                provider: Some(winner),
                overridden: losers,
                load_index: None,
                active: false,
                managed: false,
            });
        }
    }
    from_mods.sort_by_key(|p| p.provider.as_ref().map_or(0, |m| m.priority));
    let mut by_name: HashMap<String, PluginEntry> = from_mods
        .iter()
        .map(|p| (p.name.to_lowercase(), p.clone()))
        .collect();
    if let Some(lines) = read_plugins_txt(game, files)? {
        for (i, l) in lines.into_iter().enumerate() {
            let mut e = by_name
                .remove(&l.name.to_lowercase())
                .unwrap_or(PluginEntry {
                    name: l.name.clone(),
                    provider: None,
                    overridden: Vec::new(),
                    load_index: None,
                    active: false,
                    managed: false,
                });
            e.load_index = Some(i);
            e.active = l.active;
            e.managed = l.managed;
            out.plugins.push(e);
        }
    }
    out.plugins.extend(
        from_mods
            .into_iter()
            .filter(|p| by_name.contains_key(&p.name.to_lowercase())),
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Catches: plugins in subfolders (e.g. `Data/meshes/x.esp`) or other
    /// files counted as load-order plugins.
    #[test]
    fn plugin_at_only_matches_top_of_deploy_folder() {
        assert_eq!(plugin_at("Data/A.esp", "Data"), Some("A.esp".into()));
        assert_eq!(plugin_at("data/B.ESM", "Data"), Some("B.ESM".into()));
        assert_eq!(plugin_at("Data/sub/C.esp", "Data"), None);
        assert_eq!(plugin_at("Data/x.bsa", "Data"), None);
        assert_eq!(plugin_at("D.esl", ""), Some("D.esl".into()));
    }
}
