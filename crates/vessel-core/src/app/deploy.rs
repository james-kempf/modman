// SPDX-License-Identifier: GPL-3.0-or-later
//! Deploy and purge runs with their user-facing summaries (legacy
//! `cmd_deploy` L1738, `cmd_purge` L1899), and the auto-deploy step after a
//! change (`after_change`, L2809-2816).

use std::collections::BTreeMap;
use std::path::Path;

use super::{games_hint, Cx, Msg, R};
use crate::deploy::{
    self, DeployEnv, DeployOptions, DeployReport, Method, PurgeReport, RecoveryOutcome,
    RecoveryReport,
};
use crate::games::{self, GameConfig};
use crate::loadorder::{PluginStage, SkipReason, SortMethod};

/// `2 hardlink, 1 copy`.
pub fn methods_summary(by: &BTreeMap<Method, usize>) -> String {
    by.iter()
        .map(|(m, n)| format!("{n} {}", m.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn env_for(cx: &Cx<'_>, game: &GameConfig) -> R<(DeployEnv, crate::config::Config)> {
    let cfg = cx.config()?;
    Ok((DeployEnv::new(cx.paths, &cfg.data_dir, &game.domain), cfg))
}

fn recovery_msg(rec: Option<&RecoveryReport>) -> Option<Msg> {
    rec.map(|r| {
        let what = match r.outcome {
            RecoveryOutcome::RolledBack => "rolled back",
            RecoveryOutcome::Resumed => "finished",
        };
        Msg::warn(format!(
            "An interrupted {} was found and {what} (removed {}, restored {}).",
            r.op, r.undo.removed, r.undo.restored
        ))
    })
}

/// One-line summary of a plugin stage, shared by deploy, purge and
/// `plugins sort`.
pub fn plugin_stage_msgs(stage: Option<&PluginStage>) -> Vec<Msg> {
    let mut v = Vec::new();
    match stage {
        None | Some(PluginStage::Skipped(SkipReason::NotBethesda)) => {}
        Some(PluginStage::Skipped(SkipReason::NoPluginsFile)) => v.push(Msg::warn(
            "plugins.txt location unknown (no Proton prefix). Set it: vessel game set plugins-file <path>",
        )),
        Some(PluginStage::Skipped(SkipReason::FolderMissing(dir))) => v.push(Msg::warn(format!(
            "plugins.txt folder not found ({}). Launch the game once, then run 'vessel deploy' again.",
            dir.display()
        ))),
        Some(PluginStage::Failed(e)) => v.push(Msg::warn(format!("plugins.txt not updated: {e}"))),
        Some(PluginStage::Updated(r)) => {
            v.extend(r.warnings.iter().map(Msg::warn));
            if !r.added.is_empty() {
                let how = match r.sorted_by {
                    Some(SortMethod::Loot { masterlist: true }) => "sorted by LOOT with masterlist",
                    Some(SortMethod::Loot { masterlist: false }) => {
                        "sorted by LOOT (no masterlist; run: vessel plugins masterlist)"
                    }
                    _ => "in mod order",
                };
                v.push(Msg::info(format!(
                    "plugins.txt: {} plugin(s) activated ({} provided by mods), {how}.",
                    r.added.len(),
                    r.provided
                )));
            } else if !r.removed.is_empty() {
                v.push(Msg::info(format!(
                    "plugins.txt: removed {} plugin(s) added by Vessel.",
                    r.removed.len()
                )));
            }
        }
    }
    v
}

/// The summary `deploy` shows for a report.
pub fn deploy_msgs(game: &GameConfig, r: &DeployReport) -> Vec<Msg> {
    let mut v: Vec<Msg> = recovery_msg(r.recovered.as_ref()).into_iter().collect();
    for s in &r.skipped {
        v.push(Msg::warn(format!(
            "Skipping {}: {} — run: vessel reinstall '{}'",
            s.uid, s.reason, s.uid
        )));
    }
    if let Some(p) = &r.purged {
        v.push(Msg::info(format!(
            "Removed the previous deployment ({} files, {} originals restored).",
            p.removed, p.restored
        )));
    }
    if r.placed == 0 {
        v.extend(plugin_stage_msgs(r.plugins.as_ref()));
        v.push(Msg::info(
            "Nothing to deploy (no enabled mods with staged files).",
        ));
        return v;
    }
    if !r.conflicts.is_empty() {
        v.push(Msg::info(format!(
            "{} file conflict(s); the later mod in the load order wins.",
            r.conflicts.len()
        )));
    }
    if r.dry_run {
        v.push(Msg::info(format!(
            "Dry run: would deploy {} file(s) from {} mod(s) with method {}, back up {} game file(s), create {} folder(s). Nothing was changed.",
            r.placed,
            r.mods,
            r.method.as_str(),
            r.backed_up,
            r.new_dirs
        )));
        return v;
    }
    v.push(Msg::success(format!(
        "Deployed {} file(s) from {} mod(s) to {} ({}); backed up {} original(s).",
        r.placed,
        r.mods,
        game.name,
        methods_summary(&r.by_method),
        r.backed_up
    )));
    v.extend(plugin_stage_msgs(r.plugins.as_ref()));
    v
}

/// Deploys `game`. `method` overrides the game and global setting.
pub fn run_deploy(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    method: Option<&str>,
    dry_run: bool,
) -> R<DeployReport> {
    let (env, cfg) = env_for(cx, game)?;
    let method = deploy::resolve_method(method, game, &cfg.deploy_method)?;
    let opts = DeployOptions {
        method,
        dry_run,
        progress: cx.ui.deploy_progress(),
    };
    let r = deploy::deploy(cx.store, game, &env, &opts)?;
    for m in deploy_msgs(game, &r) {
        cx.ui.say(m);
    }
    Ok(r)
}

/// The summary `purge` shows for a report.
pub fn purge_msgs(r: &PurgeReport, overwrite_dir: &Path) -> Vec<Msg> {
    let mut v: Vec<Msg> = recovery_msg(r.recovered.as_deref()).into_iter().collect();
    v.push(Msg::success(format!(
        "Removed {}, restored {} original(s){}{}{}",
        r.removed,
        r.restored,
        if r.missing > 0 {
            format!(", {} already missing", r.missing)
        } else {
            String::new()
        },
        if r.changed > 0 {
            format!(
                ", {} changed since deploy (kept / moved to overwrite)",
                r.changed
            )
        } else {
            String::new()
        },
        if r.kept > 0 {
            format!(", {} backup(s) kept (path occupied)", r.kept)
        } else {
            String::new()
        },
    )));
    v.extend(plugin_stage_msgs(r.plugins.as_ref()));
    if r.changed > 0 {
        v.push(Msg::info(format!(
            "Changed files were kept in {}",
            overwrite_dir.display()
        )));
    }
    v
}

/// Removes the deployment of `game` after a confirmation. `None` when
/// nothing is deployed or the user declined.
pub fn run_purge(cx: &mut Cx<'_>, game: &GameConfig) -> R<Option<PurgeReport>> {
    let (env, _) = env_for(cx, game)?;
    if !games::deployment_active(cx.store.conn(), &game.domain).map_err(games_hint)? {
        cx.ui
            .info(&format!("Nothing is deployed for {}.", game.name));
        return Ok(None);
    }
    if !cx.ui.confirm(
        &format!(
            "Remove deployed mods from {} and restore the original files?",
            game.name
        ),
        true,
    ) {
        cx.ui.info("Cancelled.");
        return Ok(None);
    }
    let r = deploy::purge(cx.store, game, &env)?;
    for m in purge_msgs(&r, &env.overwrite_dir) {
        cx.ui.say(m);
    }
    Ok(Some(r))
}

/// What happened after a change of the mod set.
#[derive(Debug, Clone, Default)]
pub struct AfterChange {
    /// `auto-deploy` ran a deploy.
    pub deployed: Option<DeployReport>,
    /// A deployment is active and now out of date.
    pub needs_deploy: bool,
}

/// Legacy `after_change`: deploy when `auto-deploy` is on and the game
/// folder exists, else hint when a deployment is active.
pub fn after_change(cx: &mut Cx<'_>, game: &GameConfig) -> R<AfterChange> {
    let cfg = cx.config()?;
    if cfg.auto_deploy && game.folder.as_deref().is_some_and(Path::is_dir) {
        cx.ui.blank();
        let r = run_deploy(cx, game, None, false)?;
        return Ok(AfterChange {
            deployed: Some(r),
            needs_deploy: false,
        });
    }
    let active = games::deployment_active(cx.store.conn(), &game.domain)?;
    if active {
        cx.ui
            .info("Run 'vessel deploy' to apply the changes to the game folder.");
    }
    Ok(AfterChange {
        deployed: None,
        needs_deploy: active,
    })
}

/// `--deploy` / `auto-deploy` after a collection install (legacy
/// L2556-2560).
pub fn deploy_after_collection(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    explicit: bool,
) -> R<Option<DeployReport>> {
    let cfg = cx.config()?;
    if explicit || cfg.auto_deploy {
        cx.ui.blank();
        return run_deploy(cx, game, None, false).map(Some);
    }
    cx.ui
        .say(Msg::success("Collection installed. Apply it to the game with: ").b("vessel deploy"));
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadorder::PluginsReport;

    #[test]
    fn plugin_stage_summary_names_the_sort_method() {
        let r = PluginsReport {
            added: vec!["a.esp".into()],
            provided: 1,
            sorted_by: Some(SortMethod::Loot { masterlist: true }),
            ..Default::default()
        };
        let m = plugin_stage_msgs(Some(&PluginStage::Updated(r)));
        assert_eq!(
            m[0].text(),
            "plugins.txt: 1 plugin(s) activated (1 provided by mods), sorted by LOOT with masterlist."
        );
        assert!(plugin_stage_msgs(None).is_empty());
    }

    #[test]
    fn empty_deploy_says_nothing_to_deploy() {
        let g = GameConfig::custom("g");
        let m = deploy_msgs(&g, &DeployReport::default());
        assert_eq!(
            m.last().unwrap().text(),
            "Nothing to deploy (no enabled mods with staged files)."
        );
    }
}
