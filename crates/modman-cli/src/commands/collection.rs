// SPDX-License-Identifier: GPL-3.0-or-later
//! Collection commands: install, download, list, info, update, remove
//! (legacy `cmd_collection` L2787-2804 and `collection_*` L2328-2785).
//!
//! The core `collections` module does the fetching, dedupe, ordering and
//! bookkeeping; this file drives prompts, downloads and installs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use clap::{Args, Parser, Subcommand};
use modman_core::collections::{self, CollectionRef, Finish, Manifest, ManifestMod, OptionalMode};
use modman_core::config::Config;
use modman_core::download::{self, DownloadStatus, FetchRequest, Fetcher};
use modman_core::fomod::FomodMode;
use modman_core::games::GameConfig;
use modman_core::mods;

use super::install::{after_change, game_for, install_archive, staging_lock};
use super::{deploy, mods as modcmd};
use crate::context::Ctx;
use crate::nexus::{self as nx, Nexus};
use crate::output::human_size;

const URL_HELP: &str = "  Supported formats:
    nxm://{game}/collections/{slug}[/revisions/{rev}]
    https://www.nexusmods.com/games/{game}/collections/{slug}
    https://www.nexusmods.com/{game}/collections/{slug}
    {slug}   (for the active game)";

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
        Some(Sub::Update(o)) => update(ctx, o),
        Some(Sub::Remove(r)) => remove(ctx, r),
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

fn parse(reference: &str) -> Result<CollectionRef> {
    collections::parse_ref(reference)
        .ok_or_else(|| anyhow!("Cannot parse collection reference: {reference}\n{URL_HELP}"))
}

/// What an install did.
pub struct Installed {
    pub game: GameConfig,
    /// Collection mods in their new load order.
    pub ordered: Vec<i64>,
    pub manifest: Manifest,
}

fn optional_picks(ctx: &Ctx, m: &Manifest, mode: OptionalMode) -> Vec<usize> {
    let opt = collections::optional_indexes(m);
    if opt.is_empty() {
        return Vec::new();
    }
    let mode = match mode {
        OptionalMode::Ask if !ctx.is_interactive() => OptionalMode::None,
        other => other,
    };
    match mode {
        OptionalMode::All => (0..opt.len()).collect(),
        OptionalMode::None => {
            ctx.out.info(&format!(
                "Skipping {} optional mods (use --optional all to include them).",
                opt.len()
            ));
            Vec::new()
        }
        OptionalMode::Ask => {
            println!("{}", ctx.out.bold("Optional mods:"));
            for (n, &i) in opt.iter().enumerate() {
                let mm = &m.mods[i];
                println!(
                    "  {:>3}. {} {}",
                    n + 1,
                    mm.name,
                    ctx.out.dim(&format!(
                        "{}  ({})",
                        mm.version.as_deref().unwrap_or(""),
                        human_size(mm.file_size)
                    ))
                );
            }
            loop {
                let a = ctx.ask(
                    "Install which optional mods? (all, none, or e.g. 1,3-5)",
                    "all",
                );
                match collections::parse_selection(&a, opt.len()) {
                    Some(p) => return p,
                    None => ctx.out.warn("Invalid selection."),
                }
            }
        }
    }
}

fn open_url(url: &str) {
    if let Some(x) = modcmd::which("xdg-open") {
        let _ = std::process::Command::new(x)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

/// `browse` / `manual` sources: only interactively (legacy L2637-2651).
fn manual(ctx: &Ctx, mm: &ManifestMod, dl: &Path) -> Result<PathBuf> {
    if !ctx.is_interactive() {
        bail!("manual download required");
    }
    println!(
        "\n  {} {}",
        ctx.out.yellow("Manual download required:"),
        ctx.out.bold(&mm.name)
    );
    if let Some(i) = mm.instructions() {
        println!("    {i}");
    }
    if let Some(u) = mm.url() {
        println!("    {u}");
        open_url(u);
    }
    let p = ctx.ask("Path to the downloaded file (empty to skip)", "");
    let p = match p.strip_prefix("~/") {
        Some(rest) => ctx.paths.home.join(rest),
        None => PathBuf::from(&p),
    };
    if p.as_os_str().is_empty() || !p.exists() {
        bail!("manual download required");
    }
    if p.is_dir() {
        return Ok(p);
    }
    let dest = dl.join(p.file_name().ok_or_else(|| anyhow!("not a file"))?);
    if dest != p {
        std::fs::create_dir_all(dl)?;
        std::fs::copy(&p, &dest)?;
    }
    Ok(dest)
}

/// Gets the archive of one collection mod (legacy L2452-2463, L2576-2635).
fn acquire(
    ctx: &Ctx,
    nexus: &Nexus,
    game: &str,
    dir: &Path,
    dl: &Path,
    mm: &ManifestMod,
) -> Result<PathBuf> {
    match mm.source_type.as_str() {
        "nexus" => {
            let (Some(m), Some(f)) = (mm.mod_id, mm.file_id) else {
                bail!("the collection names no Nexus mod/file id");
            };
            let (path, md5) = nx::download_file(ctx, nexus, game, m, f, None, None, None)
                .map_err(|e| anyhow!("download failed: {e:#}"))?;
            if let (Some(want), Some(got)) = (&mm.md5, &md5) {
                if !want.eq_ignore_ascii_case(got) {
                    bail!("download failed: checksum mismatch (expected {want}, got {got})");
                }
            }
            Ok(path)
        }
        "bundle" => collections::find_bundled(dir, mm)
            .ok_or_else(|| anyhow!("bundled file missing from collection archive")),
        "direct" => {
            let url = mm
                .url()
                .ok_or_else(|| anyhow!("direct download failed: no URL"))?;
            let name = collections::direct_file_name(url, &mm.name);
            println!("  {}", ctx.out.dim(&format!("↓ {name} (direct)")));
            let dest = dl.join(&name);
            let fetcher = Fetcher::new()?;
            let f = nexus
                .run(fetcher.fetch(FetchRequest {
                    url,
                    dest: &dest,
                    md5: mm.md5.as_deref(),
                    sha256: None,
                    events: None,
                }))?
                .map_err(|e| anyhow!("direct download failed: {e}"))?;
            Ok(f.path)
        }
        "browse" | "manual" => manual(ctx, mm, dl),
        other => bail!("unsupported source type '{other}'"),
    }
}

fn print_failures(ctx: &Ctx, game: &str, m: &Manifest, failed: &[(usize, String)]) {
    if failed.is_empty() {
        return;
    }
    println!();
    for (i, why) in failed {
        let mm = &m.mods[*i];
        eprintln!("    ✗ {} {}", mm.name, ctx.out.dim(&format!("— {why}")));
        if let Some(id) = mm.mod_id {
            eprintln!(
                "      {}",
                ctx.out.dim(&format!(
                    "https://www.nexusmods.com/{game}/mods/{id}?tab=files&file_id={}",
                    mm.file_id.map_or(String::new(), |f| f.to_string())
                ))
            );
        }
    }
}

/// Fetches, selects, downloads and installs a collection (legacy
/// `collection_install` L2328-2555, without the final deploy). `None` when
/// cancelled or with `--download-only`.
pub fn install_collection(ctx: &mut Ctx, opts: &InstallOpts) -> Result<Option<Installed>> {
    let out = ctx.out;
    let r = parse(&opts.reference)?;
    let optional: OptionalMode = opts.optional.as_deref().unwrap_or("ask").parse()?;
    let fomod_mode: FomodMode = opts.fomod.as_deref().unwrap_or("collection").parse()?;
    let nexus = nx::require(ctx)?;
    let game = game_for(ctx, r.game.as_deref())?;
    let d = game.domain.clone();
    let cfg = Config::load(&ctx.store, &ctx.paths)?;
    let premium = nx::is_premium(ctx, &nexus).unwrap_or(false);
    out.heading(&format!("Collection — {}", r.slug));
    out.info(&format!(
        "Querying collection {} ({d}, revision {})...",
        out.bold(&r.slug),
        opts.revision
            .or(r.revision)
            .map_or("latest".into(), |n| n.to_string())
    ));
    let rev = nexus.run(collections::fetch_revision(
        &nexus.client,
        &d,
        &r.slug,
        opts.revision.or(r.revision),
        cfg.adult,
    ))??;
    let dir = collections::revision_dir(&cfg.data_dir, &d, &r.slug, rev.number);
    let fetcher = Fetcher::new()?;
    let fm = nexus.run(collections::fetch_manifest(
        &nexus.client,
        &fetcher,
        &rev,
        &dir,
    ))??;
    if fm.from_api {
        out.warn(&format!(
            "Collection archive unavailable ({}) — using the API mod list (no FOMOD choices or load-order rules).",
            fm.archive_error.as_deref().unwrap_or("unknown reason")
        ));
    }
    let m = fm.manifest;
    let n_opt = collections::optional_indexes(&m).len();
    println!();
    out.info(&format!(
        "Collection : {} by {}",
        out.bold(&rev.name),
        out.bold(&rev.author)
    ));
    let latest = match rev.latest {
        Some(l) if l != rev.number => out.yellow(&format!("  (latest is {l})")),
        _ => String::new(),
    };
    out.info(&format!(
        "Revision   : {}  [{}]{latest}",
        out.bold(&rev.number.to_string()),
        rev.status
    ));
    out.info(&format!(
        "Mods       : {}  ({} required, {n_opt} optional)",
        out.bold(&m.mods.len().to_string()),
        m.mods.len() - n_opt
    ));
    out.info(&format!(
        "Total size : {}",
        out.bold(&human_size(rev.total_size))
    ));
    if let Some(i) = m.install_instructions() {
        println!(
            "\n{} {}",
            out.bold("Curator's instructions"),
            out.dim("(written for Vortex — modman handles staging/deploy itself)")
        );
        for line in i.lines() {
            println!("    │ {line}");
        }
    }
    println!();

    let picks = optional_picks(ctx, &m, optional);
    let selected = collections::select(&m, &picks);
    let installed = mods::list_mods(ctx.store.conn(), &d)?;
    let plan = collections::plan(&m, &selected, &installed);
    let n_dl = plan
        .todo
        .iter()
        .filter(|&&i| m.mods[i].source_type == "nexus")
        .count();
    out.info(&format!("Already installed : {}", plan.have.len()));
    out.info(&format!("To install        : {}", plan.todo.len()));
    out.info(&format!(
        "To download       : {n_dl}  ({})",
        human_size(plan.download_bytes)
    ));
    let dl = mods::downloads_dir(&cfg.data_dir, &d);
    if let Some(free) = collections::free_bytes(&dl) {
        if plan.download_bytes.saturating_mul(2) > free {
            out.warn(&format!(
                "Low disk space: ~{} needed (archives + extracted files), {} free.",
                human_size(plan.download_bytes.saturating_mul(2)),
                human_size(free)
            ));
        }
    }
    if n_dl > 0 && !premium {
        out.warn(&format!(
            "Free account: Nexus requires a click on \"Slow download\" for each of the {n_dl} files."
        ));
    }
    if plan.todo.is_empty() && plan.have.is_empty() {
        bail!("Nothing selected.");
    }
    println!();
    if !ctx.confirm("Proceed?", true) {
        out.info("Cancelled.");
        return Ok(None);
    }

    let mut failed: Vec<(usize, String)> = Vec::new();
    let mut archives: BTreeMap<usize, PathBuf> = BTreeMap::new();
    if !plan.todo.is_empty() {
        out.heading("Downloading");
    }
    for (k, &i) in plan.todo.iter().enumerate() {
        let mm = &m.mods[i];
        println!(
            "  [{}/{}] {}  {}",
            k + 1,
            plan.todo.len(),
            mm.name,
            out.dim(&human_size(mm.file_size))
        );
        match acquire(ctx, &nexus, &d, &dir, &dl, mm) {
            Ok(p) => {
                archives.insert(i, p);
            }
            Err(e) => failed.push((i, format!("{e:#}"))),
        }
    }
    if opts.download_only {
        println!();
        out.success(&format!(
            "Downloaded {} archive(s) into {}",
            archives.len(),
            dl.display()
        ));
        print_failures(ctx, &d, &m, &failed);
        out.info(&format!(
            "Install with: modman collection install nxm://{d}/collections/{}/revisions/{}",
            r.slug, rev.number
        ));
        return Ok(None);
    }

    let mut ids: BTreeMap<usize, i64> = plan.have.clone();
    if !archives.is_empty() {
        out.heading("Installing");
    }
    let total = plan.todo.len();
    for (k, &i) in plan.todo.iter().enumerate() {
        let Some(path) = archives.get(&i) else {
            continue;
        };
        let mm = &m.mods[i];
        println!(
            "  [{}/{total}] {} {}",
            k + 1,
            out.bold(&mm.name),
            out.dim(mm.version.as_deref().unwrap_or(""))
        );
        let meta = mm.install_meta(fomod_mode);
        let lock = staging_lock(ctx)?;
        let res = install_archive(ctx, &game, path, &meta, fomod_mode == FomodMode::Ask);
        drop(lock);
        match res {
            Ok(o) => {
                ids.insert(i, o.mod_id);
            }
            Err(e) => failed.push((i, format!("install failed: {e:#}"))),
        }
    }

    failed.sort_by_key(|f| f.0);
    let failed_names: Vec<String> = failed
        .iter()
        .map(|(i, _)| m.mods[*i].name.clone())
        .collect();
    let lock = staging_lock(ctx)?;
    let fin = collections::finish(
        &mut ctx.store,
        &Finish {
            game: &d,
            revision: &rev,
            manifest: &m,
            dir: &dir,
            selected: &selected,
            ids: &ids,
            failed: &failed_names,
        },
    )?;
    drop(lock);
    if fin.in_cycle > 0 {
        out.warn(&format!(
            "{} mods are part of a rule cycle; keeping collection order for them",
            fin.in_cycle
        ));
    }
    if !fin.ordered.is_empty() {
        out.info(&format!(
            "Applied the collection's load order ({} mods, {} rules).",
            fin.ordered.len(),
            fin.rules
        ));
    }
    let ok = ids.len() - plan.have.len();
    println!();
    out.heading("Collection Summary");
    println!("  Collection : {}  (rev {})", rev.name, rev.number);
    println!("  Installed  : {ok}");
    println!("  Present    : {}  (already installed)", plan.have.len());
    println!("  Failed     : {}", failed.len());
    print_failures(ctx, &d, &m, &failed);
    println!();
    if !failed.is_empty() {
        out.warn("Re-run the same command to retry — finished mods are skipped.");
    }
    let members = mods::collection_mod_ids(ctx.store.conn(), &d, &r.slug)?;
    modcmd::requirements_after_collection(ctx, &game, &members);
    Ok(Some(Installed {
        game,
        ordered: fin.ordered,
        manifest: m,
    }))
}

/// `--deploy` / `auto-deploy` after an install (legacy L2556-2560).
fn deploy_after(ctx: &mut Ctx, game: &GameConfig, explicit: bool) -> Result<()> {
    let cfg = Config::load(&ctx.store, &ctx.paths)?;
    if explicit || cfg.auto_deploy {
        println!();
        let saved = ctx.game.replace(game.domain.clone());
        let r = deploy::deploy(
            ctx,
            deploy::DeployArgs {
                dry_run: false,
                method: None,
            },
        );
        ctx.game = saved;
        return r;
    }
    ctx.out.success(&format!(
        "Collection installed. Apply it to the game with: {}",
        ctx.out.bold("modman deploy")
    ));
    Ok(())
}

pub fn install(ctx: &mut Ctx, opts: InstallOpts) -> Result<()> {
    match install_collection(ctx, &opts)? {
        Some(done) => deploy_after(ctx, &done.game, opts.deploy),
        None => Ok(()),
    }
}

/// An nxm:// collection link (queued as `row`): installs the pinned
/// revision and closes the row.
pub fn install_link(
    ctx: &mut Ctx,
    game: &str,
    slug: &str,
    revision: Option<u64>,
    row: i64,
) -> Result<()> {
    download::set_status(&ctx.store, row, DownloadStatus::Downloading)?;
    let opts = InstallOpts {
        reference: match revision {
            Some(n) => format!("nxm://{game}/collections/{slug}/revisions/{n}"),
            None => format!("nxm://{game}/collections/{slug}"),
        },
        ..Default::default()
    };
    match install_collection(ctx, &opts) {
        Ok(Some(done)) => {
            download::set_status(&ctx.store, row, DownloadStatus::Done)?;
            deploy_after(ctx, &done.game, false)
        }
        Ok(None) => {
            download::set_status(&ctx.store, row, DownloadStatus::Cancelled)?;
            Ok(())
        }
        Err(e) => {
            let _ = download::mark_failed(&ctx.store, row, &format!("{e:#}"));
            Err(e)
        }
    }
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
    let r = parse(reference)?;
    let nexus = nx::require(ctx)?;
    let game = game_for(ctx, r.game.as_deref())?;
    let cfg = Config::load(&ctx.store, &ctx.paths)?;
    let rev = nexus.run(collections::fetch_revision(
        &nexus.client,
        &game.domain,
        &r.slug,
        r.revision,
        cfg.adult,
    ))??;
    let out = ctx.out;
    out.heading(&format!("Collection — {}", r.slug));
    println!("  Name      : {}", rev.name);
    println!("  Author    : {}", rev.author);
    println!("  Game      : {} ({})", rev.game_name, game.domain);
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
    println!(
        "  URL       : https://www.nexusmods.com/games/{}/collections/{}",
        game.domain, r.slug
    );
    println!("  Summary   : {}", rev.summary);
    println!();
    match collections::get(ctx.store.conn(), &game.domain, &r.slug)? {
        Some(rec) => {
            println!(
                "  {}: revision {}, {} mods, on {}",
                out.green("Installed"),
                rec.revision,
                rec.mods.len(),
                modcmd::fmt_time(rec.installed_at)
            );
            if let Some(l) = rev.latest.filter(|l| *l != rec.revision) {
                println!(
                    "  {}: revision {l} → modman collection update {}",
                    out.yellow("Update available"),
                    r.slug
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

fn update(ctx: &mut Ctx, opts: InstallOpts) -> Result<()> {
    let r = parse(&opts.reference)?;
    let game = game_for(ctx, r.game.as_deref())?;
    let d = game.domain.clone();
    let rec = collections::get(ctx.store.conn(), &d, &r.slug)?.ok_or_else(|| {
        anyhow!(
            "Collection '{}' is not installed for {}.",
            r.slug,
            game.name
        )
    })?;
    let nexus = nx::require(ctx)?;
    let cfg = Config::load(&ctx.store, &ctx.paths)?;
    // Legacy ambiguity A6 forced viewAdultContent:false here; the setting applies.
    let latest = nexus
        .run(collections::fetch_revision(
            &nexus.client,
            &d,
            &r.slug,
            None,
            cfg.adult,
        ))??
        .number;
    if latest == rec.revision && rec.failed.is_empty() {
        ctx.out
            .success(&format!("{} is up to date (revision {latest}).", r.slug));
        return Ok(());
    }
    ctx.out.info(&format!(
        "Updating {}: revision {} → {latest}",
        r.slug, rec.revision
    ));
    let o = InstallOpts {
        reference: format!("nxm://{d}/collections/{}", r.slug),
        revision: Some(latest),
        ..opts.clone()
    };
    let Some(done) = install_collection(ctx, &o)? else {
        return Ok(());
    };
    let dropped = collections::dropped_members(
        ctx.store.conn(),
        &d,
        &r.slug,
        &rec.mods,
        &done.ordered,
        &done.manifest,
    )?;
    let ids = |v: &[mods::ModRecord]| v.iter().map(|m| m.id).collect::<Vec<_>>();
    collections::detach(&ctx.store, &d, &r.slug, &ids(&dropped.shared))?;
    if !dropped.exclusive.is_empty() {
        println!();
        ctx.out.warn(&format!(
            "{} mod(s) are no longer part of {}:",
            dropped.exclusive.len(),
            r.slug
        ));
        for m in &dropped.exclusive {
            println!("    {} {}", ctx.out.yellow("−"), m.uid);
        }
        let gone = ids(&dropped.exclusive);
        if ctx.confirm("Uninstall them?", true) {
            let lock = staging_lock(ctx)?;
            mods::remove_mods(&mut ctx.store, &d, &gone, None)?;
            drop(lock);
            ctx.out.success(&format!(
                "Removed {} dropped mod(s).",
                dropped.exclusive.len()
            ));
        } else {
            collections::detach(&ctx.store, &d, &r.slug, &gone)?;
        }
    }
    deploy_after(ctx, &done.game, opts.deploy)
}

fn remove(ctx: &mut Ctx, args: RemoveOpts) -> Result<()> {
    let r = parse(&args.reference)?;
    let game = game_for(ctx, r.game.as_deref())?;
    let d = game.domain.clone();
    if collections::get(ctx.store.conn(), &d, &r.slug)?.is_none() {
        bail!("Collection '{}' is not installed.", r.slug);
    }
    let (ex, sh) = collections::exclusive_members(ctx.store.conn(), &d, &r.slug)?;
    let out = ctx.out;
    out.heading(&format!("Remove collection — {}", r.slug));
    if args.keep_mods {
        out.info("Mods stay installed (only the collection record is removed).");
    } else {
        out.info(&format!(
            "{} mod(s) will be uninstalled; {} shared with other collections stay.",
            ex.len(),
            sh.len()
        ));
    }
    if !ctx.confirm(&format!("Remove collection {}?", r.slug), true) {
        out.info("Cancelled.");
        return Ok(());
    }
    let lock = staging_lock(ctx)?;
    let res = collections::remove(&mut ctx.store, &d, &r.slug, args.keep_mods)?;
    drop(lock);
    if !res.uninstalled.is_empty() {
        out.info(&format!("Uninstalled {} mod(s).", res.uninstalled.len()));
    }
    out.success(&format!("Collection {} removed.", r.slug));
    after_change(ctx, &game)
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
