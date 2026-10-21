// SPDX-License-Identifier: GPL-3.0-or-later
//! Collection commands: install, download, list, info, update, remove
//! (legacy `cmd_collection` L2787-2804). The driver lives in
//! `modman_core::app::collection`.

use anyhow::{anyhow, bail, Result};
use clap::{Args, Parser, Subcommand};
use modman_core::app::collection as app;
use modman_core::collections;

use super::mods as modcmd;
use crate::context::Ctx;

#[derive(Args, Debug)]
pub struct CollectionArgs {
    #[command(subcommand)]
    pub cmd: Option<Sub>,
}

#[derive(Subcommand, Debug)]
pub enum Sub {
    /// Install a collection.
    #[command(visible_aliases = ["add", "i"])]
    Install(InstallOpts),
    /// Download a collection's archives without installing.
    #[command(visible_alias = "dl")]
    Download(InstallOpts),
    /// List installed collections (default).
    #[command(visible_alias = "ls")]
    List,
    /// Show a collection on Nexus and whether an update is available.
    #[command(visible_alias = "show")]
    Info(RefArg),
    /// Install the latest revision and remove mods it dropped.
    #[command(visible_alias = "upgrade")]
    Update(InstallOpts),
    /// Remove a collection and the mods only it uses.
    #[command(visible_aliases = ["rm", "uninstall"])]
    Remove(RemoveOpts),
    /// `modman collection <url>` installs it.
    #[command(external_subcommand)]
    Other(Vec<String>),
}

#[derive(Args, Debug, Clone, Default)]
pub struct InstallOpts {
    /// Collection URL, nxm:// link or slug.
    pub reference: String,
    /// Optional mods: all, none or ask (default ask; none without a terminal).
    #[arg(long, value_name = "WHICH", value_parser = ["all", "none", "ask"])]
    pub optional: Option<String>,
    /// FOMOD installers: collection (recorded choices, default), ask or default.
    #[arg(long, value_name = "MODE", value_parser = ["collection", "ask", "default"])]
    pub fomod: Option<String>,
    /// Install this revision instead of the latest.
    #[arg(short = 'r', long, value_name = "N")]
    pub revision: Option<u64>,
    /// Deploy afterwards.
    #[arg(long)]
    pub deploy: bool,
    /// Only download the archives.
    #[arg(long = "download-only")]
    pub download_only: bool,
}

impl From<InstallOpts> for app::InstallOpts {
    fn from(o: InstallOpts) -> Self {
        Self {
            reference: o.reference,
            optional: o.optional,
            fomod: o.fomod,
            revision: o.revision,
            deploy: o.deploy,
            download_only: o.download_only,
        }
    }
}

#[derive(Args, Debug)]
pub struct RefArg {
    /// Collection URL, nxm:// link or slug.
    pub reference: String,
}

#[derive(Args, Debug)]
pub struct RemoveOpts {
    /// Collection URL, nxm:// link or slug.
    pub reference: String,
    /// Keep the mods installed; only forget the collection.
    #[arg(long = "keep-mods")]
    pub keep_mods: bool,
}

#[derive(Parser, Debug)]
#[command(no_binary_name = true)]
struct PassThrough {
    #[command(flatten)]
    opts: InstallOpts,
}

/// Collection options passed through `install` / `download` (legacy
/// L3346-3348, L3376-3378).
pub fn passthrough(target: &str, rest: &[String]) -> Result<InstallOpts> {
    let args = std::iter::once(target.to_string()).chain(rest.iter().cloned());
    PassThrough::try_parse_from(args)
        .map(|p| p.opts)
        .map_err(|e| anyhow!("{}", e.render().to_string().trim_end()))
}

pub fn run(ctx: &mut Ctx, args: CollectionArgs) -> Result<()> {
    match args.cmd {
        None | Some(Sub::List) => list(ctx),
        Some(Sub::Install(o)) => install(ctx, o),
        Some(Sub::Download(mut o)) => {
            o.download_only = true;
            install(ctx, o)
        }
        Some(Sub::Info(r)) => info(ctx, &r.reference),
        Some(Sub::Update(o)) => {
            let ui = ctx.ui();
            app::update(&mut ctx.cx(&ui), &o.into())?;
            Ok(())
        }
        Some(Sub::Remove(r)) => {
            let ui = ctx.ui();
            app::remove(&mut ctx.cx(&ui), &r.reference, r.keep_mods)?;
            Ok(())
        }
        Some(Sub::Other(v)) => {
            let first = v.first().cloned().unwrap_or_default();
            if first.contains("collections") && collections::parse_ref(&first).is_some() {
                install(ctx, passthrough(&first, &v[1..])?)
            } else {
                bail!("Unknown collection command '{first}'. Use: install | download | list | info | update | remove")
            }
        }
    }
}

pub fn install(ctx: &mut Ctx, opts: InstallOpts) -> Result<()> {
    let ui = ctx.ui();
    app::install(&mut ctx.cx(&ui), &opts.into())?;
    Ok(())
}

fn list(ctx: &mut Ctx) -> Result<()> {
    let game = ctx.need_game()?;
    let out = ctx.out;
    out.heading(&format!("Collections — {}", game.name));
    let cols = collections::list(ctx.store.conn(), &game.domain)?;
    if cols.is_empty() {
        out.info("No collections installed. Install one with: modman collection install <url>");
        return Ok(());
    }
    for c in &cols {
        let r = &c.record;
        println!(
            "  • {}  {}",
            out.bold(r.name.as_deref().unwrap_or(&r.slug)),
            out.dim(&format!(
                "({}, rev {}, by {})",
                r.slug,
                r.revision,
                r.author.as_deref().unwrap_or("?")
            ))
        );
        let failed = if r.failed.is_empty() {
            String::new()
        } else {
            format!(", {} failed", r.failed.len())
        };
        println!(
            "      {} mods ({} enabled){failed}  · installed {}",
            r.mods.len(),
            c.enabled,
            modcmd::fmt_time(r.installed_at)
        );
    }
    Ok(())
}

fn info(ctx: &mut Ctx, reference: &str) -> Result<()> {
    let ui = ctx.ui();
    let i = app::info(&ctx.cx(&ui), reference)?;
    let (rev, out) = (&i.revision, ctx.out);
    out.heading(&format!("Collection — {}", i.slug));
    println!("  Name      : {}", rev.name);
    println!("  Author    : {}", rev.author);
    println!("  Game      : {} ({})", rev.game_name, i.game);
    println!(
        "  Revision  : {} [{}]  — latest published: {}",
        rev.number,
        rev.status,
        rev.latest.map_or("?".into(), |l| l.to_string())
    );
    println!(
        "  Mods      : {}  ({} optional)",
        rev.mod_count, rev.optional_count
    );
    println!("  Size      : {} MB", rev.total_size / 1_048_576);
    println!("  URL       : {}", rev.url);
    println!("  Summary   : {}", rev.summary);
    println!();
    match &i.installed {
        Some(rec) => {
            println!(
                "  {}: revision {}, {} mods, on {}",
                out.green("Installed"),
                rec.revision,
                rec.mods.len(),
                modcmd::fmt_time(rec.installed_at)
            );
            if let Some(l) = i.update_available {
                println!(
                    "  {}: revision {l} → modman collection update {}",
                    out.yellow("Update available"),
                    i.slug
                );
            }
        }
        None => println!(
            "  {}",
            out.dim(&format!(
                "Not installed. Install with: modman collection install {reference}"
            ))
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_parses_collection_flags() {
        let o = passthrough(
            "nxm://g/collections/abc",
            &["--optional".into(), "all".into(), "-r".into(), "3".into()],
        )
        .unwrap();
        assert_eq!(o.reference, "nxm://g/collections/abc");
        assert_eq!(o.optional.as_deref(), Some("all"));
        assert_eq!(o.revision, Some(3));
        assert!(passthrough("x", &["--bogus".into()]).is_err());
        assert!(passthrough("x", &["--optional".into(), "some".into()]).is_err());
    }
}
