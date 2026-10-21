// SPDX-License-Identifier: GPL-3.0-or-later
//! `modman` command-line interface: a thin adapter over `modman-core`.

mod cli;
mod commands;
mod context;
mod logging;
mod nexus;
mod output;

use std::process::ExitCode;

use anyhow::Result;
use clap::{CommandFactory, FromArgMatches};
use modman_core::paths::Paths;
use modman_core::store::Store;

use cli::{Cli, Commands};
use context::Ctx;
use output::Printer;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    // Colour must be known before clap renders help or errors.
    let no_color_flag = argv
        .iter()
        .skip(1)
        .take_while(|a| *a != "--")
        .any(|a| a == "--no-color");
    let out = Printer {
        color: output::use_color(no_color_flag),
    };
    let mut cmd = Cli::command().color(if out.color {
        clap::ColorChoice::Always
    } else {
        clap::ColorChoice::Never
    });
    let cli = match cmd
        .try_get_matches_from_mut(&argv)
        .and_then(|m| Cli::from_arg_matches(&m))
    {
        Ok(cli) => cli,
        Err(e) => e.exit(),
    };
    match run(cli, out, &argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            out.error(&format!("{e:#}"));
            ExitCode::FAILURE
        }
    }
}

/// Command line for the log: URL queries (nxm `key`/`expires`, signed CDN
/// parameters) are dropped.
fn redact_args(args: &[String]) -> String {
    args.iter()
        .map(|a| match (a.contains("://"), a.split_once('?')) {
            (true, Some((base, _))) => format!("{base}?<redacted>"),
            _ => a.clone(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn run(cli: Cli, out: Printer, argv: &[String]) -> Result<()> {
    let command = match cli.command {
        None => {
            // No command: show help (legacy default, L4995).
            Cli::command()
                .color(if out.color {
                    clap::ColorChoice::Always
                } else {
                    clap::ColorChoice::Never
                })
                .print_help()?;
            return Ok(());
        }
        Some(Commands::Version) => {
            println!("modman {}", modman_core::VERSION);
            return Ok(());
        }
        Some(c) => c,
    };
    let paths = Paths::from_env()?;
    let debug = cli.debug || std::env::var("MODMAN_DEBUG").is_ok_and(|v| v == "1");
    logging::init(&paths.log_path(), debug, out.color);
    // Never log login arguments (legacy L5006).
    if matches!(command, Commands::Login(_)) {
        tracing::info!("CMD login");
    } else {
        tracing::info!("CMD {}", redact_args(&argv[1..]));
    }
    tracing::debug!(
        config = %paths.config_dir.display(),
        data = %paths.data_dir.display(),
        "paths"
    );
    if let Some(legacy) = paths.legacy_state_dir() {
        out.warn(&format!(
            "Legacy modman state found in {}. This version does not read it. \
             Purge deployments with the legacy modman first, then remove that folder.",
            legacy.display()
        ));
    }
    let store = Store::open(&paths.db_path())?;
    let mut ctx = Ctx {
        paths,
        store,
        out,
        yes: cli.yes,
        game: cli.game,
    };
    commands::dispatch(&mut ctx, command)
}

#[cfg(test)]
mod tests {
    #[test]
    fn logged_command_line_drops_url_queries() {
        let a: Vec<String> = ["nxm", "nxm://g/mods/1/files/2?key=K&expires=9", "-y"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            super::redact_args(&a),
            "nxm nxm://g/mods/1/files/2?<redacted> -y"
        );
    }
}
