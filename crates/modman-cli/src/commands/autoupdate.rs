// SPDX-License-Identifier: GPL-3.0-or-later
//! `autoupdate enable | disable | status | run [--force] [--dry-run]`
//! (legacy `cmd_autoupdate`, L4760-4769). Scheduling is a systemd user timer
//! only; the gates, state and last-run log live in `modman_core::autoupdate`.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use modman_core::autoupdate::{
    self, Account, Actions, GameOutcome, LiveHost, RunOptions, Systemctl, UnitSpec, UserSystemctl,
};
use modman_core::config::Config;
use modman_core::games::GameConfig;
use modman_core::store::Store;

use super::collection::{self, CollectionArgs, InstallOpts, Sub as CollectionSub};
use super::deploy::{self, DeployArgs};
use super::mods::{self, fmt_time, UpdatesArgs};
use crate::context::Ctx;
use crate::nexus as nx;

/// Test hook: a `systemctl` stand-in (path to an executable).
pub const SYSTEMCTL_ENV: &str = "MODMAN_SYSTEMCTL";
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

/// `systemctl --user`, or the [`SYSTEMCTL_ENV`] stand-in.
struct Ctl(Option<PathBuf>);

impl Systemctl for Ctl {
    fn run(&self, args: &[&str]) -> std::result::Result<String, String> {
        let Some(bin) = &self.0 else {
            return UserSystemctl.run(args);
        };
        let out = std::process::Command::new(bin)
            .arg("--user")
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() {
            Ok(stdout)
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(if stderr.is_empty() { stdout } else { stderr })
        }
    }
}

fn ctl() -> Ctl {
    Ctl(std::env::var_os(SYSTEMCTL_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from))
}

pub fn run(ctx: &mut Ctx, args: AutoupdateArgs) -> Result<()> {
    match args.cmd {
        None | Some(Sub::Status) => status(ctx),
        Some(Sub::Enable) => enable(ctx),
        Some(Sub::Disable) => disable(ctx),
        Some(Sub::Run(a)) => run_now(ctx, &a),
    }
}

fn options(ctx: &Ctx) -> Result<(Config, RunOptions)> {
    let cfg = Config::load(&ctx.store, &ctx.paths)?;
    let opts = RunOptions::new(&cfg, &ctx.paths);
    Ok((cfg, opts))
}

/// The binary the timer runs: `$APPIMAGE` when running from an AppImage.
fn self_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let exe = std::env::current_exe().context("cannot locate the modman binary")?;
    Ok(exe.canonicalize().unwrap_or(exe))
}

fn enable(ctx: &mut Ctx) -> Result<()> {
    let (cfg, opts) = options(ctx)?;
    let exec = self_path()?;
    if exec.starts_with("/tmp") {
        ctx.out.warn(&format!(
            "modman runs from {} — move it somewhere permanent first, the timer will call this path.",
            exec.display()
        ));
    }
    // The user manager may not share this shell's environment.
    let mut env = Vec::new();
    for k in ["PATH", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME"] {
        if let Some(v) = std::env::var_os(k).filter(|v| !v.is_empty()) {
            env.push((k.to_string(), v.to_string_lossy().into_owned()));
        }
    }
    let spec = UnitSpec {
        exec,
        interval_hours: cfg.autoupdate_interval_hours,
        env,
    };
    autoupdate::enable(&ctx.paths, &spec, &ctl())?;
    let hours = autoupdate::timer_hours(cfg.autoupdate_interval_hours);
    ctx.out.success(&format!(
        "Auto-update enabled (systemd user timer: at boot and every {hours} h)."
    ));
    // Record the current state so the first timer run does not redeploy for
    // no reason (legacy L4705).
    let host = LiveHost::new(&ctx.paths.home);
    if let Err(e) = autoupdate::run(&mut CliActions { ctx }, &host, &opts) {
        tracing::warn!("auto-update baseline failed: {e}");
    }
    let out = ctx.out;
    out.info(&format!(
        "Triggers: system updates, Steam game updates{}.",
        if cfg.autoupdate_interval_hours > 0 {
            format!(", every {} h", cfg.autoupdate_interval_hours)
        } else {
            String::new()
        }
    ));
    out.info(&format!(
        "Collections follow their newest revision: {}  (modman config autoupdate-collections true|false)",
        cfg.autoupdate_collections
    ));
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
    let (_, opts) = options(ctx)?;
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
                    "Timer interval differs from autoupdate-interval; run: modman autoupdate enable"
                )
            );
        }
    } else {
        println!(
            "  Scheduler : {} — enable with: modman autoupdate enable",
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
        out.dim("Run it now: modman autoupdate run --force   ·   history: modman log")
    );
    Ok(())
}

fn run_now(ctx: &mut Ctx, a: &RunArgs) -> Result<()> {
    let (_, mut opts) = options(ctx)?;
    opts.force = a.force;
    opts.dry_run = a.dry_run;
    // Legacy ASSUME_YES=1: prompts take their defaults.
    ctx.yes = true;
    let saved_game = ctx.game.take();
    let host = LiveHost::new(&ctx.paths.home);
    let report = autoupdate::run(&mut CliActions { ctx }, &host, &opts);
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
        return Err(anyhow!("auto-update had problems — see: modman log"));
    }
    Ok(())
}

/// Desktop notification (legacy `notify`, L157-160); best effort.
fn notify(msg: &str) {
    tracing::info!("NOTIFY {msg}");
    if let Some(n) = mods::which("notify-send") {
        let _ = std::process::Command::new(n)
            .args(["-a", "modman", "-i", "applications-games", "modman", msg])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// Runs the auto-update actions through the regular commands.
struct CliActions<'a> {
    ctx: &'a mut Ctx,
}

impl CliActions<'_> {
    fn on_game(
        &mut self,
        game: &GameConfig,
        f: impl FnOnce(&mut Ctx) -> Result<()>,
    ) -> std::result::Result<(), String> {
        self.ctx.game = Some(game.domain.clone());
        let r = f(self.ctx).map_err(|e| format!("{e:#}"));
        if let Err(e) = &r {
            tracing::warn!("auto-update {}: {e}", game.domain);
        }
        r
    }

    fn updates(&mut self, game: &GameConfig, install: bool) -> std::result::Result<(), String> {
        self.on_game(game, |ctx| {
            mods::updates(
                ctx,
                UpdatesArgs {
                    install,
                    all: false,
                    refresh: false,
                },
            )
        })
    }
}

impl Actions for CliActions<'_> {
    fn store(&mut self) -> &mut Store {
        &mut self.ctx.store
    }

    fn account(&mut self) -> Account {
        if !matches!(nx::load_key(self.ctx), Ok(Some(_))) {
            return Account::None;
        }
        // Unknown tier counts as free (legacy: IS_PREMIUM empty).
        match nx::require(self.ctx).and_then(|n| nx::is_premium(self.ctx, &n)) {
            Ok(true) => Account::Premium,
            _ => Account::Free,
        }
    }

    fn update_collection(
        &mut self,
        game: &GameConfig,
        slug: &str,
    ) -> std::result::Result<(), String> {
        let opts = InstallOpts {
            reference: format!("nxm://{}/collections/{slug}", game.domain),
            optional: Some("none".into()),
            ..Default::default()
        };
        self.on_game(game, |ctx| {
            collection::run(
                ctx,
                CollectionArgs {
                    cmd: Some(CollectionSub::Update(opts)),
                },
            )
        })
    }

    fn install_mod_updates(&mut self, game: &GameConfig) -> std::result::Result<(), String> {
        self.updates(game, true)
    }

    fn check_mod_updates(&mut self, game: &GameConfig) -> std::result::Result<(), String> {
        self.updates(game, false)
    }

    fn redeploy(&mut self, game: &GameConfig) -> std::result::Result<(), String> {
        self.on_game(game, |ctx| {
            deploy::deploy(
                ctx,
                DeployArgs {
                    dry_run: false,
                    method: None,
                },
            )
        })
    }
}
