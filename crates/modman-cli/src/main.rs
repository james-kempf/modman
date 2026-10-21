use anyhow::{bail, Result};
use clap::{Parser, Subcommand};

/// Nexus Mods manager for Linux.
#[derive(Parser)]
#[command(name = "modman", version = modman_core::VERSION, about)]
struct Cli {
    /// Use this game (Nexus domain) instead of the active one.
    #[arg(short = 'g', long = "game", global = true)]
    game: Option<String>,

    /// Assume "yes" to prompts.
    #[arg(short = 'y', long = "yes", global = true)]
    yes: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage games (detect, add, list, use, info, set, remove).
    Games {
        #[command(subcommand)]
        action: GamesAction,
    },
}

#[derive(Subcommand)]
enum GamesAction {
    /// List supported games installed through Steam.
    Detect,
    /// Show configured games.
    List,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let _ = (&cli.game, cli.yes);

    match cli.command {
        Commands::Games { action } => match action {
            GamesAction::Detect | GamesAction::List => {
                bail!("not implemented yet (milestone M1)")
            }
        },
    }
}
