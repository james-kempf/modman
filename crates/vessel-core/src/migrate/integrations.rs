// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration steps: keyring secret, systemd units, nxm handler and Steam
//! shortcut. Each is idempotent and non-fatal (an `Err` keeps the step
//! pending for the next start) and is a no-op when the legacy item is absent.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use keyring_core::{Entry, Error as KrError};

use super::{KeyringHook, MigrateError, Migration, StepState};
use crate::autoupdate::{self, UnitSpec};
use crate::legacy::{
    LEGACY_APP_DIR, LEGACY_KEYRING_SERVICE, LEGACY_NXM_DESKTOP_FILE, LEGACY_STEAM_APP_NAME,
    LEGACY_UNIT,
};
use crate::nexus::{KEYRING_SERVICE, KEYRING_USER, PROBE_TIMEOUT};

fn fail(msg: impl Into<String>) -> MigrateError {
    MigrateError::Integration(msg.into())
}

/// What the program path in a unit or desktop entry should be: the AppImage
/// when this runs from one, else `cli` itself.
fn launch_path(cli: &Path) -> PathBuf {
    crate::app::actions::timer_exec(cli)
}

/// The CLI as units and desktop entries should call it; `None` when it is
/// not installed (a permanent condition: the caller warns and finishes).
fn cli(m: &Migration<'_>) -> Option<PathBuf> {
    m.hooks.programs.cli.as_deref().map(launch_path)
}

/// Outcome of [`move_secret`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum KeyMove {
    NoLegacy,
    Moved,
    /// Vessel already had a different secret; it wins, the legacy one stays.
    KeptExisting,
    NoKeyring(String),
}

fn kr(e: &KrError) -> String {
    crate::nexus::kr_err(e).to_string()
}

/// Copies the legacy secret to `new`, reads it back, then deletes the
/// legacy one. Never overwrites a secret already in `new`.
pub(super) fn move_secret(old: &Entry, new: &Entry) -> Result<KeyMove, String> {
    let secret = match old.get_password() {
        Ok(s) => s,
        Err(KrError::NoEntry) => return Ok(KeyMove::NoLegacy),
        Err(e) => return Err(kr(&e)),
    };
    match new.get_password() {
        // Same value: an earlier run copied it but did not delete the old.
        Ok(s) if s == secret => {}
        Ok(_) => return Ok(KeyMove::KeptExisting),
        Err(KrError::NoEntry) => {
            new.set_password(&secret).map_err(|e| kr(&e))?;
            if new.get_password().ok().as_deref() != Some(secret.as_str()) {
                return Err("the copied API key did not read back; the old entry is kept".into());
            }
        }
        Err(e) => return Err(kr(&e)),
    }
    match old.delete_credential() {
        Ok(()) | Err(KrError::NoEntry) => Ok(KeyMove::Moved),
        Err(e) => Err(kr(&e)),
    }
}

/// Opens the keyring through `open` and runs [`move_secret`] on a worker:
/// the Secret Service can hang. Unreachable or silent = `NoKeyring`.
fn move_in_keyring(open: KeyringHook) -> Result<KeyMove, String> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("migrate-keyring".into())
        .spawn(move || {
            let r = (|| {
                let store = match open() {
                    Ok(s) => s,
                    Err(e) => return Ok(KeyMove::NoKeyring(e.to_string())),
                };
                let entry = |svc| store.build(svc, KEYRING_USER, None).map_err(|e| kr(&e));
                move_secret(&entry(LEGACY_KEYRING_SERVICE)?, &entry(KEYRING_SERVICE)?)
            })();
            let _ = tx.send(r);
        })
        .map_err(|e| format!("cannot start the keyring worker: {e}"))?;
    rx.recv_timeout(PROBE_TIMEOUT)
        .unwrap_or_else(|_| Ok(KeyMove::NoKeyring("it did not answer in time".into())))
}

/// Moves the Nexus API key from the legacy keyring service to ours. An
/// unreachable keyring does not block the migration: the step is done, and
/// `legacy_key_pending` makes the key lookup finish the move later
/// ([`retry_legacy_key`]).
pub(super) fn keyring(m: &mut Migration<'_>) -> Result<StepState, MigrateError> {
    let outcome =
        move_in_keyring(m.hooks.keyring.clone()).map_err(|e| fail(format!("keyring: {e}")))?;
    m.marker.legacy_key_pending = matches!(outcome, KeyMove::NoKeyring(_));
    match outcome {
        KeyMove::NoLegacy => {}
        KeyMove::Moved => m
            .report
            .notes
            .push("moved the Nexus API key to the Vessel keyring entry".into()),
        KeyMove::KeptExisting => m.report.notes.push(format!(
            "the keyring already holds a Vessel API key; the {LEGACY_KEYRING_SERVICE} entry was left as is"
        )),
        KeyMove::NoKeyring(r) => m.report.warnings.push(format!(
            "the keyring was unreachable ({r}), so a Nexus API key stored there by {LEGACY_APP_DIR} \
             (if any) was not moved yet; Vessel moves it the next time it reads the key"
        )),
    }
    Ok(StepState::Done)
}

/// Result of [`retry_legacy_key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyKeyRetry {
    /// Nothing pending, or a migration holds the lock: nothing done.
    Skipped,
    /// The keyring is still unreachable; the move stays pending.
    Unreachable,
    /// The keyring answered and the flag was cleared; `moved` = the Vessel
    /// entry now holds the legacy key (re-read it).
    Done { moved: bool },
}

/// Finishes a keyring move the migration had to defer, holding the
/// migration lock (skipped while another process holds it). Only
/// `legacy_key_pending` of the freshly reloaded marker is changed.
pub fn retry_legacy_key(paths: &crate::paths::Paths, open: KeyringHook) -> LegacyKeyRetry {
    let path = paths.config_dir.join(super::MARKER_FILE);
    let pending = |p: &Path| matches!(super::Marker::load(p), Ok(Some(m)) if m.legacy_key_pending);
    // Cheap check first: the common case takes no lock.
    if !pending(&path) {
        return LegacyKeyRetry::Skipped;
    }
    let Ok(_lock) = super::MigrationLock::try_acquire(&super::lock_path(paths)) else {
        return LegacyKeyRetry::Skipped;
    };
    if !pending(&path) {
        return LegacyKeyRetry::Skipped;
    }
    let moved = match move_in_keyring(open) {
        Ok(KeyMove::NoKeyring(_)) | Err(_) => return LegacyKeyRetry::Unreachable,
        Ok(outcome) => outcome == KeyMove::Moved,
    };
    // Reload under the lock so no concurrent change to the marker is lost.
    match super::Marker::load(&path) {
        Ok(Some(mut marker)) => {
            marker.legacy_key_pending = false;
            if let Err(e) = marker.save(&path) {
                tracing::warn!("migration: cannot update {}: {e}", path.display());
            }
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("migration: cannot read {}: {e}", path.display()),
    }
    LegacyKeyRetry::Done { moved }
}

fn remove_file(p: &Path) -> Result<(), MigrateError> {
    match std::fs::remove_file(p) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(MigrateError::io(p, e)),
        _ => Ok(()),
    }
}

/// Replaces the legacy auto-update units with ours: same interval, enabled
/// only if the legacy timer was. The new units go in first, so a crash
/// before the old ones are removed just reruns this step.
pub(super) fn systemd_units(m: &mut Migration<'_>) -> Result<StepState, MigrateError> {
    let dir = autoupdate::systemd_user_dir(m.paths);
    let timer = format!("{LEGACY_UNIT}.timer");
    let old = [dir.join(&timer), dir.join(format!("{LEGACY_UNIT}.service"))];
    if !old.iter().any(|p| p.exists()) {
        return Ok(StepState::Done);
    }
    let hours = std::fs::read_to_string(&old[0])
        .ok()
        .and_then(|t| autoupdate::timer_file_hours(&t))
        .unwrap_or(0);
    let enabled = m.marker.legacy_timer_enabled.unwrap_or(false);
    let Some(exec) = cli(m) else {
        // The old units call the removed legacy program: drop them and
        // tell the user how to turn auto-update back on.
        remove_legacy_units(m, &dir, &old)?;
        if enabled {
            m.report.warnings.push(format!(
                "auto-update was turned off: the vessel command-line program was not found, so the \
                 {LEGACY_UNIT} timer (every {} h) could not be replaced; install it and run \
                 'vessel autoupdate enable'",
                autoupdate::timer_hours(hours)
            ));
        }
        return Ok(StepState::Done);
    };
    let spec = UnitSpec {
        exec,
        interval_hours: hours,
        env: crate::app::actions::unit_env(),
    };
    autoupdate::install(m.paths, &spec, m.hooks.systemctl, enabled)
        .map_err(|e| fail(e.to_string()))?;
    remove_legacy_units(m, &dir, &old)?;
    m.report.notes.push(format!(
        "replaced the {LEGACY_UNIT} units ({}, every {} h)",
        if enabled { "enabled" } else { "not enabled" },
        autoupdate::timer_hours(hours)
    ));
    Ok(StepState::Done)
}

fn remove_legacy_units(
    m: &Migration<'_>,
    dir: &Path,
    old: &[PathBuf; 2],
) -> Result<(), MigrateError> {
    let wants = dir
        .join("timers.target.wants")
        .join(format!("{LEGACY_UNIT}.timer"));
    for p in old.iter().chain([&wants]) {
        remove_file(p)?;
    }
    let _ = m.hooks.systemctl.run(&["daemon-reload"]);
    Ok(())
}

/// Replaces a user-level legacy nxm handler with ours (R13).
pub(super) fn nxm_handler(m: &mut Migration<'_>) -> Result<StepState, MigrateError> {
    let apps = crate::nxm_handler::applications_dir(m.paths);
    if !apps.join(LEGACY_NXM_DESKTOP_FILE).is_file() {
        return Ok(StepState::Done);
    }
    let Some(program) = cli(m) else {
        m.report.warnings.push(format!(
            "the {LEGACY_NXM_DESKTOP_FILE} nxm:// handler was left in place because the vessel \
             command-line program was not found; install it and run 'vessel nxm register'"
        ));
        return Ok(StepState::Done);
    };
    let program = program
        .to_str()
        .ok_or_else(|| fail("the vessel path is not valid UTF-8"))?;
    crate::nxm_handler::replace_legacy(m.paths, LEGACY_NXM_DESKTOP_FILE, program)
        .map_err(|e| fail(format!("nxm:// handler: {e}")))?;
    m.report.notes.push(format!(
        "replaced the {LEGACY_NXM_DESKTOP_FILE} nxm:// handler"
    ));
    Ok(StepState::Done)
}

/// Renames the legacy Steam shortcut in place, keeping its appid (R14).
pub(super) fn steam_shortcut(m: &mut Migration<'_>) -> Result<StepState, MigrateError> {
    let r = crate::app::shortcut::migrate_legacy(&m.paths.home, m.hooks.programs.gui.as_deref())
        .map_err(|e| fail(format!("Steam shortcut: {e}")))?;
    if r.accounts > 0 {
        m.report.notes.push(format!(
            "renamed the Steam shortcut to Vessel in {} account(s)",
            r.accounts
        ));
    }
    if r.left > 0 {
        m.report.warnings.push(format!(
            "the Steam shortcut '{LEGACY_STEAM_APP_NAME}' was left unchanged in {} account(s) \
             because the Vessel app was not found; use 'Add to Steam' in Vessel's settings to \
             update it (its appid, playtime and artwork are kept)",
            r.left
        ));
    }
    if r.steam_running {
        m.report
            .warnings
            .push("Steam is running: restart it to see the renamed Vessel shortcut".into());
    }
    Ok(StepState::Done)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::sync::{Arc, Mutex};

    use keyring_core::api::CredentialApi;
    use keyring_core::{Credential, CredentialStore};

    use super::*;
    use crate::autoupdate::Systemctl;
    use crate::ipc::{NXM_DESKTOP_FILE, NXM_SCHEMES};
    use crate::migrate::{BusProbe, Faults, Hooks, KeyringHook, Marker, Programs, Sources};
    use crate::nxm_handler::{applications_dir, mimeapps_path};
    use crate::paths::Paths;

    struct NoBus;
    impl BusProbe for NoBus {
        fn has_owner(&self, _: &str) -> bool {
            false
        }
    }

    /// Records calls; `.1` = the user manager is unreachable.
    #[derive(Default)]
    struct Ctl(RefCell<Vec<String>>, std::cell::Cell<bool>);
    impl Systemctl for Ctl {
        fn run(&self, args: &[&str]) -> Result<String, String> {
            self.0.borrow_mut().push(args.join(" "));
            if self.1.get() {
                return Err("Failed to connect to bus".into());
            }
            Ok(String::new())
        }
    }

    const CLI: &str = "/opt/vessel/vessel";

    struct Fx {
        _tmp: tempfile::TempDir,
        paths: Paths,
        ctl: Ctl,
        store: Arc<CredentialStore>,
        keyring: KeyringHook,
        cli: Option<PathBuf>,
        timer_enabled: Option<bool>,
    }

    impl Fx {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let store: Arc<CredentialStore> = keyring_core::mock::Store::new().unwrap();
            let s = store.clone();
            Self {
                paths: Paths::for_home(tmp.path()),
                _tmp: tmp,
                ctl: Ctl::default(),
                store,
                keyring: Arc::new(move || Ok(s.clone())),
                cli: Some(CLI.into()),
                timer_enabled: None,
            }
        }

        /// Runs one step; returns its result and the report notes/warnings.
        fn step(
            &self,
            f: fn(&mut Migration<'_>) -> Result<StepState, MigrateError>,
        ) -> (Result<StepState, MigrateError>, Vec<String>, Marker) {
            let hooks = Hooks {
                bus: &NoBus,
                systemctl: &self.ctl,
                keyring: self.keyring.clone(),
                programs: Programs {
                    cli: self.cli.clone(),
                    gui: None,
                },
                faults: Faults::default(),
            };
            let dir = self.paths.home.join("old");
            let mut marker = Marker::new(Sources {
                config: dir.clone(),
                data: dir.clone(),
                state: dir,
            });
            marker.legacy_timer_enabled = self.timer_enabled;
            let mut m = Migration {
                paths: &self.paths,
                hooks: &hooks,
                marker,
                report: Default::default(),
                marker_path: self.paths.config_dir.join(crate::migrate::MARKER_FILE),
            };
            let r = f(&mut m);
            let msgs = m
                .report
                .notes
                .into_iter()
                .chain(m.report.warnings)
                .collect();
            (r, msgs, m.marker)
        }

        fn entry(&self, service: &str) -> Entry {
            self.store.build(service, KEYRING_USER, None).unwrap()
        }

        fn secret(&self, service: &str) -> Option<String> {
            self.entry(service).get_password().ok()
        }

        fn calls(&self) -> Vec<String> {
            self.ctl.0.borrow().clone()
        }
    }

    // ---- keyring ----

    // Fails when the key is not moved, or the old copy survives a move.
    #[test]
    fn keyring_secret_moves_to_the_vessel_entry() {
        let fx = Fx::new();
        fx.entry(LEGACY_KEYRING_SERVICE)
            .set_password("KEY1")
            .unwrap();
        let (r, msgs, _) = fx.step(keyring);
        assert_eq!(r.unwrap(), StepState::Done);
        assert_eq!(fx.secret(KEYRING_SERVICE).as_deref(), Some("KEY1"));
        assert_eq!(fx.secret(LEGACY_KEYRING_SERVICE), None);
        assert!(msgs.iter().any(|m| m.contains("moved")), "{msgs:?}");
        // Rerun: nothing left to move.
        assert_eq!(fx.step(keyring).0.unwrap(), StepState::Done);
        assert_eq!(fx.secret(KEYRING_SERVICE).as_deref(), Some("KEY1"));
    }

    // Fails when an existing Vessel key is overwritten by the legacy one.
    #[test]
    fn keyring_never_overwrites_a_vessel_secret() {
        let fx = Fx::new();
        fx.entry(KEYRING_SERVICE).set_password("MINE").unwrap();
        fx.entry(LEGACY_KEYRING_SERVICE)
            .set_password("OLD")
            .unwrap();
        assert_eq!(fx.step(keyring).0.unwrap(), StepState::Done);
        assert_eq!(fx.secret(KEYRING_SERVICE).as_deref(), Some("MINE"));
        assert_eq!(fx.secret(LEGACY_KEYRING_SERVICE).as_deref(), Some("OLD"));
    }

    // Fails when an empty keyring gets entries, or an unreachable keyring
    // blocks the migration forever.
    #[test]
    fn keyring_without_legacy_secret_is_a_noop() {
        let fx = Fx::new();
        let (r, _, marker) = fx.step(keyring);
        assert_eq!(r.unwrap(), StepState::Done);
        assert_eq!(fx.secret(KEYRING_SERVICE), None);
        assert!(!marker.legacy_key_pending);
    }

    // Fails when an unreachable keyring blocks cleanup forever, is silent,
    // or the deferred move is never finished by the key lookup.
    #[test]
    fn unreachable_keyring_defers_the_move_to_the_key_lookup() {
        let mut fx = Fx::new();
        fx.entry(LEGACY_KEYRING_SERVICE)
            .set_password("KEY1")
            .unwrap();
        let working = fx.keyring.clone();
        fx.keyring = Arc::new(|| Err(KrError::NoDefaultStore));
        let (r, msgs, marker) = fx.step(keyring);
        assert_eq!(r.unwrap(), StepState::Done);
        assert!(marker.legacy_key_pending);
        assert!(msgs.iter().any(|m| m.contains("not moved yet")), "{msgs:?}");
        let path = fx.paths.config_dir.join(crate::migrate::MARKER_FILE);
        marker.save(&path).unwrap();

        // Still unreachable: nothing moves, the flag stays.
        assert_eq!(
            retry_legacy_key(&fx.paths, fx.keyring.clone()),
            LegacyKeyRetry::Unreachable
        );
        assert!(Marker::load(&path).unwrap().unwrap().legacy_key_pending);
        // Reachable: moved and the flag cleared.
        assert_eq!(
            retry_legacy_key(&fx.paths, working.clone()),
            LegacyKeyRetry::Done { moved: true }
        );
        assert_eq!(fx.secret(KEYRING_SERVICE).as_deref(), Some("KEY1"));
        assert_eq!(fx.secret(LEGACY_KEYRING_SERVICE), None);
        assert!(!Marker::load(&path).unwrap().unwrap().legacy_key_pending);
        assert_eq!(
            retry_legacy_key(&fx.paths, working),
            LegacyKeyRetry::Skipped
        );
    }

    // Fails when the deferred key move writes migration.json while a
    // migration holds vessel-migration.lock (lost update).
    #[test]
    fn key_retry_skips_while_the_migration_lock_is_held() {
        let fx = Fx::new();
        fx.entry(LEGACY_KEYRING_SERVICE)
            .set_password("KEY1")
            .unwrap();
        let path = fx.paths.config_dir.join(crate::migrate::MARKER_FILE);
        let mut marker = Marker::new(Sources {
            config: "/x".into(),
            data: "/x".into(),
            state: "/x".into(),
        });
        marker.legacy_key_pending = true;
        marker.save(&path).unwrap();
        let before = fs::read(&path).unwrap();
        let held =
            crate::migrate::MigrationLock::try_acquire(&crate::migrate::lock_path(&fx.paths))
                .unwrap();
        assert_eq!(
            retry_legacy_key(&fx.paths, fx.keyring.clone()),
            LegacyKeyRetry::Skipped
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fx.secret(LEGACY_KEYRING_SERVICE).as_deref(), Some("KEY1"));
        drop(held);
        assert_eq!(
            retry_legacy_key(&fx.paths, fx.keyring.clone()),
            LegacyKeyRetry::Done { moved: true }
        );
    }

    /// A credential that stores a corrupted copy of what it is given.
    #[derive(Default)]
    struct Lossy(Mutex<Option<Vec<u8>>>);
    impl CredentialApi for Lossy {
        fn set_secret(&self, s: &[u8]) -> keyring_core::Result<()> {
            *self.0.lock().unwrap() = Some(s[1..].to_vec());
            Ok(())
        }
        fn get_secret(&self) -> keyring_core::Result<Vec<u8>> {
            self.0.lock().unwrap().clone().ok_or(KrError::NoEntry)
        }
        fn delete_credential(&self) -> keyring_core::Result<()> {
            Ok(())
        }
        fn get_credential(&self) -> keyring_core::Result<Option<Arc<Credential>>> {
            Ok(None)
        }
        fn get_specifiers(&self) -> Option<(String, String)> {
            None
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    // Fails when the old key is deleted although the copy did not read back.
    #[test]
    fn keyring_keeps_the_old_secret_when_read_back_fails() {
        let fx = Fx::new();
        let old = fx.entry(LEGACY_KEYRING_SERVICE);
        old.set_password("KEY1").unwrap();
        let new = Entry::new_with_credential(Arc::new(Lossy::default()));
        move_secret(&old, &new).unwrap_err();
        assert_eq!(fx.secret(LEGACY_KEYRING_SERVICE).as_deref(), Some("KEY1"));
    }

    // ---- systemd ----

    fn legacy_units(fx: &Fx, hours: u32) -> [PathBuf; 2] {
        let dir = autoupdate::systemd_user_dir(&fx.paths);
        fs::create_dir_all(&dir).unwrap();
        let t = dir.join(format!("{LEGACY_UNIT}.timer"));
        let s = dir.join(format!("{LEGACY_UNIT}.service"));
        fs::write(
            &t,
            format!("[Timer]\nOnBootSec=3min\nOnUnitActiveSec={hours}h\n"),
        )
        .unwrap();
        fs::write(&s, "[Service]\nExecStart=/usr/bin/legacy autoupdate run\n").unwrap();
        [t, s]
    }

    fn enable_call() -> String {
        format!("enable --now {}.timer", autoupdate::UNIT)
    }

    // Fails when the interval is lost, the old units stay, or the new
    // timer is not enabled although the old one was.
    #[test]
    fn enabled_legacy_timer_is_replaced_with_same_interval() {
        let mut fx = Fx::new();
        fx.timer_enabled = Some(true);
        let old = legacy_units(&fx, 6);
        assert_eq!(fx.step(systemd_units).0.unwrap(), StepState::Done);
        assert!(old.iter().all(|p| !p.exists()));
        let timer = fs::read_to_string(autoupdate::timer_path(&fx.paths)).unwrap();
        assert!(timer.contains("OnUnitActiveSec=6h"), "{timer}");
        let service = fs::read_to_string(autoupdate::service_path(&fx.paths)).unwrap();
        assert!(service.contains(CLI), "{service}");
        let calls = fx.calls();
        assert!(calls.contains(&enable_call()), "{calls:?}");
        assert_eq!(calls.last().map(String::as_str), Some("daemon-reload"));
        // Rerun: no old units, nothing to do.
        fx.ctl.0.borrow_mut().clear();
        assert_eq!(fx.step(systemd_units).0.unwrap(), StepState::Done);
        assert!(fx.calls().is_empty());
    }

    // Fails when a disabled legacy timer comes back enabled.
    #[test]
    fn disabled_legacy_timer_is_installed_but_not_enabled() {
        let mut fx = Fx::new();
        fx.timer_enabled = Some(false);
        legacy_units(&fx, 12);
        assert_eq!(fx.step(systemd_units).0.unwrap(), StepState::Done);
        assert!(autoupdate::timer_path(&fx.paths).is_file());
        assert!(!fx.calls().contains(&enable_call()), "{:?}", fx.calls());
    }

    // Fails when units are installed for a user who never had the timer.
    #[test]
    fn no_legacy_units_is_a_noop() {
        let fx = Fx::new();
        assert_eq!(fx.step(systemd_units).0.unwrap(), StepState::Done);
        assert!(fx.calls().is_empty());
        assert!(!autoupdate::timer_path(&fx.paths).exists());
    }

    // Fails when a transient systemd failure loses the old units instead of
    // keeping the step pending for a retry.
    #[test]
    fn unreachable_user_manager_keeps_the_old_units_for_a_retry() {
        let mut fx = Fx::new();
        fx.timer_enabled = Some(true);
        fx.ctl.1.set(true);
        let old = legacy_units(&fx, 6);
        fx.step(systemd_units).0.unwrap_err();
        assert!(old.iter().all(|p| p.exists()));
        assert!(!autoupdate::timer_path(&fx.paths).exists());
    }

    // Fails when a missing CLI blocks cleanup forever (step never done) or
    // the user is not told what was left to do.
    #[test]
    fn missing_cli_finishes_with_warnings() {
        let mut fx = Fx::new();
        fx.cli = None;
        fx.timer_enabled = Some(true);
        let old = legacy_units(&fx, 6);
        let (r, msgs, _) = fx.step(systemd_units);
        assert_eq!(r.unwrap(), StepState::Done);
        assert!(old.iter().all(|p| !p.exists()));
        assert!(!autoupdate::timer_path(&fx.paths).exists());
        assert!(
            msgs.iter()
                .any(|m| m.contains("every 6 h") && m.contains("'vessel autoupdate enable'")),
            "{msgs:?}"
        );

        let nxm = legacy_nxm(
            &fx,
            &format!(
                "[Default Applications]\n{}",
                defaults(LEGACY_NXM_DESKTOP_FILE)
            ),
        );
        let (r, msgs, _) = fx.step(nxm_handler);
        assert_eq!(r.unwrap(), StepState::Done);
        assert!(nxm.exists());
        assert!(
            msgs.iter().any(|m| m.contains("'vessel nxm register'")),
            "{msgs:?}"
        );
    }

    // ---- nxm ----

    fn legacy_nxm(fx: &Fx, mime: &str) -> PathBuf {
        let f = applications_dir(&fx.paths).join(LEGACY_NXM_DESKTOP_FILE);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(&f, "[Desktop Entry]\nExec=legacy nxm %u\n").unwrap();
        let m = mimeapps_path(&fx.paths);
        fs::create_dir_all(m.parent().unwrap()).unwrap();
        fs::write(&m, mime).unwrap();
        f
    }

    fn defaults(handler: &str) -> String {
        NXM_SCHEMES
            .iter()
            .map(|s| format!("{s}={handler};\n"))
            .collect()
    }

    // Fails when a legacy default handler is not replaced with ours, or a
    // reference to the removed legacy entry is left behind.
    #[test]
    fn legacy_nxm_registration_is_replaced() {
        let fx = Fx::new();
        let mime = format!(
            "[Default Applications]\n{}[Added Associations]\nx-scheme-handler/nxm={LEGACY_NXM_DESKTOP_FILE};\n",
            defaults(LEGACY_NXM_DESKTOP_FILE)
        );
        let old = legacy_nxm(&fx, &mime);
        assert_eq!(fx.step(nxm_handler).0.unwrap(), StepState::Done);
        assert!(!old.exists());
        let entry = fs::read_to_string(applications_dir(&fx.paths).join(NXM_DESKTOP_FILE)).unwrap();
        assert!(entry.contains(&format!("Exec=\"{CLI}\" nxm %u")), "{entry}");
        let text = fs::read_to_string(mimeapps_path(&fx.paths)).unwrap();
        assert!(text.contains(&defaults(NXM_DESKTOP_FILE)), "{text}");
        assert!(!text.contains(LEGACY_NXM_DESKTOP_FILE), "{text}");
        assert!(crate::nxm_handler::registered(&fx.paths));
    }

    // Fails when another handler the user chose as default is taken over.
    #[test]
    fn foreign_nxm_default_is_left_alone() {
        let fx = Fx::new();
        let mime = format!("[Default Applications]\n{}", defaults("some-other.desktop"));
        legacy_nxm(&fx, &mime);
        assert_eq!(fx.step(nxm_handler).0.unwrap(), StepState::Done);
        assert_eq!(fs::read_to_string(mimeapps_path(&fx.paths)).unwrap(), mime);
    }

    // Fails when a handler is registered for a user who never had one.
    #[test]
    fn unregistered_nxm_is_a_noop() {
        let fx = Fx::new();
        let m = mimeapps_path(&fx.paths);
        fs::create_dir_all(m.parent().unwrap()).unwrap();
        fs::write(&m, "[Default Applications]\nx=y;\n").unwrap();
        assert_eq!(fx.step(nxm_handler).0.unwrap(), StepState::Done);
        assert!(!applications_dir(&fx.paths).exists());
        assert_eq!(
            fs::read_to_string(&m).unwrap(),
            "[Default Applications]\nx=y;\n"
        );
    }
}
