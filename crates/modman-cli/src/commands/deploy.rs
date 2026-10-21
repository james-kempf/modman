// SPDX-License-Identifier: GPL-3.0-or-later
//! Deployment commands: deploy, purge, verify, status (M2; legacy
//! `cmd_deploy` L1738, `cmd_purge` L1899, `cmd_verify` L1949, `cmd_status`).

use anyhow::{bail, Result};
use clap::Args;
use modman_core::app::deploy::{methods_summary, run_deploy, run_purge};
use modman_core::deploy::{self, DeployStatus, VerifyKind};
use modman_core::games;

use crate::context::Ctx;

#[derive(Args, Debug)]
pub struct DeployArgs {
    /// Show what would be deployed and backed up; change nothing.
    #[arg(short = 'n', long)]
    pub dry_run: bool,
    /// Deploy method for this run: auto, hardlink, reflink, symlink or copy.
    #[arg(long, value_name = "METHOD")]
    pub method: Option<String>,
}

#[derive(Args, Debug)]
pub struct PurgeArgs {}

#[derive(Args, Debug)]
pub struct VerifyArgs {}

#[derive(Args, Debug)]
pub struct StatusArgs {}

/// Problems listed by `verify` (legacy L1980).
const MAX_PROBLEMS: usize = 40;

pub fn deploy(ctx: &mut Ctx, args: DeployArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let ui = ctx.ui();
    run_deploy(
        &mut ctx.cx(&ui),
        &game,
        args.method.as_deref(),
        args.dry_run,
    )?;
    Ok(())
}

pub fn purge(ctx: &mut Ctx, _args: PurgeArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let ui = ctx.ui();
    run_purge(&mut ctx.cx(&ui), &game)?;
    Ok(())
}

pub fn verify(ctx: &mut Ctx, _args: VerifyArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let r = deploy::verify(ctx.store.conn(), &game)?;
    let out = ctx.out;
    if r.interrupted {
        out.warn("A deploy or purge was interrupted; run: modman deploy (or purge) to recover.");
    }
    let missing = r
        .issues
        .iter()
        .filter(|i| i.kind == VerifyKind::Missing)
        .count();
    let changed = r.issues.len() - missing;
    println!(
        "  Intact: {}   Changed: {changed}   Missing: {missing}",
        r.intact
    );
    for i in r.issues.iter().take(MAX_PROBLEMS) {
        let kind = match i.kind {
            VerifyKind::Missing => "missing",
            VerifyKind::Modified => "changed",
        };
        println!("    {kind:<8} {} ({})", i.rel_path, i.method.as_str());
    }
    if r.issues.len() > MAX_PROBLEMS {
        println!("    ... and {} more", r.issues.len() - MAX_PROBLEMS);
    }
    if !r.issues.is_empty() {
        bail!(
            "{} deployed file(s) changed or missing. Run: modman deploy",
            r.issues.len()
        );
    }
    out.success("All deployed files are intact.");
    Ok(())
}

fn status_line(ctx: &Ctx, s: &DeployStatus) -> String {
    let out = ctx.out;
    if s.interrupted {
        return out.yellow("interrupted deploy/purge (run deploy or purge to recover)");
    }
    if !s.active {
        return "not deployed".into();
    }
    let mut line = out.green(&format!("deployed: {} file(s)", s.files));
    if !s.by_method.is_empty() {
        line.push_str(&format!(" ({})", methods_summary(&s.by_method)));
    }
    if s.backups_held > 0 {
        line.push_str(&format!(", {} original(s) backed up", s.backups_held));
    }
    line
}

/// Account lines (legacy `cmd_status` L4323-4345). Offline: shows what the
/// last login recorded and never probes the keyring.
fn account_section(ctx: &Ctx) -> Result<()> {
    use crate::nexus::{key_source, PREMIUM_KEY, USER_KEY};
    let out = ctx.out;
    println!("{}", out.bold("Account"));
    match key_source(ctx)? {
        Some(src) => println!("  API key       : {} ({src})", out.green("configured")),
        None => println!(
            "  API key       : {}",
            out.yellow("not set — run: modman login")
        ),
    }
    if let Some(user) = ctx.store.setting(USER_KEY)? {
        let tier = match ctx.store.setting(PREMIUM_KEY)?.as_deref() {
            Some("true") => "Premium",
            Some(_) => "Free",
            None => "unknown",
        };
        println!("  Logged in as  : {} ({tier})", out.green(&user));
    }
    if let Some(rl) = modman_core::nexus::RateLimit::load(&ctx.store)? {
        println!("  API quota     : {}", rl.summary());
    }
    let nxm = if super::nxm::registered(ctx) {
        out.green("registered")
    } else {
        format!("{} (modman nxm register)", out.yellow("not registered"))
    };
    println!("  nxm:// links  : {nxm}");
    println!();
    Ok(())
}

pub fn status(ctx: &mut Ctx, _args: StatusArgs) -> Result<()> {
    let out = ctx.out;
    out.heading("Status");
    account_section(ctx)?;
    let all = games::list_games(ctx.store.conn())?;
    if all.is_empty() {
        out.info("No games configured. Run: modman game add <domain>");
        return Ok(());
    }
    let active = match &ctx.game {
        Some(g) => Some(g.clone()),
        None => games::active_game(&ctx.store)?,
    };
    for g in all {
        let mark = if active.as_deref() == Some(g.domain.as_str()) {
            out.green("* ")
        } else {
            "  ".to_string()
        };
        let s = deploy::status(ctx.store.conn(), &g.domain)?;
        let mods = games::mod_count(ctx.store.conn(), &g.domain)?;
        println!(
            "  {mark}{}  {}  {}",
            out.bold(&g.domain),
            g.name,
            out.dim(&format!("— {mods} mods"))
        );
        println!("      {}", status_line(ctx, &s));
    }
    Ok(())
}
