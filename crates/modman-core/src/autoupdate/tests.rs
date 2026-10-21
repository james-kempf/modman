// SPDX-License-Identifier: GPL-3.0-or-later
//! Auto-update tests: fake host and actions, fake proc/root trees, fixture
//! appmanifest files, mocked systemctl. Nothing touches the real system.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::*;
use crate::games::{save_game, GameConfig};

const NOW: i64 = 1_800_000_000;

struct FakeHost {
    now: Cell<i64>,
    fp: RefCell<String>,
    newest: Cell<i64>,
    pm: RefCell<Option<String>>,
    steam: RefCell<Option<SteamState>>,
    running: Cell<bool>,
}

impl FakeHost {
    fn new() -> Self {
        Self {
            now: Cell::new(NOW),
            fp: RefCell::new("fp1".into()),
            newest: Cell::new(NOW - 3600),
            pm: RefCell::new(None),
            steam: RefCell::new(Some(SteamState {
                buildid: Some("100".into()),
                state_flags: Some(4),
            })),
            running: Cell::new(false),
        }
    }
}

impl Host for FakeHost {
    fn now(&self) -> i64 {
        self.now.get()
    }
    fn system_snapshot(&self) -> SystemSnapshot {
        SystemSnapshot {
            fingerprint: self.fp.borrow().clone(),
            newest_change: self.newest.get(),
        }
    }
    fn package_manager_busy(&self) -> Option<String> {
        self.pm.borrow().clone()
    }
    fn steam_state(&self, _: &GameConfig) -> Option<SteamState> {
        self.steam.borrow().clone()
    }
    fn game_running(&self, _: &GameConfig) -> bool {
        self.running.get()
    }
}

struct FakeActions {
    store: Store,
    account: Account,
    calls: Vec<String>,
    fail_deploy: bool,
    /// New file id given to every mod by `install_mod_updates`.
    bump_files: bool,
}

impl Actions for FakeActions {
    fn store(&mut self) -> &mut Store {
        &mut self.store
    }
    fn account(&mut self) -> Account {
        self.account
    }
    fn update_collection(&mut self, g: &GameConfig, slug: &str) -> Result<(), String> {
        self.calls.push(format!("collection {} {slug}", g.domain));
        Ok(())
    }
    fn install_mod_updates(&mut self, g: &GameConfig) -> Result<(), String> {
        self.calls.push(format!("updates --install {}", g.domain));
        if self.bump_files {
            self.store
                .conn()
                .execute("UPDATE mods SET nexus_file_id = nexus_file_id + 1", [])
                .unwrap();
        }
        Ok(())
    }
    fn check_mod_updates(&mut self, g: &GameConfig) -> Result<(), String> {
        self.calls.push(format!("updates {}", g.domain));
        Ok(())
    }
    fn redeploy(&mut self, g: &GameConfig) -> Result<(), String> {
        self.calls.push(format!("deploy {}", g.domain));
        if self.fail_deploy {
            Err("boom".into())
        } else {
            Ok(())
        }
    }
}

struct Fx {
    _tmp: tempfile::TempDir,
    host: FakeHost,
    act: FakeActions,
    opts: RunOptions,
    game: GameConfig,
}

fn fx() -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("game");
    std::fs::create_dir_all(&folder).unwrap();
    let mut game = GameConfig::custom("skyrimspecialedition");
    game.name = "Skyrim SE".into();
    game.folder = Some(folder);
    game.appid = Some(489830);
    let store = Store::open_in_memory().unwrap();
    save_game(store.conn(), &game).unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO deployments (game, target, rel_path, method, source, deployed_at)
             VALUES (?1, 'game_dir', 'Data/a.esp', 'copy', '/s/a.esp', 1)",
            [&game.domain],
        )
        .unwrap();
    let opts = RunOptions {
        force: false,
        dry_run: false,
        collections: true,
        interval_hours: 0,
        lock_path: tmp.path().join("autoupdate.lock"),
    };
    Fx {
        _tmp: tmp,
        host: FakeHost::new(),
        act: FakeActions {
            store,
            account: Account::Premium,
            calls: Vec::new(),
            fail_deploy: false,
            bump_files: false,
        },
        opts,
        game,
    }
}

impl Fx {
    fn run(&mut self) -> RunReport {
        run(&mut self.act, &self.host, &self.opts).unwrap()
    }
    fn state(&self) -> GameState {
        load_state(self.act.store.conn(), &self.game.domain).unwrap()
    }
    fn set_state(&self, s: &GameState) {
        state::save_state(self.act.store.conn(), &self.game.domain, s).unwrap();
    }
    /// A recorded baseline with the current fingerprint and build.
    fn baseline(&self) {
        self.set_state(&GameState {
            system: Some("fp1".into()),
            build: Some("100".into()),
            ok_at: Some(NOW - 7200),
            ..Default::default()
        });
    }
    fn outcome(r: &RunReport) -> GameOutcome {
        r.games[0].outcome.clone()
    }
}

// ── gates (legacy_spec §15 order) ─────────────────────────────

#[test]
fn first_run_records_baseline_without_acting() {
    let mut f = fx();
    let r = f.run();
    assert_eq!(Fx::outcome(&r), GameOutcome::Nothing(Skip::Baseline));
    assert_eq!(r.games[0].outcome.code(), 0);
    assert!(f.act.calls.is_empty());
    let s = f.state();
    assert_eq!(s.system.as_deref(), Some("fp1"));
    assert_eq!(s.build.as_deref(), Some("100"));
    assert_eq!(s.ok_at, Some(NOW));
}

#[test]
fn package_manager_busy_skips_the_whole_run() {
    let mut f = fx();
    f.baseline();
    *f.host.fp.borrow_mut() = "fp2".into();
    *f.host.pm.borrow_mut() = Some("dnf5".into());
    let r = f.run();
    assert_eq!(
        r.busy,
        Some(Busy::SystemUpdating {
            by: Some("dnf5".into())
        })
    );
    assert!(r.games.is_empty() && f.act.calls.is_empty());
    assert_eq!(r.exit_code(), 0);
    let log = last_runs(f.act.store.conn(), 5).unwrap();
    assert_eq!(log[0].outcome, Outcome::Skipped);
    assert!(log[0].summary.as_deref().unwrap().contains("dnf5"));
}

#[test]
fn recent_system_change_waits_for_settle_time() {
    let mut f = fx();
    f.baseline();
    *f.host.fp.borrow_mut() = "fp2".into();
    f.host.newest.set(NOW - SETTLE_SECS + 1);
    let r = f.run();
    assert_eq!(r.busy, Some(Busy::SystemUpdating { by: None }));
    assert!(f.act.calls.is_empty());
    // Forced runs ignore the settle and package-manager gates.
    f.opts.force = true;
    let r = f.run();
    assert_eq!(Fx::outcome(&r), GameOutcome::Acted);
}

#[test]
fn not_deployed_game_is_left_alone_even_when_forced() {
    let mut f = fx();
    f.act
        .store
        .conn()
        .execute("DELETE FROM deployments", [])
        .unwrap();
    f.opts.force = true;
    let r = f.run();
    assert_eq!(Fx::outcome(&r), GameOutcome::Nothing(Skip::NotDeployed));
    assert!(f.act.calls.is_empty());
}

#[test]
fn unchanged_system_and_build_do_nothing() {
    let mut f = fx();
    f.baseline();
    let r = f.run();
    assert_eq!(Fx::outcome(&r), GameOutcome::Nothing(Skip::NoReasons));
    assert!(f.act.calls.is_empty());
}

#[test]
fn back_off_three_hours_after_failure() {
    let mut f = fx();
    f.set_state(&GameState {
        system: Some("old".into()),
        build: Some("100".into()),
        ok_at: Some(NOW - 90_000),
        tried_at: Some(NOW - RETRY_SECS + 60),
        failed: true,
    });
    let r = f.run();
    assert_eq!(
        Fx::outcome(&r),
        GameOutcome::Nothing(Skip::BackOff { retry_at: NOW + 60 })
    );
    assert!(f.act.calls.is_empty());
    f.host.now.set(NOW + 61);
    f.host.newest.set(0);
    assert_eq!(Fx::outcome(&f.run()), GameOutcome::Acted);
}

#[test]
fn missing_game_folder_is_retried_later() {
    let mut f = fx();
    f.baseline();
    *f.host.fp.borrow_mut() = "fp2".into();
    std::fs::remove_dir(f.game.folder.as_ref().unwrap()).unwrap();
    let r = f.run();
    assert_eq!(Fx::outcome(&r), GameOutcome::Nothing(Skip::FolderMissing));
    assert!(f.act.calls.is_empty());
    assert_eq!(f.state().system.as_deref(), Some("fp1"), "state unchanged");
}

#[test]
fn steam_still_updating_blocks() {
    let mut f = fx();
    f.baseline();
    *f.host.steam.borrow_mut() = Some(SteamState {
        buildid: Some("101".into()),
        state_flags: Some(6),
    });
    let r = f.run();
    assert_eq!(
        Fx::outcome(&r),
        GameOutcome::Nothing(Skip::SteamUpdating { state_flags: 6 })
    );
    assert_eq!(r.games[0].reasons, ["game updated (build 101)"]);
    assert!(f.act.calls.is_empty());
}

#[test]
fn never_redeploys_while_the_game_runs() {
    let mut f = fx();
    f.baseline();
    *f.host.fp.borrow_mut() = "fp2".into();
    f.host.running.set(true);
    f.opts.force = true;
    let r = f.run();
    assert_eq!(Fx::outcome(&r), GameOutcome::Nothing(Skip::GameRunning));
    assert!(f.act.calls.is_empty());
    let log = last_runs(f.act.store.conn(), 5).unwrap();
    assert!(log
        .iter()
        .any(|e| e.game.is_some() && e.outcome == Outcome::Skipped));
}

// ── acting ───────────────────────────────────────────────────

#[test]
fn system_update_runs_collections_updates_and_redeploy_with_exit_10() {
    let mut f = fx();
    f.baseline();
    f.act
        .store
        .conn()
        .execute(
            "INSERT INTO collections (game, slug, revision, installed_at)
             VALUES ('skyrimspecialedition', 'abc123', 1, 1)",
            [],
        )
        .unwrap();
    *f.host.fp.borrow_mut() = "fp2".into();
    let r = f.run();
    assert_eq!(Fx::outcome(&r), GameOutcome::Acted);
    assert_eq!(r.games[0].outcome.code(), 10);
    assert_eq!(r.exit_code(), 0);
    assert_eq!(
        f.act.calls,
        [
            "collection skyrimspecialedition abc123",
            "updates --install skyrimspecialedition",
            "deploy skyrimspecialedition"
        ]
    );
    let s = f.state();
    assert_eq!(s.system.as_deref(), Some("fp2"));
    assert_eq!(s.tried_at, Some(NOW));
    assert!(!s.failed);
    assert!(r.games[0]
        .summary
        .as_deref()
        .unwrap()
        .contains("mods redeployed (system updated)"));
}

#[test]
fn collections_skipped_when_setting_disabled() {
    let mut f = fx();
    f.baseline();
    f.act
        .store
        .conn()
        .execute(
            "INSERT INTO collections (game, slug, revision, installed_at)
             VALUES ('skyrimspecialedition', 'abc123', 1, 1)",
            [],
        )
        .unwrap();
    f.opts.collections = false;
    *f.host.fp.borrow_mut() = "fp2".into();
    f.run();
    assert!(!f.act.calls.iter().any(|c| c.starts_with("collection")));
    assert!(f
        .act
        .calls
        .contains(&"deploy skyrimspecialedition".to_string()));
}

#[test]
fn free_account_only_checks_and_no_key_only_redeploys() {
    let mut f = fx();
    f.baseline();
    f.act.account = Account::Free;
    f.opts.force = true;
    f.run();
    assert_eq!(
        f.act.calls,
        [
            "updates skyrimspecialedition",
            "deploy skyrimspecialedition"
        ]
    );
    f.act.calls.clear();
    f.act.account = Account::None;
    f.run();
    assert_eq!(f.act.calls, ["deploy skyrimspecialedition"]);
}

#[test]
fn steam_build_change_triggers_and_counts_updated_mods() {
    let mut f = fx();
    f.baseline();
    f.act
        .store
        .conn()
        .execute(
            "INSERT INTO mods (game, uid, name, nexus_mod_id, nexus_file_id, source, priority, enabled, installed_at)
             VALUES ('skyrimspecialedition', 'A-1-2', 'A', 1, 2, 'nexus', 1, 1, 1)",
            [],
        )
        .unwrap();
    f.act.bump_files = true;
    *f.host.steam.borrow_mut() = Some(SteamState {
        buildid: Some("101".into()),
        state_flags: Some(4),
    });
    let r = f.run();
    assert_eq!(r.games[0].reasons, ["game updated (build 101)"]);
    assert_eq!(f.state().build.as_deref(), Some("101"));
    let summary = r.games[0].summary.clone().unwrap();
    assert!(
        summary.contains("1 mod(s) updated and redeployed"),
        "{summary}"
    );
}

#[test]
fn failed_redeploy_marks_failed_and_exits_1() {
    let mut f = fx();
    f.baseline();
    *f.host.fp.borrow_mut() = "fp2".into();
    f.act.fail_deploy = true;
    let r = f.run();
    assert_eq!(Fx::outcome(&r), GameOutcome::Failed);
    assert_eq!(r.games[0].outcome.code(), 1);
    assert_eq!(r.exit_code(), 1);
    let s = f.state();
    assert!(s.failed);
    assert_eq!(s.system.as_deref(), Some("fp1"), "not marked successful");
    assert!(r.games[0]
        .summary
        .as_deref()
        .unwrap()
        .contains("had problems (deploy)"));
}

#[test]
fn interval_adds_scheduled_check() {
    let mut f = fx();
    f.baseline(); // ok_at = NOW - 2 h
    f.opts.interval_hours = 3;
    assert_eq!(Fx::outcome(&f.run()), GameOutcome::Nothing(Skip::NoReasons));
    f.opts.interval_hours = 2;
    let r = f.run();
    assert_eq!(r.games[0].reasons, ["scheduled check"]);
    assert_eq!(Fx::outcome(&r), GameOutcome::Acted);
}

#[test]
fn dry_run_changes_no_state() {
    let mut f = fx();
    f.baseline();
    *f.host.fp.borrow_mut() = "fp2".into();
    f.opts.dry_run = true;
    let r = f.run();
    assert_eq!(Fx::outcome(&r), GameOutcome::Acted);
    assert!(f.act.calls.is_empty());
    assert_eq!(f.state().system.as_deref(), Some("fp1"));
    assert_eq!(f.state().tried_at, None);
}

#[test]
fn concurrent_run_returns_at_once() {
    let mut f = fx();
    let _held = DeployLock::try_acquire(&f.opts.lock_path).unwrap();
    let r = f.run();
    assert_eq!(r.busy, Some(Busy::AlreadyRunning));
    assert!(f.state().ok_at.is_none());
}

// ── last-run log, status, startup check ──────────────────────

#[test]
fn last_run_log_persists_timestamp_outcome_and_actions() {
    let mut f = fx();
    f.baseline();
    *f.host.fp.borrow_mut() = "fp2".into();
    f.host.now.set(NOW + 5);
    f.run();
    let log = last_runs(f.act.store.conn(), 10).unwrap();
    let summary = &log[0];
    assert_eq!(summary.game, None);
    assert_eq!(summary.outcome, Outcome::Acted);
    assert_eq!(summary.started_at, NOW + 5);
    let g = &log[1];
    assert_eq!(g.game.as_deref(), Some("skyrimspecialedition"));
    assert_eq!(g.outcome, Outcome::Acted);
    assert_eq!(g.reasons, ["system updated"]);
    assert_eq!(g.actions, ["mod updates installed", "redeploy"]);
    // Returned by the status API too.
    let paths = Paths::for_home(f._tmp.path());
    let st = status(
        f.act.store.conn(),
        &paths,
        &f.host,
        &MockSystemctl::default(),
        &f.opts,
        10,
    )
    .unwrap();
    assert_eq!(st.runs, log);
    assert_eq!(st.games[0].last_ok, Some(NOW + 5));
    assert!(st.games[0].pending.is_empty());
    assert!(!st.timer.installed);
}

#[test]
fn log_is_pruned_to_keep_limit() {
    let store = Store::open_in_memory().unwrap();
    let e = RunLogEntry {
        id: 0,
        game: None,
        started_at: 1,
        finished_at: None,
        forced: false,
        dry_run: false,
        outcome: Outcome::Nothing,
        reasons: vec![],
        actions: vec![],
        summary: None,
    };
    for _ in 0..LOG_KEEP + 3 {
        state::log_run(store.conn(), &e).unwrap();
    }
    let n: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM autoupdate_runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, LOG_KEEP);
}

#[test]
fn startup_check_reports_pending_without_side_effects() {
    let f = fx();
    f.baseline();
    *f.host.fp.borrow_mut() = "fp2".into();
    f.host.running.set(true);
    let snapshot = |c: &Connection| -> (i64, GameState) {
        let n = c
            .query_row("SELECT COUNT(*) FROM autoupdate_runs", [], |r| r.get(0))
            .unwrap();
        (n, load_state(c, "skyrimspecialedition").unwrap())
    };
    let before = snapshot(f.act.store.conn());
    let c = startup_check(f.act.store.conn(), &f.host, &f.opts).unwrap();
    assert_eq!(c.system_busy, None);
    assert_eq!(
        c.pending,
        [Pending {
            domain: "skyrimspecialedition".into(),
            name: "Skyrim SE".into(),
            reasons: vec!["system updated".into()],
            blocked: Some(Skip::GameRunning),
        }]
    );
    f.host.running.set(false);
    let c = startup_check(f.act.store.conn(), &f.host, &f.opts).unwrap();
    assert_eq!(c.pending[0].blocked, None);
    assert_eq!(snapshot(f.act.store.conn()), before, "no writes");
    assert!(f.act.calls.is_empty(), "no actions");
}

#[test]
fn startup_check_on_first_run_has_nothing_pending_and_records_nothing() {
    let f = fx();
    let c = startup_check(f.act.store.conn(), &f.host, &f.opts).unwrap();
    assert!(c.pending.is_empty());
    assert_eq!(f.state(), GameState::default());
}

// ── host probes (fake /proc and root) ────────────────────────

fn live(tmp: &Path) -> LiveHost {
    LiveHost {
        root: tmp.join("root"),
        proc_root: tmp.join("proc"),
        home: tmp.join("home"),
    }
}

fn proc_entry(tmp: &Path, pid: &str, comm: &str, cmdline: &[u8]) {
    let d = tmp.join("proc").join(pid);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("comm"), format!("{comm}\n")).unwrap();
    std::fs::write(d.join("cmdline"), cmdline).unwrap();
}

#[test]
fn package_manager_detected_from_fake_proc() {
    let tmp = tempfile::tempdir().unwrap();
    let h = live(tmp.path());
    proc_entry(tmp.path(), "1", "systemd", b"/sbin/init\0");
    proc_entry(tmp.path(), "self", "dnf", b"");
    assert_eq!(h.package_manager_busy(), None, "non-pid dirs ignored");
    proc_entry(tmp.path(), "42", "dnf5", b"dnf5\0upgrade\0");
    assert_eq!(h.package_manager_busy().as_deref(), Some("dnf5"));
}

#[test]
fn pacman_db_lock_means_busy() {
    let tmp = tempfile::tempdir().unwrap();
    let h = live(tmp.path());
    std::fs::create_dir_all(tmp.path().join("proc")).unwrap();
    let lck = tmp.path().join("root/var/lib/pacman/db.lck");
    std::fs::create_dir_all(lck.parent().unwrap()).unwrap();
    std::fs::write(&lck, "").unwrap();
    assert_eq!(h.package_manager_busy().as_deref(), Some("pacman"));
}

#[test]
fn game_running_detected_from_fake_proc() {
    let tmp = tempfile::tempdir().unwrap();
    let h = live(tmp.path());
    let mut g = GameConfig::custom("g");
    g.appid = Some(489830);
    proc_entry(tmp.path(), "7", "wine", b"reaper\0AppId=4898300\0");
    assert!(!h.game_running(&g));
    proc_entry(
        tmp.path(),
        "8",
        "reaper",
        b"reaper\0SteamLaunch\0AppId=489830\0",
    );
    assert!(h.game_running(&g));
}

fn set_mtime(p: &Path, epoch: u64) {
    let t = SystemTime::UNIX_EPOCH + Duration::from_secs(epoch);
    std::fs::File::options()
        .write(true)
        .open(p)
        .or_else(|_| std::fs::File::open(p))
        .unwrap()
        .set_modified(t)
        .unwrap();
}

#[test]
fn system_fingerprint_follows_os_release_and_package_db() {
    let tmp = tempfile::tempdir().unwrap();
    let h = live(tmp.path());
    let root = tmp.path().join("root");
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::fs::create_dir_all(root.join("var/lib/rpm")).unwrap();
    std::fs::create_dir_all(tmp.path().join("proc/sys/kernel")).unwrap();
    std::fs::write(tmp.path().join("proc/sys/kernel/osrelease"), "6.1.0\n").unwrap();
    std::fs::write(
        root.join("etc/os-release"),
        "NAME=X\nID=nobara\nVERSION_ID=43\n",
    )
    .unwrap();
    std::fs::write(root.join("var/lib/rpm/rpmdb.sqlite"), "").unwrap();
    std::fs::write(root.join("var/lib/rpm/rpmdb.sqlite-shm"), "").unwrap();
    set_mtime(&root.join("etc/os-release"), 1000);
    set_mtime(&root.join("var/lib/rpm/rpmdb.sqlite"), 2000);
    set_mtime(&root.join("var/lib/rpm/rpmdb.sqlite-shm"), 9000);
    set_mtime(&root.join("var/lib/rpm"), 1500);
    let a = h.system_snapshot();
    assert_eq!(a.fingerprint.len(), 16);
    assert_eq!(a.newest_change, 2000, "-shm files ignored");
    assert_eq!(h.system_snapshot(), a, "stable");
    // A NAME change is not an update; a package DB change is.
    std::fs::write(
        root.join("etc/os-release"),
        "NAME=Y\nID=nobara\nVERSION_ID=43\n",
    )
    .unwrap();
    set_mtime(&root.join("etc/os-release"), 1000);
    assert_eq!(h.system_snapshot().fingerprint, a.fingerprint);
    set_mtime(&root.join("var/lib/rpm/rpmdb.sqlite"), 3000);
    let b = h.system_snapshot();
    assert_ne!(b.fingerprint, a.fingerprint);
    assert_eq!(b.newest_change, 3000);
    // Kernel change alone changes it too.
    std::fs::write(tmp.path().join("proc/sys/kernel/osrelease"), "6.2.0\n").unwrap();
    assert_ne!(h.system_snapshot().fingerprint, b.fingerprint);
}

fn acf(lib: &Path, appid: u32, build: &str, flags: u32) {
    let d = lib.join("steamapps");
    std::fs::create_dir_all(d.join("common/Skyrim")).unwrap();
    std::fs::write(
        d.join(format!("appmanifest_{appid}.acf")),
        format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"name\"\t\t\"Skyrim\"\n\t\"StateFlags\"\t\t\"{flags}\"\n\t\"installdir\"\t\t\"Skyrim\"\n\t\"buildid\"\t\t\"{build}\"\n}}\n"
        ),
    )
    .unwrap();
}

#[test]
fn steam_buildid_change_detected_from_fixture_acf() {
    let tmp = tempfile::tempdir().unwrap();
    let h = live(tmp.path());
    let lib = tmp.path().join("home/.local/share/Steam");
    acf(&lib, 489830, "100", 4);
    let mut g = GameConfig::custom("skyrimspecialedition");
    g.appid = Some(489830);
    let s = h.steam_state(&g).unwrap();
    assert_eq!(s.buildid.as_deref(), Some("100"));
    assert_eq!(s.state_flags, Some(4));
    // Steam updates the game: new build, still downloading.
    acf(&lib, 489830, "101", 1030);
    let s2 = h.steam_state(&g).unwrap();
    assert_eq!(s2.buildid.as_deref(), Some("101"));
    // Through the gates: reason "game updated", blocked by StateFlags.
    let fh = FakeHost::new();
    *fh.steam.borrow_mut() = Some(s2);
    let st = GameState {
        system: Some("fp1".into()),
        build: Some("100".into()),
        ok_at: Some(NOW - 10),
        ..Default::default()
    };
    g.folder = Some(tmp.path().to_path_buf());
    let snap = fh.system_snapshot();
    let steam = fh.steam_state(&g);
    let facts = Facts {
        game: &g,
        state: &st,
        deployed: true,
        snapshot: &snap,
        steam: steam.as_ref(),
        now: NOW,
    };
    let (reasons, verdict) = decide(&facts, &fh, false, 0);
    assert_eq!(reasons, ["game updated (build 101)"]);
    assert_eq!(
        verdict,
        Verdict::Skip(Skip::SteamUpdating { state_flags: 1030 })
    );
    // No appid: not a Steam game.
    g.appid = None;
    assert_eq!(h.steam_state(&g), None);
    // Explicit library wins.
    let other = tmp.path().join("sdcard");
    acf(&other, 489830, "200", 4);
    g.appid = Some(489830);
    g.library = Some(other);
    assert_eq!(h.steam_state(&g).unwrap().buildid.as_deref(), Some("200"));
}

// ── systemd units ────────────────────────────────────────────

#[derive(Default)]
struct MockSystemctl {
    calls: RefCell<Vec<String>>,
    unavailable: bool,
    fail_enable: bool,
}

impl Systemctl for MockSystemctl {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let line = args.join(" ");
        self.calls.borrow_mut().push(line.clone());
        match args.first().copied() {
            Some("show-environment") if self.unavailable => Err("Failed to connect to bus".into()),
            Some("enable") if self.fail_enable => Err("nope".into()),
            Some("is-active") => Ok("active".into()),
            Some("show") => Ok("Wed 2026-10-21 13:00:00 UTC".into()),
            _ => Ok(String::new()),
        }
    }
}

fn xdg_paths(tmp: &Path) -> Paths {
    Paths {
        home: tmp.join("home"),
        config_dir: tmp.join("xdgconf/modman"),
        data_dir: tmp.join("data"),
        state_dir: tmp.join("state"),
    }
}

fn spec(interval: u32) -> UnitSpec {
    UnitSpec {
        exec: PathBuf::from("/opt/mod man/modman"),
        interval_hours: interval,
        env: vec![("PATH".into(), "/usr/bin:/bin".into())],
    }
}

#[test]
fn enable_writes_units_under_xdg_config_home_with_interval() {
    let tmp = tempfile::tempdir().unwrap();
    let p = xdg_paths(tmp.path());
    let sc = MockSystemctl::default();
    enable(&p, &spec(6), &sc).unwrap();
    let dir = tmp.path().join("xdgconf/systemd/user");
    assert_eq!(systemd_user_dir(&p), dir);
    let timer = std::fs::read_to_string(dir.join("modman-autoupdate.timer")).unwrap();
    let service = std::fs::read_to_string(dir.join("modman-autoupdate.service")).unwrap();
    assert!(timer.contains("OnUnitActiveSec=6h\n"), "{timer}");
    assert!(timer.contains("OnBootSec=3min") && timer.contains("Persistent=true"));
    assert!(service.contains("Type=oneshot"));
    assert!(
        service.contains("ExecStart=\"/opt/mod man/modman\" --no-color -y autoupdate run\n"),
        "{service}"
    );
    assert!(service.contains("Environment=\"PATH=/usr/bin:/bin\""));
    assert_eq!(
        *sc.calls.borrow(),
        [
            "show-environment",
            "daemon-reload",
            "enable --now modman-autoupdate.timer"
        ]
    );
    let st = timer_status(&p, &sc);
    assert!(st.installed);
    assert_eq!(st.every_hours, Some(6));
    assert_eq!(st.active.as_deref(), Some("active"));
    // Re-enable with interval 0: hourly checks.
    enable(&p, &spec(0), &sc).unwrap();
    let timer = std::fs::read_to_string(dir.join("modman-autoupdate.timer")).unwrap();
    assert!(timer.contains("OnUnitActiveSec=1h\n"));
}

#[test]
fn disable_removes_units() {
    let tmp = tempfile::tempdir().unwrap();
    let p = xdg_paths(tmp.path());
    let sc = MockSystemctl::default();
    assert!(!disable(&p, &sc).unwrap(), "not enabled");
    assert!(sc.calls.borrow().is_empty());
    enable(&p, &spec(0), &sc).unwrap();
    sc.calls.borrow_mut().clear();
    assert!(disable(&p, &sc).unwrap());
    assert!(!timer_path(&p).exists() && !service_path(&p).exists());
    assert_eq!(
        *sc.calls.borrow(),
        ["disable --now modman-autoupdate.timer", "daemon-reload"]
    );
    assert!(!timer_status(&p, &sc).installed);
}

#[test]
fn enable_without_user_manager_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let p = xdg_paths(tmp.path());
    let sc = MockSystemctl {
        unavailable: true,
        ..Default::default()
    };
    assert!(matches!(
        enable(&p, &spec(0), &sc),
        Err(AutoupdateError::SystemdUnavailable)
    ));
    assert!(!timer_path(&p).exists());
    let sc = MockSystemctl {
        fail_enable: true,
        ..Default::default()
    };
    assert!(enable(&p, &spec(0), &sc).is_err());
    assert!(!timer_path(&p).exists(), "rolled back");
}

#[test]
fn unit_values_are_escaped() {
    let s = UnitSpec {
        exec: PathBuf::from("/a\"b/100%/$x"),
        interval_hours: 0,
        env: vec![("XDG_DATA_HOME".into(), "/d%".into())],
    };
    let svc = render_service(&s);
    assert!(svc.contains(r#"ExecStart="/a\"b/100%%/$$x""#), "{svc}");
    assert!(svc.contains(r#"Environment="XDG_DATA_HOME=/d%%""#));
}

/// Drop rows: the systemd user timer is the only scheduler.
#[test]
fn no_other_scheduler_code() {
    let sources = [
        include_str!("../autoupdate.rs"),
        include_str!("host.rs"),
        include_str!("state.rs"),
        include_str!("units.rs"),
        include_str!("../../../modman-cli/src/commands/autoupdate.rs"),
    ];
    for src in sources {
        for needle in ["crontab", "cron ", "autostart", "--scheduler", ".desktop"] {
            assert!(!src.contains(needle), "found {needle:?}");
        }
    }
}
