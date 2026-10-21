// SPDX-License-Identifier: GPL-3.0-or-later
//! Deploy engine integration tests (M2). Every test works in temp dirs only;
//! nothing touches a real game install or Steam folder.

use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use modman_core::deploy::{
    self, DeployEnv, DeployError, DeployOptions, Faults, GameProbe, Method, MethodChoice,
    NeverRunning, VerifyKind,
};
use modman_core::games::{self, GameConfig};
use modman_core::store::{DeployLock, Store};
use sha2::{Digest, Sha256};

const GAME: &str = "testgame";

struct Fx {
    tmp: tempfile::TempDir,
    store: Store,
    game: GameConfig,
    env: DeployEnv,
}

/// path -> (kind, sha256 or link target, permission bits)
type Snapshot = BTreeMap<String, (char, String, u32)>;

fn sha_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn snapshot(dir: &Path) -> Snapshot {
    let mut out = BTreeMap::new();
    for e in walkdir::WalkDir::new(dir).min_depth(1).sort_by_file_name() {
        let e = e.unwrap();
        let rel = e
            .path()
            .strip_prefix(dir)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let meta = std::fs::symlink_metadata(e.path()).unwrap();
        let mode = meta.permissions().mode() & 0o7777;
        let v = if meta.is_symlink() {
            (
                'l',
                std::fs::read_link(e.path())
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                0,
            )
        } else if meta.is_dir() {
            ('d', String::new(), mode)
        } else {
            ('f', sha_hex(&std::fs::read(e.path()).unwrap()), mode)
        };
        out.insert(rel, v);
    }
    out
}

fn write(path: &Path, data: &str, mode: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Make a previous read-only file writable again before overwriting.
    if path.exists() {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    std::fs::write(path, data).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

impl Fx {
    fn new() -> Self {
        Self::with_dirs(None, None)
    }

    /// `game_base` / `staging_base` override where the game and staging
    /// trees live (for cross-device and reflink tests).
    fn with_dirs(game_base: Option<&Path>, staging_base: Option<&Path>) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let game_dir = game_base.unwrap_or(tmp.path()).join("game");
        write(&game_dir.join("Data/Original.esp"), "orig-esp", 0o644);
        write(&game_dir.join("Data/Textures/sky.dds"), "orig-sky", 0o600);
        write(&game_dir.join("game.exe"), "orig-exe", 0o755);
        write(&game_dir.join("readonly.ini"), "orig-ini", 0o444);
        std::os::unix::fs::symlink("game.exe", game_dir.join("launcher")).unwrap();

        let store = Store::open(&tmp.path().join("data/modman.db")).unwrap();
        let mut game = GameConfig::custom(GAME);
        game.folder = Some(game_dir);
        games::save_game(store.conn(), &game).unwrap();
        let data = tmp.path().join("data");
        let staging = staging_base.unwrap_or(tmp.path()).join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        let env = DeployEnv {
            lock_path: data.join("deploy.lock"),
            backup_dir: data.join("backups"),
            overwrite_dir: data.join(GAME).join("overwrite"),
            probe: Arc::new(NeverRunning),
            faults: Faults::default(),
        };
        let fx = Self {
            tmp,
            store,
            game,
            env,
        };
        std::fs::write(fx.staging_marker(), staging.to_string_lossy().as_bytes()).unwrap();
        fx
    }

    fn staging_marker(&self) -> PathBuf {
        self.tmp.path().join("staging-base")
    }

    fn staging(&self) -> PathBuf {
        PathBuf::from(std::fs::read_to_string(self.staging_marker()).unwrap())
    }

    fn game_dir(&self) -> PathBuf {
        self.game.folder.clone().unwrap()
    }

    /// Stages a mod and registers it with the given priority.
    fn add_mod(&self, uid: &str, priority: i64, files: &[(&str, &str)]) -> i64 {
        let dir = self.staging().join(uid);
        for (p, data) in files {
            write(&dir.join(p), data, 0o644);
        }
        self.store
            .conn()
            .execute(
                "INSERT INTO mods (game, uid, name, priority, staging_dir, installed_at)
                 VALUES (?1, ?2, ?2, ?3, ?4, 0)",
                rusqlite::params![GAME, uid, priority, dir.to_string_lossy()],
            )
            .unwrap();
        self.store.conn().last_insert_rowid()
    }

    fn opts(&self, method: MethodChoice) -> DeployOptions {
        DeployOptions {
            method,
            ..Default::default()
        }
    }

    fn deploy(&mut self) -> Result<deploy::DeployReport, DeployError> {
        let opts = self.opts(MethodChoice::Auto);
        deploy::deploy(&mut self.store, &self.game, &self.env, &opts)
    }

    fn purge(&mut self) -> Result<deploy::PurgeReport, DeployError> {
        deploy::purge(&mut self.store, &self.game, &self.env)
    }

    fn reopen(&mut self) {
        self.store = Store::open(&self.tmp.path().join("data/modman.db")).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.game_dir().join(rel)).unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.store.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn methods(&self) -> BTreeMap<String, String> {
        let mut stmt = self
            .store
            .conn()
            .prepare("SELECT rel_path, method FROM deployments")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// Three mods: overlapping files, overwritten originals (with a case
    /// mismatch), new nested folders, a read-only original.
    fn three_mods(&self) {
        self.add_mod(
            "low",
            0,
            &[
                ("Data/shared.esp", "low-shared"),
                ("Data/original.esp", "low-orig"),
                ("Data/New/Deep/a.txt", "low-a"),
                ("readonly.ini", "low-ini"),
            ],
        );
        self.add_mod(
            "mid",
            1,
            &[
                ("data/SHARED.esp", "mid-shared"),
                ("Data/Textures/sky.dds", "mid-sky"),
                ("Data/New/b.txt", "mid-b"),
            ],
        );
        self.add_mod(
            "high",
            2,
            &[
                ("Data/Shared.esp", "high-shared"),
                ("launcher", "high-launcher"),
                ("Root/c.txt", "high-c"),
            ],
        );
    }
}

// ── acceptance: purge restores the game byte for byte ───────────────────

#[test]
fn deploy_overlapping_mods_then_purge_restores_game_byte_for_byte() {
    let mut fx = Fx::new();
    fx.three_mods();
    let before = snapshot(&fx.game_dir());

    let report = fx.deploy().unwrap();
    assert_eq!(report.placed, 8);
    assert_eq!(
        report.backed_up, 4,
        "esp, sky, readonly.ini, launcher symlink"
    );
    assert_eq!(fx.read("Data/shared.esp"), "high-shared");
    // Existing spelling wins for case-insensitive matches.
    assert_eq!(fx.read("Data/Original.esp"), "low-orig");
    assert!(!fx.game_dir().join("Data/original.esp").exists());
    assert!(!fx.game_dir().join("data").exists());
    assert_eq!(fx.read("Data/Textures/sky.dds"), "mid-sky");
    assert_eq!(fx.read("launcher"), "high-launcher");
    assert!(!std::fs::symlink_metadata(fx.game_dir().join("launcher"))
        .unwrap()
        .is_symlink());
    assert_eq!(fx.read("Data/New/Deep/a.txt"), "low-a");
    assert_eq!(fx.read("Root/c.txt"), "high-c");
    assert!(games::deployment_active(fx.store.conn(), GAME).unwrap());

    let purge = fx.purge().unwrap();
    assert_eq!(purge.removed, 8);
    assert_eq!(purge.restored, 4);
    assert_eq!(purge.changed + purge.missing, 0);
    assert_eq!(snapshot(&fx.game_dir()), before);
    assert!(!games::deployment_active(fx.store.conn(), GAME).unwrap());
    assert_eq!(fx.count("SELECT COUNT(*) FROM deployment_dirs"), 0);
}

#[test]
fn backups_are_content_addressed_and_released_after_purge() {
    let mut fx = Fx::new();
    fx.three_mods();
    fx.deploy().unwrap();
    let blob = fx
        .env
        .backup_dir
        .join(&sha_hex(b"orig-esp")[..2])
        .join(sha_hex(b"orig-esp"));
    assert_eq!(std::fs::read(&blob).unwrap(), b"orig-esp");
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM backups WHERE restored_at IS NULL"),
        4
    );
    fx.purge().unwrap();
    assert!(!blob.exists(), "unreferenced blob removed after restore");
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM backups WHERE restored_at IS NULL"),
        0
    );
}

#[test]
fn redeploy_purges_previous_deployment_first() {
    let mut fx = Fx::new();
    let before = snapshot(&fx.game_dir());
    fx.three_mods();
    fx.deploy().unwrap();
    // Disable the top mod and redeploy: its files must disappear and the
    // original launcher symlink come back.
    fx.store
        .conn()
        .execute("UPDATE mods SET enabled = 0 WHERE uid = 'high'", [])
        .unwrap();
    let r = fx.deploy().unwrap();
    assert!(r.purged.is_some());
    assert_eq!(fx.read("Data/shared.esp"), "mid-shared");
    assert!(std::fs::symlink_metadata(fx.game_dir().join("launcher"))
        .unwrap()
        .is_symlink());
    assert!(!fx.game_dir().join("Root").exists());
    fx.purge().unwrap();
    assert_eq!(snapshot(&fx.game_dir()), before);
}

#[test]
fn purge_keeps_files_the_game_changed() {
    let mut fx = Fx::new();
    fx.three_mods();
    fx.deploy().unwrap();
    let game = fx.game_dir();
    // Game update replaced an overridden original: keep the game's version.
    std::fs::remove_file(game.join("Data/Textures/sky.dds")).unwrap();
    write(&game.join("Data/Textures/sky.dds"), "steam-update", 0o644);
    // A deployed new file edited in place (copy-on-write by the game).
    std::fs::remove_file(game.join("Root/c.txt")).unwrap();
    write(&game.join("Root/c.txt"), "game-edited", 0o644);
    // A deployed file deleted.
    std::fs::remove_file(game.join("Data/New/b.txt")).unwrap();

    let r = fx.purge().unwrap();
    assert_eq!(r.changed, 2);
    assert_eq!(r.missing, 1);
    assert_eq!(fx.read("Data/Textures/sky.dds"), "steam-update");
    assert!(!game.join("Root/c.txt").exists());
    assert_eq!(
        std::fs::read_to_string(fx.env.overwrite_dir.join("Root/c.txt")).unwrap(),
        "game-edited"
    );
    assert_eq!(
        std::fs::read_to_string(
            fx.env
                .overwrite_dir
                .join(".stale-backups/Data/Textures/sky.dds")
        )
        .unwrap(),
        "orig-sky"
    );
    assert!(!games::deployment_active(fx.store.conn(), GAME).unwrap());
}

// ── acceptance: conflicts ───────────────────────────────────────────────

#[test]
fn later_priority_mod_wins_each_contested_path_and_conflicts_are_listed() {
    let mut fx = Fx::new();
    fx.three_mods();
    let plan = deploy::build_plan(fx.store.conn(), &fx.game).unwrap();
    let conflicts = plan.conflicts();
    assert_eq!(conflicts.len(), 1);
    let c = &conflicts[0];
    assert_eq!(c.rel_path, "Data/shared.esp", "first provider's spelling");
    assert_eq!(c.winner.uid, "high");
    let losers: Vec<_> = c.losers.iter().map(|m| m.uid.as_str()).collect();
    assert_eq!(losers, ["low", "mid"]);
    assert_eq!(
        deploy::conflicts(fx.store.conn(), &fx.game).unwrap().len(),
        1
    );

    // Reordering flips the winner.
    fx.store
        .conn()
        .execute("UPDATE mods SET priority = 9 WHERE uid = 'low'", [])
        .unwrap();
    fx.deploy().unwrap();
    // "mid" is now the first provider, so its spelling is used.
    assert_eq!(fx.read("Data/SHARED.esp"), "low-shared");
}

#[test]
fn disabled_mods_and_missing_staging_are_skipped() {
    let fx = Fx::new();
    fx.add_mod("a", 0, &[("x.txt", "a")]);
    let b = fx.add_mod("b", 1, &[("y.txt", "b")]);
    fx.store
        .conn()
        .execute("UPDATE mods SET enabled = 0 WHERE id = ?1", [b])
        .unwrap();
    fx.store
        .conn()
        .execute(
            "INSERT INTO mods (game, uid, name, priority, staging_dir, installed_at)
             VALUES (?1, 'gone', 'gone', 2, '/nonexistent/modman-test', 0)",
            [GAME],
        )
        .unwrap();
    let plan = deploy::build_plan(fx.store.conn(), &fx.game).unwrap();
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].uid, "gone");
}

#[test]
fn subdir_prefixes_mod_paths_unless_mod_root_is_game() {
    let mut fx = Fx::new();
    fx.game.subdir = "Data".into();
    games::save_game(fx.store.conn(), &fx.game).unwrap();
    fx.add_mod("plugin", 0, &[("p.esp", "p")]);
    let skse = fx.add_mod("skse", 1, &[("skse_loader.exe", "s")]);
    fx.store
        .conn()
        .execute("UPDATE mods SET root = 'game' WHERE id = ?1", [skse])
        .unwrap();
    fx.deploy().unwrap();
    assert_eq!(fx.read("Data/p.esp"), "p");
    assert_eq!(fx.read("skse_loader.exe"), "s");
}

// ── acceptance: crash recovery ──────────────────────────────────────────

#[test]
fn deploy_crash_after_k_files_rolls_back_on_next_open() {
    // Batches of 3: K=2 crashes with placed-but-unrecorded files, K=3 on a
    // batch boundary, K=8 after every file but before the journal commit.
    for k in [0, 2, 3, 5, 8] {
        let mut fx = Fx::new();
        fx.three_mods();
        let before = snapshot(&fx.game_dir());
        fx.env.faults.batch_size = Some(3);
        fx.env.faults.crash_after_files = Some(k);
        assert!(
            matches!(fx.deploy(), Err(DeployError::InjectedCrash)),
            "k={k}"
        );
        assert!(games::deployment_active(fx.store.conn(), GAME).unwrap());
        if k >= 3 {
            // Originals of replaced files are already in the backup store.
            assert!(fx.env.backup_dir.join(&sha_hex(b"orig-esp")[..2]).exists());
        }

        // Next launch: a fresh store and environment roll back.
        fx.env.faults = Faults::default();
        fx.reopen();
        let rec = deploy::recover(&mut fx.store, &fx.game, &fx.env)
            .unwrap()
            .expect("an interrupted journal");
        assert_eq!(rec.outcome, deploy::RecoveryOutcome::RolledBack, "k={k}");
        assert_eq!(snapshot(&fx.game_dir()), before, "k={k}");
        assert!(!games::deployment_active(fx.store.conn(), GAME).unwrap());
        assert_eq!(
            fx.count("SELECT COUNT(*) FROM deploy_journal WHERE state = 'started'"),
            0
        );

        // Deploying again afterwards works and purges cleanly.
        fx.deploy().unwrap();
        fx.purge().unwrap();
        assert_eq!(snapshot(&fx.game_dir()), before, "k={k}");
    }
}

#[test]
fn deploy_and_purge_recover_automatically_after_a_crash() {
    let mut fx = Fx::new();
    fx.three_mods();
    let before = snapshot(&fx.game_dir());
    fx.env.faults.crash_after_files = Some(4);
    assert!(fx.deploy().is_err());
    fx.env.faults = Faults::default();
    fx.reopen();
    let r = fx.purge().unwrap();
    assert!(r.recovered.is_some());
    assert_eq!(snapshot(&fx.game_dir()), before);
}

#[test]
fn purge_crash_resumes_on_next_open() {
    let mut fx = Fx::new();
    fx.three_mods();
    let before = snapshot(&fx.game_dir());
    fx.deploy().unwrap();
    fx.env.faults.crash_after_files = Some(3);
    assert!(matches!(fx.purge(), Err(DeployError::InjectedCrash)));
    fx.env.faults = Faults::default();
    fx.reopen();
    let rec = deploy::recover(&mut fx.store, &fx.game, &fx.env)
        .unwrap()
        .unwrap();
    assert_eq!(rec.outcome, deploy::RecoveryOutcome::Resumed);
    assert_eq!(snapshot(&fx.game_dir()), before);
    assert!(!games::deployment_active(fx.store.conn(), GAME).unwrap());
}

#[test]
fn real_failure_mid_deploy_rolls_back_immediately() {
    let mut fx = Fx::new();
    fx.three_mods();
    let before = snapshot(&fx.game_dir());
    // Every method fails for one file: the whole deploy is undone.
    fx.env.faults.deny = Some(Arc::new(|_, p: &Path| p.ends_with("Root/c.txt")));
    let err = fx.deploy().unwrap_err();
    assert!(matches!(err, DeployError::Place { .. }), "{err}");
    assert_eq!(snapshot(&fx.game_dir()), before);
    assert!(!games::deployment_active(fx.store.conn(), GAME).unwrap());
}

#[test]
fn path_conflict_with_existing_directory_is_refused_before_changes() {
    let mut fx = Fx::new();
    fx.add_mod(
        "bad",
        0,
        &[("data/textures", "file-over-dir"), ("z.txt", "z")],
    );
    let before = snapshot(&fx.game_dir());
    assert!(matches!(fx.deploy(), Err(DeployError::PathConflict { .. })));
    assert_eq!(snapshot(&fx.game_dir()), before);
    assert!(!games::deployment_active(fx.store.conn(), GAME).unwrap());
}

// ── acceptance: method fallback chain ───────────────────────────────────

#[test]
fn forced_hardlink_failure_records_fallback_method_per_file() {
    let mut fx = Fx::new();
    fx.add_mod(
        "m",
        0,
        &[
            ("hard.txt", "h"),
            ("nohard.txt", "n"),
            ("sym.txt", "s"),
            ("copy.txt", "c"),
        ],
    );
    fx.env.faults.deny = Some(Arc::new(|m: Method, p: &Path| {
        let name = p.file_name().unwrap().to_str().unwrap();
        match name {
            "nohard.txt" => m == Method::Hardlink,
            "sym.txt" => matches!(m, Method::Hardlink | Method::Reflink),
            "copy.txt" => m != Method::Copy,
            _ => false,
        }
    }));
    fx.deploy().unwrap();
    let m = fx.methods();
    assert_eq!(m["hard.txt"], "hardlink");
    assert!(
        ["reflink", "symlink"].contains(&m["nohard.txt"].as_str()),
        "{}",
        m["nohard.txt"]
    );
    assert_eq!(m["sym.txt"], "symlink");
    assert_eq!(m["copy.txt"], "copy");
    for f in ["hard.txt", "nohard.txt", "sym.txt", "copy.txt"] {
        assert_eq!(fx.read(f).len(), 1);
    }
    let link = std::fs::read_link(fx.game_dir().join("sym.txt")).unwrap();
    assert_eq!(link, fx.staging().join("m/sym.txt"));
    // Each method's purge check recognises its own placement.
    let r = fx.purge().unwrap();
    assert_eq!(r.removed, 4);
    assert_eq!(r.changed, 0);
}

#[test]
fn explicit_method_is_used_with_copy_fallback() {
    let mut fx = Fx::new();
    fx.add_mod("m", 0, &[("a.txt", "a"), ("b.txt", "b")]);
    fx.env.faults.deny = Some(Arc::new(|m: Method, p: &Path| {
        m == Method::Symlink && p.ends_with("b.txt")
    }));
    let opts = fx.opts(MethodChoice::Only(Method::Symlink));
    deploy::deploy(&mut fx.store, &fx.game, &fx.env, &opts).unwrap();
    let m = fx.methods();
    assert_eq!(m["a.txt"], "symlink");
    assert_eq!(m["b.txt"], "copy");
}

#[test]
fn copy_preserves_mode_and_mtime() {
    let mut fx = Fx::new();
    fx.add_mod("m", 0, &[("x.sh", "#!/bin/sh")]);
    let src = fx.staging().join("m/x.sh");
    std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o555)).unwrap();
    let opts = fx.opts(MethodChoice::Only(Method::Copy));
    deploy::deploy(&mut fx.store, &fx.game, &fx.env, &opts).unwrap();
    let (s, d) = (
        std::fs::metadata(&src).unwrap(),
        std::fs::metadata(fx.game_dir().join("x.sh")).unwrap(),
    );
    assert_ne!(s.ino(), d.ino());
    assert_eq!(d.permissions().mode() & 0o7777, 0o555);
    assert_eq!(s.mtime(), d.mtime());
    assert_eq!(s.mtime_nsec(), d.mtime_nsec());
    assert_eq!(fx.purge().unwrap().removed, 1);
}

/// A directory on a different filesystem than the system temp dir, if any.
fn other_device_dir() -> Option<PathBuf> {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&base).ok()?;
    let tmp_dev = std::fs::metadata(std::env::temp_dir()).ok()?.dev();
    (std::fs::metadata(&base).ok()?.dev() != tmp_dev).then_some(base)
}

#[test]
fn cross_device_staging_skips_hardlink_and_reflink() {
    let Some(base) = other_device_dir() else {
        eprintln!("skipped: temp dir and target dir share a filesystem");
        return;
    };
    let other = tempfile::tempdir_in(base).unwrap();
    // Game on the target-dir filesystem, staging on the temp filesystem.
    let mut fx = Fx::with_dirs(Some(other.path()), None);
    fx.add_mod("m", 0, &[("a.txt", "a")]);
    let before = snapshot(&fx.game_dir());
    fx.deploy().unwrap();
    assert_eq!(fx.methods()["a.txt"], "symlink");
    fx.purge().unwrap();
    assert_eq!(snapshot(&fx.game_dir()), before);
}

#[test]
fn reflink_is_used_when_hardlink_fails_on_a_reflink_filesystem() {
    let Some(base) = other_device_dir() else {
        eprintln!("skipped: no separate filesystem for reflink test");
        return;
    };
    let other = tempfile::tempdir_in(base).unwrap();
    let probe_a = other.path().join("probe-a");
    std::fs::write(&probe_a, "x").unwrap();
    if reflink_copy::reflink(&probe_a, other.path().join("probe-b")).is_err() {
        eprintln!("skipped: filesystem has no reflink support");
        return;
    }
    let mut fx = Fx::with_dirs(Some(other.path()), Some(other.path()));
    fx.add_mod("m", 0, &[("a.txt", "a")]);
    fx.env.faults.deny = Some(Arc::new(|m: Method, _: &Path| m == Method::Hardlink));
    fx.deploy().unwrap();
    assert_eq!(fx.methods()["a.txt"], "reflink");
    assert_eq!(fx.purge().unwrap().removed, 1);
}

// ── acceptance: refusals ────────────────────────────────────────────────

struct Running;
impl GameProbe for Running {
    fn is_running(&self, _: &GameConfig) -> bool {
        true
    }
}

#[test]
fn deploy_and_purge_refused_while_game_running() {
    let mut fx = Fx::new();
    fx.three_mods();
    fx.deploy().unwrap();
    let deployed = snapshot(&fx.game_dir());
    fx.env.probe = Arc::new(Running);
    assert!(matches!(fx.deploy(), Err(DeployError::GameRunning(_))));
    assert!(matches!(fx.purge(), Err(DeployError::GameRunning(_))));
    assert_eq!(snapshot(&fx.game_dir()), deployed);
}

#[test]
fn deploy_and_purge_refused_while_lock_held() {
    let mut fx = Fx::new();
    fx.three_mods();
    let before = snapshot(&fx.game_dir());
    let _held = DeployLock::try_acquire(&fx.env.lock_path).unwrap();
    assert!(matches!(fx.deploy(), Err(DeployError::LockBusy(_))));
    assert!(matches!(fx.purge(), Err(DeployError::LockBusy(_))));
    assert_eq!(snapshot(&fx.game_dir()), before);
}

#[test]
fn proc_probe_detects_appid_and_game_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let proc_root = tmp.path().join("proc");
    let mut game = GameConfig::custom(GAME);
    game.appid = Some(489830);
    game.folder = Some(PathBuf::from("/lib/steamapps/common/Skyrim"));
    let probe = deploy::ProcProbe {
        proc_root: proc_root.clone(),
    };
    let set = |pid: &str, cmdline: &[u8]| {
        std::fs::create_dir_all(proc_root.join(pid)).unwrap();
        std::fs::write(proc_root.join(pid).join("cmdline"), cmdline).unwrap();
    };
    set("1", b"/usr/lib/systemd\0");
    set("self", b"AppId=489830\0"); // not a pid
    assert!(!probe.is_running(&game));
    set("2", b"reaper\0SteamLaunch\0AppId=4898300\0"); // other appid
    assert!(!probe.is_running(&game));
    set("3", b"reaper\0SteamLaunch\0AppId=489830\0--\0");
    assert!(probe.is_running(&game));
    std::fs::remove_dir_all(proc_root.join("3")).unwrap();
    set("4", b"Z:\\lib\\steamapps\\common\\Skyrim\\SkyrimSE.exe\0");
    assert!(probe.is_running(&game));
    std::fs::remove_dir_all(proc_root.join("4")).unwrap();
    set("5", b"/lib/steamapps/common/Skyrim/SkyrimSE.exe\0");
    assert!(probe.is_running(&game));
    std::fs::remove_dir_all(proc_root.join("5")).unwrap();
    set("6", b"/lib/steamapps/common/SkyrimX/a.exe\0");
    assert!(!probe.is_running(&game));
}

// ── acceptance: verify and dry run ──────────────────────────────────────

#[test]
fn verify_reports_modified_and_missing_deployed_files() {
    let mut fx = Fx::new();
    fx.three_mods();
    fx.deploy().unwrap();
    let ok = deploy::verify(fx.store.conn(), &fx.game).unwrap();
    assert_eq!(ok.intact, 8);
    assert!(ok.issues.is_empty());

    let game = fx.game_dir();
    std::fs::remove_file(game.join("Data/New/b.txt")).unwrap();
    // Write through the hardlink: same inode, different content.
    std::fs::write(game.join("Root/c.txt"), "tampered").unwrap();
    let r = deploy::verify(fx.store.conn(), &fx.game).unwrap();
    assert_eq!(r.intact, 6);
    let mut issues: Vec<_> = r
        .issues
        .iter()
        .map(|i| (i.rel_path.as_str(), i.kind))
        .collect();
    issues.sort();
    assert_eq!(
        issues,
        [
            ("Data/New/b.txt", VerifyKind::Missing),
            ("Root/c.txt", VerifyKind::Modified)
        ]
    );
}

#[test]
fn verify_detects_broken_symlink() {
    let mut fx = Fx::new();
    fx.add_mod("m", 0, &[("a.txt", "a")]);
    let opts = fx.opts(MethodChoice::Only(Method::Symlink));
    deploy::deploy(&mut fx.store, &fx.game, &fx.env, &opts).unwrap();
    std::fs::remove_file(fx.staging().join("m/a.txt")).unwrap();
    let r = deploy::verify(fx.store.conn(), &fx.game).unwrap();
    assert_eq!(r.issues.len(), 1);
    assert_eq!(r.issues[0].kind, VerifyKind::Missing);
}

#[test]
fn dry_run_changes_nothing_on_disk() {
    let mut fx = Fx::new();
    fx.three_mods();
    let game_before = snapshot(&fx.game_dir());
    let all_before = snapshot(fx.tmp.path());
    let opts = DeployOptions {
        dry_run: true,
        ..Default::default()
    };
    let r = deploy::deploy(&mut fx.store, &fx.game, &fx.env, &opts).unwrap();
    assert!(r.dry_run);
    assert_eq!(r.placed, 8);
    assert_eq!(r.backed_up, 4);
    assert_eq!(r.new_dirs, 3, "Data/New, Data/New/Deep, Root");
    assert_eq!(r.conflicts.len(), 1);
    assert_eq!(snapshot(&fx.game_dir()), game_before);
    // Nothing else changed either: no lock, backups, journal rows.
    let mut all_after = snapshot(fx.tmp.path());
    all_after.retain(|k, _| !k.starts_with("data/modman.db"));
    let mut all_before = all_before;
    all_before.retain(|k, _| !k.starts_with("data/modman.db"));
    assert_eq!(all_after, all_before);
    assert_eq!(fx.count("SELECT COUNT(*) FROM deploy_journal"), 0);
    assert_eq!(fx.count("SELECT COUNT(*) FROM deployments"), 0);
}

#[test]
fn dry_run_is_allowed_while_game_runs_and_lock_is_held() {
    let mut fx = Fx::new();
    fx.three_mods();
    fx.env.probe = Arc::new(Running);
    let _held = DeployLock::try_acquire(&fx.env.lock_path).unwrap();
    let opts = DeployOptions {
        dry_run: true,
        ..Default::default()
    };
    assert!(deploy::deploy(&mut fx.store, &fx.game, &fx.env, &opts).is_ok());
}

// ── status, staging location, method resolution ─────────────────────────

#[test]
fn status_reports_deployment_state() {
    let mut fx = Fx::new();
    let s = deploy::status(fx.store.conn(), GAME).unwrap();
    assert!(!s.active);
    fx.three_mods();
    fx.deploy().unwrap();
    let s = deploy::status(fx.store.conn(), GAME).unwrap();
    assert!(s.active);
    assert_eq!(s.files, 8);
    assert_eq!(s.backups_held, 4);
    assert_eq!(s.by_method.get(&Method::Hardlink), Some(&8));
    assert!(s.last_deployed_at.is_some());
    assert!(!s.interrupted);
}

#[test]
fn staging_lives_per_steam_library() {
    let data = Path::new("/home/u/.local/share/modman");
    let mut g = GameConfig::custom("skyrimspecialedition");
    assert_eq!(
        deploy::staging_root(&g, data),
        data.join("skyrimspecialedition/staging")
    );
    g.folder = Some(PathBuf::from(
        "/run/media/sd/SteamLibrary/steamapps/common/Skyrim Special Edition",
    ));
    assert_eq!(
        deploy::staging_root(&g, data),
        PathBuf::from("/run/media/sd/SteamLibrary/.modman/staging/skyrimspecialedition")
    );
    g.library = Some(PathBuf::from("/mnt/lib"));
    assert_eq!(
        deploy::staging_root(&g, data),
        PathBuf::from("/mnt/lib/.modman/staging/skyrimspecialedition")
    );
}

#[test]
fn method_resolution_precedence() {
    let mut g = GameConfig::custom(GAME);
    let r = |o: Option<&str>, g: &GameConfig, global: &str| {
        deploy::resolve_method(o, g, global).unwrap()
    };
    assert_eq!(r(None, &g, "auto"), MethodChoice::Auto);
    assert_eq!(r(None, &g, "copy"), MethodChoice::Only(Method::Copy));
    g.method = Some("symlink".into());
    assert_eq!(r(None, &g, "copy"), MethodChoice::Only(Method::Symlink));
    assert_eq!(
        r(Some("reflink"), &g, "copy"),
        MethodChoice::Only(Method::Reflink)
    );
    assert!(matches!(
        deploy::resolve_method(Some("move"), &g, "auto"),
        Err(DeployError::InvalidMethod(_))
    ));
}

#[test]
fn missing_game_folder_is_a_typed_error() {
    let mut fx = Fx::new();
    fx.game.folder = None;
    assert!(matches!(fx.deploy(), Err(DeployError::NoTargetRoot { .. })));
    fx.game.folder = Some(fx.tmp.path().join("nope"));
    assert!(matches!(
        fx.deploy(),
        Err(DeployError::TargetRootMissing(_))
    ));
}
