// SPDX-License-Identifier: GPL-3.0-or-later
//! systemd user timer and service (legacy `autoupdate_enable`/
//! `autoupdate_remove`, L4606-4721). The systemd user timer is the only
//! scheduler.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{AutoupdateError, UNIT};
use crate::paths::Paths;

/// Runs `systemctl --user <args>`. Mocked in tests so the real user manager
/// is never touched.
pub trait Systemctl {
    /// Stdout on success; stderr (or the spawn error) on failure.
    fn run(&self, args: &[&str]) -> Result<String, String>;
}

/// The real `systemctl --user`.
#[derive(Debug, Clone, Copy, Default)]
pub struct UserSystemctl;

impl Systemctl for UserSystemctl {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let out = std::process::Command::new("systemctl")
            .arg("--user")
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("cannot run systemctl: {e}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() {
            Ok(stdout)
        } else {
            // `is-active` reports `inactive` on stdout with a non-zero exit.
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(if stderr.is_empty() { stdout } else { stderr })
        }
    }
}

/// What the units run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitSpec {
    /// Absolute path of the `vessel` binary (or `$APPIMAGE`).
    pub exec: PathBuf,
    /// `autoupdate-interval` setting; 0 = only after system/game updates.
    pub interval_hours: u32,
    /// `Environment=` lines (e.g. PATH, XDG dirs). Never secrets.
    pub env: Vec<(String, String)>,
}

/// Hours between timer runs: the setting, or hourly when it is 0 (the
/// fingerprint checks are cheap; legacy always ran hourly).
pub fn timer_hours(interval_hours: u32) -> u32 {
    interval_hours.max(1)
}

/// `$XDG_CONFIG_HOME/systemd/user` (the parent of the vessel config dir).
pub fn systemd_user_dir(paths: &Paths) -> PathBuf {
    paths
        .config_dir
        .parent()
        .map_or_else(|| paths.home.join(".config"), Path::to_path_buf)
        .join("systemd/user")
}

pub fn service_path(paths: &Paths) -> PathBuf {
    systemd_user_dir(paths).join(format!("{UNIT}.service"))
}

pub fn timer_path(paths: &Paths) -> PathBuf {
    systemd_user_dir(paths).join(format!("{UNIT}.timer"))
}

/// Quotes a value for a systemd unit file: `\` and `"` escaped, `%`
/// specifiers doubled.
fn quote(s: &str) -> String {
    let mut q = String::with_capacity(s.len() + 2);
    q.push('"');
    for c in s.chars() {
        match c {
            '\\' | '"' => {
                q.push('\\');
                q.push(c);
            }
            '%' => q.push_str("%%"),
            _ => q.push(c),
        }
    }
    q.push('"');
    q
}

pub fn render_service(spec: &UnitSpec) -> String {
    let mut env = String::new();
    for (k, v) in &spec.env {
        env.push_str(&format!("Environment={}\n", quote(&format!("{k}={v}"))));
    }
    // `$` would be expanded in ExecStart.
    let exec = quote(&spec.exec.to_string_lossy()).replace('$', "$$");
    format!(
        "[Unit]
Description=Vessel: update and redeploy mods after system or game updates
After=network-online.target

[Service]
Type=oneshot
{env}ExecStart={exec} --no-color -y autoupdate run
Nice=10
IOSchedulingClass=idle
"
    )
}

pub fn render_timer(spec: &UnitSpec) -> String {
    let h = timer_hours(spec.interval_hours);
    format!(
        "[Unit]
Description=Vessel: check for system or game updates every {h} h

[Timer]
OnBootSec=3min
OnUnitActiveSec={h}h
RandomizedDelaySec=2min
Persistent=true

[Install]
WantedBy=timers.target
"
    )
}

fn write(path: &Path, text: &str) -> Result<(), AutoupdateError> {
    let io = |source| AutoupdateError::Io {
        path: path.to_path_buf(),
        source,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    std::fs::write(path, text).map_err(io)
}

fn sc(sc: &dyn Systemctl, args: &[&str]) -> Result<String, AutoupdateError> {
    sc.run(args).map_err(|message| AutoupdateError::Systemctl {
        args: args.join(" "),
        message,
    })
}

/// Writes the service and timer and enables the timer. Fails without
/// writing anything when the user manager is unreachable.
pub fn enable(
    paths: &Paths,
    spec: &UnitSpec,
    systemctl: &dyn Systemctl,
) -> Result<(), AutoupdateError> {
    install(paths, spec, systemctl, true)
}

/// Writes the service and timer and reloads the user manager; `start`
/// also enables and starts the timer. Fails without writing anything when
/// the user manager is unreachable.
pub fn install(
    paths: &Paths,
    spec: &UnitSpec,
    systemctl: &dyn Systemctl,
    start: bool,
) -> Result<(), AutoupdateError> {
    sc(systemctl, &["show-environment"]).map_err(|_| AutoupdateError::SystemdUnavailable)?;
    write(&service_path(paths), &render_service(spec))?;
    write(&timer_path(paths), &render_timer(spec))?;
    let timer = format!("{UNIT}.timer");
    let res = sc(systemctl, &["daemon-reload"]).and_then(|s| {
        if start {
            sc(systemctl, &["enable", "--now", &timer])
        } else {
            Ok(s)
        }
    });
    if res.is_err() {
        // Leave no half-installed timer behind.
        let _ = remove_files(paths);
        let _ = systemctl.run(&["daemon-reload"]);
    }
    res.map(drop)
}

fn remove_files(paths: &Paths) -> Result<bool, AutoupdateError> {
    let mut any = false;
    for p in [timer_path(paths), service_path(paths)] {
        match std::fs::remove_file(&p) {
            Ok(()) => any = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(AutoupdateError::Io { path: p, source }),
        }
    }
    Ok(any)
}

/// Stops and removes the timer. `false` = it was not enabled.
pub fn disable(paths: &Paths, systemctl: &dyn Systemctl) -> Result<bool, AutoupdateError> {
    if !timer_path(paths).exists() && !service_path(paths).exists() {
        return Ok(false);
    }
    // Best effort: the user manager may be gone (e.g. outside a session).
    let _ = systemctl.run(&["disable", "--now", &format!("{UNIT}.timer")]);
    remove_files(paths)?;
    let _ = systemctl.run(&["daemon-reload"]);
    Ok(true)
}

/// Timer state for `status`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TimerStatus {
    /// The timer unit file exists.
    pub installed: bool,
    /// `systemctl --user is-active` (e.g. `active`, `inactive`).
    pub active: Option<String>,
    /// `NextElapseUSecRealtime`, as systemd prints it.
    pub next_elapse: Option<String>,
    /// `OnUnitActiveSec` hours written in the timer file.
    pub every_hours: Option<u32>,
}

pub fn timer_status(paths: &Paths, systemctl: &dyn Systemctl) -> TimerStatus {
    let Ok(text) = std::fs::read_to_string(timer_path(paths)) else {
        return TimerStatus::default();
    };
    let timer = format!("{UNIT}.timer");
    // `is-active` exits non-zero for inactive units but still prints the state.
    let active = match systemctl.run(&["is-active", &timer]) {
        Ok(s) => Some(s),
        Err(s) if !s.is_empty() && !s.contains(' ') => Some(s),
        Err(_) => None,
    };
    let next_elapse = systemctl
        .run(&["show", &timer, "-p", "NextElapseUSecRealtime", "--value"])
        .ok()
        .filter(|s| !s.is_empty());
    let every_hours = timer_file_hours(&text);
    TimerStatus {
        installed: true,
        active,
        next_elapse,
        every_hours,
    }
}

/// `OnUnitActiveSec=<n>h` of a timer file written by [`render_timer`].
pub fn timer_file_hours(text: &str) -> Option<u32> {
    text.lines()
        .find_map(|l| l.strip_prefix("OnUnitActiveSec="))
        .and_then(|v| v.trim().strip_suffix('h'))
        .and_then(|v| v.parse().ok())
}
