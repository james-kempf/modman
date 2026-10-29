// SPDX-License-Identifier: GPL-3.0-or-later
//! `autoupdate enable | disable | status | run [--force] [--dry-run]`
//! (legacy `cmd_autoupdate`, L4760-4769). Scheduling is a systemd user timer
//! only; the gates, state and last-run log live in `vessel_core::autoupdate`.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use vessel_core::app::actions::{self, systemctl, timer_exec, AppActions, Ctl};
use vessel_core::autoupdate::{self, GameOutcome, LiveHost, RunOptions};
use vessel_core::config::Config;

use super::mods::{self, fmt_time};
use crate::context::Ctx;
/// Log rows read for `status` (per-game and whole-run rows).
const STATUS_RUNS: usize = 50;

#[derive(Args, Debug)]
pub struct AutoupdateArgs {
    #[command(subcommand)]
    pub cmd: Option<Sub>,
}

#[derive(Subcommand, Debug)]
pub enum Sub {
    /// Install and start the systemd user timer.
    #[command(visible_alias = "on")]
    Enable,
    /// Stop and remove the systemd user timer.
    #[command(visible_alias = "off")]
    Disable,
    /// Timer state, per-game state and the last runs (default).
    Status,
    /// Check now; update and redeploy where needed (what the timer runs).
    Run(RunArgs),
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Act even without a system or game update; skip the settle and
    /// package-manager gates.
    #[arg(short, long)]
    pub force: bool,
    /// Report what would happen; change nothing.
    #[arg(short = 'n', long)]
    pub dry_run: bool,
}

fn ctl() -> Ctl {
    systemctl()
}

pub fn run(ctx: &mut Ctx, args: AutoupdateArgs) -> Result<()> {
    match args.cmd {
        None | Some(Sub::Status) => status(ctx),
        Some(Sub::Enable) => enable(ctx),
        Some(Sub::Disable) => disable(ctx),
        Some(Sub::Run(a)) => run_now(ctx, &a),
    }
}

fn options(ctx: &Ctx) -> Result<RunOptions> {
    let cfg = Config::load(&ctx.store, &ctx.paths)?;
    Ok(RunOptions::new(&cfg, &ctx.paths))
}

/// The binary the timer runs: `$APPIMAGE` when running from an AppImage.
fn self_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("cannot locate the vessel binary")?;
    Ok(timer_exec(&exe))
}

fn enable(ctx: &mut Ctx) -> Result<()> {
    let exec = self_path()?;
    let ui = ctx.ui();
    actions::enable(&mut ctx.cx(&ui), exec, &ctl())?;
    Ok(())
}

fn disable(ctx: &mut Ctx) -> Result<()> {
    if autoupdate::disable(&ctx.paths, &ctl())? {
        ctx.out.success("Auto-update disabled.");
    } else {
        ctx.out.info("Auto-update was not enabled.");
    }
    Ok(())
}

fn status(ctx: &mut Ctx) -> Result<()> {
    actions::ensure_supported()?;
    let opts = options(ctx)?;
    let host = LiveHost::new(&ctx.paths.home);
    let s = autoupdate::status(
        ctx.store.conn(),
        &ctx.paths,
        &host,
        &ctl(),
        &opts,
        STATUS_RUNS,
    )?;
    let out = ctx.out;
    out.heading("Auto-update");
    if s.timer.installed {
        println!(
            "  Scheduler : {} ({}){}",
            out.green("systemd user timer"),
            s.timer.active.as_deref().unwrap_or("unknown"),
            s.timer
                .next_elapse
                .as_deref()
                .map(|n| format!(", next check {n}"))
                .unwrap_or_default()
        );
        let want = autoupdate::timer_hours(s.interval_hours);
        if s.timer.every_hours.is_some_and(|h| h != want) {
            println!(
                "  {}",
                out.yellow(
                    "Timer interval differs from autoupdate-interval; run: vessel autoupdate enable"
                )
            );
        }
    } else {
        println!(
            "  Scheduler : {} — enable with: vessel autoupdate enable",
            out.yellow("disabled")
        );
    }
    println!("  Collections follow newest revision : {}", s.collections);
    println!(
        "  Extra check interval               : {}",
        if s.interval_hours > 0 {
            format!("{} h", s.interval_hours)
        } else {
            "off (only after system/game updates)".into()
        }
    );
    println!(
        "  Last system change                 : {}",
        if s.last_system_change > 0 {
            fmt_time(s.last_system_change)
        } else {
            "unknown".into()
        }
    );
    println!();
    for g in &s.games {
        println!(
            "  {} {}",
            out.bold(&g.name),
            out.dim(&format!("({})", g.domain))
        );
        println!(
            "      Last auto-update : {}{}",
            g.last_ok.map_or_else(|| "never".into(), fmt_time),
            if g.last_failed {
                "  (last attempt failed)"
            } else {
                ""
            }
        );
        println!(
            "      Steam build      : {}",
            g.steam_build.as_deref().unwrap_or("—")
        );
        if !g.pending.is_empty() {
            println!(
                "      Pending          : {}",
                out.yellow(&g.pending.join(", "))
            );
        }
    }
    // Whole-run rows; the per-game rows repeat their lines.
    let runs: Vec<_> = s.runs.iter().filter(|r| r.game.is_none()).take(5).collect();
    if !runs.is_empty() {
        println!("  Last runs:");
        for r in runs {
            println!(
                "    {}  {:<8} {}{}",
                fmt_time(r.started_at),
                r.outcome.as_str(),
                if r.dry_run { "(dry run) " } else { "" },
                r.summary.as_deref().unwrap_or("").replace('\n', "; ")
            );
        }
    }
    println!(
        "  {}",
        out.dim("Run it now: vessel autoupdate run --force   ·   history: vessel log")
    );
    Ok(())
}

fn run_now(ctx: &mut Ctx, a: &RunArgs) -> Result<()> {
    actions::ensure_supported()?;
    let mut opts = options(ctx)?;
    opts.force = a.force;
    opts.dry_run = a.dry_run;
    // Legacy ASSUME_YES=1: prompts take their defaults.
    ctx.yes = true;
    let saved_game = ctx.game.take();
    let host = LiveHost::new(&ctx.paths.home);
    let ui = ctx.ui();
    let report = autoupdate::run(
        &mut AppActions {
            cx: &mut ctx.cx(&ui),
        },
        &host,
        &opts,
    );
    ctx.game = saved_game;
    let report = report?;
    let out = ctx.out;
    if let Some(b) = &report.busy {
        tracing::info!("auto-update: {b:?}");
        if a.force {
            out.info("Another auto-update is running.");
        }
        return Ok(());
    }
    for g in &report.games {
        if let GameOutcome::Nothing(s) = &g.outcome {
            if s.is_blocked() {
                tracing::info!("auto-update {}: {}", g.domain, s.describe());
            }
        }
    }
    let lines = report.summaries();
    if lines.is_empty() {
        if a.force {
            out.info("Nothing to do.");
        }
        return Ok(());
    }
    println!();
    println!("{}", out.bold("Auto-update summary"));
    for l in &lines {
        println!("  {l}");
    }
    if !opts.dry_run {
        notify(&lines.join("\n"));
    }
    if report.exit_code() != 0 {
        return Err(anyhow!("auto-update had problems — see: vessel log"));
    }
    Ok(())
}

/// Desktop notification (legacy `notify`, L157-160); best effort.
fn notify(msg: &str) {
    tracing::info!("NOTIFY {msg}");
    if let Some(n) = mods::which("notify-send") {
        let _ = std::process::Command::new(n)
            .args(["-a", "vessel", "-i", "applications-games", "vessel", msg])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}
