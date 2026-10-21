// SPDX-License-Identifier: GPL-3.0-or-later
//! CLI tests for M3 mod management without network: install from local
//! archives, list/info, enable/disable, order, uninstall, conflicts, deps
//! with ack/unack, profiles, clean, launch, reinstall.

mod support;

use std::path::PathBuf;

use predicates::prelude::*;
use predicates::str::contains;
use support::Env;

const G: &str = "testgame";

fn setup() -> (Env, PathBuf) {
    let env = Env::new();
    let dir = env.add_game(G);
    (env, dir)
}

/// Installs a zip with the given `Data/...` files under `name`.
fn install(env: &Env, name: &str, files: &[(&str, &[u8])]) {
    let z = env.zip(&format!("{name}.zip"), files);
    env.run(&["install", z.to_str().unwrap()]).success();
}

fn three(env: &Env) {
    install(
        env,
        "Alpha",
        &[("Data/alpha.esp", b"a"), ("Data/shared.dds", b"from-a")],
    );
    install(
        env,
        "Beta",
        &[("Data/beta.esp", b"b"), ("Data/shared.dds", b"from-b")],
    );
    install(env, "Gamma", &[("Data/gamma.esp", b"g")]);
}

// ── install / list / info ───────────────────────────────────

#[test]
fn install_local_archive_stages_files_and_lists_mod() {
    let (env, _) = setup();
    let z = env.zip(
        "Cool Mod.zip",
        &[
            ("Data/cool.esp", b"esp"),
            ("Data/textures/a.dds", b"dds"),
            ("readme.txt", b"doc"),
        ],
    );
    env.run(&["install", z.to_str().unwrap()])
        .success()
        .stdout(contains("Installed").and(contains("Cool_Mod")));
    assert_eq!(env.uids(), ["Cool_Mod"]);
    let staging = env.paths().data_dir.join(G).join("staging/Cool_Mod");
    assert_eq!(std::fs::read(staging.join("cool.esp")).unwrap(), b"esp");
    assert!(staging.join("textures/a.dds").is_file());
    // Archive kept next to downloads, file list recorded.
    assert!(env
        .paths()
        .data_dir
        .join(G)
        .join("downloads/Cool Mod.zip")
        .is_file());
    assert_eq!(env.query_i64("SELECT COUNT(*) FROM mod_files"), 2);
    for alias in ["list", "ls"] {
        env.run(&[alias])
            .success()
            .stdout(contains("Cool Mod").and(contains("1 shown · 1 installed · 1 enabled")));
    }
    for alias in ["info", "show"] {
        env.run(&[alias, "cool"])
            .success()
            .stdout(contains("ID          : Cool_Mod").and(contains("Position    : #1   enabled")));
    }
}

#[test]
fn install_aliases_name_flag_and_deploy_flag() {
    let (env, game) = setup();
    let z = env.zip("a.zip", &[("Data/a.esp", b"a")]);
    env.run(&["i", z.to_str().unwrap(), "--name", "Named Mod"])
        .success();
    let z2 = env.zip("b.zip", &[("Data/b.esp", b"b")]);
    env.run(&["add", z2.to_str().unwrap(), "--deploy"])
        .success()
        .stdout(contains("Deployed"));
    assert_eq!(env.uids(), ["Named_Mod", "b"]);
    assert!(game.join("Data/b.esp").is_file() && game.join("Data/a.esp").is_file());
}

#[test]
fn install_rejects_unknown_targets_and_routes_collections() {
    let (env, _) = setup();
    env.run(&["install", "not a target"])
        .failure()
        .stderr(contains("Don't know how to install"));
    let z = env.zip("a.zip", &[("Data/a.esp", b"a")]);
    env.run(&["install", z.to_str().unwrap(), "--fomod", "bogus"])
        .failure();
    env.run(&[
        "install",
        "https://www.nexusmods.com/games/testgame/collections/abc123",
        "--optional",
        "all",
        "-r",
        "2",
        "--download-only",
    ])
    .failure()
    // Routed to the collection installer, which needs a login.
    .stderr(contains("Not logged in"));
    env.run(&[
        "install",
        "nxm://testgame/collections/abc123",
        "--revision",
        "3",
        "--deploy",
    ])
    .failure()
    .stderr(contains("Not logged in"));
    env.run(&["dl", "nxm://testgame/collections/abc123"])
        .failure()
        .stderr(contains("Not logged in"));
    assert!(env.uids().is_empty());
}

#[test]
fn install_of_docs_only_archive_fails_without_a_record() {
    let (env, _) = setup();
    let z = env.zip("docs.zip", &[("readme.txt", b"x")]);
    env.run(&["install", z.to_str().unwrap()])
        .failure()
        .stderr(contains("no installable files"));
    assert!(env.uids().is_empty());
}

#[test]
fn list_filters_by_state_text_and_collection() {
    let (env, _) = setup();
    three(&env);
    env.run(&["disable", "Beta"]).success();
    env.store()
        .conn()
        .execute_batch(
            "INSERT INTO collections (id, game, slug, revision, installed_at) VALUES (1, 'testgame', 'col1', 1, 0);
             INSERT INTO collection_mods (collection_id, mod_id, position)
               SELECT 1, id, 0 FROM mods WHERE uid = 'Gamma';",
        )
        .unwrap();
    env.run(&["list", "-e"])
        .success()
        .stdout(contains("2 shown"));
    env.run(&["list", "--enabled"])
        .success()
        .stdout(contains("Alpha").and(contains("Beta").not()));
    env.run(&["list", "-d"])
        .success()
        .stdout(contains("1 shown"));
    env.run(&["list", "--disabled"])
        .success()
        .stdout(contains("Beta").and(contains("Alpha").not()));
    env.run(&["list", "-c", "col1"])
        .success()
        .stdout(contains("1 shown").and(contains("Gamma")));
    env.run(&["list", "--collection", "col1"])
        .success()
        .stdout(contains("col1"));
    env.run(&["list", "alp"])
        .success()
        .stdout(contains("1 shown"));
}

// ── mod references ──────────────────────────────────────────

#[test]
fn mod_reference_forms_and_ambiguity_errors_from_the_cli() {
    let (env, _) = setup();
    install(&env, "Skyrim UI", &[("Data/ui.esp", b"1")]);
    install(&env, "Skyrim UI Patch", &[("Data/p.esp", b"2")]);
    env.store()
        .conn()
        .execute(
            "UPDATE mods SET nexus_mod_id = 4242 WHERE uid = 'Skyrim_UI_Patch'",
            [],
        )
        .unwrap();
    env.run(&["info", "#2"])
        .success()
        .stdout(contains("Skyrim_UI_Patch"));
    env.run(&["info", "Skyrim_UI"])
        .success()
        .stdout(contains("ID          : Skyrim_UI\n"));
    env.run(&["info", "skyrim ui"])
        .success()
        .stdout(contains("ID          : Skyrim_UI\n"));
    env.run(&["info", "4242"])
        .success()
        .stdout(contains("Skyrim_UI_Patch"));
    env.run(&["info", "patch"])
        .success()
        .stdout(contains("Skyrim_UI_Patch"));
    env.run(&["info", "sky"]).failure().stderr(
        contains("'sky' matches 2 mods")
            .and(contains("#1  Skyrim_UI"))
            .and(contains("#2  Skyrim_UI_Patch")),
    );
    env.run(&["info", "nope"])
        .failure()
        .stderr(contains("no installed mod matches 'nope'"));
}

// ── enable / disable ────────────────────────────────────────

#[test]
fn enable_disable_by_ref_all_and_collection() {
    let (env, _) = setup();
    three(&env);
    env.run(&["disable", "Alpha", "#3"])
        .success()
        .stdout(contains("Disabled: Alpha").and(contains("Disabled: Gamma")));
    assert_eq!(
        env.enabled(),
        [
            ("Alpha".into(), false),
            ("Beta".into(), true),
            ("Gamma".into(), false)
        ]
    );
    env.run(&["enable", "--all"]).success();
    assert!(env.enabled().iter().all(|e| e.1));
    env.run(&["disable", "-a"]).success();
    assert!(env.enabled().iter().all(|e| !e.1));
    env.store()
        .conn()
        .execute_batch(
            "INSERT INTO collections (id, game, slug, revision, installed_at) VALUES (1, 'testgame', 'c1', 1, 0);
             INSERT INTO collection_mods (collection_id, mod_id, position)
               SELECT 1, id, 0 FROM mods WHERE uid IN ('Alpha', 'Gamma');",
        )
        .unwrap();
    env.run(&["enable", "--collection", "c1"]).success();
    assert_eq!(
        env.enabled(),
        [
            ("Alpha".into(), true),
            ("Beta".into(), false),
            ("Gamma".into(), true)
        ]
    );
    env.run(&["disable", "-c", "c1"]).success();
    assert!(env.enabled().iter().all(|e| !e.1));
    env.run(&["enable", "-c", "missing"])
        .failure()
        .stderr(contains("no mods belong to collection"));
    env.run(&["enable"])
        .failure()
        .stderr(contains("Usage: modman enable"));
}

// ── order ───────────────────────────────────────────────────

#[test]
fn order_variants_and_aliases() {
    let (env, _) = setup();
    three(&env);
    install(&env, "Delta", &[("Data/d.esp", b"d")]);
    let o = |args: &[&str], want: [&str; 4]| {
        env.run(args).success();
        assert_eq!(env.uids(), want, "{args:?}");
    };
    o(
        &["order", "Alpha", "last"],
        ["Beta", "Gamma", "Delta", "Alpha"],
    );
    o(
        &["order", "Alpha", "first"],
        ["Alpha", "Beta", "Gamma", "Delta"],
    );
    o(
        &["move", "Alpha", "top"],
        ["Beta", "Gamma", "Delta", "Alpha"],
    );
    o(
        &["mv", "Alpha", "bottom"],
        ["Alpha", "Beta", "Gamma", "Delta"],
    );
    o(
        &["order", "Alpha", "highest"],
        ["Beta", "Gamma", "Delta", "Alpha"],
    );
    o(
        &["order", "Alpha", "lowest"],
        ["Alpha", "Beta", "Gamma", "Delta"],
    );
    o(
        &["order", "Alpha", "up"],
        ["Beta", "Alpha", "Gamma", "Delta"],
    );
    o(
        &["order", "Alpha", "up", "2"],
        ["Beta", "Gamma", "Delta", "Alpha"],
    );
    o(
        &["order", "Alpha", "down"],
        ["Beta", "Gamma", "Alpha", "Delta"],
    );
    o(
        &["order", "Alpha", "down", "9"],
        ["Alpha", "Beta", "Gamma", "Delta"],
    );
    o(
        &["order", "Alpha", "after", "Gamma"],
        ["Beta", "Gamma", "Alpha", "Delta"],
    );
    o(
        &["order", "Delta", "before", "Beta"],
        ["Delta", "Beta", "Gamma", "Alpha"],
    );
    o(
        &["order", "Alpha", "#2"],
        ["Delta", "Alpha", "Beta", "Gamma"],
    );
    o(
        &["order", "Alpha", "4"],
        ["Delta", "Beta", "Gamma", "Alpha"],
    );
    env.run(&["order", "Alpha", "#1"])
        .success()
        .stdout(contains("Alpha: #4 → #1  (of 4)"));
    env.run(&["order", "Alpha", "sideways"])
        .failure()
        .stderr(contains("invalid order"));
    env.run(&["order", "Alpha", "before", "Alpha"])
        .failure()
        .stderr(contains("relative to itself"));
    env.run(&["order", "Alpha"]).failure();
}

// ── uninstall ───────────────────────────────────────────────

#[test]
fn uninstall_aliases_and_delete_archive() {
    let (env, _) = setup();
    three(&env);
    let dl = env.paths().data_dir.join(G).join("downloads");
    env.run(&["uninstall", "Alpha"])
        .success()
        .stdout(contains("Uninstalled 1 mod(s)."));
    assert!(dl.join("Alpha.zip").is_file(), "archive kept by default");
    assert!(!env.paths().data_dir.join(G).join("staging/Alpha").exists());
    env.run(&["remove", "Beta", "--delete-archive"]).success();
    assert!(!dl.join("Beta.zip").exists());
    env.run(&["rm", "Gamma", "--purge-archive"]).success();
    assert!(!dl.join("Gamma.zip").exists());
    assert!(env.uids().is_empty());
    env.run(&["uninstall"]).failure();
    env.run(&["uninstall", "ghost"])
        .failure()
        .stderr(contains("no installed mod matches"));
}

// ── conflicts ───────────────────────────────────────────────

#[test]
fn conflicts_lists_pairs_filter_and_files() {
    let (env, _) = setup();
    three(&env);
    env.run(&["conflicts"]).success().stdout(
        contains("Alpha")
            .and(contains("overridden by"))
            .and(contains("Beta"))
            .and(contains("(1 files)")),
    );
    env.run(&["conflicts", "Beta", "--files"])
        .success()
        .stdout(contains("Data/shared.dds"));
    env.run(&["conflicts", "Alpha", "-f"])
        .success()
        .stdout(contains("Data/shared.dds"));
    env.run(&["conflicts", "Gamma"])
        .success()
        .stdout(contains("No file conflicts"));
    env.run(&["info", "Beta"])
        .success()
        .stdout(contains("Conflicts   : wins 1 file(s), overridden on 0"));
}

// ── deps ────────────────────────────────────────────────────

fn seed_requirements(env: &Env) {
    install(env, "Fancy UI", &[("Data/ui.esp", b"1")]);
    let s = env.store();
    s.conn()
        .execute_batch(
            r#"UPDATE mods SET nexus_mod_id = 100, nexus_file_id = 1 WHERE uid = 'Fancy_UI';
               INSERT INTO requirements_cache (game, nexus_mod_id, body, available, fetched_at, checked_at)
               VALUES ('testgame', 100,
                 '[{"modId":0,"modName":"SKSE64","externalRequirement":true,"url":"https://skse.silverlock.org"},
                   {"modName":"Visual C++ Redistributable","externalRequirement":true,"url":"https://www.microsoft.com/vc"}]',
                 1, strftime('%s','now'), strftime('%s','now'));"#,
        )
        .unwrap();
}

#[test]
fn deps_reports_external_tools_and_ack_unack_persist() {
    let (env, _) = setup();
    seed_requirements(&env);
    for alias in ["deps", "requirements", "req"] {
        env.run(&[alias]).success().stdout(
            contains("SKSE")
                .and(contains("missing"))
                .and(contains("Visual C++ Redistributable"))
                .and(contains("external")),
        );
    }
    env.run(&["deps", "ack", "skse"])
        .success()
        .stdout(contains("Marked"));
    // A new process sees the ack (persisted per game).
    env.run(&["deps"])
        .success()
        .stdout(contains("SKSE").and(contains("marked as installed")));
    env.run(&["deps", "--json"]).success().stdout(
        contains(r#""key":"tool:skse""#)
            .and(contains(r#""status":"ok""#))
            .and(contains(r#""acked":true"#)),
    );
    assert_eq!(
        env.query_i64("SELECT COUNT(*) FROM deps_ack WHERE key = 'tool:skse'"),
        1
    );
    env.run(&["deps", "unack", "SKSE"])
        .success()
        .stdout(contains("will be checked again"));
    env.run(&["deps", "--json"])
        .success()
        .stdout(contains(r#""status":"missing""#).and(contains(r#""acked":false"#)));
    env.run(&["deps", "unack", "skse"])
        .failure()
        .stderr(contains("No requirement matches"));
    env.run(&["deps", "ack", "visual"]).success();
    env.run(&["deps", "Fancy UI"])
        .success()
        .stdout(contains("Requirements — Fancy UI"));
    env.run(&["deps", "--refresh"])
        .success()
        .stderr(contains("Not logged in"));
    env.run(&["deps", "-r"]).success();
    env.run(&["deps", "ack"])
        .failure()
        .stderr(contains("Usage: modman deps ack"));
}

#[test]
fn deps_finds_tool_files_in_game_folder() {
    let (env, game) = setup();
    seed_requirements(&env);
    std::fs::write(game.join("skse64_loader.exe"), "x").unwrap();
    env.run(&["deps"])
        .success()
        .stdout(contains("found: skse64_loader.exe"));
}

// ── profiles ────────────────────────────────────────────────

#[test]
fn profile_save_load_roundtrip_and_aliases() {
    let (env, _) = setup();
    three(&env);
    env.run(&["disable", "Beta"]).success();
    env.run(&["profile", "save", "p1"])
        .success()
        .stdout(contains("Saved profile 'p1' (3 mods)."));
    let saved = env.enabled();
    env.run(&["order", "Alpha", "last"]).success();
    env.run(&["enable", "--all"]).success();
    install(&env, "Late", &[("Data/late.esp", b"l")]);
    env.run(&["profile", "load", "p1"])
        .success()
        .stdout(contains("Loaded profile 'p1'."));
    let mut want = saved.clone();
    want.push(("Late".into(), false));
    assert_eq!(env.enabled(), want);
    env.run(&["order", "Alpha", "last"]).success();
    env.run(&["profile", "use", "p1"]).success();
    assert_eq!(env.enabled(), want);
    for args in [
        &["profile"][..],
        &["profiles"],
        &["profile", "list"],
        &["profile", "ls"],
    ] {
        env.run(args)
            .success()
            .stdout(contains("p1").and(contains("3 mods, 2 enabled")));
    }
    env.run(&["profile", "save", "bad name"])
        .failure()
        .stderr(contains("profile names"));
    env.run(&["profile", "rm", "p1"]).success();
    env.run(&["profile", "save", "p2"]).success();
    env.run(&["profile", "delete", "p2"])
        .success()
        .stdout(contains("Deleted profile 'p2'."));
    env.run(&["profile", "load", "p2"])
        .failure()
        .stderr(contains("no profile named 'p2'"));
}

// ── clean ───────────────────────────────────────────────────

#[test]
fn clean_deletes_only_unreferenced_downloads_after_confirmation() {
    let (env, _) = setup();
    install(&env, "Kept", &[("Data/k.esp", b"k")]);
    let dl = env.paths().data_dir.join(G).join("downloads");
    std::fs::write(dl.join("orphan.7z"), "o").unwrap();
    std::fs::write(dl.join("partial.zip.part"), "p").unwrap();
    env.run(&["clean"])
        .success()
        .stdout(contains("orphan.7z").and(contains("Kept.")));
    assert!(dl.join("orphan.7z").exists());
    env.run(&["-y", "clean"])
        .success()
        .stdout(contains("Freed"));
    assert!(!dl.join("orphan.7z").exists());
    assert!(dl.join("Kept.zip").exists() && dl.join("partial.zip.part").exists());
    env.run(&["clean"])
        .success()
        .stdout(contains("No unreferenced downloads"));
}

// ── launch ──────────────────────────────────────────────────

#[test]
fn launch_aliases_need_appid_and_start_steam() {
    let (env, game) = setup();
    env.run(&["launch"])
        .failure()
        .stderr(contains("No Steam app id"));
    env.run(&["game", "set", "appid", "489830"]).success();
    install(&env, "Alpha", &[("Data/alpha.esp", b"a")]);
    // Mods changed since the last deploy: offers to deploy first (default yes).
    env.run(&["play"])
        .success()
        .stdout(contains("Deployed").and(contains("Launching")));
    assert!(game.join("Data/alpha.esp").is_file());
    for alias in ["launch", "run"] {
        env.run(&[alias])
            .success()
            .stdout(contains("Deployed").not());
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while env.calls("steam").lines().count() < 3 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let calls = env.calls("steam");
    assert_eq!(calls.lines().count(), 3, "{calls}");
    assert!(calls.lines().all(|l| l == "steam://rungameid/489830"));
}

// ── reinstall ───────────────────────────────────────────────

#[test]
fn reinstall_reextracts_and_keeps_state() {
    let (env, _) = setup();
    install(&env, "Alpha", &[("Data/alpha.esp", b"a")]);
    env.run(&["disable", "Alpha"]).success();
    let st = env.paths().data_dir.join(G).join("staging/Alpha");
    std::fs::remove_file(st.join("alpha.esp")).unwrap();
    env.run(&["reinstall", "Alpha"])
        .success()
        .stdout(contains("Reinstalled Alpha"));
    assert!(st.join("alpha.esp").is_file());
    assert_eq!(env.enabled(), [("Alpha".into(), false)]);
    // No FOMOD in the archive: the options are accepted and change nothing.
    env.run(&["reinstall", "Alpha", "--fomod", "default"])
        .success();
    env.run(&["reinstall", "Alpha", "--fomod", "bogus"])
        .failure();
}

// ── FOMOD (M5) ──────────────────────────────────────────────

const FOMOD_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<config>
  <moduleName>Fancy Fixture</moduleName>
  <requiredInstallFiles>
    <file source="core.esp" destination="core.esp"/>
    <file source="..\Fancy.zip" destination="escape.zip"/>
  </requiredInstallFiles>
  <installSteps order="Explicit">
    <installStep name="Main">
      <optionalFileGroups>
        <group name="Look" type="SelectExactlyOne">
          <plugins order="Explicit">
            <plugin name="Light">
              <description>Bright textures</description>
              <files><file source="opt\light.esp" destination="look.esp"/></files>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
            <plugin name="Dark">
              <description>Dark textures</description>
              <files><file source="opt/dark.esp" destination="look.esp"/></files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#;

fn fomod_zip(env: &Env) -> PathBuf {
    env.zip(
        "Fancy.zip",
        &[
            ("Fancy-1.0/fomod/ModuleConfig.xml", FOMOD_XML.as_bytes()),
            ("Fancy-1.0/core.esp", b"core"),
            ("Fancy-1.0/opt/light.esp", b"light"),
            ("Fancy-1.0/opt/dark.esp", b"dark"),
        ],
    )
}

fn look(env: &Env) -> String {
    let st = env.paths().data_dir.join(G).join("staging/Fancy");
    assert!(st.join("core.esp").is_file());
    assert!(!st.join("fomod").exists() && !st.join("opt").exists());
    std::fs::read_to_string(st.join("look.esp")).unwrap()
}

#[test]
fn install_fomod_default_installs_default_option_and_info_shows_choices() {
    let (env, _) = setup();
    let z = fomod_zip(&env);
    env.run(&["install", z.to_str().unwrap(), "--fomod", "default"])
        .success()
        .stdout(contains("FOMOD installer: default choices").and(contains("Installed")))
        .stderr(contains("FOMOD: '..\\Fancy.zip' not found in the archive"));
    assert_eq!(look(&env), "light");
    let st = env.paths().data_dir.join(G).join("staging/Fancy");
    assert!(!st.join("escape.zip").exists());
    env.run(&["info", "Fancy"])
        .success()
        .stdout(contains("FOMOD       :").and(contains("Main: Light")));
}

#[test]
fn install_without_tty_uses_defaults_and_fomod_none_uses_plain_layout() {
    let (env, _) = setup();
    let z = fomod_zip(&env);
    // auto (config default) without a terminal: defaults, no prompt.
    env.run(&["install", z.to_str().unwrap()])
        .success()
        .stderr(contains("Choose").not());
    assert_eq!(look(&env), "light");
    env.run(&["reinstall", "Fancy", "--fomod", "none"])
        .success();
    env.run(&["info", "Fancy"])
        .success()
        .stdout(contains("FOMOD").not());
}

#[test]
fn install_fomod_ask_reads_scripted_answers() {
    let (env, _) = setup();
    let z = fomod_zip(&env);
    env.cmd()
        .args(["install", z.to_str().unwrap(), "--fomod", "ask"])
        .write_stdin("?2\n3\n2\n")
        .assert()
        .success()
        .stderr(
            contains("FOMOD installer: Fancy Fixture")
                .and(contains("Look"))
                .and(contains("Dark textures"))
                .and(contains("invalid selection")),
        );
    assert_eq!(look(&env), "dark");
    // Closed input cancels the install.
    env.cmd()
        .args(["reinstall", "Fancy", "--ask"])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(contains("cancelled"));
    assert_eq!(look(&env), "dark");
}

#[test]
fn reinstall_reuses_choices_and_ask_reprompts() {
    let (env, _) = setup();
    let z = fomod_zip(&env);
    env.cmd()
        .args(["install", z.to_str().unwrap(), "--fomod", "ask"])
        .write_stdin("2\n")
        .assert()
        .success();
    assert_eq!(look(&env), "dark");
    // Default reinstall replays the recorded choices without asking.
    env.cmd()
        .args(["reinstall", "Fancy"])
        .write_stdin("1\n")
        .assert()
        .success()
        .stdout(contains("FOMOD installer: recorded choices"))
        .stderr(contains("Choose").not());
    assert_eq!(look(&env), "dark");
    // --ask asks again.
    env.cmd()
        .args(["reinstall", "Fancy", "--ask"])
        .write_stdin("1\n")
        .assert()
        .success()
        .stderr(contains("Choose"));
    assert_eq!(look(&env), "light");
    env.run(&["info", "Fancy"])
        .success()
        .stdout(contains("Main: Light"));
    // -y: no questions, defaults.
    env.run(&["-y", "reinstall", "Fancy", "--fomod", "ask"])
        .success()
        .stdout(contains("FOMOD installer: default choices"));
}

#[test]
fn auto_deploy_redeploys_after_changes() {
    let (env, game) = setup();
    env.run(&["config", "auto-deploy", "true"]).success();
    install(&env, "Alpha", &[("Data/alpha.esp", b"a")]);
    assert!(game.join("Data/alpha.esp").is_file());
    env.run(&["disable", "Alpha"]).success();
    assert!(!game.join("Data/alpha.esp").exists());
}
