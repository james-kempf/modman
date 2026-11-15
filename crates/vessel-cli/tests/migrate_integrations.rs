// SPDX-License-Identifier: GPL-3.0-or-later
//! First-launch migration of the legacy auto-update units and nxm handler
//! through the real CLI. systemctl is a fake script (`VESSEL_SYSTEMCTL`);
//! the session bus and keyring are unreachable in the cleared environment.

mod support;

use std::os::unix::fs::PermissionsExt;

use predicates::str::contains;
use support::Env;
use vessel_core::legacy::{LEGACY_APP_DIR, LEGACY_NXM_DESKTOP_FILE, LEGACY_UNIT};

fn write(p: &std::path::Path, body: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

/// Env with a fake systemctl that reports every unit as `enabled`.
fn env() -> Env {
    let mut env = Env::new();
    let log = env.tmp.path().join("systemctl.log");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$2\" in is-enabled) echo enabled;; esac\n",
        log.display()
    );
    let f = env.bin().join("fake-systemctl");
    std::fs::write(&f, script).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env
        .push(("VESSEL_SYSTEMCTL".into(), f.to_string_lossy().into_owned()));
    env
}

// Fails when the CLI start does not replace an enabled legacy timer (same
// interval, enabled) and a legacy default nxm handler, or redoes it later.
#[test]
fn first_start_replaces_legacy_timer_and_nxm_handler() {
    let env = env();
    let home = env.home();
    write(
        &home
            .join(".config")
            .join(LEGACY_APP_DIR)
            .join("games/x.toml"),
        "x = 1\n",
    );
    let units = home.join(".config/systemd/user");
    write(
        &units.join(format!("{LEGACY_UNIT}.timer")),
        "[Timer]\nOnUnitActiveSec=6h\n",
    );
    write(&units.join(format!("{LEGACY_UNIT}.service")), "[Service]\n");
    write(
        &home
            .join(".local/share/applications")
            .join(LEGACY_NXM_DESKTOP_FILE),
        "[Desktop Entry]\n",
    );
    write(
        &home.join(".config/mimeapps.list"),
        &format!("[Default Applications]\nx-scheme-handler/nxm={LEGACY_NXM_DESKTOP_FILE};\n"),
    );

    env.run(&["nxm", "status"])
        .success()
        .stdout(contains("vessel handles nxm:// links."));

    for p in [".timer", ".service"] {
        assert!(!units.join(format!("{LEGACY_UNIT}{p}")).exists());
    }
    let timer = std::fs::read_to_string(units.join("vessel-autoupdate.timer")).unwrap();
    assert!(timer.contains("OnUnitActiveSec=6h"), "{timer}");
    let service = std::fs::read_to_string(units.join("vessel-autoupdate.service")).unwrap();
    assert!(
        service.contains("vessel\" --no-color -y autoupdate run"),
        "{service}"
    );
    let calls = env.calls("systemctl");
    assert!(
        calls.contains(&format!("--user disable --now {LEGACY_UNIT}.timer")),
        "{calls}"
    );
    assert!(
        calls.contains("--user enable --now vessel-autoupdate.timer"),
        "{calls}"
    );
    assert!(!home
        .join(".local/share/applications")
        .join(LEGACY_NXM_DESKTOP_FILE)
        .exists());
    assert!(!home.join(".config").join(LEGACY_APP_DIR).exists());

    env.run(&["nxm", "status"]).success();
    assert_eq!(env.calls("systemctl"), calls);
}
