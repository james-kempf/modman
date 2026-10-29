// SPDX-License-Identifier: GPL-3.0-or-later
//! M7 auto-update: after a system (OS) update or a Steam update of a game,
//! install mod updates and redeploy every deployed game (legacy
//! `autoupdate_*`, L4381-4769; legacy_spec §15).
//!
//! A systemd user timer runs `vessel autoupdate run`. Each run only acts when
//! a fingerprint changed, so most runs end after a few `stat` calls. The
//! actual work (collection and mod updates, redeploy) goes through
//! [`Actions`], which the CLI implements over its commands; the gates, state
//! and the last-run log live here. Nothing here prints.

mod host;
mod state;
#[cfg(test)]
mod tests;
mod units;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

pub use host::{steam_state_in, Host, LiveHost, SteamState, SystemSnapshot, PACKAGE_MANAGERS};
pub use state::{last_runs, load_state, GameState, Outcome, RunLogEntry, LOG_KEEP};
pub use units::{
    disable, enable, install, render_service, render_timer, service_path, systemd_user_dir,
    timer_file_hours, timer_hours, timer_path, timer_status, Systemctl, TimerStatus, UnitSpec,
    UserSystemctl,
};

use crate::collections::{self, CollectionError};
use crate::config::Config;
use crate::games::{self, GameConfig, GamesError};
use crate::mods::{self, ModsError};
use crate::paths::Paths;
use crate::store::{DeployLock, Store, StoreError};

/// systemd unit name (legacy `AU_UNIT`).
pub const UNIT: &str = "vessel-autoupdate";
/// The package database must have been quiet this long (legacy `AU_SETTLE`).
pub const SETTLE_SECS: i64 = 300;
/// After a failure, retry at most this often (legacy `AU_RETRY`, 3 h).
pub const RETRY_SECS: i64 = 10_800;
/// Steam `StateFlags` of a fully installed app.
pub const STEAM_INSTALLED: u32 = 4;

#[derive(Debug, thiserror::Error)]
pub enum AutoupdateError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("systemd user services are not available here")]
    SystemdUnavailable,
    #[error(
        "auto-update is not supported inside the Flatpak; use the AppImage or a distro package"
    )]
    Flatpak,
    #[error("systemctl --user {args} failed: {message}")]
    Systemctl { args: String, message: String },
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Games(#[from] GamesError),
    #[error(transparent)]
    Mods(#[from] ModsError),
    #[error(transparent)]
    Collections(#[from] CollectionError),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}

type R<T> = Result<T, AutoupdateError>;

/// Marker file of a Flatpak sandbox, relative to the filesystem root.
pub const FLATPAK_INFO: &str = ".flatpak-info";

/// Whether this runs inside a Flatpak sandbox (`<root>/.flatpak-info`).
pub fn in_flatpak(root: &Path) -> bool {
    root.join(FLATPAK_INFO).exists()
}

/// Refuses inside a Flatpak: the sandbox cannot install a host systemd
/// user timer that would outlive it, so `enable`/`status`/`run` say so
/// instead of failing with the generic systemd message.
pub fn ensure_supported(root: &Path) -> R<()> {
    if in_flatpak(root) {
        return Err(AutoupdateError::Flatpak);
    }
    Ok(())
}

/// Nexus account tier (decides what a run may download).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Account {
    /// No API key: redeploy only.
    None,
    /// Free accounts cannot download without the website: check only.
    Free,
    Premium,
}

/// The work a run does when a game is due. Implemented by the CLI over its
/// commands; errors are messages for the log.
pub trait Actions {
    fn store(&mut self) -> &mut Store;
    fn account(&mut self) -> Account;
    /// `collection update <slug> --optional none`.
    fn update_collection(&mut self, game: &GameConfig, slug: &str) -> Result<(), String>;
    /// `updates --install`.
    fn install_mod_updates(&mut self, game: &GameConfig) -> Result<(), String>;
    /// `updates` (check only).
    fn check_mod_updates(&mut self, game: &GameConfig) -> Result<(), String>;
    /// Journaled redeploy (`deploy`), which refuses while the game runs.
    fn redeploy(&mut self, game: &GameConfig) -> Result<(), String>;
}

/// Run settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    pub force: bool,
    pub dry_run: bool,
    /// `autoupdate-collections`.
    pub collections: bool,
    /// `autoupdate-interval` (hours, 0 = only after updates).
    pub interval_hours: u32,
    /// Non-blocking run lock; a second concurrent run returns at once.
    pub lock_path: PathBuf,
}

impl RunOptions {
    pub fn new(cfg: &Config, paths: &Paths) -> Self {
        Self {
            force: false,
            dry_run: false,
            collections: cfg.autoupdate_collections,
            interval_hours: cfg.autoupdate_interval_hours,
            lock_path: paths.data_dir.join("autoupdate.lock"),
        }
    }
}

/// Why a game is left alone, in gate order (legacy_spec §15 steps 1-9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Skip {
    NotDeployed,
    /// First run: the current state was recorded instead of acted on.
    Baseline,
    NoReasons,
    /// Failed recently; retried after `retry_at`.
    BackOff {
        retry_at: i64,
    },
    FolderMissing,
    SteamUpdating {
        state_flags: u32,
    },
    GameRunning,
}

impl Skip {
    /// Due but blocked for now (logged as `skipped`, retried later).
    pub fn is_blocked(&self) -> bool {
        matches!(
            self,
            Skip::BackOff { .. }
                | Skip::FolderMissing
                | Skip::SteamUpdating { .. }
                | Skip::GameRunning
        )
    }

    pub fn describe(&self) -> String {
        match self {
            Skip::NotDeployed => "not deployed".into(),
            Skip::Baseline => "baseline recorded".into(),
            Skip::NoReasons => "nothing changed".into(),
            Skip::BackOff { .. } => "failed recently, waiting before retrying".into(),
            Skip::FolderMissing => "game folder not available, will retry".into(),
            Skip::SteamUpdating { state_flags } => {
                format!("Steam is still updating the game (StateFlags {state_flags}), will retry")
            }
            Skip::GameRunning => "the game is running, will retry".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    Skip(Skip),
    Act,
}

/// Inputs of one decision.
struct Facts<'a> {
    game: &'a GameConfig,
    state: &'a GameState,
    deployed: bool,
    snapshot: &'a SystemSnapshot,
    steam: Option<&'a SteamState>,
    now: i64,
}

/// Reasons and verdict for one game. Pure apart from the game-running probe.
fn decide(
    f: &Facts<'_>,
    host: &dyn Host,
    force: bool,
    interval_hours: u32,
) -> (Vec<String>, Verdict) {
    let mut reasons = Vec::new();
    if !f.deployed {
        return (reasons, Verdict::Skip(Skip::NotDeployed));
    }
    if f.state.system.as_deref() != Some(f.snapshot.fingerprint.as_str()) {
        reasons.push("system updated".to_string());
    }
    let build = f.steam.and_then(|s| s.buildid.as_deref());
    if let Some(b) = build {
        if f.state.build.as_deref() != Some(b) {
            reasons.push(format!("game updated (build {b})"));
        }
    }
    if interval_hours > 0 && f.now - f.state.ok_at.unwrap_or(0) >= i64::from(interval_hours) * 3600
    {
        reasons.push("scheduled check".to_string());
    }
    if f.state.ok_at.is_none() && f.state.system.is_none() && !force {
        return (reasons, Verdict::Skip(Skip::Baseline));
    }
    if force {
        reasons.push("forced".to_string());
    }
    if reasons.is_empty() {
        return (reasons, Verdict::Skip(Skip::NoReasons));
    }
    if !force && f.state.failed {
        if let Some(t) = f.state.tried_at {
            if f.now - t < RETRY_SECS {
                let retry_at = t + RETRY_SECS;
                return (reasons, Verdict::Skip(Skip::BackOff { retry_at }));
            }
        }
    }
    if !f.game.folder.as_ref().is_some_and(|p| p.is_dir()) {
        return (reasons, Verdict::Skip(Skip::FolderMissing));
    }
    if let Some(flags) = f.steam.and_then(|s| s.state_flags) {
        if flags != STEAM_INSTALLED {
            return (
                reasons,
                Verdict::Skip(Skip::SteamUpdating { state_flags: flags }),
            );
        }
    }
    if host.game_running(f.game) {
        return (reasons, Verdict::Skip(Skip::GameRunning));
    }
    (reasons, Verdict::Act)
}

/// Why a whole run did nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Busy {
    /// Another run holds the lock.
    AlreadyRunning,
    /// A package manager is running (`by`), or the system changed less than
    /// [`SETTLE_SECS`] ago (`by = None`).
    SystemUpdating { by: Option<String> },
}

/// Per-game result. Exit code: 0 nothing done, 10 acted, 1 failed
/// (legacy `autoupdate_game`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GameOutcome {
    Nothing(Skip),
    Acted,
    Failed,
}

impl GameOutcome {
    pub fn code(&self) -> i32 {
        match self {
            GameOutcome::Nothing(_) => 0,
            GameOutcome::Acted => 10,
            GameOutcome::Failed => 1,
        }
    }

    fn log_outcome(&self) -> Outcome {
        match self {
            GameOutcome::Nothing(s) if s.is_blocked() => Outcome::Skipped,
            GameOutcome::Nothing(_) => Outcome::Nothing,
            GameOutcome::Acted => Outcome::Acted,
            GameOutcome::Failed => Outcome::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GameRun {
    pub domain: String,
    pub name: String,
    pub outcome: GameOutcome,
    pub reasons: Vec<String>,
    pub actions: Vec<String>,
    /// One line for the run summary (acted / failed / blocked games).
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunReport {
    pub started_at: i64,
    pub forced: bool,
    pub dry_run: bool,
    pub busy: Option<Busy>,
    pub games: Vec<GameRun>,
}

impl RunReport {
    /// Process exit status (legacy `autoupdate_run`): 1 if a game failed,
    /// else 0. Per-game codes are in [`GameOutcome::code`].
    pub fn exit_code(&self) -> i32 {
        i32::from(self.games.iter().any(|g| g.outcome == GameOutcome::Failed))
    }

    /// Summary lines of games that acted or failed.
    pub fn summaries(&self) -> Vec<String> {
        self.games
            .iter()
            .filter(|g| matches!(g.outcome, GameOutcome::Acted | GameOutcome::Failed))
            .filter_map(|g| g.summary.clone())
            .collect()
    }

    fn outcome(&self) -> Outcome {
        let has = |o: &GameOutcome| self.games.iter().any(|g| &g.outcome == o);
        if has(&GameOutcome::Failed) {
            Outcome::Failed
        } else if has(&GameOutcome::Acted) {
            Outcome::Acted
        } else if self.busy.is_some()
            || self
                .games
                .iter()
                .any(|g| g.outcome.log_outcome() == Outcome::Skipped)
        {
            Outcome::Skipped
        } else {
            Outcome::Nothing
        }
    }
}

fn mod_files(conn: &Connection, domain: &str) -> R<BTreeSet<(u64, u64)>> {
    Ok(mods::list_mods(conn, domain)?
        .into_iter()
        .filter_map(|m| Some((m.nexus_mod_id?, m.nexus_file_id?)))
        .collect())
}

fn join(v: &[String]) -> String {
    v.join(", ")
}

/// The scheduled job (legacy `autoupdate_run` + `autoupdate_game`). Never
/// redeploys while a package manager runs, Steam updates the game or the
/// game runs. Each game's result and a run summary go to the last-run log.
pub fn run(actions: &mut dyn Actions, host: &dyn Host, opts: &RunOptions) -> R<RunReport> {
    let mut report = RunReport {
        started_at: host.now(),
        forced: opts.force,
        dry_run: opts.dry_run,
        busy: None,
        games: Vec::new(),
    };
    let _lock = match DeployLock::try_acquire(&opts.lock_path) {
        Ok(l) => l,
        Err(StoreError::LockBusy(_)) => {
            report.busy = Some(Busy::AlreadyRunning);
            return Ok(report);
        }
        Err(e) => return Err(e.into()),
    };
    let snapshot = host.system_snapshot();
    if !opts.force {
        let by = host.package_manager_busy();
        if by.is_some() || report.started_at - snapshot.newest_change < SETTLE_SECS {
            report.busy = Some(Busy::SystemUpdating { by });
            write_summary(actions.store().conn(), &report, host.now())?;
            return Ok(report);
        }
    }
    for game in games::list_games(actions.store().conn())? {
        // One failing game must not stop the others (legacy subshells).
        let g = match run_game(actions, host, opts, &game, &snapshot) {
            Ok(g) => g,
            Err(e) => GameRun {
                domain: game.domain.clone(),
                name: game.name.clone(),
                outcome: GameOutcome::Failed,
                reasons: Vec::new(),
                actions: Vec::new(),
                summary: Some(format!("{}: auto-update failed: {e}", game.name)),
            },
        };
        if !matches!(
            g.outcome,
            GameOutcome::Nothing(Skip::NotDeployed | Skip::NoReasons)
        ) {
            state::log_run(
                actions.store().conn(),
                &RunLogEntry {
                    id: 0,
                    game: Some(g.domain.clone()),
                    started_at: report.started_at,
                    finished_at: Some(host.now()),
                    forced: opts.force,
                    dry_run: opts.dry_run,
                    outcome: g.outcome.log_outcome(),
                    reasons: g.reasons.clone(),
                    actions: g.actions.clone(),
                    summary: g.summary.clone(),
                },
            )?;
        }
        report.games.push(g);
    }
    write_summary(actions.store().conn(), &report, host.now())?;
    Ok(report)
}

fn write_summary(conn: &Connection, report: &RunReport, finished: i64) -> R<()> {
    let summary = match &report.busy {
        Some(Busy::SystemUpdating { by: Some(pm) }) => Some(format!(
            "system is being updated ({pm} running), checking again later"
        )),
        Some(Busy::SystemUpdating { by: None }) => {
            Some("system is being updated, checking again later".to_string())
        }
        Some(Busy::AlreadyRunning) => None,
        None => {
            let s = report.summaries();
            Some(if s.is_empty() {
                "Nothing to do.".to_string()
            } else {
                s.join("\n")
            })
        }
    };
    state::log_run(
        conn,
        &RunLogEntry {
            id: 0,
            game: None,
            started_at: report.started_at,
            finished_at: Some(finished),
            forced: report.forced,
            dry_run: report.dry_run,
            outcome: report.outcome(),
            reasons: Vec::new(),
            actions: Vec::new(),
            summary,
        },
    )?;
    Ok(())
}

fn run_game(
    actions: &mut dyn Actions,
    host: &dyn Host,
    opts: &RunOptions,
    game: &GameConfig,
    snapshot: &SystemSnapshot,
) -> R<GameRun> {
    let conn = actions.store().conn();
    let mut st = state::load_state(conn, &game.domain)?;
    let deployed = games::deployment_active(conn, &game.domain)?;
    let steam = host.steam_state(game);
    let build = steam.as_ref().and_then(|s| s.buildid.clone());
    let now = host.now();
    let facts = Facts {
        game,
        state: &st,
        deployed,
        snapshot,
        steam: steam.as_ref(),
        now,
    };
    let (reasons, verdict) = decide(&facts, host, opts.force, opts.interval_hours);
    let why = join(&reasons);
    let mut run = GameRun {
        domain: game.domain.clone(),
        name: game.name.clone(),
        outcome: GameOutcome::Acted,
        reasons,
        actions: Vec::new(),
        summary: None,
    };
    if let Verdict::Skip(skip) = verdict {
        if skip == Skip::Baseline && !opts.dry_run {
            st.system = Some(snapshot.fingerprint.clone());
            st.build = build;
            st.ok_at = Some(now);
            state::save_state(conn, &game.domain, &st)?;
        }
        if skip.is_blocked() {
            run.summary = Some(format!("{}: {}", game.name, skip.describe()));
        }
        run.outcome = GameOutcome::Nothing(skip);
        return Ok(run);
    }
    if opts.dry_run {
        run.actions = planned(actions, opts);
        run.summary = Some(format!("{}: would update and redeploy ({why})", game.name));
        return Ok(run);
    }
    if !opts.force {
        st.tried_at = Some(now);
        state::save_state(conn, &game.domain, &st)?;
    }
    let before = mod_files(actions.store().conn(), &game.domain)?;
    let mut errs = Vec::new();
    match actions.account() {
        Account::Premium => {
            if opts.collections {
                let cols = collections::list(actions.store().conn(), &game.domain)?;
                for c in cols {
                    let slug = c.record.slug;
                    match actions.update_collection(game, &slug) {
                        Ok(()) => run.actions.push(format!("collection update {slug}")),
                        Err(e) => errs.push((format!("collection {slug}"), e)),
                    }
                }
            }
            match actions.install_mod_updates(game) {
                Ok(()) => run.actions.push("mod updates installed".into()),
                Err(e) => errs.push(("mod updates".into(), e)),
            }
        }
        Account::Free => {
            // Errors ignored as in legacy: a check must not block the redeploy.
            if actions.check_mod_updates(game).is_ok() {
                run.actions.push("update check".into());
            }
        }
        Account::None => {}
    }
    let after = mod_files(actions.store().conn(), &game.domain)?;
    let updated = after.difference(&before).count();
    match actions.redeploy(game) {
        Ok(()) => run.actions.push("redeploy".into()),
        Err(e) => errs.push(("deploy".into(), e)),
    }
    let conn = actions.store().conn();
    let mut st = state::load_state(conn, &game.domain)?;
    if !errs.is_empty() {
        st.failed = true;
        state::save_state(conn, &game.domain, &st)?;
        let names: Vec<String> = errs.iter().map(|(n, _)| n.clone()).collect();
        for (n, e) in &errs {
            run.actions.push(format!("{n} failed: {e}"));
        }
        run.outcome = GameOutcome::Failed;
        run.summary = Some(format!(
            "{}: auto-update had problems ({})",
            game.name,
            names.join(" ")
        ));
        return Ok(run);
    }
    st.system = Some(snapshot.fingerprint.clone());
    st.build = build;
    st.ok_at = Some(host.now());
    st.failed = false;
    state::save_state(conn, &game.domain, &st)?;
    run.summary = Some(if updated > 0 {
        format!(
            "{}: {updated} mod(s) updated and redeployed ({why})",
            game.name
        )
    } else {
        format!("{}: mods redeployed ({why})", game.name)
    });
    Ok(run)
}

/// What a dry run would do.
fn planned(actions: &mut dyn Actions, opts: &RunOptions) -> Vec<String> {
    let mut v = Vec::new();
    if actions.account() == Account::Premium {
        if opts.collections {
            v.push("update collections".into());
        }
        v.push("install mod updates".into());
    }
    v.push("redeploy".into());
    v
}

/// A game that is due (or due but blocked).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pending {
    pub domain: String,
    pub name: String,
    pub reasons: Vec<String>,
    /// `None` = a run now would update and redeploy it.
    pub blocked: Option<Skip>,
}

/// Result of [`startup_check`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartupCheck {
    /// A package manager runs or the system changed in the last
    /// [`SETTLE_SECS`]; a run now would wait.
    pub system_busy: Option<Busy>,
    pub pending: Vec<Pending>,
}

/// Read-only check for the GUI at start: which deployed games a run would
/// act on, and what blocks them. Writes nothing (no baseline, no log, no
/// state) and runs no action.
pub fn startup_check(conn: &Connection, host: &dyn Host, opts: &RunOptions) -> R<StartupCheck> {
    let snapshot = host.system_snapshot();
    let now = host.now();
    let by = host.package_manager_busy();
    let system_busy = (by.is_some() || now - snapshot.newest_change < SETTLE_SECS)
        .then_some(Busy::SystemUpdating { by });
    let mut pending = Vec::new();
    for game in games::list_games(conn)? {
        let st = state::load_state(conn, &game.domain)?;
        let steam = host.steam_state(&game);
        let facts = Facts {
            game: &game,
            state: &st,
            deployed: games::deployment_active(conn, &game.domain)?,
            snapshot: &snapshot,
            steam: steam.as_ref(),
            now,
        };
        let (reasons, verdict) = decide(&facts, host, false, opts.interval_hours);
        let blocked = match verdict {
            Verdict::Act => None,
            Verdict::Skip(s) if s.is_blocked() => Some(s),
            Verdict::Skip(_) => continue,
        };
        pending.push(Pending {
            domain: game.domain.clone(),
            name: game.name.clone(),
            reasons,
            blocked,
        });
    }
    Ok(StartupCheck {
        system_busy,
        pending,
    })
}

/// Per deployed game, for `status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GameStatus {
    pub domain: String,
    pub name: String,
    pub last_ok: Option<i64>,
    pub last_failed: bool,
    pub steam_build: Option<String>,
    /// `system updated` / `game updated (build N)` since the last success.
    pub pending: Vec<String>,
}

/// `autoupdate status` and the GUI settings view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub timer: TimerStatus,
    pub collections: bool,
    pub interval_hours: u32,
    /// Newest change of the watched system paths (epoch, 0 = unknown).
    pub last_system_change: i64,
    pub games: Vec<GameStatus>,
    /// Last-run log, newest first.
    pub runs: Vec<RunLogEntry>,
}

/// Status without side effects: timer state, settings, per-game state and
/// the newest `runs_limit` log rows.
pub fn status(
    conn: &Connection,
    paths: &Paths,
    host: &dyn Host,
    systemctl: &dyn Systemctl,
    opts: &RunOptions,
    runs_limit: usize,
) -> R<Status> {
    let snapshot = host.system_snapshot();
    let mut out = Vec::new();
    for game in games::list_games(conn)? {
        if !games::deployment_active(conn, &game.domain)? {
            continue;
        }
        let st = state::load_state(conn, &game.domain)?;
        let steam = host.steam_state(&game);
        let build = steam.as_ref().and_then(|s| s.buildid.clone());
        let mut pending = Vec::new();
        if st.system.as_deref() != Some(snapshot.fingerprint.as_str()) {
            pending.push("system updated".to_string());
        }
        if let Some(b) = &build {
            if st.build.as_ref() != Some(b) {
                pending.push(format!("game updated (build {b})"));
            }
        }
        out.push(GameStatus {
            domain: game.domain.clone(),
            name: game.name.clone(),
            last_ok: st.ok_at,
            last_failed: st.failed,
            steam_build: build,
            pending,
        });
    }
    Ok(Status {
        timer: timer_status(paths, systemctl),
        collections: opts.collections,
        interval_hours: opts.interval_hours,
        last_system_change: snapshot.newest_change,
        games: out,
        runs: last_runs(conn, runs_limit)?,
    })
}
