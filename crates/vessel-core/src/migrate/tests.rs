// SPDX-License-Identifier: GPL-3.0-or-later
//! Migration tests over a synthetic legacy layout in a temp home.

use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use super::*;
use crate::legacy::{
    LEGACY_DB_FILE, LEGACY_KEYRING_SERVICE, LEGACY_LOG_FILE, LEGACY_NXM_DESKTOP_FILE,
    LEGACY_STAGING_DIR, LEGACY_UNIT,
};
use crate::store::Store;

struct Bus(bool);
impl BusProbe for Bus {
    fn has_owner(&self, name: &str) -> bool {
        assert_eq!(name, LEGACY_DBUS_NAME);
        self.0
    }
}

/// Records calls; every unit reports `enabled`.
#[derive(Default)]
struct Ctl(RefCell<Vec<String>>);
impl Systemctl for Ctl {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        self.0.borrow_mut().push(args.join(" "));
        Ok(if args.first() == Some(&"is-enabled") {
            "enabled".into()
        } else {
            String::new()
        })
    }
}

/// A keyring backed by a fresh in-memory mock store.
pub(super) fn mock_keyring() -> (KeyringHook, Arc<keyring_core::CredentialStore>) {
    let store: Arc<keyring_core::CredentialStore> = keyring_core::mock::Store::new().unwrap();
    let s = store.clone();
    (Arc::new(move || Ok(s.clone())), store)
}

/// Hooks with an empty mock keyring and no known programs.
fn hooks<'a>(bus: &'a dyn BusProbe, ctl: &'a dyn Systemctl, faults: Faults) -> Hooks<'a> {
    Hooks {
        bus,
        systemctl: ctl,
        keyring: mock_keyring().0,
        programs: Programs::default(),
        faults,
    }
}

/// A legacy install: config, WAL-mode DB with rows only in -wal, log,
/// downloads + backups + per-game data, a Steam library with legacy staging
/// and symlink/hardlink/copy deployments.
struct Fx {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    paths: Paths,
    old_config: PathBuf,
    old_data: PathBuf,
    old_state: PathBuf,
    lib: PathBuf,
    game: PathBuf,
    env: EnvSnapshot,
}

fn mtime_ns(p: &Path) -> i64 {
    let m = fs::symlink_metadata(p).unwrap();
    m.mtime() * 1_000_000_000 + m.mtime_nsec()
}

fn write(p: &Path, body: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

impl Fx {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().to_path_buf();
        let paths = Paths::for_home(&home);
        let old_config = home.join(".config").join(LEGACY_APP_DIR);
        let old_data = home.join(".local/share").join(LEGACY_APP_DIR);
        let old_state = home.join(".local/state").join(LEGACY_APP_DIR);
        let env = EnvSnapshot::default();
        let lib = home.join("lib");
        let game = lib.join("steamapps/common/Game");
        let fx = Self {
            _tmp: tmp,
            home,
            paths,
            old_config,
            old_data,
            old_state,
            lib,
            game,
            env,
        };
        fx.populate();
        fx
    }

    fn old_staging(&self) -> PathBuf {
        self.lib.join(LEGACY_STAGING_DIR).join("skyrim/mod1")
    }

    fn new_staging(&self) -> PathBuf {
        self.lib.join(STAGING_DIR).join("skyrim/mod1")
    }

    fn populate(&self) {
        write(&self.old_config.join("games/custom.toml"), "x = 1\n");
        write(&self.old_state.join(LEGACY_LOG_FILE), "old log line\n");
        write(&self.old_data.join("skyrim/downloads/a.zip"), "zip bytes");
        write(&self.old_data.join("skyrim/downloads/b.zip"), "more zip");
        write(&self.old_data.join("skyrim/plugins.managed"), "p\n");
        write(&self.old_data.join("backups/ab/abcd"), "backup blob");
        let st = self.old_staging();
        for f in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            write(&st.join(f), &format!("content {f}"));
        }
        fs::create_dir_all(&self.game).unwrap();
        std::os::unix::fs::symlink(st.join("a.txt"), self.game.join("a.txt")).unwrap();
        fs::hard_link(st.join("b.txt"), self.game.join("b.txt")).unwrap();
        fs::copy(st.join("c.txt"), self.game.join("c.txt")).unwrap();

        let db = self.old_data.join(LEGACY_DB_FILE);
        let store = Store::open(&db).unwrap();
        let c = store.conn();
        c.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        let d = s(&self.old_data);
        c.execute_batch(&format!(
            "INSERT INTO settings VALUES ('data-dir', '{d}');
             INSERT INTO games (domain, name, folder, added_at) VALUES ('skyrim', 'Skyrim', '{}', 0);
             INSERT INTO mods (id, game, uid, name, archive, staging_dir, priority, installed_at)
               VALUES (1, 'skyrim', 'm1', 'Mod', '{d}/skyrim/downloads/a.zip', '{}', 0, 0);
             INSERT INTO backups (id, game, target, rel_path, sha256, store_path, size, mode, created_at)
               VALUES (1, 'skyrim', 'game_dir', 'x', 'abcd', '{d}/backups/ab/abcd', 1, 420, 0);
             INSERT INTO downloads (id, source, dest_path, part_path, created_at, updated_at)
               VALUES (1, 'nxm', '{d}/skyrim/downloads/a.zip', '{d}/skyrim/downloads/a.zip.part', 0, 0);
             INSERT INTO collections (id, game, slug, revision, dir, installed_at)
               VALUES (1, 'skyrim', 'c', 1, '{d}/skyrim/collections/c', 0);
             INSERT INTO deploy_journal (id, game, op, state, started_at)
               VALUES (1, 'skyrim', 'deploy', 'committed', 0);",
            s(&self.game),
            s(&st),
        ))
        .unwrap();
        for (i, (f, method)) in [
            ("a.txt", "symlink"),
            ("b.txt", "hardlink"),
            ("c.txt", "copy"),
        ]
        .iter()
        .enumerate()
        {
            let game_file = self.game.join(f);
            let meta = fs::symlink_metadata(&game_file).unwrap();
            let source = s(&st.join(f));
            c.execute(
                "INSERT INTO deployments (game, target, rel_path, mod_id, method, source, inode, size, mtime_ns, deployed_at)
                 VALUES ('skyrim', 'game_dir', ?1, 1, ?2, ?3, ?4, ?5, ?6, 0)",
                params![f, method, source, meta.ino() as i64, meta.size() as i64, mtime_ns(&game_file)],
            )
            .unwrap();
            let detail = serde_json::json!({"source": source, "mod_id": 1, "backup_id": null});
            c.execute(
                "INSERT INTO deploy_journal_steps (journal_id, seq, action, target, rel_path, detail)
                 VALUES (1, ?1, 'place', 'game_dir', ?2, ?3)",
                params![i as i64, f, detail.to_string()],
            )
            .unwrap();
        }
        // Snapshot DB + WAL while the rows exist only in the WAL, then put
        // that snapshot back after close (which would checkpoint).
        let wal = super::with_suffix(&db, "-wal");
        let snap_db = fs::read(&db).unwrap();
        let snap_wal = fs::read(&wal).unwrap();
        assert!(!snap_wal.is_empty());
        drop(store);
        fs::write(&db, snap_db).unwrap();
        fs::write(&wal, snap_wal).unwrap();
        let _ = fs::remove_file(super::with_suffix(&db, "-shm"));
    }

    fn run(&self, faults: Faults) -> Result<MigrationReport, MigrateError> {
        self.run_bus(faults, false)
    }

    fn run_bus(&self, faults: Faults, owned: bool) -> Result<MigrationReport, MigrateError> {
        let ctl = Ctl::default();
        let bus = Bus(owned);
        let hooks = hooks(&bus, &ctl, faults);
        run_with(&self.paths, &self.env, &hooks)
    }

    fn vessel_db(&self) -> Connection {
        Connection::open(self.paths.db_path()).unwrap()
    }

    fn marker(&self) -> Option<Marker> {
        Marker::load(&self.paths.config_dir.join(MARKER_FILE)).unwrap()
    }

    fn lock_file(&self) -> PathBuf {
        self.paths.data_dir.parent().unwrap().join(LOCK_FILE)
    }

    fn no_vessel_state(&self) -> bool {
        !self.paths.config_dir.exists()
            && !self.paths.data_dir.exists()
            && !self.paths.state_dir.exists()
    }
}

fn one(c: &Connection, sql: &str) -> String {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// The full end state of a successful migration.
fn assert_migrated(fx: &Fx) {
    let m = fx.marker().expect("marker");
    assert!(m.finished_at.is_some(), "marker finished: {m:?}");
    assert!(!fx.old_config.exists() && !fx.old_data.exists() && !fx.old_state.exists());
    assert!(!fx.lib.join(LEGACY_STAGING_DIR).exists());
    assert_eq!(
        fs::read_to_string(fx.paths.config_dir.join("games/custom.toml")).unwrap(),
        "x = 1\n"
    );
    assert_eq!(
        fs::read_to_string(fx.paths.log_path()).unwrap(),
        "old log line\n"
    );
    let nd = fx.paths.data_dir.join("skyrim/downloads");
    assert_eq!(fs::read_to_string(nd.join("a.zip")).unwrap(), "zip bytes");
    assert_eq!(fs::read_to_string(nd.join("b.zip")).unwrap(), "more zip");
    assert_eq!(
        fs::read_to_string(fx.paths.data_dir.join("backups/ab/abcd")).unwrap(),
        "backup blob"
    );
    let st = fx.new_staging();
    for f in ["a.txt", "b.txt", "c.txt", "d.txt"] {
        assert_eq!(
            fs::read_to_string(st.join(f)).unwrap(),
            format!("content {f}")
        );
    }

    let c = fx.vessel_db();
    let d = s(&fx.paths.data_dir);
    assert_eq!(
        one(&c, "SELECT archive FROM mods"),
        format!("{d}/skyrim/downloads/a.zip")
    );
    assert_eq!(one(&c, "SELECT staging_dir FROM mods"), s(&st));
    assert_eq!(
        one(&c, "SELECT store_path FROM backups"),
        format!("{d}/backups/ab/abcd")
    );
    assert_eq!(
        one(&c, "SELECT part_path FROM downloads"),
        format!("{d}/skyrim/downloads/a.zip.part")
    );
    assert_eq!(
        one(&c, "SELECT dir FROM collections"),
        format!("{d}/skyrim/collections/c")
    );
    assert_eq!(
        one(&c, "SELECT value FROM settings WHERE key = 'data-dir'"),
        d
    );
    let mut stmt = c
        .prepare("SELECT rel_path, detail FROM deploy_journal_steps ORDER BY seq")
        .unwrap();
    let details: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(details.len(), 3);
    for (rel, detail) in details {
        let j: serde_json::Value = serde_json::from_str(&detail).unwrap();
        assert_eq!(j["source"], s(&st.join(&rel)));
        assert_eq!(j["mod_id"], 1);
    }
    assert_eq!(
        fs::read_link(fx.game.join("a.txt")).unwrap(),
        st.join("a.txt")
    );

    let game = crate::games::list_games(&c).unwrap().remove(0);
    let v = crate::deploy::verify(&c, &game).unwrap();
    assert!(v.issues.is_empty(), "{:?}", v.issues);
    assert_eq!(v.intact, 3);
}

#[test]
fn happy_path_migrates_dirs_db_staging_and_deployments() {
    let fx = Fx::new();
    let r = fx.run(Faults::default()).unwrap();
    assert!(r.ran && r.finished, "{r:?}");
    assert!(r.conflicts.is_empty() && r.warnings.is_empty(), "{r:?}");
    assert_migrated(&fx);
}

#[test]
fn second_run_is_a_noop() {
    let fx = Fx::new();
    fx.run(Faults::default()).unwrap();
    let marker = fx.marker();
    let db = fs::read(fx.paths.db_path()).unwrap();
    let r = fx.run(Faults::default()).unwrap();
    assert!(!r.ran && r.finished);
    assert_eq!(fx.marker(), marker);
    assert_eq!(fs::read(fx.paths.db_path()).unwrap(), db);
    assert_migrated(&fx);
}

#[test]
fn existing_vessel_dir_without_marker_is_a_conflict() {
    let fx = Fx::new();
    write(&fx.paths.data_dir.join("vessel.db"), "mine");
    let r = fx.run(Faults::default()).unwrap();
    assert!(!r.ran);
    assert_eq!(r.conflicts.len(), 1, "{r:?}");
    assert_eq!(
        fs::read_to_string(fx.paths.data_dir.join("vessel.db")).unwrap(),
        "mine"
    );
    assert!(fx.old_data.join(LEGACY_DB_FILE).exists());
    assert!(fx.old_staging().join("a.txt").exists());
    assert!(fx.old_config.exists() && fx.old_state.exists());
    assert!(fx.marker().is_none());
    assert!(!fx.lock_file().exists());
}

#[test]
fn fresh_install_creates_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::for_home(tmp.path());
    let ctl = Ctl::default();
    let hooks = hooks(&Bus(true), &ctl, Faults::default()); // bus never probed
    let r = run_with(&paths, &EnvSnapshot::default(), &hooks).unwrap();
    assert_eq!(r, MigrationReport::default());
    assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
}

#[test]
fn crash_after_any_step_converges_on_rerun() {
    for step in steps() {
        for point in [CrashPoint::BeforeRecord, CrashPoint::AfterRecord] {
            let fx = Fx::new();
            let e = fx
                .run(Faults {
                    crash: Some((step.name, point)),
                    ..Default::default()
                })
                .unwrap_err();
            assert!(matches!(e, MigrateError::InjectedCrash(_)), "{e}");
            let r = fx.run(Faults::default());
            assert!(r.is_ok(), "{} {point:?}: {r:?}", step.name);
            assert_migrated(&fx);
        }
    }
}

#[test]
fn partial_exdev_copy_is_redone_and_old_kept_until_verified() {
    let fx = Fx::new();
    let e = fx
        .run(Faults {
            force_exdev: true,
            partial_copy_after: Some(1),
            ..Default::default()
        })
        .unwrap_err();
    assert!(matches!(e, MigrateError::InjectedCrash(_)));
    // backups (1 file) completed; skyrim stopped after its first file.
    let leftover = fsops::migrating_path(&fx.paths.data_dir.join("skyrim"));
    assert!(leftover.is_dir());
    assert!(!fx.paths.data_dir.join("skyrim").exists());
    assert!(fx.old_data.join("skyrim/downloads/a.zip").exists());
    assert!(fx.old_data.join("skyrim/downloads/b.zip").exists());

    let r = fx
        .run(Faults {
            force_exdev: true,
            ..Default::default()
        })
        .unwrap();
    assert!(r.finished, "{r:?}");
    assert!(!leftover.exists());
    assert_migrated(&fx);
}

#[test]
fn held_old_deploy_lock_aborts_without_vessel_state() {
    let fx = Fx::new();
    let _held = DeployLock::try_acquire(&fx.old_data.join("deploy.lock")).unwrap();
    let e = fx.run(Faults::default()).unwrap_err();
    assert!(matches!(e, MigrateError::LegacyRunning(_)), "{e}");
    assert!(e
        .to_string()
        .starts_with(&format!("Close {LEGACY_APP_DIR} before starting Vessel")));
    assert!(fx.no_vessel_state());
    assert!(fx.old_data.join(LEGACY_DB_FILE).exists());
    assert!(!fx.lock_file().exists());
}

#[test]
fn owned_legacy_dbus_name_aborts_without_vessel_state() {
    let fx = Fx::new();
    let e = fx.run_bus(Faults::default(), true).unwrap_err();
    assert!(matches!(e, MigrateError::LegacyRunning(_)), "{e}");
    assert!(e.to_string().contains("before starting Vessel"));
    assert!(fx.no_vessel_state());
    assert!(!fx.lock_file().exists());
}

// Fails when an abort deletes a lock file this run did not create.
#[test]
fn abort_keeps_a_lock_file_it_did_not_create() {
    let fx = Fx::new();
    write(&fx.lock_file(), "");
    fx.run_bus(Faults::default(), true).unwrap_err();
    assert!(fx.lock_file().exists());
}

#[test]
fn rows_only_in_wal_survive() {
    let fx = Fx::new();
    assert!(
        fs::metadata(super::with_suffix(
            &fx.old_data.join(LEGACY_DB_FILE),
            "-wal"
        ))
        .unwrap()
        .len()
            > 0
    );
    fx.run(Faults::default()).unwrap();
    let c = fx.vessel_db();
    let n: i64 = c
        .query_row("SELECT COUNT(*) FROM deployments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 3);
    assert_eq!(one(&c, "SELECT name FROM mods"), "Mod");
}

#[test]
fn offline_library_is_deferred_then_migrated() {
    let fx = Fx::new();
    let away = fx.home.join("unplugged");
    fs::rename(&fx.lib, &away).unwrap();
    let r = fx.run(Faults::default()).unwrap();
    assert!(!r.finished);
    assert_eq!(r.offline_libraries, vec![fx.lib.clone()]);
    assert_eq!(
        fx.marker().unwrap().libraries.get(&fx.lib),
        Some(&LibraryState::Offline)
    );
    let c = fx.vessel_db();
    assert_eq!(
        one(&c, "SELECT staging_dir FROM mods"),
        s(&fx.old_staging())
    );
    // The data prefix is rewritten independently of the library.
    assert!(one(&c, "SELECT archive FROM mods").starts_with(&s(&fx.paths.data_dir)));
    drop(c);

    fs::rename(&away, &fx.lib).unwrap();
    let r = fx.run(Faults::default()).unwrap();
    assert!(r.finished, "{r:?}");
    assert_eq!(
        fx.marker().unwrap().libraries.get(&fx.lib),
        Some(&LibraryState::Done)
    );
    assert_migrated(&fx);
}

// Fails when MODMAN_DATA_DIR (read only by the legacy bash tool) is used
// as a migration source: that would move the bash tool's data and strand
// the real legacy app data.
#[test]
fn legacy_data_dir_env_is_not_a_source() {
    let mut fx = Fx::new();
    let bash = fx.home.join("bash-tool");
    write(&bash.join("state.conf"), "bash tool\n");
    let var = format!("{LEGACY_ENV_PREFIX}DATA_DIR");
    fx.env = EnvSnapshot::from_vars([
        (var.clone(), s(&bash)),
        (format!("{LEGACY_ENV_PREFIX}DEBUG"), "1".into()),
    ]);
    let r = fx.run(Faults::default()).unwrap();
    assert!(r.finished, "{r:?}");
    assert_eq!(
        fs::read_to_string(bash.join("state.conf")).unwrap(),
        "bash tool\n"
    );
    assert_eq!(fs::read_dir(&bash).unwrap().count(), 1);
    assert!(r.warnings.contains(&format!(
        "{var} is used only by the legacy bash tool; ignored by Vessel"
    )));
    assert!(r.warnings.contains(&format!(
        "{LEGACY_ENV_PREFIX}DEBUG is deprecated and ignored; use VESSEL_DEBUG instead"
    )));
    assert_migrated(&fx);
}

#[test]
fn custom_data_dir_setting_is_preserved() {
    let fx = Fx::new();
    let c = Connection::open(fx.old_data.join(LEGACY_DB_FILE)).unwrap();
    c.execute(
        "UPDATE settings SET value = '/mnt/mods' WHERE key = 'data-dir'",
        [],
    )
    .unwrap();
    drop(c);
    fx.run(Faults::default()).unwrap();
    assert_eq!(
        one(
            &fx.vessel_db(),
            "SELECT value FROM settings WHERE key = 'data-dir'"
        ),
        "/mnt/mods"
    );
}

#[test]
fn failed_db_rewrite_leaves_vessel_db_unchanged() {
    let fx = Fx::new();
    let e = fx
        .run(Faults {
            fail_db_rewrite: true,
            ..Default::default()
        })
        .unwrap_err();
    assert!(matches!(e, MigrateError::InjectedCrash(_)));
    let c = fx.vessel_db();
    let od = s(&fx.old_data);
    assert!(one(&c, "SELECT archive FROM mods").starts_with(&od));
    assert_eq!(
        one(&c, "SELECT staging_dir FROM mods"),
        s(&fx.old_staging())
    );
    assert_eq!(
        one(&c, "SELECT value FROM settings WHERE key = 'data-dir'"),
        od
    );
    assert!(
        one(&c, "SELECT detail FROM deploy_journal_steps WHERE seq = 0")
            .contains(&s(&fx.old_staging()))
    );
    drop(c);
    fx.run(Faults::default()).unwrap();
    assert_migrated(&fx);
}

#[test]
fn user_changed_symlink_is_skipped_and_reported() {
    let fx = Fx::new();
    let link = fx.game.join("a.txt");
    fs::remove_file(&link).unwrap();
    let mine = fx.home.join("mine.txt");
    write(&mine, "user");
    std::os::unix::fs::symlink(&mine, &link).unwrap();
    let r = fx.run(Faults::default()).unwrap();
    assert_eq!(r.skipped_links, vec![link.clone()]);
    assert_eq!(fs::read_link(&link).unwrap(), mine);
}

#[test]
fn old_timer_is_stopped_before_anything_moves() {
    let fx = Fx::new();
    let unit_dir = crate::autoupdate::systemd_user_dir(&fx.paths);
    write(&unit_dir.join(format!("{LEGACY_UNIT}.timer")), "[Timer]\n");
    write(
        &unit_dir.join(format!("{LEGACY_UNIT}.service")),
        "[Service]\n",
    );
    let ctl = Ctl::default();
    let faults = Faults {
        crash: Some(("stop_timer", CrashPoint::AfterRecord)),
        ..Default::default()
    };
    let hooks = hooks(&Bus(false), &ctl, faults);
    run_with(&fx.paths, &fx.env, &hooks).unwrap_err();
    assert_eq!(
        *ctl.0.borrow(),
        vec![
            format!("is-enabled {LEGACY_UNIT}.timer"),
            format!("disable --now {LEGACY_UNIT}.timer"),
            format!("stop {LEGACY_UNIT}.service")
        ]
    );
    // Recorded before disabling, for the Vessel timer.
    assert_eq!(fx.marker().unwrap().legacy_timer_enabled, Some(true));
    assert!(fx.old_staging().exists() && fx.old_data.join("skyrim").exists());
    // Unit files are left for the integration step.
    assert!(unit_dir.join(format!("{LEGACY_UNIT}.timer")).exists());
}

#[test]
fn prefix_rewrite_matches_whole_components_only() {
    let mut m = db::PrefixMap::default();
    m.push(Path::new("/h/old"), Path::new("/h/new"));
    assert_eq!(m.map("/h/old"), Some("/h/new".into()));
    assert_eq!(m.map("/h/old/x"), Some("/h/new/x".into()));
    assert_eq!(m.map("/h/older/x"), None);
    assert_eq!(m.map("/x/h/old/y"), None);
    assert_eq!(m.unmap("/h/new/x"), Some("/h/old/x".into()));
}

#[test]
fn step_list_keeps_core_order_and_cleanup_last() {
    let names: Vec<_> = steps().iter().map(|s| s.name).collect();
    assert_eq!(
        names,
        [
            "stop_timer",
            "wal_checkpoint",
            "copy_state",
            "move_bulk",
            "move_staging",
            "rewrite_db",
            "retarget_symlinks",
            "keyring",
            "systemd_units",
            "nxm_handler",
            "steam_shortcut",
            "cleanup"
        ]
    );
}

/// Legacy timer (6 h), nxm handler as default and keyring secret.
fn legacy_integrations(fx: &Fx) -> KeyringHook {
    let units = crate::autoupdate::systemd_user_dir(&fx.paths);
    write(
        &units.join(format!("{LEGACY_UNIT}.timer")),
        "[Timer]\nOnUnitActiveSec=6h\n",
    );
    write(&units.join(format!("{LEGACY_UNIT}.service")), "[Service]\n");
    let apps = crate::nxm_handler::applications_dir(&fx.paths);
    write(&apps.join(LEGACY_NXM_DESKTOP_FILE), "[Desktop Entry]\n");
    write(
        &crate::nxm_handler::mimeapps_path(&fx.paths),
        &format!("[Default Applications]\nx-scheme-handler/nxm={LEGACY_NXM_DESKTOP_FILE};\n"),
    );
    let (hook, store) = mock_keyring();
    store
        .build(LEGACY_KEYRING_SERVICE, crate::nexus::KEYRING_USER, None)
        .unwrap()
        .set_password("KEY")
        .unwrap();
    hook
}

fn run_integrations(
    fx: &Fx,
    ctl: &dyn Systemctl,
    keyring: &KeyringHook,
    cli: Option<&str>,
) -> MigrationReport {
    let hooks = Hooks {
        bus: &Bus(false),
        systemctl: ctl,
        keyring: keyring.clone(),
        programs: Programs {
            cli: cli.map(PathBuf::from),
            gui: None,
        },
        faults: Faults::default(),
    };
    run_with(&fx.paths, &fx.env, &hooks).unwrap()
}

// Fails when an integration step is skipped by the full run, not recorded
// in the marker, or redone on a later start.
#[test]
fn integrations_migrate_in_the_full_run_once() {
    let fx = Fx::new();
    let keyring = legacy_integrations(&fx);
    let ctl = Ctl::default();
    let r = run_integrations(&fx, &ctl, &keyring, Some("/opt/vessel/vessel"));
    assert!(r.finished, "{r:?}");
    let marker = fx.marker().unwrap();
    for s in INTEGRATION_STEPS {
        assert_eq!(marker.step(s.name), StepState::Done, "{}", s.name);
    }
    let store = keyring().unwrap();
    let get = |svc| {
        store
            .build(svc, crate::nexus::KEYRING_USER, None)
            .unwrap()
            .get_password()
            .ok()
    };
    assert_eq!(get(crate::nexus::KEYRING_SERVICE).as_deref(), Some("KEY"));
    assert_eq!(get(LEGACY_KEYRING_SERVICE), None);
    let timer = fs::read_to_string(crate::autoupdate::timer_path(&fx.paths)).unwrap();
    assert!(timer.contains("OnUnitActiveSec=6h"), "{timer}");
    let enable = format!("enable --now {}.timer", crate::autoupdate::UNIT);
    assert!(ctl.0.borrow().contains(&enable), "{:?}", ctl.0.borrow());
    assert!(crate::nxm_handler::registered(&fx.paths));

    let ctl2 = Ctl::default();
    let r = run_integrations(&fx, &ctl2, &keyring, Some("/opt/vessel/vessel"));
    assert!(r.finished && !r.ran);
    assert!(ctl2.0.borrow().is_empty());
}

// Fails when a failing integration aborts the launch, is marked done, or
// lets cleanup remove the old dirs before it succeeded.
#[test]
fn failed_integration_is_reported_and_retried_next_start() {
    /// No user manager yet (e.g. started outside a session).
    struct NoManager;
    impl Systemctl for NoManager {
        fn run(&self, args: &[&str]) -> Result<String, String> {
            match args.first() {
                Some(&"show-environment") => Err("Failed to connect to bus".into()),
                _ => Ok(String::new()),
            }
        }
    }
    let fx = Fx::new();
    let keyring = legacy_integrations(&fx);
    let r = run_integrations(&fx, &NoManager, &keyring, Some("/opt/vessel/vessel"));
    assert!(!r.finished);
    assert!(
        r.warnings.iter().any(|w| w.starts_with("systemd_units:")),
        "{:?}",
        r.warnings
    );
    let m = fx.marker().unwrap();
    assert_eq!(m.step("systemd_units"), StepState::Pending);
    assert_eq!(m.step("cleanup"), StepState::Pending);
    assert!(fx.paths.db_path().is_file() && fx.old_config.exists());

    let ctl = Ctl::default();
    let r = run_integrations(&fx, &ctl, &keyring, Some("/opt/vessel/vessel"));
    assert!(r.finished, "{r:?}");
    assert!(crate::autoupdate::timer_path(&fx.paths).is_file());
    assert!(!fx.old_config.exists());
}
