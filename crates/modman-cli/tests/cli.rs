// SPDX-License-Identifier: GPL-3.0-or-later
//! CLI integration tests (M1): global flags, aliases, game/config/log.
//! Every test runs with a temp HOME and XDG dirs; nothing touches the real
//! home, Steam folders or game installs.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use modman_core::paths::Paths;
use modman_core::store::Store;
use predicates::prelude::*;
use predicates::str::contains;

struct Env {
    tmp: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        Self {
            tmp: tempfile::tempdir().unwrap(),
        }
    }

    fn home(&self) -> PathBuf {
        self.tmp.path().join("home")
    }

    fn paths(&self) -> Paths {
        Paths::for_home(&self.home())
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

    /// A game folder that exists.
    fn game_dir(&self, name: &str) -> PathBuf {
        let d = self.tmp.path().join("games").join(name);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn add_game(&self, domain: &str) -> PathBuf {
        let dir = self.game_dir(domain);
        self.run(&["-y", "game", "add", domain, dir.to_str().unwrap()])
            .success();
        dir
    }

    fn store(&self) -> Store {
        Store::open(&self.paths().db_path()).unwrap()
    }

    fn mark_deployed(&self, domain: &str) {
        self.store()
            .conn()
            .execute(
                "INSERT INTO deployments (game, target, rel_path, method, source, deployed_at)
                 VALUES (?1, 'game_dir', 'Data/a.esp', 'hardlink', '/s/a.esp', 0)",
                [domain],
            )
            .unwrap();
    }

    /// Fake Steam install: root library plus an SD-card library holding SSE.
    fn fake_steam_with_sse(&self) -> PathBuf {
        let root = self.home().join(".local/share/Steam");
        let sd = self.tmp.path().join("run/media/mmcblk0p1");
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::create_dir_all(sd.join("steamapps/common/Skyrim Special Edition/Data")).unwrap();
        std::fs::create_dir_all(sd.join("steamapps/compatdata/489830/pfx")).unwrap();
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../modman-core/tests/fixtures/steam");
        let vdf = std::fs::read_to_string(fixtures.join("libraryfolders.vdf"))
            .unwrap()
            .replace("/home/deck/.local/share/Steam", root.to_str().unwrap())
            .replace("/run/media/mmcblk0p1", sd.to_str().unwrap());
        std::fs::write(root.join("steamapps/libraryfolders.vdf"), vdf).unwrap();
        std::fs::copy(
            fixtures.join("appmanifest_489830.acf"),
            sd.join("steamapps/appmanifest_489830.acf"),
        )
        .unwrap();
        sd.canonicalize().unwrap()
    }
}

const ESC: &str = "\x1b[";

// ── help / version ──────────────────────────────────────────

#[test]
fn help_exits_zero_in_every_form() {
    let env = Env::new();
    for args in [&[][..], &["help"], &["--help"], &["-h"]] {
        env.run(args).success().stdout(contains("Usage: modman"));
    }
}

#[test]
fn version_exits_zero_in_every_form() {
    let env = Env::new();
    for flag in ["version", "--version", "-V"] {
        env.run(&[flag])
            .success()
            .stdout(contains(format!("modman {}", modman_core::VERSION)));
    }
}

#[test]
fn unknown_command_exits_non_zero() {
    Env::new()
        .run(&["frobnicate"])
        .failure()
        .stderr(contains("Unknown command: 'frobnicate'"));
}

// ── aliases ─────────────────────────────────────────────────

/// Every alias in legacy dispatch (L5008-5047) and `cmd_game` maps to the
/// canonical command; clap help names the canonical command.
#[test]
fn every_legacy_alias_resolves_to_its_command() {
    let env = Env::new();
    let top = [
        ("games", "game"),
        ("add", "install"),
        ("i", "install"),
        ("dl", "download"),
        ("collections", "collection"),
        ("col", "collection"),
        ("ls", "list"),
        ("show", "info"),
        ("remove", "uninstall"),
        ("rm", "uninstall"),
        ("move", "order"),
        ("mv", "order"),
        ("requirements", "deps"),
        ("req", "deps"),
        ("update", "updates"),
        ("outdated", "updates"),
        ("find", "search"),
        ("profiles", "profile"),
        ("undeploy", "purge"),
        ("st", "status"),
        ("play", "launch"),
        ("run", "launch"),
        ("logs", "log"),
        ("auto-update", "autoupdate"),
        ("au", "autoupdate"),
    ];
    for (alias, canonical) in top {
        env.run(&[alias, "--help"])
            .success()
            .stdout(contains(format!("Usage: modman {canonical}")));
    }
    let game = [
        ("ls", "list"),
        ("switch", "use"),
        ("select", "use"),
        ("rm", "remove"),
        ("show", "info"),
    ];
    for (alias, canonical) in game {
        env.run(&["game", alias, "--help"])
            .success()
            .stdout(contains(format!("Usage: modman game {canonical}")));
    }
}

/// Catches: `deploy -n` rejected (legacy L1738-1747 accepts `-n|--dry-run`).
#[test]
fn deploy_short_n_is_dry_run() {
    let env = Env::new();
    env.run(&["deploy", "--help"])
        .success()
        .stdout(contains("-n, --dry-run"));
    // Parses: fails on the missing game, not on the flag.
    env.run(&["deploy", "-n"])
        .failure()
        .stderr(contains("No game selected").and(contains("unexpected argument").not()));
}

#[test]
fn former_stub_commands_are_implemented_and_bare_nxm_link_is_routed() {
    let env = Env::new();
    // Collections arrived with M5: list needs a game, not a milestone.
    env.run(&["collection", "list"])
        .failure()
        .stderr(contains("No game selected").and(contains("not implemented").not()));
    // Auto-update arrived with M7.
    env.run(&["au", "status"])
        .success()
        .stdout(contains("Scheduler").and(contains("not implemented").not()));
    // A bare nxm:// link is routed to the nxm handler (legacy L5041), which
    // rejects malformed links.
    env.cmd()
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/nonexistent/modman-test-bus",
        )
        .env("PATH", "/nonexistent")
        .arg("nxm://evil.example/mods/1/files/2")
        .assert()
        .failure()
        .stderr(contains("Unrecognised nxm link"));
}

// ── global flags ────────────────────────────────────────────

#[test]
fn game_flag_overrides_active_game_in_every_spelling() {
    let env = Env::new();
    env.add_game("skyrimspecialedition");
    env.add_game("fallout4"); // -y makes it active
    env.run(&["game", "info"])
        .success()
        .stdout(contains("(fallout4)"));
    for args in [
        &["-g", "SkyrimSpecialEdition", "game", "info"][..],
        &["--game", "skyrimspecialedition", "game", "info"],
        &["--game=skyrimspecialedition", "game", "info"],
        &["game", "info", "-g", "skyrimspecialedition"],
    ] {
        env.run(args)
            .success()
            .stdout(contains("(skyrimspecialedition)"));
    }
}

#[test]
fn yes_flag_answers_prompts() {
    let env = Env::new();
    env.add_game("skyrimspecialedition");
    // Without -y and without a tty the default (No) applies: nothing removed.
    env.run(&["game", "remove", "skyrimspecialedition"])
        .success();
    env.run(&["game", "list"])
        .success()
        .stdout(contains("skyrimspecialedition"));
    env.run(&["game", "remove", "skyrimspecialedition", "--yes"])
        .success()
        .stdout(contains("Removed skyrimspecialedition."));
    env.run(&["game", "list"])
        .success()
        .stdout(contains("No games configured"));
}

#[test]
fn colour_is_forced_only_without_no_color_settings() {
    let env = Env::new();
    let run = |args: &[&str], extra: &[(&str, &str)]| {
        let mut c = env.cmd();
        c.env("CLICOLOR_FORCE", "1").args(args);
        for (k, v) in extra {
            c.env(k, v);
        }
        String::from_utf8(c.assert().success().get_output().stdout.clone()).unwrap()
    };
    assert!(
        run(&["config", "jobs", "4"], &[]).contains(ESC),
        "forced colour expected"
    );
    assert!(!run(&["config", "jobs", "4"], &[("NO_COLOR", "1")]).contains(ESC));
    assert!(!run(&["config", "jobs", "4"], &[("MODMAN_NO_COLOR", "1")]).contains(ESC));
    assert!(!run(&["--no-color", "config", "jobs", "4"], &[]).contains(ESC));
    assert!(!run(&["--help"], &[("NO_COLOR", "1")]).contains(ESC));
    assert!(run(&["--help"], &[]).contains(ESC));
}

#[test]
fn debug_flag_prints_debug_lines_on_stderr() {
    let env = Env::new();
    env.run(&["config"])
        .success()
        .stderr(contains("DEBUG").not());
    env.run(&["--debug", "config"])
        .success()
        .stderr(contains("DEBUG"));
    env.run(&["config", "--debug"])
        .success()
        .stderr(contains("DEBUG"));
}

#[test]
fn double_dash_ends_option_parsing() {
    let env = Env::new();
    env.add_game("skyrimspecialedition");
    env.run(&["game", "set", "name", "--", "-y"]).success();
    env.run(&["game", "info"])
        .success()
        .stdout(contains("(skyrimspecialedition)"));
    let g = modman_core::games::require_game(env.store().conn(), "skyrimspecialedition").unwrap();
    assert_eq!(g.name, "-y");
}

// ── game ────────────────────────────────────────────────────

#[test]
fn game_add_preset_then_list_info_use() {
    let env = Env::new();
    let dir = env.add_game("skyrimspecialedition");
    env.run(&["game"]) // default sub-command is list
        .success()
        .stdout(contains("* skyrimspecialedition").and(contains(dir.to_str().unwrap())));
    env.run(&["game", "info"]).success().stdout(
        contains("subdir         Data")
            .and(contains("profile        bethesda"))
            .and(contains("appid          489830"))
            .and(contains("support        tested")),
    );
    env.add_game("fallout4");
    env.run(&["game", "use", "skyrimspecialedition"])
        .success()
        .stdout(contains("Active game: Skyrim Special Edition"));
    env.run(&["game", "list"])
        .success()
        .stdout(contains("* skyrimspecialedition"));
}

#[test]
fn game_add_experimental_preset_warns() {
    let env = Env::new();
    let dir = env.game_dir("cp");
    env.run(&["-y", "game", "add", "cyberpunk2077", dir.to_str().unwrap()])
        .success()
        .stderr(contains("experimental"));
}

#[test]
fn game_add_detects_steam_folder_on_sd_card() {
    let env = Env::new();
    let sd = env.fake_steam_with_sse();
    env.run(&["game", "add", "skyrimspecialedition"])
        .success()
        .stdout(contains("Found Skyrim Special Edition in Steam"));
    let g = modman_core::games::require_game(env.store().conn(), "skyrimspecialedition").unwrap();
    assert_eq!(
        g.folder,
        Some(sd.join("steamapps/common/Skyrim Special Edition"))
    );
    assert_eq!(g.prefix, Some(sd.join("steamapps/compatdata/489830/pfx")));
    assert_eq!(g.library, Some(sd.clone()));
    assert_eq!(
        g.plugins_txt(),
        Some(sd.join(
            "steamapps/compatdata/489830/pfx/drive_c/users/steamuser/AppData/Local/Skyrim Special Edition/plugins.txt"
        ))
    );
}

#[test]
fn game_add_without_path_fails_when_not_detected() {
    Env::new()
        .run(&["game", "add", "skyrimspecialedition"])
        .failure()
        .stderr(contains(
            "pass it: modman game add skyrimspecialedition <path>",
        ));
}

#[test]
fn game_add_validates_domain_path_and_profile() {
    let env = Env::new();
    let dir = env.game_dir("x");
    let d = dir.to_str().unwrap();
    env.run(&["game", "add", "bad-domain", d]).failure();
    env.run(&["game", "add", "skyrim", "relative/path"])
        .failure()
        .stderr(contains("absolute path"));
    env.run(&["game", "add", "mygame", d, "--profile", "nosuch"])
        .failure()
        .stderr(contains("Unknown profile"));
}

#[test]
fn game_add_custom_domain_uses_generic_defaults_and_flags() {
    let env = Env::new();
    let dir = env.game_dir("custom");
    env.run(&[
        "game",
        "add",
        "MyGame",
        dir.to_str().unwrap(),
        "--subdir",
        "Mods",
        "--profile",
        "ue",
    ])
    .success()
    .stdout(contains("No built-in preset"));
    env.run(&["game", "info", "mygame"]).success().stdout(
        contains("subdir         Mods")
            .and(contains("profile        ue"))
            .and(contains("support        experimental")),
    );
}

#[test]
fn game_detect_lists_installed_presets() {
    let env = Env::new();
    env.run(&["game", "detect"])
        .success()
        .stdout(contains("No supported Steam games found"));
    env.fake_steam_with_sse();
    env.run(&["game", "detect"])
        .success()
        .stdout(contains("skyrimspecialedition").and(contains("not set up")));
    env.run(&["game", "add", "skyrimspecialedition"]).success();
    env.run(&["game", "detect"])
        .success()
        .stdout(contains("configured"));
}

#[test]
fn game_set_accepts_exactly_the_nine_legacy_keys() {
    let env = Env::new();
    env.add_game("skyrimspecialedition");
    let folder = env.game_dir("moved");
    let cases = [
        ("folder", folder.to_str().unwrap()),
        ("subdir", "Data"),
        ("profile", "bethesda"),
        ("appid", "489830"),
        ("method", "copy"),
        ("prefix", "~/pfx"),
        ("plugins-file", "~/plugins.txt"),
        ("paks-dir", "Paks/~mods"),
        ("name", "SSE"),
    ];
    for (k, v) in cases {
        env.run(&["game", "set", k, v])
            .success()
            .stdout(contains(format!("skyrimspecialedition: {k} = {v}")));
    }
    for bad in ["plugins-format", "staging", "Folder"] {
        env.run(&["game", "set", bad, "x"])
            .failure()
            .stderr(contains("unknown game setting"));
    }
    env.run(&["game", "set", "method", "teleport"]).failure();
    env.run(&["game", "set", "profile", "nosuch"]).failure();
    env.run(&["game", "set", "name"]).failure();
}

#[test]
fn game_set_folder_refused_while_deployed() {
    let env = Env::new();
    env.add_game("skyrimspecialedition");
    env.mark_deployed("skyrimspecialedition");
    for key in ["folder", "subdir", "profile"] {
        env.run(&["game", "set", key, "/x"])
            .failure()
            .stderr(contains("purge"));
    }
    env.run(&["game", "set", "name", "Still OK"]).success();
}

#[test]
fn game_remove_refuses_while_deployment_active() {
    let env = Env::new();
    env.add_game("skyrimspecialedition");
    env.mark_deployed("skyrimspecialedition");
    env.run(&["-y", "game", "rm", "skyrimspecialedition"])
        .failure()
        .stderr(contains("still deployed").and(contains("purge")));
    env.run(&["game", "list"])
        .success()
        .stdout(contains("skyrimspecialedition").and(contains("deployed")));
}

#[test]
fn game_remove_delete_data_removes_game_data() {
    let env = Env::new();
    env.add_game("skyrimspecialedition");
    let data = env.paths().data_dir.join("skyrimspecialedition");
    std::fs::create_dir_all(data.join("downloads")).unwrap();
    env.run(&[
        "-y",
        "game",
        "remove",
        "skyrimspecialedition",
        "--delete-data",
    ])
    .success();
    assert!(!data.exists());
    env.run(&["game", "info"])
        .failure()
        .stderr(contains("No game selected"));
}

#[test]
fn game_commands_on_unknown_game_fail_with_hint() {
    let env = Env::new();
    env.run(&["game", "use", "skyrim"])
        .failure()
        .stderr(contains("Run: modman game add skyrim"));
    env.run(&["-g", "skyrim", "game", "info"]).failure();
}

// ── config ──────────────────────────────────────────────────

const CONFIG_KEYS: [&str; 9] = [
    "data-dir",
    "deploy-method",
    "jobs",
    "cdn",
    "adult",
    "auto-deploy",
    "fomod",
    "autoupdate-collections",
    "autoupdate-interval",
];

#[test]
fn config_lists_the_nine_keys_with_defaults() {
    let env = Env::new();
    let out = env.run(&["config"]).success().get_output().stdout.clone();
    let out = String::from_utf8(out).unwrap();
    for k in CONFIG_KEYS {
        assert!(out.contains(k), "missing {k}");
    }
    for (k, v) in [
        ("deploy-method", "auto"),
        ("jobs", "4"),
        ("cdn", ""),
        ("adult", "false"),
        ("auto-deploy", "false"),
        ("fomod", "auto"),
        ("autoupdate-collections", "true"),
        ("autoupdate-interval", "0"),
    ] {
        env.run(&["config", k]).success().stdout(format!("{v}\n"));
    }
    let data = env.paths().data_dir;
    env.run(&["config", "data-dir"])
        .success()
        .stdout(format!("{}\n", data.display()));
}

#[test]
fn config_sets_each_key_and_persists() {
    let env = Env::new();
    let data = env.tmp.path().join("moddata");
    let cases = [
        ("data-dir", data.to_str().unwrap()),
        ("deploy-method", "hardlink"),
        ("jobs", "8"),
        ("cdn", "Amsterdam"),
        ("adult", "true"),
        ("auto-deploy", "true"),
        ("fomod", "ask"),
        ("autoupdate-collections", "false"),
        ("autoupdate-interval", "6"),
    ];
    for (k, v) in cases {
        env.run(&["config", k, v])
            .success()
            .stdout(contains(format!("{k} = {v}")));
        env.run(&["config", k]).success().stdout(format!("{v}\n"));
    }
}

#[test]
fn config_unknown_key_or_bad_value_exits_non_zero() {
    let env = Env::new();
    env.run(&["config", "nosuch"])
        .failure()
        .stderr(contains("unknown setting 'nosuch'").and(contains("autoupdate-interval")));
    env.run(&["config", "nosuch", "1"]).failure();
    for (k, v) in [
        ("jobs", "0"),
        ("jobs", "17"),
        ("deploy-method", "move"),
        ("adult", "yes"),
        ("fomod", "none"),
        ("autoupdate-interval", "-1"),
        ("data-dir", "relative"),
    ] {
        env.run(&["config", k, v]).failure();
    }
}

// ── log ─────────────────────────────────────────────────────

#[test]
fn log_shows_last_lines_and_commands_are_logged() {
    let env = Env::new();
    env.run(&["config", "jobs"]).success();
    env.run(&["game", "list"]).success();
    env.run(&["log"])
        .success()
        .stdout(contains("CMD config jobs").and(contains("CMD game list")));
    let out = env
        .run(&["logs", "1"])
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.contains("CMD logs 1"));
    env.run(&["log", "abc"]).failure();
}

#[test]
fn log_follow_streams_new_lines() {
    let env = Env::new();
    env.run(&["config"]).success();
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("modman"))
        .args(["log", "-f", "1"])
        .env_clear()
        .env("HOME", env.home())
        .env("XDG_DATA_HOME", env.home().join(".local/share"))
        .env("XDG_STATE_HOME", env.home().join(".local/state"))
        .env("XDG_CONFIG_HOME", env.home().join(".config"))
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    assert!(first.contains("CMD log -f 1"), "{first}");
    env.run(&["config", "adult"]).success();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = false;
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        if line.contains("CMD config adult") {
            seen = true;
            break;
        }
    }
    child.kill().unwrap();
    let _ = child.wait();
    assert!(seen, "follow did not print the new line");
}

// ── legacy state ────────────────────────────────────────────

#[test]
fn warns_when_legacy_state_exists() {
    let env = Env::new();
    env.run(&["config"])
        .success()
        .stderr(contains("Legacy modman state").not());
    std::fs::create_dir_all(env.home().join(".config/nexus-mod-manager")).unwrap();
    env.run(&["config"])
        .success()
        .stderr(contains("Legacy modman state found").and(contains("nexus-mod-manager")));
}
