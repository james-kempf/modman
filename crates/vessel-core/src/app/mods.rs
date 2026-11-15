// SPDX-License-Identifier: GPL-3.0-or-later
//! Changes of the mod set with their messages and the auto-deploy step:
//! enable/disable, order, uninstall, profiles (legacy `cmd_enable` ..
//! `cmd_profile`, L3397-3947), plus the read-only mod views.

use serde::Serialize;

use super::deploy::{after_change, AfterChange};
use super::install::AfterChangeView;
use super::restore::{self, RestoreEntry, RestoreStore};
use super::{fail, staging_lock, ConfirmSpec, Cx, Msg, R};
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
    mods::set_enabled(cx.store, &game.id, ids, enable)?;
    let word = if enable { "Enabled" } else { "Disabled" };
    if ids.len() <= 10 {
        let all = mods::list_mods(cx.store.conn(), &game.id)?;
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
    let (m, mv) = mods::move_mod(cx.store, &game.id, reference, &spec)?;
    // Nothing moved (already first/last or same slot): no success, no deploy.
    let after = if mv.from == mv.to {
        let at = match mv.to {
            1 => "already first".to_string(),
            t if t == mv.of => "already last".to_string(),
            t => format!("already #{t}"),
        };
        cx.ui.info(&format!("{}: {at}", m.uid));
        AfterChangeView::default()
    } else {
        cx.ui.success(&format!(
            "{}: #{} → #{}  (of {})",
            m.uid, mv.from, mv.to, mv.of
        ));
        after_change(cx, game)?.into()
    };
    Ok(MoveResult {
        uid: m.uid,
        from: mv.from,
        to: mv.to,
        of: mv.of,
        after,
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
    /// One entry per uninstalled mod when restore snapshots were captured;
    /// left out (not `[]`) otherwise, so the legacy shape is unchanged.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub restore: Vec<RestoreEntry>,
}

/// Uninstalls mods (references: #position, uid, name, ...) after a
/// confirmation; `delete_archive` also deletes unused archives. With
/// `capture_restore` (GUI only), each mod is snapshotted into that store
/// first and listed in [`UninstallResult::restore`].
pub fn uninstall(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    refs: &[String],
    delete_archive: bool,
    capture_restore: Option<&RestoreStore>,
) -> R<UninstallResult> {
    let all = mods::list_mods(cx.store.conn(), &game.id)?;
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
    let answer = cx.ui.confirm_with(&uninstall_confirm(&targets));
    if !answer.yes {
        cx.ui.info("Cancelled.");
        return Ok(UninstallResult::default());
    }
    let delete_archive = delete_archive || answer.option_checked;
    let ids: Vec<i64> = targets.iter().map(|t| t.id).collect();
    let dl = mods::downloads_dir(&cx.config()?.data_dir, &game.id);
    let snaps = match capture_restore {
        Some(_) => restore::capture(cx, &targets)?,
        None => Vec::new(),
    };
    let lock = staging_lock(cx.paths)?;
    let r = mods::remove_mods(
        cx.store,
        &game.id,
        &ids,
        delete_archive.then_some(dl.as_path()),
    )?;
    drop(lock);
    let restore = match capture_restore {
        Some(store) => restore::keep(store, &game.id, snaps, &r.archives, &dl)?,
        None => Vec::new(),
    };
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
        restore,
    })
}

/// The uninstall question. `default_yes` stays true so the CLI (Enter, no
/// tty) and [`crate::app::Headless`] keep proceeding; `danger` only moves
/// GUI focus to the "keep" button.
fn uninstall_confirm(targets: &[&ModRecord]) -> ConfirmSpec {
    let (title, yes_label, no_label) = match targets {
        [one] => (
            format!("Uninstall {}?", one.name),
            format!("Uninstall {}", one.name),
            "Keep mod".to_string(),
        ),
        _ => (
            format!("Uninstall {} mods?", targets.len()),
            format!("Uninstall {} mods", targets.len()),
            "Keep mods".to_string(),
        ),
    };
    ConfirmSpec {
        question: format!("Uninstall {} mod(s)?", targets.len()),
        title: Some(title),
        yes_label: Some(yes_label),
        no_label: Some(no_label),
        danger: true,
        default_yes: true,
        option: Some("Delete archive too".into()),
    }
}

/// Saves the current order and enabled state; asks before overwriting.
/// `None` when the user declined.
pub fn save_profile(cx: &mut Cx<'_>, game: &GameConfig, name: &str) -> R<Option<usize>> {
    if !mods::valid_profile_name(name) {
        return Err(ModsError::InvalidProfileName(name.to_string()).into());
    }
    let exists = mods::list_profiles(cx.store, &game.id)?
        .iter()
        .any(|p| p.name == name);
    if exists && !cx.ui.confirm(&format!("Overwrite profile '{name}'?"), true) {
        return Ok(None);
    }
    let n = mods::save_profile(cx.store, &game.id, name)?;
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
    let r = mods::load_profile(cx.store, &game.id, name)?;
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
    /// Mods installed as a loader (e.g. SMAPI); they have no mods row.
    pub loaders: Vec<super::loaders::InstalledLoader>,
}

pub fn mods_view(cx: &Cx<'_>, game: &GameConfig) -> R<ModsView> {
    let mut v = Vec::new();
    for m in mods::list_mods(cx.store.conn(), &game.id)? {
        v.push(ModView {
            collections: mods::collections_of(cx.store.conn(), m.id)?,
            record: m,
        });
    }
    let st = deploy::status(cx.store.conn(), &game.id)?;
    let changed = st.active && mods::changed_since(cx.store, &game.id, st.last_deployed_at)?;
    let defs = cx.paths.definitions_dir();
    Ok(ModsView {
        mods: v,
        changed_since_deploy: changed,
        loaders: super::loaders::installed_loaders(Some(&defs), cx.store, game)?,
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
/// `None` for a mod that did not come from Nexus or a game not linked to it.
fn nexus_page_url(game: &GameConfig, m: &ModRecord) -> Option<String> {
    let domain = game.nexus_domain.as_deref()?;
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
    let m = mods::resolve(cx.store.conn(), &game.id, reference)?;
    let url = match nexus_page_url(game, &m) {
        Some(url) => url,
        None => fail!("{} has no Nexus Mods page.", m.name),
    };
    open(&url)?;
    Ok(url)
}

pub fn mod_detail(cx: &Cx<'_>, game: &GameConfig, reference: &str) -> R<ModDetail> {
    let m = mods::resolve(cx.store.conn(), &game.id, reference)?;
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
    let url = nexus_page_url(game, &m);
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
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    use crate::app::testing::Recorder;
    use crate::app::{ConfirmAnswer, Headless, Ui};
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

    /// Records every [`ConfirmSpec`]; answers `yes` with `option_checked`.
    struct Asker {
        specs: RefCell<Vec<ConfirmSpec>>,
        yes: bool,
        option_checked: bool,
    }

    impl Ui for Asker {
        fn say(&self, _msg: Msg) {}
        fn confirm_with(&self, spec: &ConfirmSpec) -> ConfirmAnswer {
            self.specs.borrow_mut().push(spec.clone());
            ConfirmAnswer {
                yes: self.yes,
                option_checked: self.option_checked,
            }
        }
    }

    /// Game "g" with mods a and b, each with its own archive in downloads.
    fn setup(home: &Path, store: &mut Store) -> (Paths, GameConfig, Vec<PathBuf>) {
        let paths = Paths::for_home(home);
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        crate::games::save_game(store.conn(), &GameConfig::custom("g")).unwrap();
        let cfg = crate::config::Config::load(store, &paths).unwrap();
        let dl = mods::downloads_dir(&cfg.data_dir, "g");
        std::fs::create_dir_all(&dl).unwrap();
        let mut archives = Vec::new();
        for (uid, name) in [("a", "Alpha"), ("b", "Beta")] {
            let id = crate::mods::tests::add(store, "g", uid, name, None);
            let arch = dl.join(format!("{uid}.zip"));
            std::fs::write(&arch, "x").unwrap();
            store
                .conn()
                .execute(
                    "UPDATE mods SET archive = ?1 WHERE id = ?2",
                    rusqlite::params![arch.to_string_lossy(), id],
                )
                .unwrap();
            archives.push(arch);
        }
        let g = crate::games::get_game(store.conn(), "g").unwrap().unwrap();
        (paths, g, archives)
    }

    fn left(store: &Store) -> usize {
        mods::list_mods(store.conn(), "g").unwrap().len()
    }

    // Fails when the uninstall prompt loses its title/labels/danger/option,
    // or when default_yes flips (which would make CLI Enter and Headless cancel).
    #[test]
    fn uninstall_asks_a_named_danger_confirm_with_an_archive_option() {
        let home = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory().unwrap();
        let (paths, g, _) = setup(home.path(), &mut store);
        let ui = Asker {
            specs: RefCell::default(),
            yes: false,
            option_checked: false,
        };
        let mut cx = Cx::new(&paths, &mut store, &ui);
        // Declined, so both questions are asked on the same mod set.
        let r = uninstall(&mut cx, &g, &["a".into()], false, None).unwrap();
        assert!(!r.done);
        uninstall(&mut cx, &g, &["a".into(), "b".into()], false, None).unwrap();
        assert_eq!(left(&store), 2);
        let specs = ui.specs.borrow();
        assert_eq!(
            specs[0],
            ConfirmSpec {
                question: "Uninstall 1 mod(s)?".into(),
                title: Some("Uninstall Alpha?".into()),
                yes_label: Some("Uninstall Alpha".into()),
                no_label: Some("Keep mod".into()),
                danger: true,
                default_yes: true,
                option: Some("Delete archive too".into()),
            }
        );
        assert_eq!(
            specs[1],
            ConfirmSpec {
                question: "Uninstall 2 mod(s)?".into(),
                title: Some("Uninstall 2 mods?".into()),
                yes_label: Some("Uninstall 2 mods".into()),
                no_label: Some("Keep mods".into()),
                danger: true,
                default_yes: true,
                option: Some("Delete archive too".into()),
            }
        );
    }

    // Fails when Headless (and so the CLI without a tty) stops uninstalling,
    // or when the archive is deleted without delete_archive or the option.
    #[test]
    fn headless_uninstall_proceeds_and_keeps_archives() {
        let home = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory().unwrap();
        let (paths, g, archives) = setup(home.path(), &mut store);
        let mut cx = Cx::new(&paths, &mut store, &Headless);
        let r = uninstall(&mut cx, &g, &["a".into(), "b".into()], false, None).unwrap();
        assert!(r.done);
        assert_eq!(r.mods, 2);
        assert!(r.archives.is_empty());
        assert_eq!(left(&store), 0);
        assert!(archives.iter().all(|a| a.exists()));
    }

    // Fails when a ticked "Delete archive too" is ignored by the backend.
    #[test]
    fn a_checked_option_deletes_and_reports_the_archives() {
        let home = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory().unwrap();
        let (paths, g, archives) = setup(home.path(), &mut store);
        let ui = Asker {
            specs: RefCell::default(),
            yes: true,
            option_checked: true,
        };
        let mut cx = Cx::new(&paths, &mut store, &ui);
        let r = uninstall(&mut cx, &g, &["a".into(), "b".into()], false, None).unwrap();
        assert!(r.done);
        assert_eq!(r.archives, archives);
        assert!(archives.iter().all(|a| !a.exists()));
        assert_eq!(left(&store), 0);
    }
}
