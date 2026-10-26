// SPDX-License-Identifier: GPL-3.0-or-later
//! Argument definitions: global flags, commands and legacy aliases
//! (legacy dispatch L5008-5047). Each command group owns its argument types
//! in `commands/<group>.rs`.

use clap::{Parser, Subcommand};

use crate::commands::{
    auth, autoupdate, collection, config, deploy, game, install, log, mods, nxm, plugins,
};

fn lowercase(s: &str) -> Result<String, std::convert::Infallible> {
    Ok(s.to_ascii_lowercase())
}

/// Nexus Mods manager for Linux.
#[derive(Parser, Debug)]
#[command(name = "modman", version = modman_core::VERSION, about)]
pub struct Cli {
    /// Use this game (Nexus domain) instead of the active one.
    #[arg(short = 'g', long = "game", global = true, value_name = "DOMAIN", value_parser = lowercase)]
    pub game: Option<String>,

    /// Assume "yes" to prompts.
    #[arg(short = 'y', long = "yes", global = true)]
    pub yes: bool,

    /// Disable coloured output (NO_COLOR is honoured too).
    #[arg(long = "no-color", global = true)]
    pub no_color: bool,

    /// Print debug messages on stderr.
    #[arg(long = "debug", global = true)]
    pub debug: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Authenticate with your Nexus API key.
    Login(auth::LoginArgs),
    /// Forget the API key.
    Logout(auth::LogoutArgs),
    /// Manage games (detect, add, list, use, info, set, remove).
    #[command(visible_alias = "games")]
    Game(game::GameArgs),
    /// Show or change global settings.
    Config(config::ConfigArgs),
    /// Install a mod (Nexus id/URL/nxm link or a local archive).
    #[command(visible_aliases = ["add", "i"])]
    Install(install::InstallArgs),
    /// Download archives without installing.
    #[command(visible_alias = "dl")]
    Download(install::DownloadArgs),
    /// Manage Nexus collections.
    #[command(visible_aliases = ["collections", "col"])]
    Collection(collection::CollectionArgs),
    /// List installed mods in priority order.
    #[command(visible_alias = "ls")]
    List(mods::ListArgs),
    /// Details, FOMOD choices and conflicts of a mod.
    #[command(visible_alias = "show")]
    Info(mods::InfoArgs),
    /// Enable mods.
    Enable(mods::EnableArgs),
    /// Disable mods.
    Disable(mods::DisableArgs),
    /// Remove mods.
    #[command(visible_aliases = ["remove", "rm"])]
    Uninstall(mods::UninstallArgs),
    /// Re-extract a mod.
    Reinstall(install::ReinstallArgs),
    /// Mod maintenance: restage staged files in the current layout.
    Mods(mods::ModsArgs),
    /// Change mod priority.
    #[command(visible_aliases = ["move", "mv"])]
    Order(mods::OrderArgs),
    /// Show which mods override each other.
    Conflicts(mods::ConflictsArgs),
    /// Check required mods and external tools.
    #[command(visible_aliases = ["requirements", "req"])]
    Deps(mods::DepsArgs),
    /// Check installed Nexus mods for newer files.
    #[command(visible_aliases = ["update", "outdated"])]
    Updates(mods::UpdatesArgs),
    /// Search Nexus for mods of the active game.
    #[command(visible_alias = "find")]
    Search(mods::SearchArgs),
    /// Save / restore load order and enabled state.
    #[command(visible_alias = "profiles")]
    Profile(mods::ProfileArgs),
    /// Link enabled mods into the game folder; clean-backups / restore-quarantine.
    Deploy(deploy::DeployArgs),
    /// Remove deployed files and restore original game files.
    #[command(visible_alias = "undeploy")]
    Purge(deploy::PurgeArgs),
    /// Check the deployed files are intact.
    Verify(deploy::VerifyArgs),
    /// Account, games, deployment state.
    #[command(visible_alias = "st")]
    Status(deploy::StatusArgs),
    /// Start the game through Steam.
    #[command(visible_aliases = ["play", "run"])]
    Launch(mods::LaunchArgs),
    /// Delete unused downloads and temp files.
    Clean(mods::CleanArgs),
    /// Show the log.
    #[command(visible_alias = "logs")]
    Log(log::LogArgs),
    /// nxm:// link handler (register, unregister, status, <url>).
    Nxm(nxm::NxmArgs),
    /// Update mods and redeploy after system or Steam game updates.
    #[command(visible_aliases = ["auto-update", "au"])]
    Autoupdate(autoupdate::AutoupdateArgs),
    /// Bethesda plugins.txt: list, LOOT sort, masterlist download.
    Plugins(plugins::PluginsArgs),
    /// Print the version.
    Version,
    /// Unknown commands and bare nxm:// links.
    #[command(external_subcommand)]
    External(Vec<String>),
}
