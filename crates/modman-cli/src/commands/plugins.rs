// SPDX-License-Identifier: GPL-3.0-or-later
//! Bethesda plugins (M6, new; legacy only appended to plugins.txt and hinted
//! at LOOT, L2100): `plugins [list]`, `plugins sort [--update-masterlist]`,
//! `plugins masterlist`. Deploy and purge run the plugin stage themselves.

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use modman_core::config::Config;
use modman_core::deploy::DeployEnv;
use modman_core::games::GameConfig;
use modman_core::loadorder::{
    self, MasterlistSource, PluginStage, PluginsFiles, SkipReason, SortMethod,
    DEFAULT_LOOT_BASE_URL, LOOT_URL_ENV,
};
use modman_core::store::DeployLock;

use crate::context::Ctx;
use crate::output::Printer;

#[derive(Args, Debug)]
pub struct PluginsArgs {
    #[command(subcommand)]
    pub command: Option<PluginsCommand>,
}

#[derive(Subcommand, Debug)]
pub enum PluginsCommand {
    /// Show plugins.txt with active and managed state (default).
    #[command(visible_alias = "ls")]
    List,
    /// Re-sort the plugins modman added (LOOT + collection rules).
    Sort {
        /// Download the LOOT masterlist first.
        #[arg(long)]
        update_masterlist: bool,
    },
    /// Download the LOOT masterlist into the cache.
    Masterlist,
}

fn files_for(ctx: &Ctx, game: &GameConfig) -> Result<(PluginsFiles, DeployEnv)> {
    let cfg = Config::load(&ctx.store, &ctx.paths)?;
    let env = DeployEnv::new(&ctx.paths, &cfg.data_dir, &game.domain);
    Ok((PluginsFiles::new(env.game_data_dir()), env))
}

fn loot_base() -> String {
    std::env::var(LOOT_URL_ENV)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_LOOT_BASE_URL.to_string())
}

/// One-line summary of a plugin stage, shared by deploy and purge.
pub fn report_stage(out: Printer, stage: &Option<PluginStage>) {
    match stage {
        None | Some(PluginStage::Skipped(SkipReason::NotBethesda)) => {}
        Some(PluginStage::Skipped(SkipReason::NoPluginsFile)) => {
            out.warn("plugins.txt location unknown (no Proton prefix). Set it: modman game set plugins-file <path>");
        }
        Some(PluginStage::Skipped(SkipReason::FolderMissing(dir))) => out.warn(&format!(
            "plugins.txt folder not found ({}). Launch the game once, then run 'modman deploy' again.",
            dir.display()
        )),
        Some(PluginStage::Failed(e)) => out.warn(&format!("plugins.txt not updated: {e}")),
        Some(PluginStage::Updated(r)) => {
            for w in &r.warnings {
                out.warn(w);
            }
            if !r.added.is_empty() {
                let how = match r.sorted_by {
                    Some(SortMethod::Loot { masterlist: true }) => "sorted by LOOT with masterlist",
                    Some(SortMethod::Loot { masterlist: false }) => {
                        "sorted by LOOT (no masterlist; run: modman plugins masterlist)"
                    }
                    _ => "in mod order",
                };
                out.info(&format!(
                    "plugins.txt: {} plugin(s) activated ({} provided by mods), {how}.",
                    r.added.len(),
                    r.provided
                ));
            } else if !r.removed.is_empty() {
                out.info(&format!(
                    "plugins.txt: removed {} plugin(s) added by modman.",
                    r.removed.len()
                ));
            }
        }
    }
}

fn fetch(ctx: &Ctx, game: &GameConfig, files: &PluginsFiles) -> Result<()> {
    let src = MasterlistSource::for_game(game, &loot_base())?;
    let path =
        modman_core::blocking::block_on(loadorder::fetch_masterlist(&src, &files.game_data))??;
    ctx.out
        .success(&format!("LOOT masterlist saved to {}", path.display()));
    Ok(())
}

pub fn run(ctx: &mut Ctx, args: PluginsArgs) -> Result<()> {
    let game = ctx.need_game()?;
    if !loadorder::plugins_applies(&game) {
        bail!(
            "{} has no plugins.txt (only bethesda-profile games with a plugins format).",
            game.name
        );
    }
    let (files, env) = files_for(ctx, &game)?;
    match args.command.unwrap_or(PluginsCommand::List) {
        PluginsCommand::List => list(ctx, &game, &files),
        PluginsCommand::Masterlist => fetch(ctx, &game, &files),
        PluginsCommand::Sort { update_masterlist } => {
            if update_masterlist {
                fetch(ctx, &game, &files)?;
            }
            // plugins.txt is shared state with deploy/purge.
            let _lock = DeployLock::try_acquire(&env.lock_path)?;
            let stage = loadorder::update_plugins(ctx.store.conn(), &game, &files)?;
            report_stage(ctx.out, &Some(stage.clone()));
            if let PluginStage::Updated(r) = &stage {
                if r.added.is_empty() && r.removed.is_empty() {
                    ctx.out.info("No mod plugins to sort.");
                }
            }
            Ok(())
        }
    }
}

fn list(ctx: &Ctx, game: &GameConfig, files: &PluginsFiles) -> Result<()> {
    let out = ctx.out;
    let Some(lines) = loadorder::read_plugins_txt(game, files)? else {
        out.info("No plugins.txt yet (deploy mods, or launch the game once).");
        return Ok(());
    };
    out.heading(&format!("plugins.txt — {}", game.name));
    for (i, l) in lines.iter().enumerate() {
        let state = if l.active {
            out.green("active  ")
        } else {
            out.dim("inactive")
        };
        let managed = if l.managed {
            out.dim(" (modman)")
        } else {
            String::new()
        };
        println!("  {:>3}  {state}  {}{managed}", i + 1, l.name);
    }
    Ok(())
}
