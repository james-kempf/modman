// SPDX-License-Identifier: GPL-3.0-or-later
//! `game list|add|use|remove|info|set|detect` (legacy `cmd_game` L4080-4171,
//! `game_add_flow` L4019-4078).

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use modman_core::config::{expand_tilde, Config};
use modman_core::games::{self, steam, GameConfig, GameSetting, Support, KNOWN_PROFILES};

use crate::context::{games_hint, Ctx};

#[derive(Args, Debug)]
pub struct GameArgs {
    #[command(subcommand)]
    pub cmd: Option<GameCmd>,
}

#[derive(Subcommand, Debug)]
pub enum GameCmd {
    /// Show configured games (default).
    #[command(visible_alias = "ls")]
    List,
    /// Set up a game (Steam folder auto-detected).
    Add {
        /// Nexus game domain, as in nexusmods.com/<domain>/mods/...
        domain: String,
        /// Absolute path to the game folder.
        path: Option<String>,
        /// Folder inside the game where mods go (. = game root).
        #[arg(long)]
        subdir: Option<String>,
        /// Layout profile.
        #[arg(long)]
        profile: Option<String>,
    },
    /// Switch the active game.
    #[command(visible_aliases = ["switch", "select"])]
    Use { domain: String },
    /// Forget a game (purge first).
    #[command(visible_alias = "rm")]
    Remove {
        domain: String,
        /// Also delete the game's downloads and data.
        #[arg(long = "delete-data")]
        delete_data: bool,
    },
    /// Show game settings.
    #[command(visible_alias = "show")]
    Info { domain: Option<String> },
    /// Change a game setting: folder, subdir, profile, appid, method, prefix,
    /// plugins-file, paks-dir, name.
    Set {
        key: String,
        #[arg(allow_hyphen_values = true)]
        value: String,
    },
    /// List supported games installed through Steam.
    Detect,
}

pub fn run(ctx: &mut Ctx, args: GameArgs) -> Result<()> {
    match args.cmd.unwrap_or(GameCmd::List) {
        GameCmd::List => list(ctx),
        GameCmd::Add {
            domain,
            path,
            subdir,
            profile,
        } => add(ctx, &domain, path, subdir, profile),
        GameCmd::Use { domain } => use_game(ctx, &domain),
        GameCmd::Remove {
            domain,
            delete_data,
        } => remove(ctx, &domain, delete_data),
        GameCmd::Info { domain } => info(ctx, domain.as_deref()),
        GameCmd::Set { key, value } => set(ctx, &key, &value),
        GameCmd::Detect => detect(ctx),
    }
}

fn game_data_dir(ctx: &Ctx, domain: &str) -> Result<PathBuf> {
    Ok(Config::load(&ctx.store, &ctx.paths)?.data_dir.join(domain))
}

fn list(ctx: &Ctx) -> Result<()> {
    let out = ctx.out;
    out.heading("Games");
    let all = games::list_games(ctx.store.conn())?;
    if all.is_empty() {
        out.info("No games configured. Run: modman game add <domain>   (see: modman game detect)");
        return Ok(());
    }
    let active = match &ctx.game {
        Some(g) => Some(g.clone()),
        None => games::active_game(&ctx.store)?,
    };
    for g in all {
        let mark = if active.as_deref() == Some(g.domain.as_str()) {
            out.green("* ")
        } else {
            "  ".to_string()
        };
        let count = games::mod_count(ctx.store.conn(), &g.domain)?;
        let deployed = if games::deployment_active(ctx.store.conn(), &g.domain)? {
            out.green("deployed")
        } else {
            "not deployed".to_string()
        };
        println!(
            "  {mark}{}  {}  {}{deployed}",
            out.bold(&g.domain),
            g.name,
            out.dim(&format!("— {count} mods, "))
        );
        let folder = g
            .folder
            .as_ref()
            .map_or("<no folder>".to_string(), |f| f.display().to_string());
        println!("      {}", out.dim(&folder));
    }
    Ok(())
}

fn add(
    ctx: &Ctx,
    domain: &str,
    path: Option<String>,
    subdir: Option<String>,
    profile: Option<String>,
) -> Result<()> {
    let out = ctx.out;
    let domain = games::normalize_domain(domain)?;
    let defs = ctx.definitions()?;
    let def = games::find_definition(&defs, &domain);
    let existing = games::get_game(ctx.store.conn(), &domain)?;
    let mut g = match (&existing, def) {
        (Some(g), _) => g.clone(),
        (None, Some(d)) => GameConfig::from_definition(d),
        (None, None) => GameConfig::custom(&domain),
    };
    let mut profile = profile;
    let mut subdir = subdir;
    if def.is_none() {
        out.info(&format!(
            "No built-in preset for {} — using generic settings.",
            g.name
        ));
        if profile.is_none() && ctx.is_interactive() {
            profile = Some(ctx.ask(
                &format!("Layout profile ({})", KNOWN_PROFILES.join(" ")),
                "generic",
            ));
        }
        if subdir.is_none() && ctx.is_interactive() {
            subdir = Some(ctx.ask("Folder inside the game where mods go (. = game root)", "."));
        }
    }
    if let Some(p) = profile {
        g.profile = p;
    }
    if !KNOWN_PROFILES.contains(&g.profile.as_str()) {
        bail!(
            "Unknown profile '{}'. Choose one of: {}",
            g.profile,
            KNOWN_PROFILES.join(" ")
        );
    }
    if let Some(s) = subdir.filter(|s| !s.is_empty()) {
        g.subdir = s;
    }

    let libs = steam::steam_libraries(&ctx.paths.home);
    let app = g.appid.and_then(|id| steam::find_app(&libs, id));
    let path = match path {
        Some(p) => p,
        None => {
            let mut found = None;
            if let Some(app) = &app {
                out.info(&format!(
                    "Found {} in Steam: {}",
                    g.name,
                    app.install_dir.display()
                ));
                if !ctx.is_interactive() || ctx.confirm("Use this folder?", true) {
                    found = Some(app.install_dir.display().to_string());
                }
            }
            let fallback = g.folder.as_ref().map(|f| f.display().to_string());
            match (found, fallback) {
                (Some(p), _) => p,
                (None, fallback) if ctx.is_interactive() => ctx.ask(
                    &format!("Absolute path to the {} folder", g.name),
                    fallback.as_deref().unwrap_or(""),
                ),
                (None, Some(p)) => p,
                (None, None) => bail!(
                    "Game folder not found automatically — pass it: modman game add {domain} <path>"
                ),
            }
        }
    };
    let path = expand_tilde(&path, &ctx.paths.home);
    let path = path.trim_end_matches('/');
    if !path.starts_with('/') {
        bail!("The game folder must be an absolute path.");
    }
    let folder = PathBuf::from(path);
    if !folder.is_dir() {
        out.warn(&format!("Folder does not exist yet: {}", folder.display()));
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
    games::save_game(ctx.store.conn(), &g)?;

    let active = games::active_game(&ctx.store)?;
    if active.is_none()
        || active.as_deref() == Some(domain.as_str())
        || ctx.confirm(&format!("Make {} the active game?", g.name), true)
    {
        games::set_active_game(&ctx.store, &domain)?;
    }

    out.success(&format!(
        "Game {} ({domain}) configured.",
        out.bold(&g.name)
    ));
    let folder = g.folder.as_deref().unwrap_or(Path::new(""));
    println!("    Folder : {}", folder.display());
    println!(
        "    Mods go: {}   {}",
        folder.join(&g.subdir).display(),
        out.dim(&format!("(profile: {})", g.profile))
    );
    if let Some(p) = g.plugins_txt() {
        println!("    Plugins: {}", p.display());
    }
    println!("    Data   : {}", game_data_dir(ctx, &domain)?.display());
    if g.support(&defs) == Support::Experimental {
        out.warn(&format!(
            "{} support is experimental (not tested end to end).",
            g.name
        ));
    }
    Ok(())
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn use_game(ctx: &Ctx, domain: &str) -> Result<()> {
    let domain = domain.to_ascii_lowercase();
    let g = games::require_game(ctx.store.conn(), &domain).map_err(games_hint)?;
    games::set_active_game(&ctx.store, &domain)?;
    ctx.out.success(&format!("Active game: {}", g.name));
    Ok(())
}

fn remove(ctx: &mut Ctx, domain: &str, delete_data: bool) -> Result<()> {
    let domain = domain.to_ascii_lowercase();
    let g = games::require_game(ctx.store.conn(), &domain).map_err(games_hint)?;
    if games::deployment_active(ctx.store.conn(), &domain)? {
        return Err(games_hint(games::GamesError::DeploymentActive(domain)));
    }
    let data = game_data_dir(ctx, &domain)?;
    let question = if delete_data {
        format!("Forget {}? This also DELETES {}", g.name, data.display())
    } else {
        format!("Forget {}?", g.name)
    };
    if !ctx.confirm(&question, false) {
        return Ok(());
    }
    games::remove_game(&mut ctx.store, &domain).map_err(games_hint)?;
    if delete_data && data.exists() {
        std::fs::remove_dir_all(&data)?;
    }
    ctx.out.success(&format!("Removed {domain}."));
    Ok(())
}

fn info(ctx: &Ctx, domain: Option<&str>) -> Result<()> {
    let out = ctx.out;
    let g = ctx.game_or(domain.or(ctx.game.as_deref()))?;
    let cfg = Config::load(&ctx.store, &ctx.paths)?;
    let defs = ctx.definitions()?;
    let dash = |p: &Option<PathBuf>| p.as_ref().map_or("—".into(), |p| p.display().to_string());
    out.heading(&format!("{} ({})", g.name, g.domain));
    let rows = [
        ("folder", dash(&g.folder)),
        ("subdir", g.subdir.clone()),
        ("profile", g.profile.clone()),
        ("appid", g.appid.map_or("—".into(), |a| a.to_string())),
        (
            "method",
            g.method
                .clone()
                .unwrap_or_else(|| format!("(global: {})", cfg.deploy_method)),
        ),
        ("prefix", dash(&g.prefix)),
        ("plugins-file", dash(&g.plugins_txt())),
        ("paks-dir", g.paks_dir.clone().unwrap_or_else(|| "—".into())),
        ("support", g.support(&defs).as_str().to_string()),
        ("data", cfg.data_dir.join(&g.domain).display().to_string()),
    ];
    for (k, v) in rows {
        println!("  {k:<14} {v}");
    }
    println!();
    println!(
        "  {}",
        out.dim("Change with: modman game set <key> <value>")
    );
    Ok(())
}

fn set(ctx: &Ctx, key: &str, value: &str) -> Result<()> {
    let setting: GameSetting = key.parse()?;
    let g = ctx.need_game()?;
    let g = games::apply_setting(ctx.store.conn(), &g.domain, setting, value, &ctx.paths.home)
        .map_err(games_hint)?;
    if setting == GameSetting::Folder {
        if let Some(f) = g.folder.as_ref().filter(|f| !f.is_dir()) {
            ctx.out
                .warn(&format!("Folder does not exist: {}", f.display()));
        }
    }
    ctx.out
        .success(&format!("{}: {} = {value}", g.domain, setting.name()));
    Ok(())
}

fn detect(ctx: &Ctx) -> Result<()> {
    let out = ctx.out;
    out.heading("Installed games with presets");
    let defs = ctx.definitions()?;
    let found = games::detect_installed(&ctx.paths.home, &defs);
    if found.is_empty() {
        out.info(
            "No supported Steam games found. Any Nexus game works: modman game add <domain> <path>",
        );
        return Ok(());
    }
    for d in found {
        let domain = &d.definition.domain;
        let status = if games::get_game(ctx.store.conn(), domain)?.is_some() {
            out.green("configured")
        } else {
            format!("{} → modman game add {domain}", out.yellow("not set up"))
        };
        let support = match d.definition.support {
            Support::Tested => String::new(),
            Support::Experimental => format!("  {}", out.dim("(experimental)")),
        };
        println!(
            "  {}  {}  {}{support}\n      {status}",
            out.bold(domain),
            d.definition.name,
            out.dim(&d.app.install_dir.display().to_string())
        );
    }
    Ok(())
}
