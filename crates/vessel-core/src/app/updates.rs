// SPDX-License-Identifier: GPL-3.0-or-later
//! Requirement reports (legacy `deps_report` L3183-3226, ack/unack) and the
//! update check/install (legacy `cmd_updates` L3760-3830).

use serde::Serialize;

use super::nexus::{self as nx, FileRequest};
use super::{chain, fail, staging_lock, trunc, Cx, Msg, Ui, R};
use crate::fomod::FomodMode;
use crate::games::GameConfig;
use crate::mods::{self, InstallMeta, ModRecord};
use crate::nexus::{latest_file, refresh_requirements, FileInfo};
use crate::requirements::{self, ReqRow, ReqStatus, Scope};

/// Updates the requirements cache when logged in. Never fails the caller;
/// `quiet` logs problems instead of showing them.
pub fn refresh_deps(cx: &Cx<'_>, game: &GameConfig, ids: &[u64], refresh: bool, quiet: bool) {
    let res = (|| -> R<bool> {
        let Some(nexus) =
            nx::connect(cx.paths, cx.store, false)?.filter(|n| n.client.is_authenticated())
        else {
            return Ok(false);
        };
        let mut g = game.clone();
        let gid = nx::game_id(cx.store, &nexus, &mut g)?;
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
        Ok(false) if refresh && !quiet => cx
            .ui
            .warn("Not logged in; showing cached requirements only. Run: vessel login"),
        Ok(false) => {}
        Err(e) if quiet => tracing::warn!("requirement lookup failed: {}", chain(&e)),
        Err(e) => cx
            .ui
            .warn(&format!("Could not update requirements: {}", chain(&e))),
    }
}

/// Legacy `deps_report` in `full` (every row) or `problems` mode.
pub fn report_msgs(rows: &[ReqRow], problems: bool, title: &str) -> Vec<Msg> {
    let mut v = Vec::new();
    if rows.is_empty() {
        if !problems {
            v.push(Msg::blank());
            v.push(Msg::info(format!("{title}: none listed on Nexus.")));
        }
        return v;
    }
    let counts = requirements::counts(rows);
    let n = |s: ReqStatus| counts.get(&s).copied().unwrap_or(0);
    let (n_ok, n_ext) = (n(ReqStatus::Ok), n(ReqStatus::External));
    let n_bad = n(ReqStatus::Missing) + n(ReqStatus::Disabled);
    if problems && n_bad + n_ext == 0 {
        if n_ok > 0 {
            v.push(Msg::success(format!(
                "All {n_ok} requirement(s) of the installed mods are present."
            )));
        }
        return v;
    }
    v.push(Msg::heading(title));
    for r in rows {
        if problems && matches!(r.status, ReqStatus::Ok | ReqStatus::Gone) {
            continue;
        }
        v.push(match r.status {
            ReqStatus::Ok => Msg::line("  ")
                .g("✓")
                .p(" ")
                .b(&r.name)
                .p("  ")
                .d(&r.detail),
            ReqStatus::Missing => Msg::line("  ✗ ").b(&r.name).p(format!(
                "  missing{}",
                if r.detail.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", r.detail)
                }
            )),
            ReqStatus::Disabled => Msg::line("  ")
                .y("✗")
                .p(" ")
                .b(&r.name)
                .p("  ")
                .y("installed but disabled"),
            ReqStatus::External => Msg::line("  ")
                .y("?")
                .p(" ")
                .b(&r.name)
                .p("  ")
                .y("external — install it yourself"),
            ReqStatus::Gone => Msg::line("  ").d(format!("· {}  ({})", r.name, r.detail)),
        });
        if matches!(r.status, ReqStatus::Ok | ReqStatus::Gone) {
            continue;
        }
        if !r.by.is_empty() {
            let label = if r.by.len() > 1 {
                format!("needed by {} mods", r.by.len())
            } else {
                "needed by".into()
            };
            v.push(Msg::line("      ").d(format!("{label}: {}", trunc(&r.by.join("; "), 100))));
        }
        if !r.hint.is_empty() {
            v.push(Msg::line(format!("      → vessel {}", r.hint)));
        }
        for u in &r.urls {
            v.push(Msg::line(format!("      → {u}")));
        }
    }
    if n_bad + n_ext > 0 {
        v.push(Msg::blank());
        if n_bad > 0 {
            v.push(Msg::warn(format!(
                "{n_bad} requirement(s) missing — the mods that need them will not work."
            )));
        }
        if n_ext > 0 {
            v.push(Msg::info("External requirements cannot be checked automatically. Once installed: vessel deps ack <name>"));
        }
        if problems && n_ok > 0 {
            v.push(Msg::line("  ").d(format!(
                "{n_ok} other requirement(s) present — full list: vessel deps"
            )));
        }
    }
    v
}

fn say_all(ui: &dyn Ui, msgs: Vec<Msg>) {
    for m in msgs {
        ui.say(m);
    }
}

/// After an install: refresh and show the mod's requirements.
pub fn requirements_after_install(cx: &Cx<'_>, game: &GameConfig, mod_row: i64) {
    let Ok(Some(m)) = mods::get_mod(cx.store.conn(), &game.domain, mod_row) else {
        return;
    };
    let Some(mid) = m.nexus_mod_id else { return };
    refresh_deps(cx, game, &[mid], false, true);
    if let Ok(rows) = requirements::analyze(
        cx.store,
        game,
        &Scope {
            mods: Some(vec![m.id]),
        },
    ) {
        say_all(cx.ui, report_msgs(&rows, false, "Requirements"));
    }
}

/// After a collection install: refresh and show the problems among the
/// collection's requirements, including its external resources (legacy
/// L2547-2555).
pub fn requirements_after_collection(cx: &Cx<'_>, game: &GameConfig, ids: &[i64]) {
    if ids.is_empty() {
        return;
    }
    let nexus_ids: Vec<u64> = mods::list_mods(cx.store.conn(), &game.domain)
        .unwrap_or_default()
        .iter()
        .filter(|m| ids.contains(&m.id))
        .filter_map(|m| m.nexus_mod_id)
        .collect();
    if !nexus_ids.is_empty() {
        refresh_deps(cx, game, &nexus_ids, false, true);
    }
    if let Ok(rows) = requirements::analyze(
        cx.store,
        game,
        &Scope {
            mods: Some(ids.to_vec()),
        },
    ) {
        say_all(
            cx.ui,
            report_msgs(&rows, true, "Requirements not included in the collection"),
        );
    }
}

/// Requirement rows of `game`: all enabled mods, or one mod (`mod_ref`).
/// Refreshes the cache first when logged in.
pub fn deps_rows(
    cx: &Cx<'_>,
    game: &GameConfig,
    mod_ref: Option<&str>,
    refresh: bool,
) -> R<Vec<ReqRow>> {
    let all = mods::list_mods(cx.store.conn(), &game.domain)?;
    if let Some(r) = mod_ref {
        let m = mods::resolve_in(&all, r)?;
        let Some(mid) = m.nexus_mod_id else {
            fail!(
                "{} is not a Nexus mod — its requirements are unknown.",
                m.uid
            );
        };
        refresh_deps(cx, game, &[mid], refresh, true);
        return Ok(requirements::analyze(
            cx.store,
            game,
            &Scope {
                mods: Some(vec![m.id]),
            },
        )?);
    }
    let ids: Vec<u64> = all
        .iter()
        .filter(|m| m.enabled)
        .filter_map(|m| m.nexus_mod_id)
        .collect();
    if !ids.is_empty() {
        refresh_deps(cx, game, &ids, refresh, true);
    }
    Ok(requirements::analyze(cx.store, game, &Scope::default())?)
}

/// `deps ack|unack <name>`: marks an external requirement as installed (or
/// checks it again). Returns the matched row.
pub fn set_ack(
    cx: &Cx<'_>,
    game: &GameConfig,
    reference: &str,
    ack: bool,
    refresh: bool,
) -> R<ReqRow> {
    let sub = if ack { "ack" } else { "unack" };
    if reference.trim().is_empty() {
        fail!("Usage: vessel deps {sub} <requirement name>");
    }
    let all = mods::list_mods(cx.store.conn(), &game.domain)?;
    let ids: Vec<u64> = all.iter().filter_map(|m| m.nexus_mod_id).collect();
    if !ids.is_empty() {
        refresh_deps(cx, game, &ids, refresh, true);
    }
    let rows = requirements::analyze(cx.store, game, &Scope::default())?;
    let row = match requirements::find_ack_target(&rows, reference, !ack) {
        Ok(r) => r.clone(),
        Err(0) => fail!("No requirement matches '{reference}'. See: vessel deps"),
        Err(n) => {
            fail!("'{reference}' matches {n} requirements — be more specific. See: vessel deps")
        }
    };
    if ack {
        requirements::ack(cx.store.conn(), &game.domain, &row.key)?;
    } else {
        requirements::unack(cx.store.conn(), &game.domain, &row.key)?;
    }
    Ok(row)
}

// ── updates ─────────────────────────────────────────────────

/// An installed mod with a newer file on Nexus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Update {
    #[serde(rename = "mod")]
    pub installed: ModRecord,
    pub file: FileInfo,
    /// Pinned by a collection (`updatePolicy: exact`).
    pub pinned: bool,
}

/// Result of [`check`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UpdateReport {
    /// Distinct Nexus mods checked.
    pub checked: usize,
    /// File lists fetched from Nexus (the rest came from the cache).
    pub fetched: usize,
    /// The hourly quota ran low before every mod was checked.
    pub stopped_early: bool,
    pub updates: Vec<Update>,
    /// Updates installed by [`install`].
    pub installed: usize,
}

/// Checks the installed Nexus mods of `game` for newer files.
pub fn check(cx: &Cx<'_>, nexus: &nx::Nexus, game: &GameConfig, refresh: bool) -> R<UpdateReport> {
    let ui = cx.ui;
    ui.heading(&format!("Update check — {}", game.name));
    let all = mods::list_mods(cx.store.conn(), &game.domain)?;
    let mut ids: Vec<u64> = all.iter().filter_map(|m| m.nexus_mod_id).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        ui.info("No Nexus mods installed.");
        return Ok(UpdateReport::default());
    }
    let check = nexus.run(crate::nexus::check_updates(
        &nexus.client,
        &nexus.shared,
        &game.domain,
        &ids,
        refresh,
    ))??;
    if check.stopped_early {
        ui.warn("Hourly API limit almost used up — stopping early (re-run later).");
    }
    ui.info(&format!(
        "Checked {} mods ({} refreshed from Nexus).",
        ids.len(),
        check.fetched
    ));
    let mut report = UpdateReport {
        checked: ids.len(),
        fetched: check.fetched,
        stopped_early: check.stopped_early,
        ..Default::default()
    };
    for m in &all {
        let (Some(mid), Some(fid)) = (m.nexus_mod_id, m.nexus_file_id) else {
            continue;
        };
        let Some(files) = check.files.get(&mid) else {
            continue;
        };
        if let Some(newer) = latest_file(files, fid) {
            report.updates.push(Update {
                installed: m.clone(),
                file: newer.clone(),
                pinned: m.update_policy.as_deref() == Some("exact"),
            });
        }
    }
    if report.updates.is_empty() {
        ui.success("All mods are up to date.");
        return Ok(report);
    }
    ui.blank();
    for u in &report.updates {
        let (m, f) = (&u.installed, &u.file);
        let mut line = Msg::line("  ").y("↑").p(format!(
            " {:<40} {} → ",
            trunc(&m.name, 40),
            m.version.as_deref().unwrap_or("?")
        ));
        line = line
            .g(f.version.as_deref().unwrap_or("?"))
            .p("  ")
            .d(trunc(&f.name, 40));
        if u.pinned {
            line = line.d(" (pinned by collection)");
        }
        ui.say(line);
    }
    ui.blank();
    let pinned = report.updates.iter().filter(|u| u.pinned).count();
    ui.info(&format!(
        "{} update(s) available{}.",
        report.updates.len(),
        if pinned > 0 {
            format!(", {pinned} pinned by a collection (use --all to include)")
        } else {
            String::new()
        }
    ));
    Ok(report)
}

/// Downloads and installs the updates of `report` (pinned ones only with
/// `all`), reusing the recorded FOMOD choices (legacy L3800-3802).
pub fn install(
    cx: &mut Cx<'_>,
    nexus: &nx::Nexus,
    game: &GameConfig,
    report: &mut UpdateReport,
    all: bool,
) -> R<()> {
    let data_dir = cx.config()?.data_dir;
    let mut done = 0;
    for u in &report.updates {
        let (m, f) = (&u.installed, &u.file);
        if u.pinned && !all {
            continue;
        }
        cx.ui.say(Msg::line("\n  Updating ").b(&m.name).p(format!(
            " {} → {}",
            m.version.as_deref().unwrap_or("?"),
            f.version.as_deref().unwrap_or("?")
        )));
        let mid = m.nexus_mod_id.unwrap_or_default();
        let (archive, md5) = match nx::download_file(
            cx,
            nexus,
            FileRequest {
                game: &game.domain,
                mod_id: mid,
                file_id: f.file_id,
                info: Some(f),
                nxm: None,
                row: None,
            },
        ) {
            Ok(r) => r,
            Err(e) => {
                cx.ui
                    .error(&format!("Download failed for {}: {}", m.name, chain(&e)));
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
            fomod_mode: FomodMode::Reuse,
            fomod_choices: mods::recorded_fomod_choices(cx.store.conn(), m.id)?,
        };
        let lock = staging_lock(cx.paths)?;
        let r = super::install::install_archive(cx, game, &data_dir, &archive, &meta, false);
        drop(lock);
        match r {
            Ok(o) => {
                cx.ui.success(&format!("Updated → {}", o.uid));
                done += 1;
            }
            Err(e) => cx
                .ui
                .error(&format!("Update of {} failed: {}", m.name, chain(&e))),
        }
    }
    cx.ui.blank();
    cx.ui.success(&format!("{done} mod(s) updated."));
    report.installed = done;
    Ok(())
}

/// `updates [--install] [--all] [--refresh]`.
pub fn run(
    cx: &mut Cx<'_>,
    game: &GameConfig,
    install_them: bool,
    all: bool,
    refresh: bool,
) -> R<UpdateReport> {
    let nexus = nx::require(cx.paths, cx.store)?;
    let mut report = check(cx, &nexus, game, refresh)?;
    if report.updates.is_empty() {
        return Ok(report);
    }
    if !install_them {
        cx.ui.info("Install them with: vessel updates --install");
        return Ok(report);
    }
    install(cx, &nexus, game, &mut report, all)?;
    super::deploy::after_change(cx, game)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(status: ReqStatus, name: &str) -> ReqRow {
        ReqRow {
            status,
            key: format!("k:{name}"),
            name: name.into(),
            hint: String::new(),
            urls: vec![],
            by: vec!["A".into()],
            detail: String::new(),
            acked: false,
        }
    }

    #[test]
    fn problem_reports_hide_present_requirements() {
        let rows = [row(ReqStatus::Ok, "ok"), row(ReqStatus::Missing, "gone")];
        let t: Vec<String> = report_msgs(&rows, true, "T")
            .iter()
            .map(Msg::text)
            .collect();
        assert!(t.iter().any(|l| l == "  ✗ gone  missing"));
        assert!(!t.iter().any(|l| l.contains(" ok")));
        assert!(report_msgs(&[row(ReqStatus::Ok, "ok")], true, "T")[0]
            .text()
            .starts_with("All 1 requirement(s)"));
    }
}
