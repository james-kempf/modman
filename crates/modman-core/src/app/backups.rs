// SPDX-License-Identifier: GPL-3.0-or-later
//! Poisoned-backup cleanup actions (`deploy clean-backups`,
//! `deploy restore-quarantine`). The front end asks the user and passes
//! `confirmed`; nothing here asks, because a GUI [`super::Ui`] may answer
//! questions with defaults.

use super::{Cx, Msg, R};
use crate::deploy::{
    self, DeployEnv, PoisonClass, PoisonedBackup, QuarantineManifest, QuarantineReport,
    RestoreReport,
};
use crate::games::GameConfig;

/// Entries listed in full before "... and N more".
const MAX_LISTED: usize = 40;

fn env_for(cx: &Cx<'_>, game: &GameConfig) -> R<DeployEnv> {
    let cfg = cx.config()?;
    Ok(DeployEnv::new(cx.paths, &cfg.data_dir, &game.domain))
}

/// The listing `clean-backups` shows.
pub fn dry_run_msgs(game: &GameConfig, entries: &[PoisonedBackup]) -> Vec<Msg> {
    if entries.is_empty() {
        return vec![Msg::success(format!(
            "No poisoned backups for {}: every held backup differs from the mod files at its path.",
            game.name
        ))];
    }
    let confirmed = entries
        .iter()
        .filter(|e| e.class == PoisonClass::Confirmed)
        .count();
    let mut v = vec![Msg::warn(format!(
        "{} held backup(s) of {} contain mod files instead of game originals: {confirmed} confirmed, {} ambiguous.",
        entries.len(),
        game.name,
        entries.len() - confirmed
    ))];
    for e in entries.iter().take(MAX_LISTED) {
        let class = match e.class {
            PoisonClass::Confirmed => "confirmed",
            PoisonClass::Ambiguous => "ambiguous",
        };
        let kept = if e.deployed_by_enabled_mod {
            " (deployed; file stays)"
        } else {
            ""
        };
        v.push(
            Msg::line(format!("  {class:<9} "))
                .b(e.path.clone())
                .d(format!(" — {}{kept}", e.reason)),
        );
    }
    if entries.len() > MAX_LISTED {
        v.push(Msg::line(format!(
            "  ... and {} more",
            entries.len() - MAX_LISTED
        )));
    }
    v.push(Msg::info(
        "Nothing was changed. Quarantine them with: modman deploy clean-backups --apply [--include-ambiguous]",
    ));
    v
}

/// Lists the poisoned backups of `game` (no side effects).
pub fn dry_run(cx: &mut Cx<'_>, game: &GameConfig) -> R<Vec<PoisonedBackup>> {
    let entries = deploy::poisoned_backups(cx.store.conn(), game)?;
    for m in dry_run_msgs(game, &entries) {
        cx.ui.say(m);
    }
    Ok(entries)
}

/// Quarantines the poisoned backups (confirmed ones, plus ambiguous ones
/// with `include_ambiguous`). Fails with nothing changed unless
/// `confirmed`.
pub fn apply(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    include_ambiguous: bool,
    confirmed: bool,
) -> R<QuarantineReport> {
    let env = env_for(cx, game)?;
    let r = deploy::quarantine_poisoned(cx.store, game, &env, include_ambiguous, confirmed)?;
    match &r.id {
        None => cx.ui.info("Nothing to quarantine."),
        Some(id) => {
            cx.ui.say(
                Msg::success(format!(
                    "Quarantined {} backup(s) and {} game file(s) as ",
                    r.backups, r.game_files
                ))
                .b(id.clone())
                .p("."),
            );
            if let Some(dir) = &r.dir {
                cx.ui.info(&format!("Kept in {}", dir.display()));
            }
            cx.ui
                .say(Msg::info("Undo with: ").b(format!("modman deploy restore-quarantine {id}")));
        }
    }
    if r.skipped_ambiguous > 0 {
        cx.ui.info(&format!(
            "{} ambiguous backup(s) left alone (add --include-ambiguous to include them).",
            r.skipped_ambiguous
        ));
    }
    Ok(r)
}

/// Puts quarantine `id` back. Fails with nothing changed unless
/// `confirmed`.
pub fn restore_quarantine(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    id: &str,
    confirmed: bool,
) -> R<RestoreReport> {
    let env = env_for(cx, game)?;
    let r = deploy::restore_quarantine(cx.store, game, &env, id, confirmed)?;
    cx.ui.success(&format!(
        "Restored quarantine {id}: {} backup(s), {} game file(s).",
        r.backups, r.game_files
    ));
    Ok(r)
}

/// Quarantines of `game`, oldest first.
pub fn list_quarantines(cx: &Cx<'_>, game: &GameConfig) -> R<Vec<QuarantineManifest>> {
    Ok(deploy::list_quarantines(&env_for(cx, game)?)?)
}
