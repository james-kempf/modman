// SPDX-License-Identifier: GPL-3.0-or-later
//! Bethesda plugins (M6, new; legacy only appended to plugins.txt and hinted
//! at LOOT, L2100): `plugins [list]`, `plugins sort [--update-masterlist]`,
//! `plugins masterlist`. Deploy and purge run the plugin stage themselves.

use anyhow::Result;
use clap::{Args, Subcommand};
use vessel_core::app::plugins as app;
use vessel_core::games::GameConfig;
use vessel_core::loadorder::PluginLine;

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
    /// Re-sort the plugins vessel added (LOOT + collection rules).
    Sort {
        /// Download the LOOT masterlist first.
        #[arg(long)]
        update_masterlist: bool,
    },
    /// Download the LOOT masterlist into the cache.
    Masterlist,
}

pub fn run(ctx: &mut Ctx, args: PluginsArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let ui = ctx.ui();
    let cx = ctx.cx(&ui);
    match args.command.unwrap_or(PluginsCommand::List) {
        PluginsCommand::List => list(ui.out, &game, app::list(&cx, &game)?),
        PluginsCommand::Masterlist => app::masterlist(&cx, &game).map(|_| ()),
        PluginsCommand::Sort { update_masterlist } => {
            app::sort(&cx, &game, update_masterlist).map(|_| ())
        }
    }
    .map_err(Into::into)
}

fn list(
    out: Printer,
    game: &GameConfig,
    lines: Option<Vec<PluginLine>>,
) -> vessel_core::app::R<()> {
    let Some(lines) = lines else {
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
            out.dim(" (vessel)")
        } else {
            String::new()
        };
        println!("  {:>3}  {state}  {}{managed}", i + 1, l.name);
    }
    Ok(())
}
