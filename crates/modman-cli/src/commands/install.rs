// SPDX-License-Identifier: GPL-3.0-or-later
//! install, download, reinstall (legacy `cmd_install` L3323-3360,
//! `cmd_download` L3365-3392, `cmd_reinstall` L3635-3665). The pipeline
//! lives in `modman_core::app::install`.

use anyhow::{bail, Result};
use clap::Args;
use modman_core::app::install::{self as app, InstallRequest, Target};

use super::collection;
use crate::context::Ctx;

#[derive(Args, Debug)]
pub struct InstallArgs {
    /// Nexus mod id, Nexus URL, <game>:<id>, nxm:// link, archive file or collection URL.
    pub target: String,
    /// Install this file id of the mod.
    #[arg(short = 'f', long = "file", value_name = "ID")]
    pub file: Option<u64>,
    /// FOMOD installer: ask, default, none or reuse (default: config 'fomod').
    #[arg(long, value_name = "MODE", value_parser = ["ask", "default", "none", "reuse"])]
    pub fomod: Option<String>,
    /// Name to show for the mod.
    #[arg(long)]
    pub name: Option<String>,
    /// Deploy afterwards.
    #[arg(long)]
    pub deploy: bool,
    /// Collections: optional mods (all, none, ask).
    #[arg(long, value_name = "WHICH")]
    pub optional: Option<String>,
    /// Collections: revision to install.
    #[arg(short = 'r', long, value_name = "N")]
    pub revision: Option<u64>,
    /// Collections: download only.
    #[arg(long = "download-only")]
    pub download_only: bool,
}

#[derive(Args, Debug)]
pub struct DownloadArgs {
    /// Nexus mod id, Nexus URL, <game>:<id>, nxm:// link or collection URL.
    pub target: String,
    /// Download this file id of the mod.
    #[arg(short = 'f', long = "file", value_name = "ID")]
    pub file: Option<u64>,
    /// Options passed to collection downloads.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub rest: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ReinstallArgs {
    /// Mod to re-extract.
    pub reference: String,
    /// FOMOD installer: reuse (default), ask, default or none.
    #[arg(long, value_name = "MODE", value_parser = ["ask", "default", "none", "reuse"])]
    pub fomod: Option<String>,
    /// Ask the FOMOD and variant questions again.
    #[arg(long)]
    pub ask: bool,
}

pub fn install(ctx: &mut Ctx, args: InstallArgs) -> Result<()> {
    let ui = ctx.ui();
    let mut cx = ctx.cx(&ui);
    app::install(
        &mut cx,
        InstallRequest {
            target: args.target,
            file: args.file,
            fomod: args.fomod,
            name: args.name,
            deploy: args.deploy,
            optional: args.optional,
            revision: args.revision,
            download_only: args.download_only,
        },
    )?;
    Ok(())
}

pub fn download(ctx: &mut Ctx, args: DownloadArgs) -> Result<()> {
    let (game, mod_id, file_id, key) = match app::parse_target(&args.target)? {
        Some(Target::Collection(t)) => {
            let mut opts = collection::passthrough(&t, &args.rest)?;
            opts.download_only = true;
            return collection::install(ctx, opts);
        }
        Some(Target::Nexus {
            game,
            mod_id,
            file_id,
            key,
        }) => (game, mod_id, file_id, key),
        _ => bail!(
            "Cannot parse '{}'. Give a mod id, a Nexus URL or an nxm:// link.",
            args.target
        ),
    };
    let ui = ctx.ui();
    let mut cx = ctx.cx(&ui);
    app::download_target(&mut cx, game.as_deref(), mod_id, args.file.or(file_id), key)?;
    Ok(())
}

pub fn reinstall(ctx: &mut Ctx, args: ReinstallArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let ui = ctx.ui();
    let mut cx = ctx.cx(&ui);
    app::reinstall(
        &mut cx,
        &game,
        &args.reference,
        args.fomod.as_deref(),
        args.ask,
    )?;
    Ok(())
}
