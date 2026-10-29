// SPDX-License-Identifier: GPL-3.0-or-later
//! nxm:// handling: register, unregister, status, <url> (legacy
//! `nxm_register` L4230, `nxm_handle` L4276; routing per spikes.md §5).
//!
//! `nxm <url>` parses the link with the core parser first, then forwards it
//! to a running GUI over D-Bus (5 s timeout). Without a GUI the link is
//! queued in `downloads` and processed inline: a mod link installs the
//! file, a collection link installs the (pinned) collection revision.

use std::io::IsTerminal;
use std::sync::mpsc;

use anyhow::{anyhow, bail, Result};
use clap::Args;
use vessel_core::app::actions;
use vessel_core::app::nxm as app;
use vessel_core::download::NxmLink;
use vessel_core::ipc;
use vessel_core::nxm_handler as handler;

use super::mods::which;
use crate::context::Ctx;

#[derive(Args, Debug)]
pub struct NxmArgs {
    /// register, unregister, status (default) or an nxm:// link.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

pub fn run(ctx: &mut Ctx, args: NxmArgs) -> Result<()> {
    let sub = args.args.first().map(String::as_str);
    // Trailing args swallow `--help`; a subcommand must not run with it.
    if matches!(sub, Some("register" | "unregister" | "status"))
        && args.args[1..].iter().any(|a| a == "--help" || a == "-h")
    {
        return print_help();
    }
    match sub {
        None | Some("status") => status(ctx),
        Some("register") => register(ctx),
        Some("unregister") => unregister(ctx),
        Some(u) if u.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("nxm://")) => handle(ctx, u),
        Some(_) => bail!("Usage: vessel nxm register | unregister | status | <nxm://link>"),
    }
}

fn print_help() -> Result<()> {
    use clap::CommandFactory;
    match crate::cli::Cli::command().try_get_matches_from(["vessel", "nxm", "--help"]) {
        Err(e) => e.print()?,
        Ok(_) => bail!("Usage: vessel nxm register | unregister | status | <nxm://link>"),
    }
    Ok(())
}

/// `$APPIMAGE` when this process runs from the AppImage mount
/// (current_exe points into a temporary mount), else this binary.
fn program_path() -> Result<String> {
    let exe = std::env::current_exe()?;
    if let Some(a) = actions::running_appimage(&exe) {
        return Ok(a.display().to_string());
    }
    let exe = exe.canonicalize().unwrap_or(exe);
    exe.to_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("the vessel path is not valid UTF-8"))
}

fn register(ctx: &mut Ctx) -> Result<()> {
    let file = handler::register(&ctx.paths, &program_path()?)?;
    ctx.out.success(&format!(
        "vessel is now the handler for nxm:// links ({}).",
        file.display()
    ));
    ctx.out.info(
        "\"Mod manager download\" / \"Download with Vortex\" buttons on nexusmods.com now open vessel.",
    );
    Ok(())
}

fn unregister(ctx: &mut Ctx) -> Result<()> {
    handler::unregister(&ctx.paths)?;
    ctx.out.success("nxm:// handler removed.");
    Ok(())
}

/// The handler file exists and is the default for nxm://.
pub fn registered(ctx: &Ctx) -> bool {
    handler::registered(&ctx.paths)
}

fn status(ctx: &mut Ctx) -> Result<()> {
    if registered(ctx) {
        ctx.out.success("vessel handles nxm:// links.");
    } else {
        ctx.out
            .warn("vessel is not the nxm:// handler. Run: vessel nxm register");
    }
    Ok(())
}

/// Desktop notification (legacy `notify`, L157-160); best effort.
fn notify(msg: &str) {
    tracing::info!("NOTIFY {msg}");
    if let Some(n) = which("notify-send") {
        let _ = std::process::Command::new(n)
            .args(["-a", "vessel", "-i", "applications-games", "vessel", msg])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// Hands the link to a running GUI. `Err` = no GUI accepted it.
fn forward_to_gui(url: &str) -> std::result::Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let url = url.to_string();
    let spawned = std::thread::Builder::new()
        .name("nxm-forward".into())
        .spawn(move || {
            let r = (|| -> zbus::Result<()> {
                let conn = zbus::blocking::connection::Builder::session()?
                    .method_timeout(ipc::FORWARD_TIMEOUT)
                    .build()?;
                let argv = vec![ipc::GUI_ARGV0.to_string(), "nxm".to_string(), url];
                let cwd = std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                conn.call_method(
                    Some(ipc::SINGLE_INSTANCE_BUS),
                    ipc::SINGLE_INSTANCE_PATH,
                    Some(ipc::SINGLE_INSTANCE_INTERFACE),
                    ipc::SINGLE_INSTANCE_METHOD,
                    &(argv, cwd),
                )?;
                Ok(())
            })();
            let _ = tx.send(r.map_err(|e| e.to_string()));
        });
    if spawned.is_err() {
        return Err("cannot start the forwarding thread".into());
    }
    rx.recv_timeout(ipc::FORWARD_TIMEOUT + std::time::Duration::from_secs(1))
        .unwrap_or_else(|_| Err("the GUI did not answer in time".into()))
}

fn handle(ctx: &mut Ctx, url: &str) -> Result<()> {
    if let Err(e) = app::parse(url) {
        notify("Unrecognised nxm link");
        // The message never holds the query (key/expires).
        return Err(e.into());
    }
    match forward_to_gui(url) {
        Ok(()) => {
            ctx.out.success("Link handed to the running vessel window.");
            return Ok(());
        }
        Err(e) => tracing::debug!("no GUI to forward to: {e}"),
    }
    let accepted = app::accept(&ctx.store, url)?;
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if !interactive {
        // Background run: no prompts (optional mods default to none).
        ctx.yes = true;
    }
    let (start, ok, failed) = match &accepted.link {
        NxmLink::Collection { game, slug, .. } => (
            format!("Installing collection {slug} ({game})…"),
            format!("Installed collection {slug}. Run 'vessel deploy' to apply it."),
            format!("Installing collection {slug} failed — see: vessel log"),
        ),
        NxmLink::Mod(m) => (
            format!("Installing mod {} ({})…", m.mod_id, m.game),
            format!(
                "Installed mod {}. Run 'vessel deploy' to apply it.",
                m.mod_id
            ),
            format!("Installing mod {} failed — see: vessel log", m.mod_id),
        ),
    };
    if !interactive {
        notify(&start);
    }
    let ui = ctx.ui();
    let res = app::process(&mut ctx.cx(&ui), &accepted);
    if !interactive {
        match &res {
            // A collection link notifies on every outcome (legacy).
            Ok(p) if p.installed || p.kind == "collection" => notify(&ok),
            Ok(_) => {}
            Err(_) => notify(&failed),
        }
    }
    res.map(|_| ()).map_err(Into::into)
}
