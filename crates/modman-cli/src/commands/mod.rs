// SPDX-License-Identifier: GPL-3.0-or-later
//! Command groups. Each file owns its argument types and handlers so that
//! later milestones edit disjoint files.

pub mod auth;
pub mod autoupdate;
pub mod collection;
pub mod config;
pub mod deploy;
pub mod game;
pub mod install;
pub mod log;
pub mod mods;
pub mod nxm;
pub mod plugins;

use anyhow::{bail, Result};

use crate::cli::Commands;
use crate::context::Ctx;

/// Runs a parsed command.
pub fn dispatch(ctx: &mut Ctx, command: Commands) -> Result<()> {
    use Commands as C;
    match command {
        C::Login(a) => auth::login(ctx, a),
        C::Logout(a) => auth::logout(ctx, a),
        C::Game(a) => game::run(ctx, a),
        C::Config(a) => config::run(ctx, a),
        C::Install(a) => install::install(ctx, a),
        C::Download(a) => install::download(ctx, a),
        C::Collection(a) => collection::run(ctx, a),
        C::List(a) => mods::list(ctx, a),
        C::Info(a) => mods::info(ctx, a),
        C::Enable(a) => mods::enable(ctx, a),
        C::Disable(a) => mods::disable(ctx, a),
        C::Uninstall(a) => mods::uninstall(ctx, a),
        C::Reinstall(a) => install::reinstall(ctx, a),
        C::Order(a) => mods::order(ctx, a),
        C::Conflicts(a) => mods::conflicts(ctx, a),
        C::Deps(a) => mods::deps(ctx, a),
        C::Updates(a) => mods::updates(ctx, a),
        C::Search(a) => mods::search(ctx, a),
        C::Profile(a) => mods::profile(ctx, a),
        C::Deploy(a) => deploy::deploy(ctx, a),
        C::Purge(a) => deploy::purge(ctx, a),
        C::Verify(a) => deploy::verify(ctx, a),
        C::Status(a) => deploy::status(ctx, a),
        C::Launch(a) => mods::launch(ctx, a),
        C::Clean(a) => mods::clean(ctx, a),
        C::Log(a) => log::run(ctx, a),
        C::Nxm(a) => nxm::run(ctx, a),
        C::Autoupdate(a) => autoupdate::run(ctx, a),
        C::Plugins(a) => plugins::run(ctx, a),
        C::External(args) => external(ctx, args),
        // Handled before a context exists.
        C::Version => unreachable!("version is handled in main"),
    }
}

/// A bare `nxm://...` link is handled like `nxm <link>` (legacy L5041);
/// anything else is an unknown command.
fn external(ctx: &mut Ctx, args: Vec<String>) -> Result<()> {
    match args.first() {
        Some(link) if link.starts_with("nxm://") => nxm::run(ctx, nxm::NxmArgs { args }),
        Some(cmd) => bail!("Unknown command: '{cmd}'. Run: modman help"),
        None => bail!("Unknown command. Run: modman help"),
    }
}
