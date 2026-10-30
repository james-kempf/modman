// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only views and small settings helpers for the front ends: settings,
//! log tail, download queue, search and file lists.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::nexus as nx;
use super::{Cx, R};
use crate::config::{self, ConfigKey, ValueKind};
use crate::download::{self, Download};
use crate::games::GameConfig;
use crate::mods;
use crate::nexus::{BrowseCollection, BrowseQuery, BrowseSort, FileInfo, SearchMod};
use crate::paths::Paths;

/// One setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
    pub description: String,
    /// What the value is (dropdown, toggle or text field in the GUI).
    pub kind: ValueKind,
    /// The accepted values of an enum or bool key; empty for the others.
    pub options: Vec<String>,
}

fn entry(cx: &Cx<'_>, k: ConfigKey) -> R<ConfigEntry> {
    Ok(ConfigEntry {
        key: k.name().to_string(),
        value: config::get(cx.store, cx.paths, k)?,
        description: k.describe().to_string(),
        kind: k.kind(),
        options: k.options().iter().map(|s| s.to_string()).collect(),
    })
}

/// Every setting with its current value.
pub fn config_entries(cx: &Cx<'_>) -> R<Vec<ConfigEntry>> {
    ConfigKey::ALL.into_iter().map(|k| entry(cx, k)).collect()
}

/// Result of [`config_set`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigChange {
    #[serde(flatten)]
    pub entry: ConfigEntry,
    /// `data-dir` moved away from a folder that still holds data (not moved
    /// automatically).
    pub old_data_left: Option<String>,
}

/// Validates and stores a setting.
pub fn config_set(cx: &Cx<'_>, key: &str, value: &str) -> R<ConfigChange> {
    let key: ConfigKey = key.parse()?;
    let old = config::get(cx.store, cx.paths, key)?;
    let new = config::set(cx.store, cx.paths, key, value)?;
    let left = (key == ConfigKey::DataDir && new != old && dir_has_entries(Path::new(&old)))
        .then_some(old);
    Ok(ConfigChange {
        entry: entry(cx, key)?,
        old_data_left: left,
    })
}

pub fn dir_has_entries(p: &Path) -> bool {
    std::fs::read_dir(p).is_ok_and(|mut d| d.next().is_some())
}

/// Bytes read from the end of the log for [`log_tail`].
const TAIL_BYTES: u64 = 1 << 20;

/// The last `lines` lines of the log file (empty when there is none).
pub fn log_tail(paths: &Paths, lines: usize) -> R<Vec<String>> {
    let Ok(mut f) = std::fs::File::open(paths.log_path()) else {
        return Ok(Vec::new());
    };
    let len = f.metadata()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    let all: Vec<&str> = text.lines().collect();
    Ok(all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|s| s.to_string())
        .collect())
}

/// The download queue, newest first.
pub fn downloads(cx: &Cx<'_>) -> R<Vec<Download>> {
    let mut v = download::list(cx.store, None)?;
    v.reverse();
    Ok(v)
}

/// A search hit and whether it is installed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchHit {
    #[serde(flatten)]
    pub hit: SearchMod,
    pub installed: bool,
}

/// A browsed collection and the revision installed, if any.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CollectionHit {
    #[serde(flatten)]
    pub hit: BrowseCollection,
    pub installed_revision: Option<u64>,
}

/// What a browse page lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowseKind {
    #[default]
    Mods,
    Collections,
}

/// How far back a browse page looks, by creation date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowsePeriod {
    #[default]
    All,
    Year,
    Month,
    Week,
    Day,
}

impl BrowsePeriod {
    /// Start of the window for `now` (Unix seconds); `None` = all time.
    pub fn since(self, now: i64) -> Option<i64> {
        let days = match self {
            BrowsePeriod::All => return None,
            BrowsePeriod::Year => 365,
            BrowsePeriod::Month => 30,
            BrowsePeriod::Week => 7,
            BrowsePeriod::Day => 1,
        };
        Some(now - days * 86_400)
    }
}

/// One browse page request.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct BrowseRequest {
    pub kind: BrowseKind,
    pub period: BrowsePeriod,
    pub sort: BrowseSort,
    /// Name words; empty = everything.
    pub text: String,
    pub offset: u64,
}

/// Items per browse page.
pub const BROWSE_PAGE: u64 = 24;

/// A browse page; `total` counts every match, not only this page.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowseView {
    Mods {
        total: u64,
        mods: Vec<SearchHit>,
    },
    Collections {
        total: u64,
        collections: Vec<CollectionHit>,
    },
}

/// Top, newest or matching mods or collections of `game` on Nexus (works
/// without login), marked when installed. Adult content follows the
/// `adult` setting.
pub fn browse(cx: &Cx<'_>, game: &GameConfig, req: &BrowseRequest) -> R<BrowseView> {
    let domain = game.nexus_domain()?;
    let nexus = nx::connect(cx.paths, cx.store, false)?
        .ok_or_else(|| super::AppError::Msg("no Nexus client".into()))?;
    let q = BrowseQuery {
        text: &req.text,
        since: req.period.since(crate::store::now()),
        sort: req.sort,
        offset: req.offset,
        count: BROWSE_PAGE,
        adult: cx.config()?.adult,
    };
    Ok(match req.kind {
        BrowseKind::Mods => {
            let res = nexus.run(nexus.client.browse_mods(domain, &q))??;
            let installed: Vec<u64> = mods::list_mods(cx.store.conn(), &game.id)?
                .iter()
                .filter_map(|m| m.nexus_mod_id)
                .collect();
            BrowseView::Mods {
                total: res.total,
                mods: res
                    .mods
                    .into_iter()
                    .map(|m| SearchHit {
                        installed: m.mod_id.is_some_and(|i| installed.contains(&i)),
                        hit: m,
                    })
                    .collect(),
            }
        }
        BrowseKind::Collections => {
            let res = nexus.run(nexus.client.browse_collections(domain, &q))??;
            let installed = crate::collections::list(cx.store.conn(), &game.id)?;
            BrowseView::Collections {
                total: res.total,
                collections: res
                    .collections
                    .into_iter()
                    .map(|c| CollectionHit {
                        installed_revision: installed
                            .iter()
                            .find(|i| i.record.slug == c.slug)
                            .map(|i| i.record.revision),
                        hit: c,
                    })
                    .collect(),
            }
        }
    })
}

/// Conflict data of the enabled mods: contested files and plugin entries
/// (read-only; works for every game, plugins only for Bethesda games).
pub fn conflicts(cx: &Cx<'_>, game: &GameConfig) -> R<crate::loadorder::ConflictData> {
    let cfg = cx.config()?;
    let env = crate::deploy::DeployEnv::new(cx.paths, &cfg.data_dir, &game.id);
    let files = crate::loadorder::PluginsFiles::new(env.game_data_dir());
    Ok(crate::loadorder::conflict_data(
        cx.store.conn(),
        game,
        &files,
    )?)
}

/// Downloadable files of a mod in display order and the default pick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FilesView {
    pub files: Vec<FileInfo>,
    pub default: usize,
}

pub fn mod_files(cx: &Cx<'_>, game: &GameConfig, mod_id: u64) -> R<FilesView> {
    let nexus = nx::connect(cx.paths, cx.store, false)?
        .ok_or_else(|| super::AppError::Msg("no Nexus client".into()))?;
    let files = nexus.run(nexus.client.mod_files(game.nexus_domain()?, mod_id))??;
    let (rows, default) = super::install::file_rows(&files.files);
    Ok(FilesView {
        files: rows.into_iter().cloned().collect(),
        default,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fails when config_list stops carrying the metadata the GUI builds its controls from.
    #[test]
    fn config_entries_carry_kind_and_options() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::for_home(home.path());
        let mut store = crate::store::Store::open_in_memory().unwrap();
        let ui = crate::app::testing::Recorder::default();
        let cx = Cx::new(&paths, &mut store, &ui);
        let all = config_entries(&cx).unwrap();
        let get = |k: &str| all.iter().find(|e| e.key == k).unwrap();
        let dm = get("deploy-method");
        assert_eq!(dm.kind, ValueKind::Enum);
        assert_eq!(
            dm.options,
            ["auto", "hardlink", "reflink", "symlink", "copy"]
        );
        assert_eq!(get("auto-deploy").kind, ValueKind::Bool);
        assert_eq!(get("auto-deploy").options, ["true", "false"]);
        assert_eq!(get("jobs").kind, ValueKind::Number);
        assert!(get("data-dir").options.is_empty());
        let json = serde_json::to_value(get("fomod")).unwrap();
        assert_eq!(json["kind"], "enum");
        assert_eq!(
            json["options"],
            serde_json::json!(["auto", "ask", "default"])
        );
    }

    #[test]
    fn log_tail_returns_the_last_lines_or_nothing() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::for_home(home.path());
        assert!(log_tail(&paths, 5).unwrap().is_empty());
        std::fs::create_dir_all(&paths.state_dir).unwrap();
        std::fs::write(paths.log_path(), "a\nb\nc\n").unwrap();
        assert_eq!(log_tail(&paths, 2).unwrap(), ["b", "c"]);
    }
}
