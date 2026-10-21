// SPDX-License-Identifier: GPL-3.0-or-later
//! login / logout (legacy `cmd_login` L4176-4219, `cmd_logout` L4221-4225).
//!
//! The API key is read from a no-echo prompt (tty) or one line of stdin,
//! never from argv. It is stored in the Secret Service keyring, or in a 0600
//! file when no keyring answers.

use std::io::{BufRead, IsTerminal, Write};
use std::process::{Command, Stdio};

use anyhow::{bail, Result};
use clap::Args;
use modman_core::app::nexus as nx;
use modman_core::games::{self, steam};

use crate::context::Ctx;

#[derive(Args, Debug)]
pub struct LoginArgs {
    /// Rejected: the key must never be passed on the command line.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub rest: Vec<String>,
}

#[derive(Args, Debug)]
pub struct LogoutArgs {}

/// Turns terminal echo back on when dropped.
struct EchoOff;

impl EchoOff {
    fn new() -> Option<Self> {
        Command::new("stty")
            .arg("-echo")
            .stdin(Stdio::inherit())
            .status()
            .ok()
            .filter(|s| s.success())
            .map(|_| EchoOff)
    }
}

impl Drop for EchoOff {
    fn drop(&mut self) {
        let _ = Command::new("stty")
            .arg("echo")
            .stdin(Stdio::inherit())
            .status();
    }
}

fn read_key() -> Result<String> {
    let stdin = std::io::stdin();
    let mut line = String::new();
    if stdin.is_terminal() {
        eprint!("Paste your Nexus Mods API key: ");
        let _ = std::io::stderr().flush();
        let guard = EchoOff::new();
        if guard.is_none() {
            bail!("cannot hide terminal input (stty failed); pipe the key on stdin instead");
        }
        stdin.lock().read_line(&mut line)?;
        drop(guard);
        eprintln!();
    } else {
        stdin.lock().read_line(&mut line)?;
    }
    // Legacy strips all whitespace (L4187).
    Ok(line.chars().filter(|c| !c.is_whitespace()).collect())
}

pub fn login(ctx: &mut Ctx, args: LoginArgs) -> Result<()> {
    if !args.rest.is_empty() {
        // Do not echo the arguments: they may be the key.
        bail!("login takes no arguments: the API key is read from a hidden prompt or from stdin");
    }
    let out = ctx.out;
    out.heading("Nexus Mods — Login");
    println!("modman authenticates with your Nexus Mods personal API key.");
    println!("  Get yours → https://www.nexusmods.com/users/myaccount?tab=api");
    println!();
    let raw = read_key()?;
    if raw.is_empty() {
        bail!("API key cannot be empty.");
    }
    // Format check first, before anything is shown or sent.
    drop(modman_core::nexus::ApiKey::new(&raw)?);
    out.info("Validating key...");
    let r = nx::login(&ctx.paths, &ctx.store, &raw)?;
    drop(raw);
    for m in nx::login_msgs(&r) {
        out.say(&m);
    }

    if games::list_games(ctx.store.conn())?.is_empty() && ctx.is_interactive() {
        println!();
        println!("{}", out.bold("Game Setup"));
        let defs = ctx.definitions()?;
        let libs = steam::steam_libraries(&ctx.paths.home);
        let detected: Vec<&str> = defs
            .iter()
            .filter(|d| {
                d.steam_appid
                    .is_some_and(|id| steam::find_app(&libs, id).is_some())
            })
            .map(|d| d.domain.as_str())
            .collect();
        if !detected.is_empty() {
            out.info(&format!("Detected in Steam: {}", detected.join(" ")));
        }
        let domain = ctx.ask(
            "Game domain on Nexus (e.g. cyberpunk2077, skyrimspecialedition)",
            detected.first().copied().unwrap_or(""),
        );
        if !domain.is_empty() {
            super::game::add(ctx, &domain, None, None, None)?;
        }
    }
    Ok(())
}

pub fn logout(ctx: &mut Ctx, _args: LogoutArgs) -> Result<()> {
    let env_still_set = nx::logout(&ctx.paths, &ctx.store)?;
    ctx.out.success("API key removed.");
    if env_still_set {
        ctx.out
            .warn("NEXUS_API_KEY is still set in the environment and will be used.");
    }
    Ok(())
}
