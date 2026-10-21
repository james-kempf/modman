// SPDX-License-Identifier: GPL-3.0-or-later
//! login / logout (legacy `cmd_login` L4176-4219, `cmd_logout` L4221-4225).
//!
//! The API key is read from a no-echo prompt (tty) or one line of stdin,
//! never from argv. It is stored in the Secret Service keyring, or in a 0600
//! file when no keyring answers.

use std::io::{BufRead, IsTerminal, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;

use anyhow::{bail, Result};
use clap::Args;
use modman_core::games::{self, steam};
use modman_core::nexus::{
    keyring_opener, ApiKey, ApiKeyAuth, Backend, KeyStorage, NexusClient, ProbeOutcome,
    PROBE_TIMEOUT,
};

use crate::context::Ctx;
use crate::nexus::{self as nx, AUTH_BACKEND_KEY, PREMIUM_KEY, USER_KEY};

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
    let key = ApiKey::new(&raw)?;
    drop(raw);
    out.info("Validating key...");
    let shared = nx::shared_store(ctx)?;
    let client = NexusClient::new(
        nx::config()?,
        Some(Arc::new(ApiKeyAuth::new(key.clone()))),
        Some(shared.clone()),
    )?;
    let nexus = nx::Nexus { client, shared };
    let acct = nexus
        .run(nexus.client.validate())?
        .map_err(|e| anyhow::anyhow!("Authentication failed: {e}"))?;

    let storage = KeyStorage::open(&ctx.paths, keyring_opener(), PROBE_TIMEOUT)?;
    let previous = ctx.store.setting(AUTH_BACKEND_KEY)?;
    let backend = storage.save(&key)?;
    ctx.store.set_setting(AUTH_BACKEND_KEY, backend.as_str())?;
    ctx.store.set_setting(USER_KEY, &acct.name)?;
    ctx.store
        .set_setting(PREMIUM_KEY, if acct.is_premium { "true" } else { "false" })?;
    if backend == Backend::File && previous.as_deref() != Some("file") {
        let why = storage
            .unavailable_reason()
            .unwrap_or("the keyring refused the write");
        out.warn(&format!(
            "Secret Service keyring not available ({why}). The API key is stored in {} (readable only by you).",
            storage.file_path().display()
        ));
    }
    out.success(&format!("Authenticated as {}", out.bold(&acct.name)));
    if acct.is_premium {
        out.success("Account tier: Premium — direct and parallel downloads available ✓");
    } else {
        out.warn(
            "Account tier: Free — each file needs a click on \"Slow download\" on the website.",
        );
        out.warn("Run 'modman nxm register' so the browser can hand links to modman.");
    }
    out.success(&format!(
        "API key saved ({}).",
        match backend {
            Backend::Keyring => "keyring".to_string(),
            Backend::File => storage.file_path().display().to_string(),
        }
    ));

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
    let storage = match ctx.store.setting(AUTH_BACKEND_KEY)?.as_deref() {
        Some("keyring") => KeyStorage::open(&ctx.paths, keyring_opener(), PROBE_TIMEOUT)?,
        _ => KeyStorage::new(&ctx.paths, ProbeOutcome::Unavailable("logout".into())),
    };
    storage.clear()?;
    for k in [AUTH_BACKEND_KEY, USER_KEY, PREMIUM_KEY] {
        ctx.store.delete_setting(k)?;
    }
    ctx.out.success("API key removed.");
    if std::env::var_os(modman_core::nexus::API_KEY_ENV).is_some() {
        ctx.out
            .warn("NEXUS_API_KEY is still set in the environment and will be used.");
    }
    Ok(())
}
