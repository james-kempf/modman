// SPDX-License-Identifier: GPL-3.0-or-later
//! CLI tests for M3 Nexus commands against a local HTTP mock: login/logout,
//! install/download by Nexus id, updates, search, status account section,
//! game add domain check, nxm register/unregister/status and nxm <url>
//! without a running GUI. No request leaves 127.0.0.1.

mod support;

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use predicates::prelude::*;
use predicates::str::contains;
use support::{zip_bytes, Env, FakeNexus, Mock};

const G: &str = "testgame";
const KEY: &str = "SuperSecretApiKey123";

fn fake(premium: bool) -> FakeNexus {
    FakeNexus {
        premium,
        game: G,
        game_id: 77,
        mod_id: 500,
        mod_name: "Cool Mod",
        files: vec![
            (11, "Cool Mod Main", "MAIN", "1.0", zip_bytes(&[("Data/cool.esp", b"v1")])),
            (12, "Cool Mod Optional", "OPTIONAL", "1.0", zip_bytes(&[("Data/opt.esp", b"o")])),
        ],
        file_updates: vec![],
        search_body: r#"{"data":{"mods":{"totalCount":1,"nodes":[{"modId":500,"name":"Cool Mod","author":"Ann","version":"1.0","summary":"Makes things cool","downloads":42,"endorsements":7}]}}}"#.into(),
    }
}

fn mocked(f: FakeNexus) -> (Env, Mock) {
    let f = Arc::new(f);
    let mock = Mock::start(move |r, base| f.handle(r, base));
    let mut env = Env::new();
    env.nexus = Some(mock.url.clone());
    (env, mock)
}

fn login(env: &Env) -> assert_cmd::assert::Assert {
    env.cmd()
        .arg("login")
        .write_stdin(format!("  {KEY}\n"))
        .assert()
}

fn add_game(env: &Env) -> std::path::PathBuf {
    let dir = env.game_dir(G);
    std::fs::create_dir_all(dir.join("Data")).unwrap();
    env.run(&[
        "-y",
        "game",
        "add",
        G,
        dir.to_str().unwrap(),
        "--profile",
        "bethesda",
        "--subdir",
        "Data",
    ])
    .success();
    dir
}

// ── login / logout ──────────────────────────────────────────

#[test]
fn login_reads_key_from_stdin_stores_it_and_never_prints_it() {
    let (env, mock) = mocked(fake(true));
    let a = login(&env).success();
    let o = a.get_output();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr),
    );
    assert!(
        stdout.contains("Authenticated as") && stdout.contains("Premium"),
        "{stdout}"
    );
    for (what, text) in [
        ("stdout", &*stdout),
        ("stderr", &*stderr),
        ("log", &env.log()),
    ] {
        assert!(!text.contains(KEY), "key leaked to {what}: {text}");
    }
    // The key reached Nexus only in the apikey header, never in an URL.
    let reqs = mock.requests();
    assert!(reqs
        .iter()
        .any(|r| r.headers.get("apikey").map(String::as_str) == Some(KEY)));
    assert!(reqs.iter().all(|r| !r.target.contains(KEY)));
    // No session bus here: the key lands in the 0600 file inside a 0700 dir.
    let cred = env.paths().config_dir.join("credentials.toml");
    assert!(std::fs::read_to_string(&cred).unwrap().contains(KEY));
    assert_eq!(
        std::fs::metadata(&cred).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&env.paths().config_dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(
        stderr.contains("Secret Service keyring not available"),
        "{stderr}"
    );
    let s = env.store();
    assert_eq!(s.setting("auth.backend").unwrap().as_deref(), Some("file"));
    assert_eq!(s.setting("nexus.user").unwrap().as_deref(), Some("tester"));
    // The fallback notice is shown once.
    let again = login(&env).success();
    assert!(!String::from_utf8_lossy(&again.get_output().stderr).contains("keyring not available"));

    env.run(&["status"]).success().stdout(
        contains("API key       : configured")
            .and(contains("tester (Premium)"))
            .and(contains(KEY).not()),
    );
    env.run(&["logout"])
        .success()
        .stdout(contains("API key removed."));
    assert!(!cred.exists());
    env.run(&["status"])
        .success()
        .stdout(contains("not set — run: modman login"));
}

#[test]
fn login_never_accepts_the_key_as_an_argument() {
    let (env, mock) = mocked(fake(true));
    for args in [
        &["login", "--key", KEY][..],
        &["login", KEY],
        &["login", "--api-key", KEY],
    ] {
        let a = env.cmd().args(args).write_stdin("").assert().failure();
        let o = a.get_output();
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.contains("login takes no arguments"), "{err}");
        assert!(!err.contains(KEY) && !String::from_utf8_lossy(&o.stdout).contains(KEY));
    }
    assert!(!env.log().contains(KEY));
    assert!(mock.requests().is_empty());
    env.cmd()
        .arg("login")
        .write_stdin("\n")
        .assert()
        .failure()
        .stderr(contains("cannot be empty"));
}

#[test]
fn login_rejected_key_is_not_stored() {
    let mock = Mock::start(|_, _| {
        support::Response::status(401, r#"{"message":"Please provide a valid API Key"}"#)
    });
    let mut env = Env::new();
    env.nexus = Some(mock.url.clone());
    login(&env)
        .failure()
        .stderr(contains("Authentication failed").and(contains(KEY).not()));
    assert!(!env.paths().config_dir.join("credentials.toml").exists());
}

// ── install / download by Nexus id ──────────────────────────

#[test]
fn install_by_nexus_id_with_premium_link() {
    let (env, mock) = mocked(fake(true));
    login(&env).success();
    let game = add_game(&env);
    env.run(&["install", "500", "--deploy"]).success().stdout(
        contains("Cool Mod")
            .and(contains("Installed"))
            .and(contains("Deployed")),
    );
    assert_eq!(env.uids(), ["Cool_Mod-500-11"]);
    assert_eq!(std::fs::read(game.join("Data/cool.esp")).unwrap(), b"v1");
    assert_eq!(
        env.query_i64(
            "SELECT COUNT(*) FROM downloads WHERE status = 'done' AND nexus_file_id = 11"
        ),
        1
    );
    let s = env.store();
    let (src, ver): (String, String) = s
        .conn()
        .query_row("SELECT source, version FROM mods", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((src.as_str(), ver.as_str()), ("nexus", "1.0"));
    // Premium path: no key/expires on the download link request.
    let dl: Vec<_> = mock
        .requests()
        .into_iter()
        .filter(|r| r.path().ends_with("download_link.json"))
        .collect();
    assert_eq!(dl.len(), 1);
    assert!(!dl[0].target.contains("key="));

    // Other target forms; --file/-f picks the optional file, which replaces
    // the installed file of the same mod (legacy default Y) in place.
    env.run(&["-y", "install", "testgame:500", "--file", "12"])
        .success();
    assert_eq!(env.uids(), ["Cool_Mod_-_Cool_Mod_Optional-500-12"]);
    // Same file again: "Reinstall?" defaults to no without a terminal.
    env.run(&[
        "i",
        "https://www.nexusmods.com/testgame/mods/500?tab=files&file_id=12",
    ])
    .success()
    .stdout(contains("Nothing to do."));
    env.run(&["install", "nxm://testgame/mods/500/files/12", "-f", "12"])
        .success()
        .stdout(contains("Nothing to do."));
    assert_eq!(env.uids().len(), 1);
    env.run(&["install", "othergame:500"])
        .failure()
        .stderr(contains("not set up"));
}

#[test]
fn install_by_id_requires_login() {
    let (env, _mock) = mocked(fake(true));
    add_game(&env);
    env.run(&["install", "500"])
        .failure()
        .stderr(contains("Not logged in"));
}

#[test]
fn download_saves_archive_without_installing() {
    let (env, _mock) = mocked(fake(true));
    login(&env).success();
    add_game(&env);
    for (alias, flag, file) in [("download", "--file", "11"), ("dl", "-f", "12")] {
        env.run(&[alias, "500", flag, file])
            .success()
            .stdout(contains("Saved →"));
    }
    let dl = env.paths().data_dir.join(G).join("downloads");
    assert!(dl.join("Cool Mod Main.zip").is_file() && dl.join("Cool Mod Optional.zip").is_file());
    assert!(env.uids().is_empty());
    env.run(&["download", "nonsense"])
        .failure()
        .stderr(contains("Cannot parse"));
}

// ── updates / search ────────────────────────────────────────

#[test]
fn updates_lists_and_installs_newer_files() {
    let mut f = fake(true);
    f.files.push((
        13,
        "Cool Mod Main",
        "MAIN",
        "2.0",
        zip_bytes(&[("Data/cool.esp", b"v2")]),
    ));
    f.files.retain(|x| x.0 != 12);
    f.file_updates = vec![(11, 13)];
    let (env, _mock) = mocked(f);
    login(&env).success();
    add_game(&env);
    env.run(&["-y", "install", "500", "-f", "11"]).success();
    for alias in ["updates", "update", "outdated"] {
        env.run(&[alias])
            .success()
            .stdout(contains("1.0 → 2.0").and(contains("1 update(s) available")));
    }
    env.run(&["updates", "--refresh"]).success();
    env.run(&["-y", "updates", "--install"])
        .success()
        .stdout(contains("1 mod(s) updated."));
    assert_eq!(env.uids(), ["Cool_Mod-500-13"]);
    let st = env
        .paths()
        .data_dir
        .join(G)
        .join("staging/Cool_Mod-500-13/cool.esp");
    assert_eq!(std::fs::read(st).unwrap(), b"v2");
    env.run(&["updates", "-i", "--all"])
        .success()
        .stdout(contains("All mods are up to date."));
}

#[test]
fn search_prints_results_and_json() {
    let (env, _mock) = mocked(fake(false));
    add_game(&env);
    for alias in ["search", "find"] {
        env.run(&[alias, "cool", "mod"]).success().stdout(
            contains("Cool Mod")
                .and(contains("42 downloads"))
                .and(contains("1 result(s)")),
        );
    }
    env.run(&["search", "--json", "cool"]).success().stdout(
        contains(r#"{"total":1,"mods":[{"modId":500,"name":"Cool Mod""#)
            .and(contains(r#""installed":false"#)),
    );
    env.run(&["search"]).failure();
}

// ── game add domain check ───────────────────────────────────

#[test]
fn game_add_checks_domain_on_nexus_when_logged_in() {
    let (env, _mock) = mocked(fake(true));
    login(&env).success();
    let d = env.game_dir("unknowngame");
    env.run(&["-y", "game", "add", "unknowngame", d.to_str().unwrap()])
        .failure()
        .stderr(contains(
            "Nexus does not know the game domain 'unknowngame'",
        ));
    add_game(&env);
    let s = env.store();
    let (name, id): (String, i64) = s
        .conn()
        .query_row(
            "SELECT name, nexus_game_id FROM games WHERE domain = 'testgame'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((name.as_str(), id), ("Test Game", 77));
}

// ── nxm register / unregister / status ──────────────────────

#[test]
fn nxm_register_writes_desktop_entry_and_mimeapps_then_unregister_removes_both() {
    let env = Env::new();
    let home = env.home();
    let mime = home.join(".config/mimeapps.list");
    std::fs::create_dir_all(mime.parent().unwrap()).unwrap();
    std::fs::write(
        &mime,
        "[Default Applications]\ntext/html=firefox.desktop;\n",
    )
    .unwrap();
    env.run(&["nxm", "status"])
        .success()
        .stderr(contains("not the nxm:// handler"));
    env.run(&["nxm", "register"])
        .success()
        .stdout(contains("handler for nxm:// links"));
    let desktop = home.join(".local/share/applications/modman-nxm.desktop");
    let text = std::fs::read_to_string(&desktop).unwrap();
    let bin = assert_cmd::cargo::cargo_bin("modman")
        .canonicalize()
        .unwrap();
    assert!(
        text.lines()
            .any(|l| l == format!("Exec=\"{}\" nxm %u", bin.display())),
        "{text}"
    );
    assert!(text.contains("\nMimeType=x-scheme-handler/nxm;x-scheme-handler/nxm-protocol;\n"));
    let m = std::fs::read_to_string(&mime).unwrap();
    assert!(
        m.contains("x-scheme-handler/nxm=modman-nxm.desktop;")
            && m.contains("x-scheme-handler/nxm-protocol=modman-nxm.desktop;")
    );
    assert!(m.contains("text/html=firefox.desktop;"));
    env.run(&["nxm"])
        .success()
        .stdout(contains("modman handles nxm:// links."));
    env.run(&["status"])
        .success()
        .stdout(contains("nxm:// links  : registered"));
    assert!(env
        .calls("update-desktop-database")
        .contains("applications"));

    env.run(&["nxm", "unregister"])
        .success()
        .stdout(contains("handler removed"));
    assert!(!desktop.exists());
    let m = std::fs::read_to_string(&mime).unwrap();
    assert!(!m.contains("modman-nxm.desktop"), "{m}");
    assert!(m.contains("text/html=firefox.desktop;"));
    env.run(&["nxm", "status"])
        .success()
        .stderr(contains("not the nxm:// handler"));
    env.run(&["nxm", "bogus"])
        .failure()
        .stderr(contains("Usage: modman nxm"));
}

#[test]
fn nxm_register_uses_appimage_path() {
    let mut env = Env::new();
    env.extra_env
        .push(("APPIMAGE".into(), "/opt/My Apps/modman.AppImage".into()));
    env.run(&["nxm", "register"]).success();
    let text = std::fs::read_to_string(
        env.home()
            .join(".local/share/applications/modman-nxm.desktop"),
    )
    .unwrap();
    assert!(
        text.contains("\nExec=\"/opt/My Apps/modman.AppImage\" nxm %u\n"),
        "{text}"
    );
}

// ── nxm <url> ───────────────────────────────────────────────

fn nxm_url() -> String {
    format!(
        "nxm://testgame/mods/500/files/11?key=NxmKeyValue&expires={}&user_id=7",
        4_000_000_000u64
    )
}

#[test]
fn nxm_link_without_gui_is_queued_and_installed_inline() {
    let (env, mock) = mocked(fake(false));
    env.cmd()
        .arg("login")
        .write_stdin(format!("{KEY}\n"))
        .assert()
        .success();
    add_game(&env);
    let a = env.run(&["--debug", "nxm", &nxm_url()]).success();
    let o = a.get_output();
    assert_eq!(env.uids(), ["Cool_Mod-500-11"]);
    // Free account: the link's key/expires were exchanged for the CDN link.
    let dl: Vec<_> = mock
        .requests()
        .into_iter()
        .filter(|r| r.path().ends_with("download_link.json"))
        .collect();
    assert!(dl[0].target.contains("key=NxmKeyValue") && dl[0].target.contains("expires="));
    assert_eq!(
        env.query_i64("SELECT COUNT(*) FROM downloads WHERE source = 'nxm' AND status = 'done' AND url = 'nxm://testgame/mods/500/files/11'"),
        1
    );
    // Not a tty: notifications instead of prompts.
    let notes = env.calls("notify-send");
    assert!(
        notes.contains("Installing mod 500") && notes.contains("Installed mod 500"),
        "{notes}"
    );
    for (what, text) in [
        ("stdout", String::from_utf8_lossy(&o.stdout).into_owned()),
        ("stderr", String::from_utf8_lossy(&o.stderr).into_owned()),
        ("log", env.log()),
    ] {
        assert!(
            !text.contains("NxmKeyValue"),
            "nxm key leaked to {what}: {text}"
        );
        assert!(!text.contains(KEY), "API key leaked to {what}");
    }
    assert!(env
        .log()
        .contains("nxm link received: nxm://testgame/mods/500/files/11"));
}

#[test]
fn bare_nxm_link_routes_to_handler_and_bad_links_fail() {
    let (env, _mock) = mocked(fake(false));
    env.cmd()
        .arg("login")
        .write_stdin(format!("{KEY}\n"))
        .assert()
        .success();
    add_game(&env);
    env.run(&[&nxm_url()]).success();
    assert_eq!(env.uids(), ["Cool_Mod-500-11"]);
    env.run(&["nxm", "nxm://evil.example/mods/1/files/2?key=SECRETQ"])
        .failure()
        .stderr(contains("Unrecognised nxm link").and(contains("SECRETQ").not()));
    assert!(env.calls("notify-send").contains("Unrecognised nxm link"));
    // A link for a game that is not set up is queued, then fails clearly.
    env.run(&[
        "nxm",
        "nxm://othergame/mods/1/files/2?key=k&expires=4000000000",
    ])
    .failure()
    .stderr(contains("not set up"));
    assert_eq!(
        env.query_i64(
            "SELECT COUNT(*) FROM downloads WHERE game = 'othergame' AND status = 'failed'"
        ),
        1
    );
}

#[test]
fn nxm_collection_link_without_login_fails_and_marks_row_failed() {
    let env = Env::new();
    env.run(&["nxm", "nxm://testgame/collections/abc123/revisions/4"])
        .failure()
        .stderr(contains("Not logged in"));
    assert_eq!(
        env.query_i64("SELECT COUNT(*) FROM downloads WHERE collection_slug = 'abc123' AND collection_revision = 4 AND status = 'failed'"),
        1
    );
    assert!(env.calls("notify-send").contains("collection abc123"));
}
