// SPDX-License-Identifier: GPL-3.0-or-later
//! CLI tests for deploy / purge / verify / status (M2). Temp HOME and game
//! folders only.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use modman_core::paths::Paths;
use modman_core::store::{DeployLock, Store};
use predicates::prelude::*;
use predicates::str::contains;

const GAME: &str = "testgame";

struct Env {
    tmp: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let env = Self {
            tmp: tempfile::tempdir().unwrap(),
        };
        let game = env.game_dir();
        std::fs::create_dir_all(game.join("Data")).unwrap();
        std::fs::write(game.join("Data/Original.esp"), "orig").unwrap();
        std::fs::write(game.join("game.exe"), "exe").unwrap();
        env.run(&["-y", "game", "add", GAME, game.to_str().unwrap()])
            .success();
        env
    }

    fn home(&self) -> PathBuf {
        self.tmp.path().join("home")
    }

    fn paths(&self) -> Paths {
        Paths::for_home(&self.home())
    }

    fn game_dir(&self) -> PathBuf {
        self.tmp.path().join("games").join(GAME)
    }

    fn cmd(&self) -> Command {
        let home = self.home();
        let mut c = Command::cargo_bin("modman").unwrap();
        c.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"));
        c
    }

    fn run(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        self.cmd().args(args).assert()
    }

    fn store(&self) -> Store {
        Store::open(&self.paths().db_path()).unwrap()
    }

    fn add_mod(&self, uid: &str, priority: i64, files: &[(&str, &str)]) {
        let dir = self.tmp.path().join("staging").join(uid);
        for (p, data) in files {
            let f = dir.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, data).unwrap();
        }
        self.store()
            .conn()
            .execute(
                "INSERT INTO mods (game, uid, name, priority, staging_dir, installed_at)
                 VALUES (?1, ?2, ?2, ?3, ?4, 0)",
                (GAME, uid, priority, dir.to_string_lossy()),
            )
            .unwrap();
    }

    fn two_mods(&self) {
        self.add_mod(
            "a",
            0,
            &[("Data/original.esp", "a-esp"), ("Data/x.txt", "a-x")],
        );
        self.add_mod("b", 1, &[("Data/X.txt", "b-x"), ("New/y.txt", "b-y")]);
    }

    fn deployed_methods(&self) -> Vec<String> {
        let store = self.store();
        let mut stmt = store
            .conn()
            .prepare("SELECT method FROM deployments ORDER BY rel_path")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }
}

fn snapshot(dir: &Path) -> BTreeMap<String, (Vec<u8>, u32)> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, (Vec<u8>, u32)>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let meta = std::fs::symlink_metadata(&p).unwrap();
            let data = if meta.is_file() {
                std::fs::read(&p).unwrap()
            } else {
                Vec::new()
            };
            let rel = p.strip_prefix(root).unwrap().display().to_string();
            out.insert(rel, (data, meta.permissions().mode()));
            if meta.is_dir() {
                walk(root, &p, out);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

#[test]
fn deploy_verify_status_purge_roundtrip_restores_game() {
    let env = Env::new();
    env.two_mods();
    let before = snapshot(&env.game_dir());

    env.run(&["deploy"])
        .success()
        .stdout(contains("Deployed 3 file(s) from 2 mod(s)").and(contains("1 file conflict")));
    assert_eq!(
        std::fs::read_to_string(env.game_dir().join("Data/Original.esp")).unwrap(),
        "a-esp"
    );
    assert_eq!(
        std::fs::read_to_string(env.game_dir().join("Data/x.txt")).unwrap(),
        "b-x"
    );
    env.run(&["verify"])
        .success()
        .stdout(contains("Intact: 3").and(contains("All deployed files are intact")));
    env.run(&["st"])
        .success()
        .stdout(contains("deployed: 3 file(s)").and(contains("1 original(s) backed up")));

    env.run(&["-y", "purge"])
        .success()
        .stdout(contains("Removed 3, restored 1 original(s)"));
    assert_eq!(snapshot(&env.game_dir()), before);
    env.run(&["status"])
        .success()
        .stdout(contains("not deployed"));
    env.run(&["undeploy"])
        .success()
        .stdout(contains("Nothing is deployed"));
}

#[test]
fn deploy_dry_run_changes_nothing() {
    let env = Env::new();
    env.two_mods();
    let before = snapshot(&env.game_dir());
    env.run(&["deploy", "--dry-run"])
        .success()
        .stdout(contains("Dry run: would deploy 3 file(s)").and(contains("back up 1")));
    assert_eq!(snapshot(&env.game_dir()), before);
    assert!(env.deployed_methods().is_empty());
}

#[test]
fn deploy_method_flag_is_recorded_and_validated() {
    let env = Env::new();
    env.two_mods();
    env.run(&["deploy", "--method", "copy"])
        .success()
        .stdout(contains("3 copy"));
    assert_eq!(env.deployed_methods(), ["copy", "copy", "copy"]);
    env.run(&["deploy", "--method", "move"])
        .failure()
        .stderr(contains("invalid deploy method 'move'"));
}

#[test]
fn verify_fails_on_changed_or_missing_files() {
    let env = Env::new();
    env.two_mods();
    env.run(&["deploy"]).success();
    std::fs::remove_file(env.game_dir().join("New/y.txt")).unwrap();
    env.run(&["verify"])
        .failure()
        .stdout(contains("Missing: 1").and(contains("New/y.txt")))
        .stderr(contains("changed or missing"));
}

#[test]
fn deploy_refused_while_lock_held() {
    let env = Env::new();
    env.two_mods();
    let _held = DeployLock::try_acquire(&env.paths().deploy_lock_path()).unwrap();
    env.run(&["deploy"]).failure().stderr(contains("lock"));
    assert!(env.deployed_methods().is_empty());
}

#[test]
fn deploy_and_purge_refused_while_game_process_runs() {
    let env = Env::new();
    env.two_mods();
    env.run(&["deploy"]).success();
    // A process whose command line names a file inside the game folder.
    let exe = env.game_dir().join("game.exe");
    let mut child = std::process::Command::new("sh")
        .args(["-c", "sleep 30; true"])
        .arg(&exe)
        .spawn()
        .unwrap();
    // Wait until the probe the CLI uses actually sees the child, so the
    // refusal below is not racing process start-up.
    let game = modman_core::games::require_game(env.store().conn(), GAME).unwrap();
    let probe = modman_core::deploy::ProcProbe::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut seen = false;
    while std::time::Instant::now() < deadline {
        if modman_core::deploy::GameProbe::is_running(&probe, &game) {
            seen = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let res = std::panic::catch_unwind(|| {
        assert!(seen, "ProcProbe never saw the child process");
        for args in [&["deploy"][..], &["-y", "purge"][..]] {
            let out = env.cmd().args(args).output().unwrap();
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                !out.status.success() && stderr.contains("is running"),
                "{args:?}: status {:?}\nstdout:\n{}\nstderr:\n{stderr}",
                out.status,
                String::from_utf8_lossy(&out.stdout)
            );
        }
    });
    let _ = child.kill();
    let _ = child.wait();
    res.unwrap();
    env.run(&["-y", "purge"]).success();
}
