// SPDX-License-Identifier: GPL-3.0-or-later
//! `game list|add|use|remove|info|set|detect` (legacy `cmd_game` L4080-4171,
//! `game_add_flow` L4019-4078).

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};
use modman_core::app::games::AddGame;
use modman_core::config::Config;
use modman_core::games::{self, Support};

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

pub(crate) fn add(
    ctx: &mut Ctx,
    domain: &str,
    path: Option<String>,
    subdir: Option<String>,
    profile: Option<String>,
) -> Result<()> {
    let ui = ctx.ui();
    modman_core::app::games::add_game(
        &ctx.cx(&ui),
        AddGame {
            domain: domain.to_string(),
            path,
            subdir,
            profile,
        },
    )?;
    Ok(())
}

fn use_game(ctx: &Ctx, domain: &str) -> Result<()> {
    let domain = domain.to_ascii_lowercase();
    let g = games::require_game(ctx.store.conn(), &domain).map_err(games_hint)?;
    games::set_active_game(&ctx.store, &domain)?;
    ctx.out.success(&format!("Active game: {}", g.name));
    Ok(())
}

fn remove(ctx: &mut Ctx, domain: &str, delete_data: bool) -> Result<()> {
    let ui = ctx.ui();
    modman_core::app::games::remove(&mut ctx.cx(&ui), domain, delete_data)?;
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

fn set(ctx: &mut Ctx, key: &str, value: &str) -> Result<()> {
    // Validate the key before resolving the game (legacy order).
    let _: modman_core::games::GameSetting = key.parse()?;
    let g = ctx.need_game()?;
    let ui = ctx.ui();
    modman_core::app::games::set_setting(&ctx.cx(&ui), &g.domain, key, value)?;
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
