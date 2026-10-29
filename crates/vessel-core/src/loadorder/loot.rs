// SPDX-License-Identifier: GPL-3.0-or-later
//! LOOT sorting through libloot (pure Rust, spikes.md §1) and the masterlist
//! download/cache. On Linux the game handle is always created with
//! [`Game::with_local_path`]; the local path is the plugins.txt folder in the
//! Proton prefix.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use libloot::metadata::{File, PluginMetadata};
use libloot::{Game, GameType};

use super::{io_err, LoadOrderError};
use crate::collections::PluginRule;
use crate::games::GameConfig;

/// Environment variable overriding [`DEFAULT_LOOT_BASE_URL`] (tests, mirrors).
pub const LOOT_URL_ENV: &str = "VESSEL_LOOT_URL";
/// Base of the LOOT metadata repositories on GitHub.
pub const DEFAULT_LOOT_BASE_URL: &str = "https://raw.githubusercontent.com/loot";
/// Masterlist branch read by current LOOT releases (0.26 and later).
const MASTERLIST_BRANCH: &str = "v0.26";
/// Refuse masterlists larger than this (the real ones are ~2 MiB).
const MAX_MASTERLIST_BYTES: usize = 32 << 20;
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// libloot game type of a configured game, by definition (or domain).
/// `None` for games LOOT does not handle.
pub fn loot_game_type(game: &GameConfig) -> Option<GameType> {
    let key = game.definition.as_deref().unwrap_or(&game.domain);
    Some(match key {
        "skyrimspecialedition" | "enderalspecialedition" => GameType::SkyrimSE,
        "skyrim" | "enderal" => GameType::Skyrim,
        "skyrimvr" => GameType::SkyrimVR,
        "fallout4" => GameType::Fallout4,
        "fallout4vr" => GameType::Fallout4VR,
        "newvegas" | "falloutnewvegas" => GameType::FalloutNV,
        "fallout3" => GameType::Fallout3,
        "oblivion" => GameType::Oblivion,
        "starfield" => GameType::Starfield,
        "morrowind" => GameType::Morrowind,
        _ => return None,
    })
}

/// LOOT metadata repository of a game.
fn masterlist_repo(game: &GameConfig, gt: GameType) -> &'static str {
    let enderal = game
        .definition
        .as_deref()
        .unwrap_or(&game.domain)
        .starts_with("enderal");
    match gt {
        GameType::SkyrimSE if enderal => "enderalse",
        GameType::Skyrim if enderal => "enderal",
        GameType::SkyrimSE => "skyrimse",
        GameType::SkyrimVR => "skyrimvr",
        GameType::Skyrim => "skyrim",
        GameType::Fallout4 => "fallout4",
        GameType::Fallout4VR => "fallout4vr",
        GameType::FalloutNV => "falloutnv",
        GameType::Fallout3 => "fallout3",
        GameType::Oblivion => "oblivion",
        GameType::Starfield => "starfield",
        GameType::Morrowind => "morrowind",
        _ => "unknown",
    }
}

/// Where the masterlist (and its prelude) are downloaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MasterlistSource {
    pub masterlist_url: String,
    pub prelude_url: Option<String>,
}

impl MasterlistSource {
    /// `<base>/<repo>/v0.26/masterlist.yaml` and the shared prelude.
    /// `base` is usually [`DEFAULT_LOOT_BASE_URL`] or `$VESSEL_LOOT_URL`.
    pub fn for_game(game: &GameConfig, base: &str) -> Result<Self, LoadOrderError> {
        let gt = loot_game_type(game)
            .ok_or_else(|| LoadOrderError::UnsupportedGame(game.domain.clone()))?;
        let base = base.trim_end_matches('/');
        Ok(Self {
            masterlist_url: format!(
                "{base}/{}/{MASTERLIST_BRANCH}/masterlist.yaml",
                masterlist_repo(game, gt)
            ),
            prelude_url: Some(format!("{base}/prelude/{MASTERLIST_BRANCH}/prelude.yaml")),
        })
    }
}

/// Cached masterlist files under the per-game data folder.
pub(crate) fn cache_dir(game_data: &Path) -> PathBuf {
    game_data.join("cache/loot")
}

pub(crate) fn cached_masterlist(game_data: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    let dir = cache_dir(game_data);
    let ml = dir.join("masterlist.yaml");
    let prelude = dir.join("prelude.yaml");
    ml.is_file()
        .then(|| (ml, prelude.is_file().then_some(prelude)))
}

async fn download(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, LoadOrderError> {
    let fail = |message: String| LoadOrderError::Masterlist {
        url: url.to_string(),
        message,
    };
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| fail(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(fail(format!("HTTP {}", resp.status())));
    }
    let body = resp.bytes().await.map_err(|e| fail(e.to_string()))?;
    if body.is_empty() || body.len() > MAX_MASTERLIST_BYTES {
        return Err(fail(format!("unexpected size {} bytes", body.len())));
    }
    Ok(body.to_vec())
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), LoadOrderError> {
    let tmp = path.with_extension("yaml.tmp");
    let mut f = std::fs::File::create(&tmp).map_err(io_err(&tmp))?;
    f.write_all(data).map_err(io_err(&tmp))?;
    f.sync_all().map_err(io_err(&tmp))?;
    std::fs::rename(&tmp, path).map_err(io_err(path))
}

/// Downloads the masterlist (and prelude, when the source names one) into
/// the cache of `game_data`, replacing the cached copy only when every
/// download succeeded. Returns the masterlist path. A missing prelude is not
/// an error (old masterlists have none).
pub async fn fetch_masterlist(
    src: &MasterlistSource,
    game_data: &Path,
) -> Result<PathBuf, LoadOrderError> {
    crate::nexus::install_crypto_provider();
    let http = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|e| LoadOrderError::Masterlist {
            url: src.masterlist_url.clone(),
            message: e.to_string(),
        })?;
    let ml = download(&http, &src.masterlist_url).await?;
    let prelude = match &src.prelude_url {
        Some(u) => download(&http, u).await.ok(),
        None => None,
    };
    let dir = cache_dir(game_data);
    std::fs::create_dir_all(&dir).map_err(io_err(&dir))?;
    let ml_path = dir.join("masterlist.yaml");
    write_atomic(&ml_path, &ml)?;
    let prelude_path = dir.join("prelude.yaml");
    match prelude {
        Some(p) => write_atomic(&prelude_path, &p)?,
        None => {
            let _ = std::fs::remove_file(&prelude_path);
        }
    }
    Ok(ml_path)
}

fn loot_err(e: impl std::fmt::Display) -> LoadOrderError {
    LoadOrderError::Loot(e.to_string())
}

/// Sorts `names` (plugins present in the game's data folder) with LOOT.
/// `names` order is the current load order. The masterlist (with optional
/// prelude) is used when given; collection rules become user `after`
/// metadata. Plugins whose headers cannot be read fail the sort.
pub fn sort_with_loot(
    game_type: GameType,
    game_dir: &Path,
    local_dir: &Path,
    names: &[String],
    masterlist: Option<(&Path, Option<&Path>)>,
    rules: &[PluginRule],
) -> Result<Vec<String>, LoadOrderError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let mut game = Game::with_local_path(game_type, game_dir, local_dir).map_err(loot_err)?;
    let paths: Vec<&Path> = names.iter().map(Path::new).collect();
    game.load_plugin_headers(&paths).map_err(loot_err)?;
    {
        let db = game.database();
        let mut db = db
            .write()
            .map_err(|_| LoadOrderError::Loot("database lock poisoned".into()))?;
        match masterlist {
            Some((ml, Some(prelude))) => db.load_masterlist_with_prelude(ml, prelude),
            Some((ml, None)) => db.load_masterlist(ml),
            None => Ok(()),
        }
        .map_err(loot_err)?;
        let present: std::collections::HashSet<String> =
            names.iter().map(|n| n.to_lowercase()).collect();
        let mut after: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
        for r in rules {
            if !present.contains(&r.plugin.to_lowercase()) {
                continue;
            }
            let e = after
                .entry(r.plugin.to_lowercase())
                .or_insert_with(|| (r.plugin.clone(), Vec::new()));
            for a in &r.after {
                if present.contains(&a.to_lowercase())
                    && !e.1.iter().any(|x| x.eq_ignore_ascii_case(a))
                {
                    e.1.push(a.clone());
                }
            }
        }
        for (_, (plugin, files)) in after {
            if files.is_empty() {
                continue;
            }
            let mut md = PluginMetadata::new(&plugin).map_err(loot_err)?;
            md.set_load_after_files(files.into_iter().map(File::new).collect());
            db.set_plugin_user_metadata(md);
        }
    }
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    game.sort_plugins(&refs).map_err(loot_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Catches: a wrong repo or branch in the masterlist URL.
    #[test]
    fn masterlist_url_uses_game_repo_and_branch() {
        let mut g = GameConfig::custom("skyrimspecialedition");
        g.definition = Some("skyrimspecialedition".into());
        let s = MasterlistSource::for_game(&g, "http://x/").unwrap();
        assert_eq!(s.masterlist_url, "http://x/skyrimse/v0.26/masterlist.yaml");
        assert_eq!(
            s.prelude_url.as_deref(),
            Some("http://x/prelude/v0.26/prelude.yaml")
        );
        let mut e = GameConfig::custom("enderalspecialedition");
        e.definition = Some("enderalspecialedition".into());
        assert!(MasterlistSource::for_game(&e, "http://x")
            .unwrap()
            .masterlist_url
            .contains("/enderalse/"));
        assert!(matches!(
            MasterlistSource::for_game(&GameConfig::custom("cyberpunk2077"), "http://x"),
            Err(LoadOrderError::UnsupportedGame(_))
        ));
    }
}
