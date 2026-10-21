// SPDX-License-Identifier: GPL-3.0-or-later
//! CLI tests for `autoupdate` (M7). systemctl is a fake script recording its
//! arguments; units land under the temp XDG_CONFIG_HOME. The real user
//! systemd is never touched.

mod support;

use std::os::unix::fs::PermissionsExt;

use predicates::prelude::*;
use predicates::str::contains;
use support::Env;

/// Env with a fake `systemctl` wired through `MODMAN_SYSTEMCTL`.
fn env() -> Env {
    let mut env = Env::new();
    let log = env.tmp.path().join("systemctl.log");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$2\" in is-active) echo active;; esac\n",
        log.display()
    );
    let f = env.bin().join("fake-systemctl");
    std::fs::write(&f, script).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env
        .push(("MODMAN_SYSTEMCTL".into(), f.to_string_lossy().into_owned()));
    env
}

#[test]
fn enable_status_disable_manage_user_timer_units() {
    let env = env();
    env.run(&["config", "autoupdate-interval", "6"]).success();
    env.run(&["autoupdate", "enable"])
        .success()
        .stdout(contains("every 6 h"));
    let dir = env.home().join(".config/systemd/user");
    let timer = std::fs::read_to_string(dir.join("modman-autoupdate.timer")).unwrap();
    let service = std::fs::read_to_string(dir.join("modman-autoupdate.service")).unwrap();
    assert!(timer.contains("OnUnitActiveSec=6h"), "{timer}");
    assert!(
        service.contains("modman\" --no-color -y autoupdate run"),
        "{service}"
    );
    assert!(service.contains("Environment=\"XDG_DATA_HOME="));
    let calls = env.calls("systemctl");
    assert!(calls.contains("--user daemon-reload"), "{calls}");
    assert!(calls.contains("--user enable --now modman-autoupdate.timer"));
    env.run(&["au", "status"])
        .success()
        .stdout(contains("systemd user timer").and(contains("(active)")));
    env.run(&["au", "off"])
        .success()
        .stdout(contains("Auto-update disabled."));
    assert!(!dir.join("modman-autoupdate.timer").exists());
    assert!(!dir.join("modman-autoupdate.service").exists());
    assert!(env
        .calls("systemctl")
        .contains("--user disable --now modman-autoupdate.timer"));
    env.run(&["au", "disable"])
        .success()
        .stdout(contains("was not enabled"));
}

#[test]
fn appimage_path_is_used_for_exec_start() {
    let mut env = env();
    env.extra_env
        .push(("APPIMAGE".into(), "/opt/apps/modman.AppImage".into()));
    env.run(&["autoupdate", "enable"]).success();
    let service = std::fs::read_to_string(
        env.home()
            .join(".config/systemd/user/modman-autoupdate.service"),
    )
    .unwrap();
    assert!(
        service.contains("ExecStart=\"/opt/apps/modman.AppImage\" --no-color -y autoupdate run")
    );
}

#[test]
fn no_scheduler_flag() {
    let env = env();
    env.run(&["autoupdate", "enable", "--scheduler", "cron"])
        .failure();
    assert!(!env.home().join(".config/systemd/user").exists());
}

#[test]
fn forced_run_redeploys_and_logs_the_run() {
    let env = env();
    env.add_game("testgame");
    let zip = env.zip("a.zip", &[("Data/a.esp", b"A")]);
    env.run(&["-y", "-g", "testgame", "install", zip.to_str().unwrap()])
        .success();
    env.run(&["-g", "testgame", "deploy"]).success();
    env.run(&["autoupdate", "run", "--dry-run", "--force"])
        .success()
        .stdout(contains(
            "would update and redeploy (system updated, forced)",
        ));
    env.run(&["autoupdate", "run", "--force"]).success().stdout(
        contains("Auto-update summary").and(contains("mods redeployed (system updated, forced)")),
    );
    let game_dir = env.tmp.path().join("games/testgame");
    assert_eq!(std::fs::read(game_dir.join("Data/a.esp")).unwrap(), b"A");
    assert_eq!(
        env.query_i64(
            "SELECT COUNT(*) FROM autoupdate_runs WHERE game = 'testgame' AND outcome = 'acted' AND dry_run = 0"
        ),
        1
    );
    env.run(&["autoupdate", "status"])
        .success()
        .stdout(contains("Last runs:").and(contains("acted    testgame: mods redeployed")));
    // A desktop notification carries the summary.
    assert!(env.calls("notify-send").contains("mods redeployed"));
}

#[test]
fn run_without_games_is_quiet() {
    let env = env();
    env.run(&["autoupdate", "run", "--force"])
        .success()
        .stdout(contains("Nothing to do."));
}
