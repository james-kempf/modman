// SPDX-License-Identifier: GPL-3.0-or-later
//! Changes of the mod set with their messages and the auto-deploy step:
//! enable/disable, order, uninstall, profiles (legacy `cmd_enable` ..
//! `cmd_profile`, L3397-3947), plus the read-only mod views.

use serde::Serialize;

use super::deploy::{after_change, AfterChange};
use super::install::AfterChangeView;
use super::{fail, staging_lock, Cx, Msg, R};
use crate::deploy;
use crate::fomod::FomodChoices;
use crate::games::GameConfig;
use crate::mods::{self, ModRecord, ModsError, OrderSpec, ProfileLoad};

/// Enables or disables mods (row ids), then auto-deploys or hints.
pub fn set_enabled(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    ids: &[i64],
    enable: bool,
) -> R<AfterChange> {
    mods::set_enabled(cx.store, &game.domain, ids, enable)?;
    let word = if enable { "Enabled" } else { "Disabled" };
    if ids.len() <= 10 {
        let all = mods::list_mods(cx.store.conn(), &game.domain)?;
        for m in all.iter().filter(|m| ids.contains(&m.id)) {
            cx.ui.success(&format!("{word}: {}", m.uid));
        }
    } else {
        cx.ui.success(&format!("{word} {} mods.", ids.len()));
    }
    after_change(cx, game)
}

/// Result of [`move_mod`].
#[derive(Debug, Clone, Serialize)]
pub struct MoveResult {
    pub uid: String,
    pub from: usize,
    pub to: usize,
    pub of: usize,
    pub after: AfterChangeView,
}

/// `order <mod> <spec> [arg]`: #N, first, last, up [n], down [n],
/// before <mod>, after <mod>.
pub fn move_mod(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    reference: &str,
    spec: &str,
    arg: Option<&str>,
) -> R<MoveResult> {
    let spec = OrderSpec::parse(spec, arg)?;
    let (m, mv) = mods::move_mod(cx.store, &game.domain, reference, &spec)?;
    cx.ui.success(&format!(
        "{}: #{} → #{}  (of {})",
        m.uid, mv.from, mv.to, mv.of
    ));
    let after = after_change(cx, game)?;
    Ok(MoveResult {
        uid: m.uid,
        from: mv.from,
        to: mv.to,
        of: mv.of,
        after: after.into(),
    })
}

/// Result of [`uninstall`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct UninstallResult {
    /// `false` when the user declined.
    pub done: bool,
    pub mods: usize,
    pub archives: Vec<std::path::PathBuf>,
    pub after: AfterChangeView,
}

/// Uninstalls mods (references: #position, uid, name, ...) after a
/// confirmation; `delete_archive` also deletes unused archives.
pub fn uninstall(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    refs: &[String],
    delete_archive: bool,
) -> R<UninstallResult> {
    let all = mods::list_mods(cx.store.conn(), &game.domain)?;
    let mut targets: Vec<&ModRecord> = Vec::new();
    for r in refs {
        let m = mods::resolve_in(&all, r)?;
        if let Some(found) = all.iter().find(|x| x.id == m.id) {
            if !targets.iter().any(|t| t.id == found.id) {
                targets.push(found);
            }
        }
    }
    for t in &targets {
        cx.ui.say(Msg::line("  ").y("−").p(format!(" {}", t.uid)));
    }
    if !cx
        .ui
        .confirm(&format!("Uninstall {} mod(s)?", targets.len()), true)
    {
        cx.ui.info("Cancelled.");
        return Ok(UninstallResult::default());
    }
    let ids: Vec<i64> = targets.iter().map(|t| t.id).collect();
    let dl = mods::downloads_dir(&cx.config()?.data_dir, &game.domain);
    let lock = staging_lock(cx.paths)?;
    let r = mods::remove_mods(
        cx.store,
        &game.domain,
        &ids,
        delete_archive.then_some(dl.as_path()),
    )?;
    drop(lock);
    for a in &r.archives {
        cx.ui.info(&format!("Deleted archive {}", a.display()));
    }
    cx.ui.success(&format!("Uninstalled {} mod(s).", r.mods));
    let after = after_change(cx, game)?;
    Ok(UninstallResult {
        done: true,
        mods: r.mods,
        archives: r.archives,
        after: after.into(),
    })
}

/// Saves the current order and enabled state; asks before overwriting.
/// `None` when the user declined.
pub fn save_profile(cx: &mut Cx<'_>, game: &GameConfig, name: &str) -> R<Option<usize>> {
    if !mods::valid_profile_name(name) {
        return Err(ModsError::InvalidProfileName(name.to_string()).into());
    }
    let exists = mods::list_profiles(cx.store, &game.domain)?
        .iter()
        .any(|p| p.name == name);
    if exists && !cx.ui.confirm(&format!("Overwrite profile '{name}'?"), true) {
        return Ok(None);
    }
    let n = mods::save_profile(cx.store, &game.domain, name)?;
    cx.ui
        .success(&format!("Saved profile '{name}' ({n} mods)."));
    Ok(Some(n))
}

/// Result of [`load_profile`].
#[derive(Debug, Clone, Serialize)]
pub struct ProfileLoaded {
    #[serde(flatten)]
    pub load: ProfileLoad,
    pub after: AfterChangeView,
}

/// Restores a saved profile, then auto-deploys or hints.
pub fn load_profile(cx: &mut Cx<'_>, game: &GameConfig, name: &str) -> R<ProfileLoaded> {
    let r = mods::load_profile(cx.store, &game.domain, name)?;
    cx.ui.success(&format!("Loaded profile '{name}'."));
    if r.missing > 0 {
        cx.ui.warn(&format!(
            "{} mod(s) from the profile are no longer installed.",
            r.missing
        ));
    }
    let after = after_change(cx, game)?;
    Ok(ProfileLoaded {
        load: r,
        after: after.into(),
    })
}

// ── views ───────────────────────────────────────────────────

/// An installed mod with its collections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModView {
    #[serde(flatten)]
    pub record: ModRecord,
    pub collections: Vec<String>,
}

/// The mod list of a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModsView {
    /// Load order, lowest priority first.
    pub mods: Vec<ModView>,
    /// Mods changed since the last deploy while a deployment is active.
    pub changed_since_deploy: bool,
}

pub fn mods_view(cx: &Cx<'_>, game: &GameConfig) -> R<ModsView> {
    let mut v = Vec::new();
    for m in mods::list_mods(cx.store.conn(), &game.domain)? {
        v.push(ModView {
            collections: mods::collections_of(cx.store.conn(), m.id)?,
            record: m,
        });
    }
    let st = deploy::status(cx.store.conn(), &game.domain)?;
    let changed = st.active && mods::changed_since(cx.store, &game.domain, st.last_deployed_at)?;
    Ok(ModsView {
        mods: v,
        changed_since_deploy: changed,
    })
}

/// One FOMOD step and the options picked in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FomodStepView {
    pub step: String,
    pub options: Vec<String>,
}

/// Everything the mod detail view shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModDetail {
    #[serde(rename = "mod")]
    pub record: ModRecord,
    pub collections: Vec<String>,
    /// Recorded FOMOD choices, per step.
    pub fomod: Vec<FomodStepView>,
    /// Files this mod wins / loses (enabled mods only).
    pub wins: usize,
    pub loses: usize,
    /// Nexus page.
    pub url: Option<String>,
}

/// The mod's Nexus Mods page (its files tab, on the mod's file when known);
/// `None` for a mod that did not come from Nexus.
fn nexus_page_url(domain: &str, m: &ModRecord) -> Option<String> {
    m.nexus_mod_id.map(|mi| {
        format!(
            "https://www.nexusmods.com/{domain}/mods/{mi}?tab=files{}",
            m.nexus_file_id
                .map_or(String::new(), |f| format!("&file_id={f}"))
        )
    })
}

/// Opens the mod's Nexus page with `open`, which gets the URL built here
/// (never a URL from the webview). Returns the URL; a mod with no Nexus id
/// has no page to open.
pub fn open_mod_page(
    cx: &Cx<'_>,
    game: &GameConfig,
    reference: &str,
    open: impl FnOnce(&str) -> R<()>,
) -> R<String> {
    let m = mods::resolve(cx.store.conn(), &game.domain, reference)?;
    let url = match nexus_page_url(&game.domain, &m) {
        Some(url) => url,
        None => fail!("{} has no Nexus Mods page.", m.name),
    };
    open(&url)?;
    Ok(url)
}

pub fn mod_detail(cx: &Cx<'_>, game: &GameConfig, reference: &str) -> R<ModDetail> {
    let m = mods::resolve(cx.store.conn(), &game.domain, reference)?;
    let fomod = mods::fomod_choices(cx.store.conn(), m.id)?
        .and_then(|raw| FomodChoices::from_json(&raw))
        .map(|ch| {
            ch.summary()
                .into_iter()
                .map(|(step, options)| FomodStepView { step, options })
                .collect()
        })
        .unwrap_or_default();
    let (mut wins, mut loses) = (0, 0);
    if m.enabled {
        for c in deploy::conflicts(cx.store.conn(), game)? {
            if c.winner.id == m.id {
                wins += 1;
            } else if c.losers.iter().any(|l| l.id == m.id) {
                loses += 1;
            }
        }
    }
    let url = nexus_page_url(&game.domain, &m);
    Ok(ModDetail {
        collections: mods::collections_of(cx.store.conn(), m.id)?,
        record: m,
        fomod,
        wins,
        loses,
        url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::Recorder;
    use crate::paths::Paths;
    use crate::store::Store;

    #[test]
    fn profile_names_are_validated_before_saving() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::for_home(home.path());
        let mut store = Store::open_in_memory().unwrap();
        let ui = Recorder::default();
        let mut cx = Cx::new(&paths, &mut store, &ui);
        let g = GameConfig::custom("g");
        assert!(save_profile(&mut cx, &g, "../x").is_err());
    }
}
