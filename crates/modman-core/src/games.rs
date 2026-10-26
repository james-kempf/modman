// SPDX-License-Identifier: GPL-3.0-or-later
//! Games: definitions, Steam detection and the configured-games table
//! (replaces legacy `games/<domain>.cfg`, `cmd_game` L4080-4171).

pub mod definitions;
pub mod steam;

use std::path::{Path, PathBuf};
use std::str::FromStr;

use rusqlite::{params, Connection, OptionalExtension, Row};

pub use definitions::{
    builtin_definitions, find_definition, load_definitions, GameDefinition, PluginsDef,
    PluginsFormat, Support, Target,
};

use crate::config::{expand_tilde, DEPLOY_METHODS};
use crate::store::{now, Store, StoreError};

/// Layout profile names (legacy `KNOWN_PROFILES`, L301). The rules for each
/// live in `staging.rs`; this list only validates names.
pub const KNOWN_PROFILES: &[&str] = &[
    "cyberpunk",
    "bethesda",
    "stardew",
    "bannerlord",
    "witcher3",
    "bepinex",
    "melonloader",
    "ue",
    "generic",
];

/// Settings key that stores the active game domain.
pub const ACTIVE_GAME_KEY: &str = "active_game";

/// Errors from game handling.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GamesError {
    #[error("invalid game domain '{0}' (as in nexusmods.com/<domain>/mods/...)")]
    InvalidDomain(String),
    #[error("game '{0}' is not set up")]
    NotConfigured(String),
    #[error("no game selected")]
    NoActiveGame,
    #[error("unknown game setting '{0}' (keys: {keys})", keys = GameSetting::NAMES.join(" "))]
    UnknownSetting(String),
    #[error("invalid value '{value}' for {key}: expected {expected}")]
    InvalidValue {
        key: &'static str,
        value: String,
        expected: String,
    },
    #[error("mods are still deployed for {0}; purge first")]
    DeploymentActive(String),
    #[error("invalid game definition {origin}: {message}")]
    Definition { origin: String, message: String },
    #[error("cannot parse Steam file {origin}: {message}")]
    Vdf { origin: String, message: String },
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for GamesError {
    fn from(e: rusqlite::Error) -> Self {
        GamesError::Store(StoreError::Sqlite(e))
    }
}

/// `^[a-z0-9]+$` (legacy `game_add_flow`).
pub fn is_valid_domain(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Lower-cases and validates a domain.
pub fn normalize_domain(s: &str) -> Result<String, GamesError> {
    let d = s.to_ascii_lowercase();
    if is_valid_domain(&d) {
        Ok(d)
    } else {
        Err(GamesError::InvalidDomain(s.to_string()))
    }
}

/// A configured game (one row of `games`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GameConfig {
    pub domain: String,
    pub name: String,
    pub folder: Option<PathBuf>,
    pub subdir: String,
    pub profile: String,
    pub target: Target,
    pub appid: Option<u32>,
    pub method: Option<String>,
    pub prefix: Option<PathBuf>,
    pub library: Option<PathBuf>,
    pub plugins_file: Option<PathBuf>,
    pub plugins_format: Option<PluginsFormat>,
    pub appdata_folder: Option<String>,
    pub paks_dir: Option<String>,
    pub nexus_game_id: Option<i64>,
    /// Domain of the definition this game came from; `None` = custom.
    pub definition: Option<String>,
    pub added_at: i64,
}

impl GameConfig {
    /// Defaults for a domain with no definition (legacy `game_defaults`).
    pub fn custom(domain: &str) -> Self {
        Self {
            domain: domain.to_string(),
            name: domain.to_string(),
            folder: None,
            subdir: ".".into(),
            profile: "generic".into(),
            target: Target::GameDir,
            appid: None,
            method: None,
            prefix: None,
            library: None,
            plugins_file: None,
            plugins_format: None,
            appdata_folder: None,
            paks_dir: None,
            nexus_game_id: None,
            definition: None,
            added_at: now(),
        }
    }

    /// Defaults taken from a game definition.
    pub fn from_definition(def: &GameDefinition) -> Self {
        Self {
            name: def.name.clone(),
            subdir: def.subdir.clone(),
            profile: def.profile.clone(),
            target: def.target,
            appid: def.steam_appid,
            plugins_format: def.plugins.as_ref().map(|p| p.format),
            appdata_folder: def.plugins.as_ref().map(|p| p.appdata_folder.clone()),
            paks_dir: def.paks_dir.clone(),
            definition: Some(def.domain.clone()),
            ..Self::custom(&def.domain)
        }
    }

    /// Default plugins.txt location (legacy `detect_plugins_file`, L434-439):
    /// only for bethesda games with a plugins format and AppData folder.
    pub fn default_plugins_file(&self) -> Option<PathBuf> {
        if self.profile != "bethesda" || self.plugins_format.is_none() {
            return None;
        }
        let folder = self.appdata_folder.as_deref()?;
        let prefix = self.prefix.as_deref()?;
        Some(prefix.join(format!(
            "drive_c/users/steamuser/AppData/Local/{folder}/plugins.txt"
        )))
    }

    /// Explicit plugins file, else the default location.
    pub fn plugins_txt(&self) -> Option<PathBuf> {
        self.plugins_file
            .clone()
            .or_else(|| self.default_plugins_file())
    }

    /// Support level from the definitions (custom games are experimental).
    pub fn support(&self, defs: &[GameDefinition]) -> Support {
        self.definition
            .as_deref()
            .and_then(|d| find_definition(defs, d))
            .map_or(Support::Experimental, |d| d.support)
    }
}

const GAME_COLUMNS: &str = "domain, name, folder, subdir, profile, target, appid, method, prefix, \
     library, plugins_file, plugins_format, appdata_folder, paks_dir, nexus_game_id, definition, added_at";

fn path_col(v: Option<String>) -> Option<PathBuf> {
    v.filter(|s| !s.is_empty()).map(PathBuf::from)
}

fn row_to_game(r: &Row<'_>) -> rusqlite::Result<GameConfig> {
    Ok(GameConfig {
        domain: r.get(0)?,
        name: r.get(1)?,
        folder: path_col(r.get(2)?),
        subdir: r.get(3)?,
        profile: r.get(4)?,
        target: Target::parse(&r.get::<_, String>(5)?).unwrap_or_default(),
        appid: r.get(6)?,
        method: r.get(7)?,
        prefix: path_col(r.get(8)?),
        library: path_col(r.get(9)?),
        plugins_file: path_col(r.get(10)?),
        plugins_format: r
            .get::<_, Option<String>>(11)?
            .and_then(|s| PluginsFormat::parse(&s)),
        appdata_folder: r.get(12)?,
        paks_dir: r.get(13)?,
        nexus_game_id: r.get(14)?,
        definition: r.get(15)?,
        added_at: r.get(16)?,
    })
}

fn path_str(p: &Option<PathBuf>) -> Option<String> {
    p.as_ref().map(|p| p.to_string_lossy().into_owned())
}

pub fn get_game(conn: &Connection, domain: &str) -> Result<Option<GameConfig>, GamesError> {
    Ok(conn
        .query_row(
            &format!("SELECT {GAME_COLUMNS} FROM games WHERE domain = ?1"),
            [domain],
            row_to_game,
        )
        .optional()?)
}

pub fn require_game(conn: &Connection, domain: &str) -> Result<GameConfig, GamesError> {
    get_game(conn, domain)?.ok_or_else(|| GamesError::NotConfigured(domain.to_string()))
}

/// All configured games, ordered by domain.
pub fn list_games(conn: &Connection) -> Result<Vec<GameConfig>, GamesError> {
    let mut stmt = conn.prepare(&format!("SELECT {GAME_COLUMNS} FROM games ORDER BY domain"))?;
    let rows = stmt.query_map([], row_to_game)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Checks the game-relative folders (`subdir`, `paks_dir`): they are
/// joined onto the game folder, so `..` or an absolute path would point
/// deploys and probes outside it.
pub fn check_game_dirs(g: &GameConfig) -> Result<(), GamesError> {
    let dirs = [
        (GameSetting::Subdir, Some(g.subdir.as_str())),
        (GameSetting::PaksDir, g.paks_dir.as_deref()),
    ];
    for (key, value) in dirs {
        if let Some(v) = value {
            crate::deploy::check_rel_dir(v).map_err(|m| GamesError::InvalidValue {
                key: key.name(),
                value: v.to_string(),
                expected: format!("a relative folder ({m})"),
            })?;
        }
    }
    Ok(())
}

/// Inserts or replaces a game row (dependent rows are kept). Refuses
/// folders that fail [`check_game_dirs`].
pub fn save_game(conn: &Connection, g: &GameConfig) -> Result<(), GamesError> {
    check_game_dirs(g)?;
    conn.execute(
        &format!(
            "INSERT INTO games ({GAME_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
             ON CONFLICT(domain) DO UPDATE SET name=excluded.name, folder=excluded.folder,
               subdir=excluded.subdir, profile=excluded.profile, target=excluded.target,
               appid=excluded.appid, method=excluded.method, prefix=excluded.prefix,
               library=excluded.library, plugins_file=excluded.plugins_file,
               plugins_format=excluded.plugins_format, appdata_folder=excluded.appdata_folder,
               paks_dir=excluded.paks_dir, nexus_game_id=excluded.nexus_game_id,
               definition=excluded.definition"
        ),
        params![
            g.domain,
            g.name,
            path_str(&g.folder),
            g.subdir,
            g.profile,
            g.target.as_str(),
            g.appid,
            g.method,
            path_str(&g.prefix),
            path_str(&g.library),
            path_str(&g.plugins_file),
            g.plugins_format.map(PluginsFormat::as_str),
            g.appdata_folder,
            g.paks_dir,
            g.nexus_game_id,
            g.definition,
            g.added_at,
        ],
    )?;
    Ok(())
}

/// True while files are deployed, original backups are held, or a deploy or
/// purge journal is unfinished (legacy `deployment_active`, L1663).
pub fn deployment_active(conn: &Connection, domain: &str) -> Result<bool, GamesError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM deployments WHERE game = ?1)
             OR EXISTS (SELECT 1 FROM backups WHERE game = ?1 AND restored_at IS NULL)
             OR EXISTS (SELECT 1 FROM deploy_journal WHERE game = ?1 AND state = 'started')",
        [domain],
        |r| r.get(0),
    )?)
}

/// Number of installed mods for a game.
pub fn mod_count(conn: &Connection, domain: &str) -> Result<i64, GamesError> {
    Ok(
        conn.query_row("SELECT COUNT(*) FROM mods WHERE game = ?1", [domain], |r| {
            r.get(0)
        })?,
    )
}

/// Forgets a game and all its rows. Refuses while a deployment is active.
/// Clears the active game if it was this one.
pub fn remove_game(store: &mut Store, domain: &str) -> Result<(), GamesError> {
    let tx = store.conn_mut().transaction().map_err(StoreError::from)?;
    require_game(&tx, domain)?;
    if deployment_active(&tx, domain)? {
        return Err(GamesError::DeploymentActive(domain.to_string()));
    }
    tx.execute("DELETE FROM games WHERE domain = ?1", [domain])?;
    tx.execute(
        "DELETE FROM settings WHERE key = ?1 AND value = ?2",
        params![ACTIVE_GAME_KEY, domain],
    )?;
    tx.commit().map_err(StoreError::from)?;
    Ok(())
}

pub fn active_game(store: &Store) -> Result<Option<String>, GamesError> {
    Ok(store.setting(ACTIVE_GAME_KEY)?)
}

pub fn set_active_game(store: &Store, domain: &str) -> Result<(), GamesError> {
    require_game(store.conn(), domain)?;
    Ok(store.set_setting(ACTIVE_GAME_KEY, domain)?)
}

/// Game to act on (legacy `need_game`, L365-373): explicit override, then the
/// active game, then the only configured game.
pub fn resolve_game(store: &Store, explicit: Option<&str>) -> Result<GameConfig, GamesError> {
    let domain = match explicit {
        Some(d) => d.to_ascii_lowercase(),
        None => match active_game(store)? {
            Some(d) => d,
            None => {
                let games = list_games(store.conn())?;
                match games.as_slice() {
                    [only] => only.domain.clone(),
                    _ => return Err(GamesError::NoActiveGame),
                }
            }
        },
    };
    require_game(store.conn(), &domain)
}

/// Keys accepted by `game set` (legacy L4134-4154, exactly these 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameSetting {
    Folder,
    Subdir,
    Profile,
    Appid,
    Method,
    Prefix,
    PluginsFile,
    PaksDir,
    Name,
}

impl GameSetting {
    pub const ALL: [GameSetting; 9] = [
        GameSetting::Folder,
        GameSetting::Subdir,
        GameSetting::Profile,
        GameSetting::Appid,
        GameSetting::Method,
        GameSetting::Prefix,
        GameSetting::PluginsFile,
        GameSetting::PaksDir,
        GameSetting::Name,
    ];
    pub const NAMES: [&'static str; 9] = [
        "folder",
        "subdir",
        "profile",
        "appid",
        "method",
        "prefix",
        "plugins-file",
        "paks-dir",
        "name",
    ];

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }

    /// Settings that change where files land; refused while deployed.
    pub fn needs_purge(self) -> bool {
        matches!(
            self,
            GameSetting::Folder | GameSetting::Subdir | GameSetting::Profile
        )
    }
}

impl FromStr for GameSetting {
    type Err = GamesError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::NAMES
            .iter()
            .position(|n| *n == s)
            .map(|i| Self::ALL[i])
            .ok_or_else(|| GamesError::UnknownSetting(s.to_string()))
    }
}

fn opt(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

/// Applies `game set <key> <value>` and saves the game. `~` expands to `home`.
/// Note: unlike legacy, `paks-dir` no longer overwrites the bethesda AppData
/// folder (ambiguity A1): they are separate fields.
pub fn apply_setting(
    conn: &Connection,
    domain: &str,
    key: GameSetting,
    value: &str,
    home: &Path,
) -> Result<GameConfig, GamesError> {
    let mut g = require_game(conn, domain)?;
    if key.needs_purge() && deployment_active(conn, domain)? {
        return Err(GamesError::DeploymentActive(domain.to_string()));
    }
    let invalid = |expected: String| GamesError::InvalidValue {
        key: key.name(),
        value: value.to_string(),
        expected,
    };
    match key {
        GameSetting::Folder => {
            let v = expand_tilde(value, home);
            g.folder = opt(v.trim_end_matches('/')).map(PathBuf::from);
        }
        GameSetting::Subdir => {
            crate::deploy::check_rel_dir(value)
                .map_err(|m| invalid(format!("a relative folder ({m})")))?;
            g.subdir = opt(value).unwrap_or_else(|| ".".into())
        }
        GameSetting::Profile => {
            if !KNOWN_PROFILES.contains(&value) {
                return Err(invalid(KNOWN_PROFILES.join("|")));
            }
            g.profile = value.to_string();
        }
        GameSetting::Appid => {
            g.appid = match value {
                "" => None,
                v => Some(
                    v.parse()
                        .map_err(|_| invalid("a Steam app id (digits)".into()))?,
                ),
            }
        }
        GameSetting::Method => {
            if !value.is_empty() && !DEPLOY_METHODS.contains(&value) {
                return Err(invalid(format!("empty or {}", DEPLOY_METHODS.join("|"))));
            }
            g.method = opt(value);
        }
        GameSetting::Prefix => g.prefix = opt(&expand_tilde(value, home)).map(PathBuf::from),
        GameSetting::PluginsFile => {
            g.plugins_file = opt(&expand_tilde(value, home)).map(PathBuf::from)
        }
        GameSetting::PaksDir => {
            crate::deploy::check_rel_dir(value)
                .map_err(|m| invalid(format!("a relative folder ({m})")))?;
            g.paks_dir = opt(value)
        }
        GameSetting::Name => g.name = value.to_string(),
    }
    save_game(conn, &g)?;
    Ok(g)
}

/// A preset game found in a Steam library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedGame {
    pub definition: GameDefinition,
    pub app: steam::SteamApp,
}

/// Definitions (in order) whose Steam app is installed under `home`.
/// Read-only: never writes to Steam folders.
pub fn detect_installed(home: &Path, defs: &[GameDefinition]) -> Vec<DetectedGame> {
    let libs = steam::steam_libraries(home);
    defs.iter()
        .filter_map(|d| {
            let app = steam::find_app(&libs, d.steam_appid?)?;
            Some(DetectedGame {
                definition: d.clone(),
                app,
            })
        })
        .collect()
}

/// Resolved UE4SS mods folder of `game` (relative to its deploy folder),
/// from its built-in definition's `[ue4ss]` block. The current/legacy
/// probe looks under `game_root.join(subdir)`; no root: the default folder.
/// `None` when the definition has no `[ue4ss]` block.
pub(crate) fn ue4ss_mods_dir(game: &GameConfig, game_root: Option<&Path>) -> Option<String> {
    let defs = builtin_definitions();
    let u = find_definition(&defs, game.definition.as_deref()?)?
        .ue4ss
        .as_ref()?;
    let root = game_root.map(|r| r.join(&game.subdir));
    let exists = |rel: &str| root.as_ref().is_some_and(|r| r.join(rel).exists());
    Some(u.resolve_mods_dir(exists).to_string())
}

/// Fills Steam-derived fields (library, prefix, plugins file) of `g` from a
/// detected app when they are unset.
pub fn apply_steam_app(g: &mut GameConfig, app: &steam::SteamApp) {
    if g.library.is_none() {
        g.library = Some(app.library.clone());
    }
    if g.prefix.is_none() {
        g.prefix = app.prefix.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_game_refuses_folders_outside_the_game() {
        let store = Store::open_in_memory().unwrap();
        for bad in ["../../x", "/etc", "Data/../../x"] {
            let mut g = GameConfig::custom("foo");
            g.subdir = bad.into();
            let e = save_game(store.conn(), &g).unwrap_err();
            assert!(
                matches!(e, GamesError::InvalidValue { key: "subdir", .. }),
                "{bad}: {e}"
            );
            let mut g = GameConfig::custom("foo");
            g.paks_dir = Some(bad.into());
            assert!(save_game(store.conn(), &g).is_err(), "paks {bad}");
        }
        assert!(get_game(store.conn(), "foo").unwrap().is_none());
        let mut g = GameConfig::custom("foo");
        g.subdir = "./Data/".into();
        save_game(store.conn(), &g).unwrap();
    }

    fn store_with(domain: &str) -> Store {
        let store = Store::open_in_memory().unwrap();
        let defs = builtin_definitions();
        let g = find_definition(&defs, domain)
            .map(GameConfig::from_definition)
            .unwrap_or_else(|| GameConfig::custom(domain));
        save_game(store.conn(), &g).unwrap();
        store
    }

    fn mark_deployed(store: &Store, domain: &str) {
        store
            .conn()
            .execute(
                "INSERT INTO deployments (game, target, rel_path, method, source, deployed_at)
                 VALUES (?1, 'game_dir', 'Data/a.esp', 'hardlink', '/s/a.esp', 0)",
                [domain],
            )
            .unwrap();
    }

    #[test]
    fn game_setting_accepts_exactly_the_nine_legacy_keys() {
        let keys = [
            "folder",
            "subdir",
            "profile",
            "appid",
            "method",
            "prefix",
            "plugins-file",
            "paks-dir",
            "name",
        ];
        for k in keys {
            assert_eq!(k.parse::<GameSetting>().unwrap().name(), k);
        }
        for bad in ["plugins-format", "Folder", "staging", ""] {
            assert!(matches!(
                bad.parse::<GameSetting>(),
                Err(GamesError::UnknownSetting(_))
            ));
        }
    }

    #[test]
    fn subdir_and_paks_dir_settings_refuse_escaping_paths() {
        let store = store_with("newgame");
        let home = Path::new("/home/u");
        for (key, bad) in [
            (GameSetting::Subdir, "../escape"),
            (GameSetting::Subdir, "/etc"),
            (GameSetting::PaksDir, "Pal/../../x"),
        ] {
            assert!(
                matches!(
                    apply_setting(store.conn(), "newgame", key, bad, home),
                    Err(GamesError::InvalidValue { .. })
                ),
                "{bad}"
            );
        }
        let g = apply_setting(
            store.conn(),
            "newgame",
            GameSetting::Subdir,
            "./Data/",
            home,
        )
        .unwrap();
        assert_eq!(g.subdir, "./Data/");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let store = store_with("skyrimspecialedition");
        let g = require_game(store.conn(), "skyrimspecialedition").unwrap();
        assert_eq!(g.subdir, "Data");
        assert_eq!(g.plugins_format, Some(PluginsFormat::Star));
        assert_eq!(g.appdata_folder.as_deref(), Some("Skyrim Special Edition"));
        assert_eq!(g.support(&builtin_definitions()), Support::Tested);
        assert_eq!(list_games(store.conn()).unwrap().len(), 1);
    }

    #[test]
    fn set_values_are_validated_and_normalised() {
        let store = store_with("skyrimspecialedition");
        let c = store.conn();
        let home = Path::new("/home/u");
        let d = "skyrimspecialedition";
        let g = apply_setting(c, d, GameSetting::Folder, "~/Games/SSE/", home).unwrap();
        assert_eq!(g.folder, Some(PathBuf::from("/home/u/Games/SSE")));
        assert_eq!(
            apply_setting(c, d, GameSetting::Subdir, "", home)
                .unwrap()
                .subdir,
            "."
        );
        assert!(apply_setting(c, d, GameSetting::Profile, "nosuch", home).is_err());
        assert!(apply_setting(c, d, GameSetting::Method, "teleport", home).is_err());
        assert_eq!(
            apply_setting(c, d, GameSetting::Method, "", home)
                .unwrap()
                .method,
            None
        );
        assert!(apply_setting(c, d, GameSetting::Appid, "abc", home).is_err());
        let g = apply_setting(c, d, GameSetting::PaksDir, "Foo/Paks", home).unwrap();
        assert_eq!(g.paks_dir.as_deref(), Some("Foo/Paks"));
        assert_eq!(
            g.appdata_folder.as_deref(),
            Some("Skyrim Special Edition"),
            "A1: separate fields"
        );
        let g = apply_setting(c, d, GameSetting::Prefix, "~/pfx", home).unwrap();
        assert_eq!(
            g.plugins_txt(),
            Some(PathBuf::from(
                "/home/u/pfx/drive_c/users/steamuser/AppData/Local/Skyrim Special Edition/plugins.txt"
            ))
        );
    }

    #[test]
    fn folder_subdir_profile_refused_while_deployed() {
        let store = store_with("skyrimspecialedition");
        mark_deployed(&store, "skyrimspecialedition");
        let home = Path::new("/h");
        for key in [
            GameSetting::Folder,
            GameSetting::Subdir,
            GameSetting::Profile,
        ] {
            assert!(matches!(
                apply_setting(store.conn(), "skyrimspecialedition", key, "generic", home),
                Err(GamesError::DeploymentActive(_))
            ));
        }
        apply_setting(
            store.conn(),
            "skyrimspecialedition",
            GameSetting::Name,
            "x",
            home,
        )
        .unwrap();
    }

    #[test]
    fn remove_refuses_while_deployed_or_backups_held() {
        let mut store = store_with("skyrimspecialedition");
        set_active_game(&store, "skyrimspecialedition").unwrap();
        mark_deployed(&store, "skyrimspecialedition");
        assert!(matches!(
            remove_game(&mut store, "skyrimspecialedition"),
            Err(GamesError::DeploymentActive(_))
        ));
        store.conn().execute("DELETE FROM deployments", []).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO backups (game, target, rel_path, sha256, store_path, size, mode, created_at)
                 VALUES ('skyrimspecialedition', 'game_dir', 'Data/x', 'h', '/b/h', 1, 420, 0)",
                [],
            )
            .unwrap();
        assert!(deployment_active(store.conn(), "skyrimspecialedition").unwrap());
        store
            .conn()
            .execute("UPDATE backups SET restored_at = 1", [])
            .unwrap();
        remove_game(&mut store, "skyrimspecialedition").unwrap();
        assert!(get_game(store.conn(), "skyrimspecialedition")
            .unwrap()
            .is_none());
        assert_eq!(active_game(&store).unwrap(), None);
    }

    #[test]
    fn resolve_game_order_is_override_active_then_single() {
        let store = store_with("skyrimspecialedition");
        assert_eq!(
            resolve_game(&store, None).unwrap().domain,
            "skyrimspecialedition"
        );
        save_game(store.conn(), &GameConfig::custom("othergame")).unwrap();
        assert!(matches!(
            resolve_game(&store, None),
            Err(GamesError::NoActiveGame)
        ));
        set_active_game(&store, "othergame").unwrap();
        assert_eq!(resolve_game(&store, None).unwrap().domain, "othergame");
        assert_eq!(
            resolve_game(&store, Some("SkyrimSpecialEdition"))
                .unwrap()
                .domain,
            "skyrimspecialedition"
        );
        assert!(matches!(
            resolve_game(&store, Some("nosuch")),
            Err(GamesError::NotConfigured(_))
        ));
    }

    #[test]
    fn domain_validation() {
        assert_eq!(normalize_domain("Fallout4").unwrap(), "fallout4");
        assert!(normalize_domain("fall-out").is_err());
        assert!(normalize_domain("").is_err());
    }

    #[test]
    fn builtin_profiles_are_known() {
        for d in builtin_definitions() {
            assert!(KNOWN_PROFILES.contains(&d.profile.as_str()));
        }
    }
}
