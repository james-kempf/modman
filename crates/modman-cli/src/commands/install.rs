// SPDX-License-Identifier: GPL-3.0-or-later
//! install, download, reinstall (legacy `cmd_install` L3323-3360,
//! `cmd_download` L3365-3392, `install_nexus_mod` L3239-3288,
//! `install_local` L3290-3318, `cmd_reinstall` L3635-3665).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use clap::Args;
use modman_core::config::Config;
use modman_core::download::NxmLink;
use modman_core::games::{self, GameConfig};
use modman_core::mods::{self, InstallMeta, InstallOutcome, ModRecord};
use modman_core::nexus::{FileInfo, NxmKey};
use modman_core::store::{DeployLock, StoreError};

use super::{deploy, mods as modcmd, not_yet};
use crate::context::{games_hint, Ctx};
use crate::nexus::{self as nx, Nexus};
use crate::output::human_size;

#[derive(Args, Debug)]
pub struct InstallArgs {
    /// Nexus mod id, Nexus URL, <game>:<id>, nxm:// link, archive file or collection URL.
    pub target: String,
    /// Install this file id of the mod.
    #[arg(short = 'f', long = "file", value_name = "ID")]
    pub file: Option<u64>,
    /// FOMOD installer mode (milestone M5).
    #[arg(long, value_name = "MODE", value_parser = ["ask", "default", "none", "reuse"])]
    pub fomod: Option<String>,
    /// Name to show for the mod.
    #[arg(long)]
    pub name: Option<String>,
    /// Deploy afterwards.
    #[arg(long)]
    pub deploy: bool,
    /// Collections: optional mods (all, none, ask).
    #[arg(long, value_name = "WHICH")]
    pub optional: Option<String>,
    /// Collections: revision to install.
    #[arg(short = 'r', long, value_name = "N")]
    pub revision: Option<u64>,
    /// Collections: download only.
    #[arg(long = "download-only")]
    pub download_only: bool,
}

#[derive(Args, Debug)]
pub struct DownloadArgs {
    /// Nexus mod id, Nexus URL, <game>:<id>, nxm:// link or collection URL.
    pub target: String,
    /// Download this file id of the mod.
    #[arg(short = 'f', long = "file", value_name = "ID")]
    pub file: Option<u64>,
    /// Options passed to collection downloads.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub rest: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ReinstallArgs {
    /// Mod to re-extract.
    pub reference: String,
    /// FOMOD mode (milestone M5).
    #[arg(long, value_name = "MODE")]
    pub fomod: Option<String>,
    /// Re-run the FOMOD installer (milestone M5).
    #[arg(long)]
    pub ask: bool,
}

/// What an install target names (legacy `parse_mod_target`, L2829-2846).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Local(PathBuf),
    Collection(String),
    Nexus {
        game: Option<String>,
        mod_id: u64,
        file_id: Option<u64>,
        key: Option<NxmKey>,
    },
}

fn digits(s: &str) -> Option<u64> {
    (!s.is_empty() && s.len() <= 19 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// Classifies `t`; `None` when it is none of the known forms.
pub fn parse_target(t: &str) -> Result<Option<Target>> {
    if Path::new(t).exists() {
        return Ok(Some(Target::Local(PathBuf::from(t))));
    }
    if t.contains("collections/") {
        return Ok(Some(Target::Collection(t.to_string())));
    }
    if t.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("nxm://")) {
        return match NxmLink::parse(t)? {
            NxmLink::Mod(m) => Ok(Some(Target::Nexus {
                game: Some(m.game),
                mod_id: m.mod_id,
                file_id: Some(m.file_id),
                key: m.key,
            })),
            NxmLink::Collection { .. } => Ok(Some(Target::Collection(t.to_string()))),
        };
    }
    let lower = t.to_ascii_lowercase();
    if let Some(i) = lower.find("nexusmods.com/") {
        let rest = &lower[i + "nexusmods.com/".len()..];
        let rest = rest.strip_prefix("games/").unwrap_or(rest);
        if let Some((g, rest)) = rest.split_once('/') {
            if let Some(after) = rest.strip_prefix("mods/") {
                let id: String = after.chars().take_while(char::is_ascii_digit).collect();
                let file_id = lower
                    .split(['?', '&'])
                    .find_map(|p| p.strip_prefix("file_id="))
                    .and_then(|v| {
                        digits(
                            &v.chars()
                                .take_while(char::is_ascii_digit)
                                .collect::<String>(),
                        )
                    });
                if let (Some(mod_id), false) = (digits(&id), g.is_empty()) {
                    return Ok(Some(Target::Nexus {
                        game: Some(games::normalize_domain(g)?),
                        mod_id,
                        file_id,
                        key: None,
                    }));
                }
            }
        }
    }
    if let Some((g, m)) = t.split_once(':') {
        if games::is_valid_domain(g) {
            if let Some(mod_id) = digits(m) {
                return Ok(Some(Target::Nexus {
                    game: Some(g.to_string()),
                    mod_id,
                    file_id: None,
                    key: None,
                }));
            }
        }
    }
    Ok(digits(t).map(|mod_id| Target::Nexus {
        game: None,
        mod_id,
        file_id: None,
        key: None,
    }))
}

/// Holds the deploy lock while staging trees change.
pub fn staging_lock(ctx: &Ctx) -> Result<DeployLock> {
    DeployLock::try_acquire(&ctx.paths.deploy_lock_path()).map_err(|e| match e {
        StoreError::LockBusy(_) => anyhow!(
            "Another modman process is deploying or changing mods; try again when it has finished."
        ),
        e => e.into(),
    })
}

/// Legacy `after_change` (L2809-2816): deploy when `auto-deploy` is on and
/// the game folder exists, else hint when a deployment is active.
pub fn after_change(ctx: &mut Ctx, game: &GameConfig) -> Result<()> {
    let cfg = Config::load(&ctx.store, &ctx.paths)?;
    if cfg.auto_deploy && game.folder.as_deref().is_some_and(Path::is_dir) {
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
    if games::deployment_active(ctx.store.conn(), &game.domain)? {
        ctx.out
            .info("Run 'modman deploy' to apply the changes to the game folder.");
    }
    Ok(())
}

/// Game named by a target, else the active one.
pub fn game_for(ctx: &Ctx, domain: Option<&str>) -> Result<GameConfig> {
    match domain {
        Some(d) => games::require_game(ctx.store.conn(), d).map_err(games_hint),
        None => ctx.need_game(),
    }
}

fn variant_prompt(choices: &[String]) -> Option<usize> {
    eprintln!("This archive has several variants:");
    for (i, c) in choices.iter().enumerate() {
        eprintln!("  {:>2}. {c}", i + 1);
    }
    eprint!("Which one? [1]: ");
    let mut s = String::new();
    std::io::stdin().read_line(&mut s).ok()?;
    let s = s.trim();
    if s.is_empty() {
        return Some(0);
    }
    s.parse::<usize>()
        .ok()
        .filter(|n| (1..=choices.len()).contains(n))
        .map(|n| n - 1)
}

/// Runs the core install pipeline with the CLI's prompts and messages.
pub fn install_archive(
    ctx: &mut Ctx,
    game: &GameConfig,
    archive: &Path,
    meta: &InstallMeta,
) -> Result<InstallOutcome> {
    let cfg = Config::load(&ctx.store, &ctx.paths)?;
    let chooser: &dyn Fn(&[String]) -> Option<usize> = &variant_prompt;
    let interactive = ctx.is_interactive();
    let o = mods::install_archive(
        &mut ctx.store,
        game,
        &cfg.data_dir,
        archive,
        meta,
        interactive.then_some(chooser),
    )?;
    for w in &o.layout.warnings {
        ctx.out.warn(w);
    }
    if o.fomod_found {
        ctx.out.warn(
            "This archive has a FOMOD installer; FOMOD support arrives with milestone M5, so its files were installed with the plain layout rules. Reinstall it once FOMOD support is available.",
        );
    }
    Ok(o)
}

/// Legacy `choose_file`: MAIN, UPDATE, OPTIONAL, MISCELLANEOUS, newest first.
fn choose_file(ctx: &Ctx, files: &[FileInfo]) -> Result<u64> {
    const CATS: [&str; 4] = ["MAIN", "UPDATE", "OPTIONAL", "MISCELLANEOUS"];
    let mut rows: Vec<&FileInfo> = files
        .iter()
        .filter(|f| {
            f.category_name
                .as_deref()
                .is_some_and(|c| CATS.contains(&c))
        })
        .collect();
    rows.sort_by_key(|f| {
        let c = CATS
            .iter()
            .position(|c| Some(*c) == f.category_name.as_deref())
            .unwrap_or(9);
        (c, std::cmp::Reverse(f.uploaded_timestamp.unwrap_or(0)))
    });
    let Some(first) = rows.first() else {
        bail!("This mod has no downloadable files.");
    };
    if rows.len() == 1 || !ctx.is_interactive() {
        return Ok(first.file_id);
    }
    eprintln!("Files:");
    for (i, f) in rows.iter().enumerate() {
        eprintln!(
            "  {:>2}. {:<13} {:<44} {:<12} {:>9}",
            i + 1,
            f.category_name.as_deref().unwrap_or(""),
            f.name,
            f.version.as_deref().unwrap_or(""),
            human_size(f.size_in_bytes.unwrap_or(0))
        );
    }
    loop {
        let a = ctx.ask("Which file?", "1");
        match a.parse::<usize>() {
            Ok(n) if (1..=rows.len()).contains(&n) => return Ok(rows[n - 1].file_id),
            _ => ctx
                .out
                .warn(&format!("Enter a number between 1 and {}.", rows.len())),
        }
    }
}

/// Installs a Nexus mod file. `row` is the downloads row of an nxm link.
#[allow(clippy::too_many_arguments)]
pub fn install_nexus(
    ctx: &mut Ctx,
    nexus: &Nexus,
    game: &GameConfig,
    mod_id: u64,
    file_id: Option<u64>,
    key: Option<NxmKey>,
    name_override: Option<&str>,
    row: Option<i64>,
) -> Result<Option<InstallOutcome>> {
    let out = ctx.out;
    let d = &game.domain;
    let info = nexus.run(nexus.client.mod_info(d, mod_id))??;
    let mname = info.name.clone().unwrap_or_else(|| format!("mod {mod_id}"));
    println!(
        "{} by {} {}",
        out.bold(&mname),
        info.author.as_deref().unwrap_or("?"),
        out.dim(&format!(
            "(v{}) — https://www.nexusmods.com/{d}/mods/{mod_id}",
            info.version.as_deref().unwrap_or("?")
        ))
    );
    if let Some(s) = info.status.as_deref().filter(|s| *s != "published") {
        out.warn(&format!("Mod status on Nexus: {s}"));
    }
    let files = nexus.run(nexus.client.mod_files(d, mod_id))??;
    let file_id = match file_id {
        Some(f) => f,
        None => choose_file(ctx, &files.files)?,
    };
    let file = files
        .files
        .iter()
        .find(|f| f.file_id == file_id)
        .ok_or_else(|| anyhow!("File {file_id} does not belong to mod {mod_id}."))?
        .clone();
    println!(
        "  File: {} {}",
        file.name,
        out.dim(&format!(
            "({}, v{}, {})",
            file.category_name.as_deref().unwrap_or("?"),
            file.version.as_deref().unwrap_or("?"),
            human_size(file.size_in_bytes.unwrap_or(0))
        ))
    );
    let installed = mods::list_mods(ctx.store.conn(), d)?;
    let mut replaces: Option<&ModRecord> = None;
    if let Some(same) = installed
        .iter()
        .find(|m| m.nexus_mod_id == Some(mod_id) && m.nexus_file_id == Some(file_id))
    {
        if !ctx.confirm(
            &format!("Already installed ({}). Reinstall?", same.uid),
            false,
        ) {
            out.info("Nothing to do.");
            if let Some(r) = row {
                modman_core::download::set_status(
                    &ctx.store,
                    r,
                    modman_core::download::DownloadStatus::Cancelled,
                )?;
            }
            return Ok(None);
        }
        replaces = Some(same);
    } else if let Some(older) = installed.iter().find(|m| m.nexus_mod_id == Some(mod_id)) {
        if ctx.confirm(
            &format!("Replace the installed file {} with this one?", older.uid),
            true,
        ) {
            replaces = Some(older);
        }
    }
    let (archive, md5) = nx::download_file(ctx, nexus, d, mod_id, file_id, Some(&file), key, row)?;
    let name = match name_override {
        Some(n) => n.to_string(),
        None if file.category_name.as_deref() != Some("MAIN") && file.name != mname => {
            format!("{mname} - {}", file.name)
        }
        None => mname.clone(),
    };
    out.info(&format!("Installing {name}..."));
    let meta = InstallMeta {
        name: Some(name.clone()),
        version: file.version.clone(),
        nexus_mod_id: Some(mod_id),
        nexus_file_id: Some(file_id),
        md5,
        source: Some("nexus".into()),
        author: info.author.clone(),
        variant: replaces.and_then(|r| r.variant.clone()),
        replaces: replaces.map(|r| r.id),
        update_policy: replaces.and_then(|r| r.update_policy.clone()),
        ..Default::default()
    };
    let lock = staging_lock(ctx)?;
    let o = install_archive(ctx, game, &archive, &meta)?;
    drop(lock);
    out.success(&format!("Installed {} → {}", out.bold(&name), o.uid));
    modcmd::requirements_after_install(ctx, game, o.mod_id);
    Ok(Some(o))
}

fn install_local(ctx: &mut Ctx, game: &GameConfig, path: &Path, name: Option<&str>) -> Result<()> {
    let out = ctx.out;
    let path = path
        .canonicalize()
        .map_err(|e| anyhow!("{}: {e}", path.display()))?;
    let base = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cfg = Config::load(&ctx.store, &ctx.paths)?;
    let dl = mods::downloads_dir(&cfg.data_dir, &game.domain);
    std::fs::create_dir_all(&dl)?;
    let dest = dl.join(&base);
    // Keep a copy next to Nexus downloads, like legacy.
    let archive = if path.is_file() && dest != path {
        std::fs::copy(&path, &dest)
            .map_err(|e| anyhow!("cannot copy {} to {}: {e}", path.display(), dest.display()))?;
        dest
    } else {
        path.clone()
    };
    let meta = InstallMeta {
        name: name.map(str::to_string),
        source: Some("local".into()),
        ..Default::default()
    };
    out.info(&format!("Installing from {base}..."));
    let lock = staging_lock(ctx)?;
    let o = install_archive(ctx, game, &archive, &meta)?;
    drop(lock);
    out.success(&format!("Installed {} → {}", out.bold(&o.name), o.uid));
    Ok(())
}

pub fn install(ctx: &mut Ctx, args: InstallArgs) -> Result<()> {
    if args.fomod.is_some() {
        return not_yet("install --fomod", "M5");
    }
    let target = parse_target(&args.target)?.ok_or_else(|| {
        anyhow!(
            "Don't know how to install '{}'. Give a mod id, a Nexus URL, an nxm:// link, an archive or a collection URL.",
            args.target
        )
    })?;
    let game = match target {
        Target::Local(p) => {
            let game = ctx.need_game()?;
            install_local(ctx, &game, &p, args.name.as_deref())?;
            game
        }
        Target::Collection(_) => return not_yet("collection install", "M5"),
        Target::Nexus {
            game,
            mod_id,
            file_id,
            key,
        } => {
            let nexus = nx::require(ctx)?;
            let game = game_for(ctx, game.as_deref())?;
            let file_id = args.file.or(file_id);
            if install_nexus(
                ctx,
                &nexus,
                &game,
                mod_id,
                file_id,
                key,
                args.name.as_deref(),
                None,
            )?
            .is_none()
            {
                return Ok(());
            }
            game
        }
    };
    if args.deploy {
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
        r
    } else {
        after_change(ctx, &game)
    }
}

pub fn download(ctx: &mut Ctx, args: DownloadArgs) -> Result<()> {
    let target = parse_target(&args.target)?;
    let (game, mod_id, file_id, key) = match target {
        Some(Target::Collection(_)) => return not_yet("collection download", "M5"),
        Some(Target::Nexus {
            game,
            mod_id,
            file_id,
            key,
        }) => (game, mod_id, file_id, key),
        _ => bail!(
            "Cannot parse '{}'. Give a mod id, a Nexus URL or an nxm:// link.",
            args.target
        ),
    };
    let nexus = nx::require(ctx)?;
    let game = game_for(ctx, game.as_deref())?;
    let files = nexus.run(nexus.client.mod_files(&game.domain, mod_id))??;
    let file_id = match args.file.or(file_id) {
        Some(f) => f,
        None => choose_file(ctx, &files.files)?,
    };
    let info = files.files.iter().find(|f| f.file_id == file_id);
    let (path, _) = nx::download_file(ctx, &nexus, &game.domain, mod_id, file_id, info, key, None)?;
    ctx.out.success(&format!("Saved → {}", path.display()));
    Ok(())
}

pub fn reinstall(ctx: &mut Ctx, args: ReinstallArgs) -> Result<()> {
    if args.ask || args.fomod.is_some() {
        return not_yet("reinstall --ask/--fomod", "M5");
    }
    let game = ctx.need_game()?;
    let rec = mods::resolve(ctx.store.conn(), &game.domain, &args.reference)?;
    let archive = match &rec.archive {
        Some(a) if a.exists() => a.clone(),
        other => {
            let (Some(m), Some(f)) = (rec.nexus_mod_id, rec.nexus_file_id) else {
                bail!(
                    "Archive missing: {}",
                    other
                        .as_ref()
                        .map_or("(none)".into(), |p| p.display().to_string())
                );
            };
            let nexus = nx::require(ctx)?;
            nx::download_file(ctx, &nexus, &game.domain, m, f, None, None, None)?.0
        }
    };
    let meta = InstallMeta {
        name: Some(rec.name.clone()),
        version: rec.version.clone(),
        nexus_mod_id: rec.nexus_mod_id,
        nexus_file_id: rec.nexus_file_id,
        md5: rec.md5.clone(),
        source: Some(rec.source.clone()),
        author: rec.author.clone(),
        category: rec.category.clone(),
        update_policy: rec.update_policy.clone(),
        variant: rec.variant.clone(),
        replaces: Some(rec.id),
        uid: Some(rec.uid.clone()),
    };
    ctx.out.info(&format!("Reinstalling {}...", rec.uid));
    let lock = staging_lock(ctx)?;
    install_archive(ctx, &game, &archive, &meta)?;
    drop(lock);
    ctx.out.success(&format!("Reinstalled {}", rec.uid));
    after_change(ctx, &game)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nexus(game: Option<&str>, m: u64, f: Option<u64>) -> Option<Target> {
        Some(Target::Nexus {
            game: game.map(str::to_string),
            mod_id: m,
            file_id: f,
            key: None,
        })
    }

    #[test]
    fn targets_parse_like_legacy() {
        let p = |t: &str| parse_target(t).unwrap();
        assert_eq!(p("12604"), nexus(None, 12604, None));
        assert_eq!(
            p("skyrimspecialedition:266"),
            nexus(Some("skyrimspecialedition"), 266, None)
        );
        assert_eq!(
            p("https://www.nexusmods.com/skyrimspecialedition/mods/266?tab=files&file_id=99"),
            nexus(Some("skyrimspecialedition"), 266, Some(99))
        );
        assert_eq!(
            p("https://www.nexusmods.com/games/stardewvalley/mods/5"),
            nexus(Some("stardewvalley"), 5, None)
        );
        let Some(Target::Nexus { key, file_id, .. }) = p(
            "nxm://skyrimspecialedition/mods/266/files/1000?key=abc&expires=9999999999&user_id=2",
        ) else {
            panic!()
        };
        assert_eq!(file_id, Some(1000));
        assert_eq!(key.unwrap().key, "abc");
        assert_eq!(
            p("nxm://skyrimspecialedition/collections/abc123"),
            Some(Target::Collection(
                "nxm://skyrimspecialedition/collections/abc123".into()
            ))
        );
        assert!(matches!(
            p("https://next.nexusmods.com/skyrimspecialedition/collections/x"),
            Some(Target::Collection(_))
        ));
        assert_eq!(p("not a target"), None);
        assert_eq!(p("Bad:12"), None);
        assert!(parse_target("nxm://evil.example/mods/1/files/2").is_err());
    }
}
