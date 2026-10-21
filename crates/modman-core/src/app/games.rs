// SPDX-License-Identifier: GPL-3.0-or-later
//! Game setup (legacy `game_add_flow` L4019-4078) and read-only overviews of
//! the configured games for the front ends.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{fail, Cx, Msg, R};
use crate::config::expand_tilde;
use crate::deploy::{self, DeployStatus};
use crate::games::{self, steam, GameConfig, Support, KNOWN_PROFILES};

/// Options of [`add_game`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddGame {
    pub domain: String,
    /// Absolute game folder; detected through Steam when absent.
    pub path: Option<String>,
    pub subdir: Option<String>,
    pub profile: Option<String>,
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Sets up (or updates) a game: preset defaults, Nexus domain check when
/// logged in, Steam folder/prefix detection, active-game switch.
pub fn add_game(cx: &Cx<'_>, req: AddGame) -> R<GameConfig> {
    let ui = cx.ui;
    let domain = games::normalize_domain(&req.domain)?;
    let defs = games::load_definitions(Some(&cx.paths.definitions_dir()))?;
    let def = games::find_definition(&defs, &domain);
    let existing = games::get_game(cx.store.conn(), &domain)?;
    let mut g = match (&existing, def) {
        (Some(g), _) => g.clone(),
        (None, Some(d)) => GameConfig::from_definition(d),
        (None, None) => GameConfig::custom(&domain),
    };
    // Logged in: Nexus must know the domain (legacy L4027-4034).
    if let Some(nexus) =
        super::nexus::connect(cx.paths, cx.store, false)?.filter(|n| n.client.is_authenticated())
    {
        match nexus.run(nexus.client.game(&domain))? {
            Ok(info) => {
                if def.is_none() && existing.is_none() && !info.name.is_empty() {
                    g.name = info.name.clone();
                }
                g.nexus_game_id = Some(info.id as i64);
            }
            Err(e) => fail!("Nexus does not know the game domain '{domain}': {e}"),
        }
    }
    let mut profile = req.profile;
    let mut subdir = req.subdir;
    if def.is_none() {
        ui.info(&format!(
            "No built-in preset for {} — using generic settings.",
            g.name
        ));
        if profile.is_none() {
            profile = ui.ask(
                &format!("Layout profile ({})", KNOWN_PROFILES.join(" ")),
                "generic",
            );
        }
        if subdir.is_none() {
            subdir = ui.ask("Folder inside the game where mods go (. = game root)", ".");
        }
    }
    if let Some(p) = profile {
        g.profile = p;
    }
    if !KNOWN_PROFILES.contains(&g.profile.as_str()) {
        fail!(
            "Unknown profile '{}'. Choose one of: {}",
            g.profile,
            KNOWN_PROFILES.join(" ")
        );
    }
    if let Some(s) = subdir.filter(|s| !s.is_empty()) {
        g.subdir = s;
    }

    let libs = steam::steam_libraries(&cx.paths.home);
    let app = g.appid.and_then(|id| steam::find_app(&libs, id));
    let path = match req.path {
        Some(p) => p,
        None => {
            let mut found = None;
            if let Some(app) = &app {
                ui.info(&format!(
                    "Found {} in Steam: {}",
                    g.name,
                    app.install_dir.display()
                ));
                if ui.confirm("Use this folder?", true) {
                    found = Some(app.install_dir.display().to_string());
                }
            }
            let fallback = g.folder.as_ref().map(|f| f.display().to_string());
            match found {
                Some(p) => p,
                None => match ui.ask(
                    &format!("Absolute path to the {} folder", g.name),
                    fallback.as_deref().unwrap_or(""),
                ) {
                    Some(p) => p,
                    None => match fallback {
                        Some(p) => p,
                        None => fail!(
                            "Game folder not found automatically — pass it: modman game add {domain} <path>"
                        ),
                    },
                },
            }
        }
    };
    let path = expand_tilde(&path, &cx.paths.home);
    let path = path.trim_end_matches('/');
    if !path.starts_with('/') {
        fail!("The game folder must be an absolute path.");
    }
    let folder = PathBuf::from(path);
    if !folder.is_dir() {
        ui.warn(&format!("Folder does not exist yet: {}", folder.display()));
    }
    if let Some(app) = &app {
        if same_dir(&folder, &app.install_dir) {
            games::apply_steam_app(&mut g, app);
        }
    }
    if g.prefix.is_none() {
        if let Some(id) = g.appid {
            g.prefix = steam::find_prefix(&libs, id, None);
        }
    }
    g.folder = Some(folder);
    games::save_game(cx.store.conn(), &g)?;

    let active = games::active_game(cx.store)?;
    if active.is_none()
        || active.as_deref() == Some(domain.as_str())
        || ui.confirm(&format!("Make {} the active game?", g.name), true)
    {
        games::set_active_game(cx.store, &domain)?;
    }

    ui.say(
        Msg::success("Game ")
            .b(&g.name)
            .p(format!(" ({domain}) configured.")),
    );
    let folder = g.folder.as_deref().unwrap_or(Path::new(""));
    ui.line(&format!("    Folder : {}", folder.display()));
    ui.say(
        Msg::line(format!(
            "    Mods go: {}   ",
            folder.join(&g.subdir).display()
        ))
        .d(format!("(profile: {})", g.profile)),
    );
    if let Some(p) = g.plugins_txt() {
        ui.line(&format!("    Plugins: {}", p.display()));
    }
    ui.line(&format!(
        "    Data   : {}",
        cx.config()?.data_dir.join(&domain).display()
    ));
    if g.support(&defs) == Support::Experimental {
        ui.warn(&format!(
            "{} support is experimental (not tested end to end).",
            g.name
        ));
    }
    Ok(g)
}

/// Forgets a game after a confirmation (refused while deployed);
/// `delete_data` also deletes its downloads and staging. `false` when the
/// user declined.
pub fn remove(cx: &mut Cx<'_>, domain: &str, delete_data: bool) -> R<bool> {
    let domain = domain.to_ascii_lowercase();
    let g = games::require_game(cx.store.conn(), &domain).map_err(super::games_hint)?;
    if games::deployment_active(cx.store.conn(), &domain)? {
        return Err(super::games_hint(games::GamesError::DeploymentActive(
            domain,
        )));
    }
    let data = cx.config()?.data_dir.join(&domain);
    let question = if delete_data {
        format!("Forget {}? This also DELETES {}", g.name, data.display())
    } else {
        format!("Forget {}?", g.name)
    };
    if !cx.ui.confirm(&question, false) {
        return Ok(false);
    }
    games::remove_game(cx.store, &domain).map_err(super::games_hint)?;
    if delete_data && data.exists() {
        std::fs::remove_dir_all(&data)?;
    }
    cx.ui.success(&format!("Removed {domain}."));
    Ok(true)
}

/// `game set <key> <value>` for `domain`.
pub fn set_setting(cx: &Cx<'_>, domain: &str, key: &str, value: &str) -> R<GameConfig> {
    let setting: games::GameSetting = key.parse()?;
    let g = games::apply_setting(cx.store.conn(), domain, setting, value, &cx.paths.home)
        .map_err(super::games_hint)?;
    if setting == games::GameSetting::Folder {
        if let Some(f) = g.folder.as_ref().filter(|f| !f.is_dir()) {
            cx.ui
                .warn(&format!("Folder does not exist: {}", f.display()));
        }
    }
    cx.ui
        .success(&format!("{}: {} = {value}", g.domain, setting.name()));
    Ok(g)
}

/// A configured game with its state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GameView {
    #[serde(flatten)]
    pub config: GameConfig,
    pub active: bool,
    pub support: Support,
    pub mod_count: i64,
    pub deploy: DeployStatus,
    /// Folder for downloads and staging.
    pub data_dir: PathBuf,
    /// Effective plugins.txt.
    pub plugins_txt: Option<PathBuf>,
}

/// Every configured game; `active` is `-g` or the active game.
pub fn overview(cx: &Cx<'_>) -> R<Vec<GameView>> {
    let defs = games::load_definitions(Some(&cx.paths.definitions_dir()))?;
    let active = match cx.game {
        Some(g) => Some(g.to_string()),
        None => games::active_game(cx.store)?,
    };
    let data = cx.config()?.data_dir;
    let mut v = Vec::new();
    for g in games::list_games(cx.store.conn())? {
        v.push(GameView {
            active: active.as_deref() == Some(g.domain.as_str()),
            support: g.support(&defs),
            mod_count: games::mod_count(cx.store.conn(), &g.domain)?,
            deploy: deploy::status(cx.store.conn(), &g.domain)?,
            data_dir: data.join(&g.domain),
            plugins_txt: g.plugins_txt(),
            config: g,
        });
    }
    Ok(v)
}

/// A supported game found in a Steam library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detected {
    pub domain: String,
    pub name: String,
    pub install_dir: PathBuf,
    pub support: Support,
    pub configured: bool,
}

/// Supported games installed through Steam (read-only).
pub fn detect(cx: &Cx<'_>) -> R<Vec<Detected>> {
    let defs = games::load_definitions(Some(&cx.paths.definitions_dir()))?;
    let mut v = Vec::new();
    for d in games::detect_installed(&cx.paths.home, &defs) {
        v.push(Detected {
            configured: games::get_game(cx.store.conn(), &d.definition.domain)?.is_some(),
            domain: d.definition.domain.clone(),
            name: d.definition.name.clone(),
            install_dir: d.app.install_dir.clone(),
            support: d.definition.support,
        });
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::Recorder;
    use crate::paths::Paths;
    use crate::store::Store;

    #[test]
    fn non_interactive_add_needs_a_folder_and_marks_experimental_games() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::for_home(home.path());
        let mut store = Store::open_in_memory().unwrap();
        let ui = Recorder::default();
        let cx = Cx::new(&paths, &mut store, &ui);
        let e = add_game(
            &cx,
            AddGame {
                domain: "stardewvalley".into(),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(e.to_string().contains("pass it: modman game add"), "{e}");
        let dir = home.path().join("sv");
        std::fs::create_dir(&dir).unwrap();
        let g = add_game(
            &cx,
            AddGame {
                domain: "stardewvalley".into(),
                path: Some(dir.display().to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(g.folder.as_deref(), Some(dir.as_path()));
        let views = overview(&cx).unwrap();
        assert!(views[0].active);
        assert_eq!(views[0].support, Support::Experimental);
        assert!(ui.texts().iter().any(|t| t.contains("experimental")));
    }
}
