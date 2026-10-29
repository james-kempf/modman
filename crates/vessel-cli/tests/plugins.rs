// SPDX-License-Identifier: GPL-3.0-or-later
//! CLI tests for the plugin stage (M6): deploy/purge update plugins.txt in a
//! temp Proton prefix; `plugins list|sort|masterlist`. Temp HOME only; the
//! masterlist server is mocked.

use std::path::PathBuf;

use assert_cmd::Command;
use predicates::str::contains;
use vessel_core::paths::Paths;
use vessel_core::store::Store;

mod support;

const GAME: &str = "skyrimspecialedition";

struct Env {
    tmp: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let env = Self {
            tmp: tempfile::tempdir().unwrap(),
        };
        let game = env.tmp.path().join("games/sse");
        std::fs::create_dir_all(game.join("Data")).unwrap();
        std::fs::write(game.join("SkyrimSE.exe"), "exe").unwrap();
        std::fs::create_dir_all(env.appdata()).unwrap();
        env.run(&["-y", "game", "add", GAME, game.to_str().unwrap()])
            .success();
        env.run(&["game", "set", "prefix", env.prefix().to_str().unwrap()])
            .success();
        env
    }

    fn prefix(&self) -> PathBuf {
        self.tmp.path().join("prefix")
    }

    fn appdata(&self) -> PathBuf {
        self.prefix()
            .join("drive_c/users/steamuser/AppData/Local/Skyrim Special Edition")
    }

    fn home(&self) -> PathBuf {
        self.tmp.path().join("home")
    }

    fn cmd(&self) -> Command {
        let home = self.home();
        let mut c = Command::cargo_bin("vessel").unwrap();
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

    fn add_mod(&self, uid: &str, priority: i64, files: &[&str]) {
        let dir = self.tmp.path().join("staging").join(uid);
        std::fs::create_dir_all(&dir).unwrap();
        for f in files {
            std::fs::write(dir.join(f), "not a real plugin").unwrap();
        }
        Store::open(&Paths::for_home(&self.home()).db_path())
            .unwrap()
            .conn()
            .execute(
                "INSERT INTO mods (game, uid, name, priority, staging_dir, installed_at)
                 VALUES (?1, ?2, ?2, ?3, ?4, 0)",
                (GAME, uid, priority, dir.to_string_lossy()),
            )
            .unwrap();
    }
}

/// Catches: deploy/purge CLI not wiring the plugin stage, `plugins list`
/// not showing managed state.
#[test]
fn deploy_list_purge_manage_plugins_txt() {
    let env = Env::new();
    let pf = env.appdata().join("plugins.txt");
    std::fs::write(&pf, "*User.esp\r\n").unwrap();
    env.add_mod("one", 0, &["ModA.esp", "ModM.esm"]);
    env.run(&["-y", "deploy"])
        .success()
        .stdout(contains("plugins.txt: 2 plugin(s) activated"));
    let text = std::fs::read_to_string(&pf).unwrap();
    assert!(
        text.ends_with("*User.esp\r\n*ModM.esm\r\n*ModA.esp\r\n"),
        "{text:?}"
    );

    env.run(&["plugins"])
        .success()
        .stdout(contains("ModA.esp (vessel)"))
        .stdout(contains("User.esp"));

    env.run(&["-y", "purge"]).success();
    let text = std::fs::read_to_string(&pf).unwrap();
    assert!(text.contains("*User.esp\r\n") && !text.contains("ModA.esp"));
}

/// Catches: `plugins` running for games without plugins.txt.
#[test]
fn plugins_command_rejects_non_bethesda_game() {
    let env = Env::new();
    let other = env.tmp.path().join("games/cp");
    std::fs::create_dir_all(&other).unwrap();
    env.run(&[
        "-y",
        "game",
        "add",
        "cyberpunk2077",
        other.to_str().unwrap(),
    ])
    .success();
    env.run(&["-g", "cyberpunk2077", "plugins", "list"])
        .failure()
        .stderr(contains("has no plugins.txt"));
}

/// Catches: the masterlist URL not injectable (real network in tests) or
/// the download not cached.
#[test]
fn plugins_masterlist_downloads_from_override_url() {
    let env = Env::new();
    let mock = support::Mock::start(|r, _| {
        if r.path() == "/skyrimse/v0.26/masterlist.yaml" {
            support::Response::bytes(b"plugins: []\n".to_vec())
        } else {
            support::Response::status(404, "")
        }
    });
    env.cmd()
        .env("VESSEL_LOOT_URL", &mock.url)
        .args(["plugins", "masterlist"])
        .assert()
        .success()
        .stdout(contains("masterlist saved"));
    let cached = Paths::for_home(&env.home())
        .data_dir
        .join(GAME)
        .join("cache/loot/masterlist.yaml");
    assert_eq!(std::fs::read_to_string(cached).unwrap(), "plugins: []\n");
    assert!(mock
        .requests()
        .iter()
        .any(|r| r.path() == "/skyrimse/v0.26/masterlist.yaml"));
}
