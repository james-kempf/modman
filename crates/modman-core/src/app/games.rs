// SPDX-License-Identifier: GPL-3.0-or-later
//! Game setup (legacy `game_add_flow` L4019-4078) and read-only overviews of
//! the configured games for the front ends.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{fail, Cx, Msg, R};
use crate::config::expand_tilde;
use crate::deploy::{self, DeployStatus};
use crate::games::{self, steam, GameConfig, Support, Target, KNOWN_PROFILES};

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
    // Before any folder question; save_game checks again.
    games::check_game_dirs(&g)?;

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

/// What [`launch`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Launched {
    pub appid: u32,
    /// Pending mod changes were deployed first.
    pub deployed: bool,
}

/// Starts `game` through Steam (`steam://rungameid/<appid>`). When mods
/// changed since the last deploy, asks whether to deploy first (default
/// yes). Legacy `cmd_launch`.
pub fn launch(cx: &mut Cx<'_>, game: &GameConfig) -> R<Launched> {
    let Some(appid) = game.appid else {
        fail!(
            "No Steam app id for {}. Set one with: modman game set appid <id>",
            game.name
        );
    };
    let enabled = crate::mods::list_mods(cx.store.conn(), &game.domain)?
        .iter()
        .any(|m| m.enabled);
    let st = deploy::status(cx.store.conn(), &game.domain)?;
    let mut deployed = false;
    if enabled && crate::mods::changed_since(cx.store, &game.domain, st.last_deployed_at)? {
        cx.ui.warn("Mods changed since the last deploy.");
        if cx.ui.confirm("Deploy before launching?", true) {
            super::deploy::run_deploy(cx, game, None, false)?;
            deployed = true;
        }
    }
    cx.ui
        .info(&format!("Launching {} (Steam app {appid})...", game.name));
    let url = format!("steam://rungameid/{appid}");
    let (prog, args): (PathBuf, Vec<String>) = match super::which("steam") {
        // setsid: the game outlives a CLI run's terminal.
        Some(steam) => match super::which("setsid") {
            Some(setsid) => (setsid, vec![steam.display().to_string(), url]),
            None => (steam, vec![url]),
        },
        None => match super::which("xdg-open") {
            Some(x) => (x, vec![url]),
            None => fail!("Neither steam nor xdg-open was found to start the game."),
        },
    };
    std::process::Command::new(prog)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(Launched { appid, deployed })
}

/// A folder of a game that [`open_folder`] opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameFolder {
    /// The game's install folder.
    Game,
    /// The Proton (Wine) prefix.
    Prefix,
}

/// The folder's path, taken only from the game's stored configuration: an
/// existing directory, as an absolute canonical path.
pub fn folder_path(game: &GameConfig, which: GameFolder) -> R<PathBuf> {
    let target = match which {
        GameFolder::Game => Target::GameDir,
        GameFolder::Prefix => Target::PrefixDir,
    };
    let root = deploy::target_root(game, target)?;
    Ok(root.canonicalize()?)
}

/// Opens a folder of `domain` with `open` (the file manager, see
/// [`launch_file_manager`]); returns the path opened.
pub fn open_folder(
    cx: &Cx<'_>,
    domain: &str,
    which: GameFolder,
    open: impl FnOnce(&Path) -> R<()>,
) -> R<PathBuf> {
    let g = games::require_game(cx.store.conn(), domain)?;
    let p = folder_path(&g, which)?;
    open(&p)?;
    Ok(p)
}

/// Shows `dir` in the desktop's file manager (`xdg-open`, else `gio open`),
/// detached: no shell, output discarded, the child reaped in the background.
pub fn launch_file_manager(dir: &Path) -> R<()> {
    let (prog, pre): (PathBuf, &[&str]) = match super::which("xdg-open") {
        Some(x) => (x, &[]),
        None => match super::which("gio") {
            Some(g) => (g, &["open"]),
            None => fail!(
                "Neither xdg-open nor gio was found to open {}.",
                dir.display()
            ),
        },
    };
    let mut child = std::process::Command::new(prog)
        .args(pre)
        .arg(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
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

    #[test]
    fn add_rejects_a_subdir_outside_the_game_folder() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::for_home(home.path());
        let mut store = Store::open_in_memory().unwrap();
        let ui = Recorder::default();
        let cx = Cx::new(&paths, &mut store, &ui);
        for bad in ["../../x", "/etc"] {
            let e = add_game(
                &cx,
                AddGame {
                    domain: "foo".into(),
                    path: Some("/g".into()),
                    subdir: Some(bad.into()),
                    profile: Some("generic".into()),
                },
            )
            .unwrap_err();
            assert!(e.to_string().contains("subdir"), "{bad}: {e}");
        }
        assert!(games::get_game(cx.store.conn(), "foo").unwrap().is_none());
    }

    /// Answers every text question with a traversal.
    struct Traversal;
    impl crate::app::Ui for Traversal {
        fn say(&self, _msg: Msg) {}
        fn ask(&self, _q: &str, _d: &str) -> Option<String> {
            Some("../../x".into())
        }
    }

    #[test]
    fn add_rejects_a_traversal_typed_at_the_subdir_question() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::for_home(home.path());
        let mut store = Store::open_in_memory().unwrap();
        let cx = Cx::new(&paths, &mut store, &Traversal);
        let e = add_game(
            &cx,
            AddGame {
                domain: "foo".into(),
                path: Some("/g".into()),
                profile: Some("generic".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(e.to_string().contains("subdir"), "{e}");
    }

    fn stored_game(cx: &Cx<'_>, folder: Option<PathBuf>, prefix: Option<PathBuf>) {
        let mut g = GameConfig::custom("foo");
        g.folder = folder;
        g.prefix = prefix;
        games::save_game(cx.store.conn(), &g).unwrap();
    }

    // Fails when a folder other than the stored game folder / prefix could be opened,
    // or a missing / unset one reaches the file manager.
    #[test]
    fn open_folder_resolves_only_configured_existing_folders() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::for_home(home.path());
        let mut store = Store::open_in_memory().unwrap();
        let ui = Recorder::default();
        let cx = Cx::new(&paths, &mut store, &ui);
        let game = home.path().join("game");
        let pfx = home.path().join("pfx");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::create_dir_all(&pfx).unwrap();
        // A symlinked game folder opens as its real path.
        let link = home.path().join("link");
        std::os::unix::fs::symlink(&game, &link).unwrap();
        stored_game(&cx, Some(link), None);
        let mut opened = Vec::new();
        let p = open_folder(&cx, "foo", GameFolder::Game, |d| {
            opened.push(d.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(p, game.canonicalize().unwrap());
        assert_eq!(opened, std::slice::from_ref(&p));
        // No prefix configured: refused, nothing opened.
        let never = |_: &Path| -> R<()> { panic!("must not open") };
        let e = open_folder(&cx, "foo", GameFolder::Prefix, never).unwrap_err();
        assert!(e.to_string().contains("Proton prefix"), "{e}");
        // Configured prefix opens.
        stored_game(&cx, Some(game.clone()), Some(pfx.clone()));
        let p = open_folder(&cx, "foo", GameFolder::Prefix, |_| Ok(())).unwrap();
        assert_eq!(p, pfx.canonicalize().unwrap());
        // A configured folder that is gone (or a file) is refused.
        stored_game(
            &cx,
            Some(home.path().join("gone")),
            Some(home.path().join("f")),
        );
        std::fs::write(home.path().join("f"), b"x").unwrap();
        assert!(open_folder(&cx, "foo", GameFolder::Game, never).is_err());
        assert!(open_folder(&cx, "foo", GameFolder::Prefix, never).is_err());
        // Unknown game.
        assert!(open_folder(&cx, "nope", GameFolder::Game, never).is_err());
    }

    // Fails when the webview names a folder kind other than game/prefix.
    #[test]
    fn folder_kinds_deserialize_from_game_and_prefix_only() {
        let g: GameFolder = serde_json::from_str("\"game\"").unwrap();
        assert_eq!(g, GameFolder::Game);
        let g: GameFolder = serde_json::from_str("\"prefix\"").unwrap();
        assert_eq!(g, GameFolder::Prefix);
        assert!(serde_json::from_str::<GameFolder>("\"/etc\"").is_err());
    }
}
