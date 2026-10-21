// SPDX-License-Identifier: GPL-3.0-or-later
//! Mod management commands (legacy `cmd_list` .. `cmd_launch`, L3397-3947;
//! `cmd_conflicts` L1994-2044; `deps_report` L3183-3226).

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use modman_core::config::Config;
use modman_core::deploy;
use modman_core::fomod::{FomodChoices, FomodMode};
use modman_core::games::GameConfig;
use modman_core::mods::{self, InstallMeta, ModRecord, OrderSpec};
use modman_core::nexus::{latest_file, refresh_requirements};
use modman_core::requirements::{self, ReqRow, ReqStatus, Scope};

use super::install::{after_change, install_archive, staging_lock};
use crate::context::Ctx;
use crate::nexus as nx;
use crate::output::{human_size, json_str, trunc};

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Only mods whose name or id contains this text.
    pub text: Option<String>,
    /// Only enabled mods.
    #[arg(short = 'e', long, conflicts_with = "disabled")]
    pub enabled: bool,
    /// Only disabled mods.
    #[arg(short = 'd', long)]
    pub disabled: bool,
    /// Only mods of this collection.
    #[arg(short = 'c', long, value_name = "SLUG")]
    pub collection: Option<String>,
}

#[derive(Args, Debug)]
pub struct InfoArgs {
    /// #position, id, name, Nexus mod id or a unique part of the name.
    pub reference: String,
}

#[derive(Args, Debug)]
pub struct ToggleArgs {
    /// Mods (#position, id, name, Nexus id or unique name part).
    pub refs: Vec<String>,
    /// Every installed mod.
    #[arg(short = 'a', long)]
    pub all: bool,
    /// Every mod of this collection.
    #[arg(short = 'c', long, value_name = "SLUG")]
    pub collection: Option<String>,
}

pub type EnableArgs = ToggleArgs;
pub type DisableArgs = ToggleArgs;

#[derive(Args, Debug)]
pub struct UninstallArgs {
    #[arg(required = true)]
    pub refs: Vec<String>,
    /// Also delete the downloaded archive when no other mod uses it.
    #[arg(long = "delete-archive", visible_alias = "purge-archive")]
    pub delete_archive: bool,
}

#[derive(Args, Debug)]
pub struct OrderArgs {
    pub reference: String,
    /// #N, first|bottom|lowest, last|top|highest, up [n], down [n], before <mod>, after <mod>.
    pub spec: String,
    /// Count for up/down, mod for before/after.
    pub arg: Option<String>,
}

#[derive(Args, Debug)]
pub struct ConflictsArgs {
    /// Only conflicts involving this mod.
    pub reference: Option<String>,
    /// List the contested files (up to 25 per pair).
    #[arg(short = 'f', long)]
    pub files: bool,
}

#[derive(Args, Debug)]
pub struct DepsArgs {
    /// A mod, or `ack <name>` / `unack <name>`.
    pub words: Vec<String>,
    /// Ignore the 7-day requirement cache.
    #[arg(short = 'r', long)]
    pub refresh: bool,
    /// Machine-readable output.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct UpdatesArgs {
    /// Install the updates.
    #[arg(short = 'i', long)]
    pub install: bool,
    /// Include mods pinned by a collection.
    #[arg(long)]
    pub all: bool,
    /// Ignore cached file lists.
    #[arg(long)]
    pub refresh: bool,
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Machine-readable output.
    #[arg(long)]
    pub json: bool,
    #[arg(required = true, num_args = 1..)]
    pub words: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ProfileArgs {
    #[command(subcommand)]
    pub cmd: Option<ProfileCmd>,
}

#[derive(Subcommand, Debug)]
pub enum ProfileCmd {
    /// List saved profiles (default).
    #[command(visible_alias = "ls")]
    List,
    /// Save the current order and enabled state.
    Save { name: String },
    /// Restore a saved profile.
    #[command(visible_alias = "use")]
    Load { name: String },
    /// Delete a saved profile.
    #[command(visible_alias = "rm")]
    Delete { name: String },
}

/// Arguments are accepted and ignored (legacy).
#[derive(Args, Debug)]
pub struct IgnoredArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub args: Vec<String>,
}

pub type LaunchArgs = IgnoredArgs;
pub type CleanArgs = IgnoredArgs;

fn changed_hint(ctx: &Ctx, game: &GameConfig) -> Result<bool> {
    let st = deploy::status(ctx.store.conn(), &game.domain)?;
    Ok(mods::changed_since(&ctx.store, &game.domain, st.last_deployed_at)? && st.active)
}

pub fn list(ctx: &mut Ctx, args: ListArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let out = ctx.out;
    let all = mods::list_mods(ctx.store.conn(), &game.domain)?;
    out.heading(&format!("Mods — {}", game.name));
    if all.is_empty() {
        out.info("No mods installed. Try: modman install <mod-id|url|archive>");
        return Ok(());
    }
    let members = match &args.collection {
        Some(slug) => Some(mods::collection_mod_ids(
            ctx.store.conn(),
            &game.domain,
            slug,
        )?),
        None => None,
    };
    let q = args.text.as_deref().map(str::to_lowercase);
    println!(
        "  {}",
        out.bold(&format!(
            "{:<5} {:<2} {:<44} {:<12} {:<8} Collection",
            "#", "", "Name", "Version", "Nexus"
        ))
    );
    let mut shown = 0;
    for m in &all {
        if (args.enabled && !m.enabled) || (args.disabled && m.enabled) {
            continue;
        }
        if members.as_ref().is_some_and(|ids| !ids.contains(&m.id)) {
            continue;
        }
        if q.as_ref().is_some_and(|q| {
            !m.name.to_lowercase().contains(q.as_str())
                && !m.uid.to_lowercase().contains(q.as_str())
        }) {
            continue;
        }
        let mark = if m.enabled {
            out.green("✓")
        } else {
            out.dim("·")
        };
        let cols = mods::collections_of(ctx.store.conn(), m.id)?.join(",");
        println!(
            "  {} {mark}  {:<44} {:<12} {:<8} {}",
            out.dim(&format!("{:<5}", m.position)),
            trunc(&m.name, 44),
            trunc(m.version.as_deref().unwrap_or(""), 12),
            m.nexus_mod_id.map_or("-".to_string(), |n| n.to_string()),
            out.dim(&trunc(&cols, 20))
        );
        shown += 1;
    }
    let enabled = all.iter().filter(|m| m.enabled).count();
    let size: i64 = all.iter().map(|m| m.size).sum();
    println!();
    println!(
        "  {shown} shown · {} installed · {enabled} enabled · {} staged",
        all.len(),
        human_size(size.max(0) as u64)
    );
    if enabled > 0 && changed_hint(ctx, &game)? {
        println!(
            "  {}",
            out.yellow("Mods changed since the last deploy — run: modman deploy")
        );
    }
    println!(
        "  {}",
        out.dim("Higher # = higher priority (wins file conflicts). Reorder: modman order <mod> <position>")
    );
    Ok(())
}

pub(crate) fn fmt_time(epoch: i64) -> String {
    // Days since 1970 -> civil date (UTC), to avoid a time crate.
    let days = epoch.div_euclid(86_400);
    let secs = epoch.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        secs / 3600,
        secs % 3600 / 60
    )
}

pub fn info(ctx: &mut Ctx, args: InfoArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let out = ctx.out;
    let m = mods::resolve(ctx.store.conn(), &game.domain, &args.reference)?;
    out.heading(&m.name);
    let d = |v: Option<&str>| v.filter(|s| !s.is_empty()).unwrap_or("—").to_string();
    println!("  ID          : {}", m.uid);
    println!(
        "  Position    : #{}   {}",
        m.position,
        if m.enabled { "enabled" } else { "disabled" }
    );
    println!("  Version     : {}", d(m.version.as_deref()));
    println!("  Author      : {}", d(m.author.as_deref()));
    let link = match (m.nexus_mod_id, m.nexus_file_id) {
        (Some(mi), f) => format!(
            "   https://www.nexusmods.com/{}/mods/{mi}?tab=files{}",
            game.domain,
            f.map_or(String::new(), |f| format!("&file_id={f}"))
        ),
        _ => String::new(),
    };
    println!("  Source      : {}{link}", m.source);
    println!(
        "  Archive     : {}",
        m.archive
            .as_ref()
            .map_or("—".into(), |p| p.display().to_string())
    );
    println!(
        "  Staged at   : {}",
        m.staging_dir
            .as_ref()
            .map_or("—".into(), |p| p.display().to_string())
    );
    println!(
        "  Files       : {}  ({})",
        m.file_count,
        human_size(m.size.max(0) as u64)
    );
    println!(
        "  Deploys to  : {}",
        if m.root == "game" {
            "game root"
        } else {
            "deploy folder"
        }
    );
    let cols = mods::collections_of(ctx.store.conn(), m.id)?;
    println!(
        "  Collections : {}",
        if cols.is_empty() {
            "—".into()
        } else {
            cols.join(", ")
        }
    );
    println!("  Variant     : {}", d(m.variant.as_deref()));
    println!("  Installed   : {}", fmt_time(m.installed_at));
    // Legacy L3467: one line per installer step.
    if let Some(raw) = mods::fomod_choices(ctx.store.conn(), m.id)? {
        println!("  FOMOD       :");
        match FomodChoices::from_json(&raw) {
            Some(ch) => {
                for (step, names) in ch.summary() {
                    println!("      {step}: {}", names.join(", "));
                }
            }
            None => println!("      {raw}"),
        }
    }
    if m.enabled {
        let (mut wins, mut loses) = (0, 0);
        for c in deploy::conflicts(ctx.store.conn(), &game)? {
            if c.winner.id == m.id {
                wins += 1;
            } else if c.losers.iter().any(|l| l.id == m.id) {
                loses += 1;
            }
        }
        println!(
            "  Conflicts   : wins {wins} file(s), overridden on {loses}  (details: modman conflicts '{}' --files)",
            m.uid
        );
    }
    if let Some(mid) = m.nexus_mod_id {
        refresh_deps(ctx, &game, &[mid], false, true);
        let rows = requirements::analyze(
            &ctx.store,
            &game,
            &Scope {
                mods: Some(vec![m.id]),
            },
        )?;
        print_report(ctx, &rows, false, "Requirements");
    }
    Ok(())
}

fn ids_for(ctx: &Ctx, game: &GameConfig, a: &ToggleArgs) -> Result<Vec<i64>> {
    let all = mods::list_mods(ctx.store.conn(), &game.domain)?;
    let mut ids: Vec<i64> = Vec::new();
    if a.all {
        ids.extend(all.iter().map(|m| m.id));
    } else if let Some(slug) = &a.collection {
        let c = mods::collection_mod_ids(ctx.store.conn(), &game.domain, slug)?;
        if c.is_empty() {
            return Err(mods::ModsError::NoCollectionMods(slug.clone()).into());
        }
        ids.extend(c);
    }
    for r in &a.refs {
        let m = mods::resolve_in(&all, r)?;
        if !ids.contains(&m.id) {
            ids.push(m.id);
        }
    }
    Ok(ids)
}

fn toggle(ctx: &mut Ctx, args: ToggleArgs, enable: bool) -> Result<()> {
    let verb = if enable { "enable" } else { "disable" };
    if !args.all && args.collection.is_none() && args.refs.is_empty() {
        bail!("Usage: modman {verb} <mod...> | --all | --collection <slug>");
    }
    let game = ctx.need_game()?;
    let ids = ids_for(ctx, &game, &args)?;
    mods::set_enabled(&mut ctx.store, &game.domain, &ids, enable)?;
    let word = if enable { "Enabled" } else { "Disabled" };
    if ids.len() <= 10 {
        let all = mods::list_mods(ctx.store.conn(), &game.domain)?;
        for m in all.iter().filter(|m| ids.contains(&m.id)) {
            ctx.out.success(&format!("{word}: {}", m.uid));
        }
    } else {
        ctx.out.success(&format!("{word} {} mods.", ids.len()));
    }
    after_change(ctx, &game)
}

pub fn enable(ctx: &mut Ctx, args: EnableArgs) -> Result<()> {
    toggle(ctx, args, true)
}

pub fn disable(ctx: &mut Ctx, args: DisableArgs) -> Result<()> {
    toggle(ctx, args, false)
}

pub fn uninstall(ctx: &mut Ctx, args: UninstallArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let out = ctx.out;
    let all = mods::list_mods(ctx.store.conn(), &game.domain)?;
    let mut targets: Vec<&ModRecord> = Vec::new();
    for r in &args.refs {
        let m = mods::resolve_in(&all, r)?;
        if let Some(found) = all.iter().find(|x| x.id == m.id) {
            if !targets.iter().any(|t| t.id == found.id) {
                targets.push(found);
            }
        }
    }
    for t in &targets {
        println!("  {} {}", out.yellow("−"), t.uid);
    }
    if !ctx.confirm(&format!("Uninstall {} mod(s)?", targets.len()), true) {
        out.info("Cancelled.");
        return Ok(());
    }
    let ids: Vec<i64> = targets.iter().map(|t| t.id).collect();
    let cfg = Config::load(&ctx.store, &ctx.paths)?;
    let dl = mods::downloads_dir(&cfg.data_dir, &game.domain);
    let lock = staging_lock(ctx)?;
    let r = mods::remove_mods(
        &mut ctx.store,
        &game.domain,
        &ids,
        args.delete_archive.then_some(dl.as_path()),
    )?;
    drop(lock);
    for a in &r.archives {
        out.info(&format!("Deleted archive {}", a.display()));
    }
    out.success(&format!("Uninstalled {} mod(s).", r.mods));
    after_change(ctx, &game)
}

pub fn order(ctx: &mut Ctx, args: OrderArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let spec = OrderSpec::parse(&args.spec, args.arg.as_deref())?;
    let (m, mv) = mods::move_mod(&mut ctx.store, &game.domain, &args.reference, &spec)?;
    ctx.out.success(&format!(
        "{}: #{} → #{}  (of {})",
        m.uid, mv.from, mv.to, mv.of
    ));
    after_change(ctx, &game)
}

pub fn conflicts(ctx: &mut Ctx, args: ConflictsArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let out = ctx.out;
    let all = mods::list_mods(ctx.store.conn(), &game.domain)?;
    let filter = match &args.reference {
        Some(r) => Some(mods::resolve_in(&all, r)?),
        None => None,
    };
    let pos = |id: i64| all.iter().find(|m| m.id == id).map_or(0, |m| m.position);
    // (loser, winner) -> files
    let mut pairs: BTreeMap<(i64, i64), (String, String, Vec<String>)> = BTreeMap::new();
    for c in deploy::conflicts(ctx.store.conn(), &game)? {
        for l in &c.losers {
            if l.id == c.winner.id {
                continue;
            }
            if filter
                .as_ref()
                .is_some_and(|f| f.id != l.id && f.id != c.winner.id)
            {
                continue;
            }
            pairs
                .entry((l.id, c.winner.id))
                .or_insert_with(|| (l.name.clone(), c.winner.name.clone(), Vec::new()))
                .2
                .push(c.rel_path.clone());
        }
    }
    out.heading(&format!("File conflicts — {}", game.name));
    if pairs.is_empty() {
        out.success(&format!(
            "No file conflicts between enabled mods{}.",
            filter.map_or(String::new(), |f| format!(" involving {}", f.name))
        ));
        return Ok(());
    }
    let mut rows: Vec<_> = pairs.into_iter().collect();
    rows.sort_by_key(|((l, w), _)| (pos(*w), pos(*l)));
    for ((l, w), (lname, wname, mut files)) in rows.iter().cloned() {
        println!(
            "  {} {:<38} {}  {} {}  {}",
            out.dim(&format!("#{:<4}", pos(l))),
            trunc(&lname, 38),
            out.yellow("◀ overridden by"),
            out.dim(&format!("#{:<4}", pos(w))),
            trunc(&wname, 38),
            out.dim(&format!("({} files)", files.len()))
        );
        if args.files {
            files.sort();
            for f in files.iter().take(25) {
                println!("          {}", out.dim(f));
            }
            if files.len() > 25 {
                println!(
                    "          {}",
                    out.dim(&format!("… {} more", files.len() - 25))
                );
            }
        }
    }
    println!();
    out.info(&format!(
        "{} conflicting mod pair(s). Higher position wins — change it with: modman order <mod> <position>",
        rows.len()
    ));
    Ok(())
}

/// Updates the requirements cache when logged in. Never fails the caller.
fn refresh_deps(ctx: &Ctx, game: &GameConfig, ids: &[u64], refresh: bool, quiet: bool) {
    let res = (|| -> Result<bool> {
        let Some(nexus) = nx::connect(ctx, false)?.filter(|n| n.client.is_authenticated()) else {
            return Ok(false);
        };
        let mut g = game.clone();
        let gid = nx::game_id(ctx, &nexus, &mut g)?;
        nexus.run(refresh_requirements(
            &nexus.client,
            &nexus.shared,
            &g.domain,
            gid,
            ids,
            refresh,
        ))??;
        Ok(true)
    })();
    match res {
        Ok(true) => {}
        Ok(false) if refresh && !quiet => ctx
            .out
            .warn("Not logged in; showing cached requirements only. Run: modman login"),
        Ok(false) => {}
        Err(e) if quiet => tracing::warn!("requirement lookup failed: {e:#}"),
        Err(e) => ctx
            .out
            .warn(&format!("Could not update requirements: {e:#}")),
    }
}

/// Legacy `deps_report` in `full` (every row) or `problems` mode.
pub fn print_report(ctx: &Ctx, rows: &[ReqRow], problems: bool, title: &str) {
    let out = ctx.out;
    if rows.is_empty() {
        if !problems {
            println!();
            out.info(&format!("{title}: none listed on Nexus."));
        }
        return;
    }
    let counts = requirements::counts(rows);
    let n = |s: ReqStatus| counts.get(&s).copied().unwrap_or(0);
    let (n_ok, n_ext) = (n(ReqStatus::Ok), n(ReqStatus::External));
    let n_bad = n(ReqStatus::Missing) + n(ReqStatus::Disabled);
    if problems && n_bad + n_ext == 0 {
        if n_ok > 0 {
            out.success(&format!(
                "All {n_ok} requirement(s) of the installed mods are present."
            ));
        }
        return;
    }
    out.heading(title);
    for r in rows {
        if problems && matches!(r.status, ReqStatus::Ok | ReqStatus::Gone) {
            continue;
        }
        let name = out.bold(&r.name);
        match r.status {
            ReqStatus::Ok => println!("  {} {name}  {}", out.green("✓"), out.dim(&r.detail)),
            ReqStatus::Missing => println!(
                "  ✗ {name}  missing{}",
                if r.detail.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", r.detail)
                }
            ),
            ReqStatus::Disabled => println!(
                "  {} {name}  {}",
                out.yellow("✗"),
                out.yellow("installed but disabled")
            ),
            ReqStatus::External => println!(
                "  {} {name}  {}",
                out.yellow("?"),
                out.yellow("external — install it yourself")
            ),
            ReqStatus::Gone => println!("  {}", out.dim(&format!("· {}  ({})", r.name, r.detail))),
        }
        if matches!(r.status, ReqStatus::Ok | ReqStatus::Gone) {
            continue;
        }
        if !r.by.is_empty() {
            let label = if r.by.len() > 1 {
                format!("needed by {} mods", r.by.len())
            } else {
                "needed by".into()
            };
            println!(
                "      {}",
                out.dim(&format!("{label}: {}", trunc(&r.by.join("; "), 100)))
            );
        }
        if !r.hint.is_empty() {
            println!("      → modman {}", r.hint);
        }
        for u in &r.urls {
            println!("      → {u}");
        }
    }
    if n_bad + n_ext > 0 {
        println!();
        if n_bad > 0 {
            out.warn(&format!(
                "{n_bad} requirement(s) missing — the mods that need them will not work."
            ));
        }
        if n_ext > 0 {
            out.info("External requirements cannot be checked automatically. Once installed: modman deps ack <name>");
        }
        if problems && n_ok > 0 {
            println!(
                "  {}",
                out.dim(&format!(
                    "{n_ok} other requirement(s) present — full list: modman deps"
                ))
            );
        }
    }
}

/// After an install: refresh and print the mod's requirements.
pub fn requirements_after_install(ctx: &Ctx, game: &GameConfig, mod_row: i64) {
    let Ok(Some(m)) = mods::get_mod(ctx.store.conn(), &game.domain, mod_row) else {
        return;
    };
    let Some(mid) = m.nexus_mod_id else { return };
    refresh_deps(ctx, game, &[mid], false, true);
    if let Ok(rows) = requirements::analyze(
        &ctx.store,
        game,
        &Scope {
            mods: Some(vec![m.id]),
        },
    ) {
        print_report(ctx, &rows, false, "Requirements");
    }
}

/// After a collection install: refresh and print the problems among the
/// collection's requirements, including its external resources (legacy
/// L2547-2555).
pub fn requirements_after_collection(ctx: &Ctx, game: &GameConfig, ids: &[i64]) {
    if ids.is_empty() {
        return;
    }
    let nexus_ids: Vec<u64> = mods::list_mods(ctx.store.conn(), &game.domain)
        .unwrap_or_default()
        .iter()
        .filter(|m| ids.contains(&m.id))
        .filter_map(|m| m.nexus_mod_id)
        .collect();
    if !nexus_ids.is_empty() {
        refresh_deps(ctx, game, &nexus_ids, false, true);
    }
    if let Ok(rows) = requirements::analyze(
        &ctx.store,
        game,
        &Scope {
            mods: Some(ids.to_vec()),
        },
    ) {
        print_report(
            ctx,
            &rows,
            true,
            "Requirements not included in the collection",
        );
    }
}

fn rows_json(rows: &[ReqRow]) -> String {
    let arr = |v: &[String]| {
        format!(
            "[{}]",
            v.iter().map(|s| json_str(s)).collect::<Vec<_>>().join(",")
        )
    };
    let items: Vec<String> = rows
        .iter()
        .map(|r| {
            format!(
                "{{\"status\":{},\"key\":{},\"name\":{},\"hint\":{},\"urls\":{},\"by\":{},\"detail\":{},\"acked\":{}}}",
                json_str(r.status.as_str()),
                json_str(&r.key),
                json_str(&r.name),
                json_str(&r.hint),
                arr(&r.urls),
                arr(&r.by),
                json_str(&r.detail),
                r.acked
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

pub fn deps(ctx: &mut Ctx, args: DepsArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let out = ctx.out;
    let all = mods::list_mods(ctx.store.conn(), &game.domain)?;
    let (sub, words) = match args.words.split_first() {
        Some((w, rest)) if w == "ack" || w == "unack" => (Some(w.as_str()), rest.to_vec()),
        _ => (None, args.words.clone()),
    };
    let reference = words.join(" ");
    if let Some(sub) = sub {
        if reference.is_empty() {
            bail!("Usage: modman deps {sub} <requirement name>");
        }
        let ids: Vec<u64> = all.iter().filter_map(|m| m.nexus_mod_id).collect();
        if !ids.is_empty() {
            refresh_deps(ctx, &game, &ids, args.refresh, true);
        }
        let rows = requirements::analyze(&ctx.store, &game, &Scope::default())?;
        let unack = sub == "unack";
        let row = match requirements::find_ack_target(&rows, &reference, unack) {
            Ok(r) => r,
            Err(0) => bail!("No requirement matches '{reference}'. See: modman deps"),
            Err(n) => {
                bail!("'{reference}' matches {n} requirements — be more specific. See: modman deps")
            }
        };
        if unack {
            requirements::unack(ctx.store.conn(), &game.domain, &row.key)?;
            out.success(&format!("{} will be checked again.", row.name));
        } else {
            requirements::ack(ctx.store.conn(), &game.domain, &row.key)?;
            out.success(&format!("Marked {} as installed.", out.bold(&row.name)));
        }
        return Ok(());
    }
    if !reference.is_empty() {
        let m = mods::resolve_in(&all, &reference)?;
        let Some(mid) = m.nexus_mod_id else {
            bail!(
                "{} is not a Nexus mod — its requirements are unknown.",
                m.uid
            );
        };
        out.heading(&format!("Requirements — {}", m.name));
        refresh_deps(ctx, &game, &[mid], args.refresh, args.json);
        let rows = requirements::analyze(
            &ctx.store,
            &game,
            &Scope {
                mods: Some(vec![m.id]),
            },
        )?;
        if args.json {
            println!("{}", rows_json(&rows));
        } else {
            print_report(ctx, &rows, false, "Requirements");
        }
        return Ok(());
    }
    let ids: Vec<u64> = all
        .iter()
        .filter(|m| m.enabled)
        .filter_map(|m| m.nexus_mod_id)
        .collect();
    if args.json {
        if !ids.is_empty() {
            refresh_deps(ctx, &game, &ids, args.refresh, true);
        }
        let rows = requirements::analyze(&ctx.store, &game, &Scope::default())?;
        println!("{}", rows_json(&rows));
        return Ok(());
    }
    out.heading(&format!("Requirements — {}", game.name));
    if ids.is_empty() {
        out.info("No enabled Nexus mods.");
        return Ok(());
    }
    refresh_deps(ctx, &game, &ids, args.refresh, false);
    let rows = requirements::analyze(&ctx.store, &game, &Scope::default())?;
    print_report(
        ctx,
        &rows,
        false,
        &format!("Requirements of {} enabled mods", ids.len()),
    );
    Ok(())
}

pub fn updates(ctx: &mut Ctx, args: UpdatesArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let out = ctx.out;
    let nexus = nx::require(ctx)?;
    out.heading(&format!("Update check — {}", game.name));
    let all = mods::list_mods(ctx.store.conn(), &game.domain)?;
    let mut ids: Vec<u64> = all.iter().filter_map(|m| m.nexus_mod_id).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        out.info("No Nexus mods installed.");
        return Ok(());
    }
    let check = nexus.run(modman_core::nexus::check_updates(
        &nexus.client,
        &nexus.shared,
        &game.domain,
        &ids,
        args.refresh,
    ))??;
    if check.stopped_early {
        out.warn("Hourly API limit almost used up — stopping early (re-run later).");
    }
    out.info(&format!(
        "Checked {} mods ({} refreshed from Nexus).",
        ids.len(),
        check.fetched
    ));
    let mut found = Vec::new();
    for m in &all {
        let (Some(mid), Some(fid)) = (m.nexus_mod_id, m.nexus_file_id) else {
            continue;
        };
        let Some(files) = check.files.get(&mid) else {
            continue;
        };
        if let Some(newer) = latest_file(files, fid) {
            found.push((m.clone(), newer.clone()));
        }
    }
    if found.is_empty() {
        out.success("All mods are up to date.");
        return Ok(());
    }
    println!();
    let mut pinned = 0;
    for (m, f) in &found {
        let exact = m.update_policy.as_deref() == Some("exact");
        pinned += usize::from(exact);
        println!(
            "  {} {:<40} {} → {}  {}{}",
            out.yellow("↑"),
            trunc(&m.name, 40),
            m.version.as_deref().unwrap_or("?"),
            out.green(f.version.as_deref().unwrap_or("?")),
            out.dim(&trunc(&f.name, 40)),
            if exact {
                out.dim(" (pinned by collection)")
            } else {
                String::new()
            }
        );
    }
    println!();
    out.info(&format!(
        "{} update(s) available{}.",
        found.len(),
        if pinned > 0 {
            format!(", {pinned} pinned by a collection (use --all to include)")
        } else {
            String::new()
        }
    ));
    if !args.install {
        out.info("Install them with: modman updates --install");
        return Ok(());
    }
    let mut done = 0;
    for (m, f) in &found {
        if m.update_policy.as_deref() == Some("exact") && !args.all {
            continue;
        }
        println!(
            "\n  Updating {} {} → {}",
            out.bold(&m.name),
            m.version.as_deref().unwrap_or("?"),
            f.version.as_deref().unwrap_or("?")
        );
        let mid = m.nexus_mod_id.unwrap_or_default();
        let (archive, md5) = match nx::download_file(
            ctx,
            &nexus,
            &game.domain,
            mid,
            f.file_id,
            Some(f),
            None,
            None,
        ) {
            Ok(r) => r,
            Err(e) => {
                out.error(&format!("Download failed for {}: {e:#}", m.name));
                continue;
            }
        };
        let meta = InstallMeta {
            name: Some(m.name.clone()),
            version: f.version.clone(),
            nexus_mod_id: Some(mid),
            nexus_file_id: Some(f.file_id),
            md5,
            source: Some(m.source.clone()),
            author: m.author.clone(),
            category: m.category.clone(),
            update_policy: m.update_policy.clone(),
            variant: m.variant.clone(),
            replaces: Some(m.id),
            uid: None,
            // Legacy L3800-3802: reuse the recorded FOMOD choices.
            fomod_mode: FomodMode::Reuse,
            fomod_choices: mods::recorded_fomod_choices(ctx.store.conn(), m.id)?,
        };
        let lock = staging_lock(ctx)?;
        let r = install_archive(ctx, &game, &archive, &meta, false);
        drop(lock);
        match r {
            Ok(o) => {
                out.success(&format!("Updated → {}", o.uid));
                done += 1;
            }
            Err(e) => out.error(&format!("Update of {} failed: {e:#}", m.name)),
        }
    }
    println!();
    out.success(&format!("{done} mod(s) updated."));
    after_change(ctx, &game)
}

pub fn search(ctx: &mut Ctx, args: SearchArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let out = ctx.out;
    let q = args.words.join(" ");
    let nexus = nx::connect(ctx, false)?.ok_or_else(|| anyhow::anyhow!("no Nexus client"))?;
    let res = nexus.run(nexus.client.search(&game.domain, &q))??;
    let installed: Vec<u64> = mods::list_mods(ctx.store.conn(), &game.domain)?
        .iter()
        .filter_map(|m| m.nexus_mod_id)
        .collect();
    if args.json {
        let opt = |v: &Option<String>| v.as_deref().map_or("null".into(), json_str);
        let num = |v: Option<u64>| v.map_or("null".into(), |n| n.to_string());
        let items: Vec<String> = res
            .mods
            .iter()
            .map(|m| {
                format!(
                    "{{\"modId\":{},\"name\":{},\"author\":{},\"version\":{},\"summary\":{},\"downloads\":{},\"endorsements\":{},\"installed\":{}}}",
                    num(m.mod_id),
                    opt(&m.name),
                    opt(&m.author),
                    opt(&m.version),
                    opt(&m.summary),
                    num(m.downloads),
                    num(m.endorsements),
                    m.mod_id.is_some_and(|i| installed.contains(&i))
                )
            })
            .collect();
        println!("{{\"total\":{},\"mods\":[{}]}}", res.total, items.join(","));
        return Ok(());
    }
    out.heading(&format!("Search — \"{q}\" ({})", game.name));
    for m in &res.mods {
        let mark = if m.mod_id.is_some_and(|i| installed.contains(&i)) {
            out.green("✓ ")
        } else {
            "  ".into()
        };
        println!(
            "  {mark}{:<7} {} {}",
            m.mod_id.map_or("?".into(), |i| i.to_string()),
            out.bold(&trunc(m.name.as_deref().unwrap_or("?"), 50)),
            out.dim(&format!(
                "v{} by {} · {} downloads · {} endorsements",
                m.version
                    .as_deref()
                    .unwrap_or("?")
                    .trim_start_matches(['v', 'V']),
                trunc(m.author.as_deref().unwrap_or("?"), 20),
                m.downloads.unwrap_or(0),
                m.endorsements.unwrap_or(0)
            ))
        );
        if let Some(s) = m.summary.as_deref().filter(|s| !s.is_empty()) {
            println!("           {}", out.dim(&trunc(s, 100)));
        }
    }
    println!();
    out.info(&format!(
        "{} result(s). Install with: modman install <id>",
        res.total
    ));
    Ok(())
}

pub fn profile(ctx: &mut Ctx, args: ProfileArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let out = ctx.out;
    match args.cmd.unwrap_or(ProfileCmd::List) {
        ProfileCmd::List => {
            out.heading(&format!("Profiles — {}", game.name));
            let list = mods::list_profiles(&ctx.store, &game.domain)?;
            if list.is_empty() {
                out.info("No profiles yet. Save one with: modman profile save <name>");
            }
            for p in list {
                println!(
                    "  • {}  {}",
                    out.bold(&p.name),
                    out.dim(&format!(
                        "{} mods, {} enabled, saved {}",
                        p.mods,
                        p.enabled,
                        fmt_time(p.saved_at)
                    ))
                );
            }
        }
        ProfileCmd::Save { name } => {
            if !mods::valid_profile_name(&name) {
                return Err(mods::ModsError::InvalidProfileName(name).into());
            }
            let exists = mods::list_profiles(&ctx.store, &game.domain)?
                .iter()
                .any(|p| p.name == name);
            if exists && !ctx.confirm(&format!("Overwrite profile '{name}'?"), true) {
                return Ok(());
            }
            let n = mods::save_profile(&mut ctx.store, &game.domain, &name)?;
            out.success(&format!("Saved profile '{name}' ({n} mods)."));
        }
        ProfileCmd::Load { name } => {
            let r = mods::load_profile(&mut ctx.store, &game.domain, &name)?;
            out.success(&format!("Loaded profile '{name}'."));
            if r.missing > 0 {
                out.warn(&format!(
                    "{} mod(s) from the profile are no longer installed.",
                    r.missing
                ));
            }
            after_change(ctx, &game)?;
        }
        ProfileCmd::Delete { name } => {
            mods::delete_profile(&ctx.store, &game.domain, &name)?;
            out.success(&format!("Deleted profile '{name}'."));
        }
    }
    Ok(())
}

/// First executable named `name` on `PATH`.
pub fn which(name: &str) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p).map(|d| d.join(name)).find(|f| {
            f.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

pub fn launch(ctx: &mut Ctx, _args: LaunchArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let out = ctx.out;
    let Some(appid) = game.appid else {
        bail!(
            "No Steam app id for {}. Set one with: modman game set appid <id>",
            game.name
        );
    };
    let enabled = mods::list_mods(ctx.store.conn(), &game.domain)?
        .iter()
        .any(|m| m.enabled);
    let st = deploy::status(ctx.store.conn(), &game.domain)?;
    if enabled && mods::changed_since(&ctx.store, &game.domain, st.last_deployed_at)? {
        out.warn("Mods changed since the last deploy.");
        if ctx.confirm("Deploy before launching?", true) {
            let saved = ctx.game.replace(game.domain.clone());
            let r = super::deploy::deploy(
                ctx,
                super::deploy::DeployArgs {
                    dry_run: false,
                    method: None,
                },
            );
            ctx.game = saved;
            r?;
        }
    }
    out.info(&format!("Launching {} (Steam app {appid})...", game.name));
    let url = format!("steam://rungameid/{appid}");
    let (prog, args): (std::path::PathBuf, Vec<String>) = match which("steam") {
        Some(steam) => match which("setsid") {
            Some(setsid) => (setsid, vec![steam.display().to_string(), url]),
            None => (steam, vec![url]),
        },
        None => match which("xdg-open") {
            Some(x) => (x, vec![url]),
            None => bail!("Neither steam nor xdg-open was found to start the game."),
        },
    };
    std::process::Command::new(prog)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

pub fn clean(ctx: &mut Ctx, _args: CleanArgs) -> Result<()> {
    let game = ctx.need_game()?;
    let out = ctx.out;
    out.heading(&format!("Clean — {}", game.name));
    let cfg = Config::load(&ctx.store, &ctx.paths)?;
    let lock = staging_lock(ctx)?;
    let tmp = deploy::staging_root(&game, &cfg.data_dir).join(".tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    let referenced: Vec<std::path::PathBuf> = mods::list_mods(ctx.store.conn(), &game.domain)?
        .into_iter()
        .filter_map(|m| m.archive)
        .collect();
    let dl = mods::downloads_dir(&cfg.data_dir, &game.domain);
    let mut cands: Vec<(std::path::PathBuf, u64)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dl) {
        for e in rd.flatten() {
            let p = e.path();
            let Ok(md) = e.metadata() else { continue };
            // `.part` files belong to unfinished (resumable) downloads.
            if md.is_file() && !referenced.contains(&p) && p.extension().is_none_or(|x| x != "part")
            {
                cands.push((p, md.len()));
            }
        }
    }
    cands.sort();
    if cands.is_empty() {
        out.success("No unreferenced downloads. Temporary files removed.");
        return Ok(());
    }
    let bytes: u64 = cands.iter().map(|c| c.1).sum();
    for (p, _) in &cands {
        println!(
            "  {}",
            out.dim(
                &p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            )
        );
    }
    println!();
    out.info(&format!(
        "{} archive(s) not used by any installed mod ({}).",
        cands.len(),
        human_size(bytes)
    ));
    out.info("Note: reinstalling or updating mods from these archives would need a re-download.");
    if !ctx.confirm("Delete them?", false) {
        out.info("Kept.");
        return Ok(());
    }
    for (p, _) in &cands {
        std::fs::remove_file(p)?;
    }
    drop(lock);
    out.success(&format!("Freed {}.", human_size(bytes)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_formats_as_utc_date() {
        assert_eq!(fmt_time(0), "1970-01-01 00:00 UTC");
        assert_eq!(fmt_time(1_700_000_000), "2023-11-14 22:13 UTC");
    }
}
