// SPDX-License-Identifier: GPL-3.0-or-later
//! T5: loader archives in the install pipeline, deploy/purge exclusion,
//! requirements status and junk cleanup.

use super::tests::{
    smapi_zip_at, vanilla_at, zip_files, FakeGithub, MELON_FILES, MELON_TAG, MELON_TOML,
};
use super::*;
use crate::app::install::{self as appinst, Installed};
use crate::app::testing::Recorder;
use crate::app::Cx;
use crate::deploy::{DeployEnv, DeployOptions, NeverRunning};
use crate::games::{self, GameConfig};
use crate::mods::{self, InstallHooks, InstallMeta, ModsError};
use crate::requirements::{self, ReqStatus, Scope};

struct P {
    tmp: tempfile::TempDir,
    paths: Paths,
    store: Store,
    game: GameConfig,
}

fn stardew() -> P {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().canonicalize().unwrap().join("home");
    fs::create_dir_all(&home).unwrap();
    let paths = Paths::for_home(&home);
    let store = Store::open_in_memory().unwrap();
    let mut game = GameConfig::custom("stardewvalley");
    game.nexus_domain = Some("stardewvalley".into());
    game.folder = Some(tmp.path().canonicalize().unwrap().join("game"));
    game.subdir = "Mods".into();
    game.profile = "stardew".into();
    game.definition = Some("stardewvalley".into());
    games::save_game(store.conn(), &game).unwrap();
    vanilla_at(game.folder.as_ref().unwrap());
    P {
        tmp,
        paths,
        store,
        game,
    }
}

fn data(p: &P) -> PathBuf {
    p.tmp.path().join("data")
}

fn mod_count(p: &P) -> i64 {
    p.store
        .conn()
        .query_row("SELECT COUNT(*) FROM mods", [], |r| r.get(0))
        .unwrap()
}

/// Catches: the SMAPI release becoming a mod under Mods/ again (the
/// original bug), while normal mods still install.
#[test]
fn smapi_archive_never_becomes_a_mod() {
    let mut p = stardew();
    let d = data(&p);
    let zip = smapi_zip_at(p.tmp.path());
    let err = mods::install_archive(
        &mut p.store,
        &p.game,
        &d,
        &zip,
        &InstallMeta::default(),
        InstallHooks::default(),
    )
    .unwrap_err();
    assert!(
        matches!(err, ModsError::LoaderArchive { ref loader_key } if loader_key == "smapi"),
        "{err}"
    );
    assert_eq!(mod_count(&p), 0);
    let staged = crate::deploy::staging_root(&p.game, &d);
    let leftovers: Vec<_> = walkdir::WalkDir::new(&staged)
        .min_depth(1)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert!(!p
        .game
        .folder
        .as_ref()
        .unwrap()
        .join("Mods/SMAPI 4.5.2 installer")
        .exists());

    // A normal SMAPI mod (manifest.json) still installs.
    let m = p.tmp.path().join("ContentPatcher");
    fs::create_dir_all(m.join("ContentPatcher")).unwrap();
    fs::write(
        m.join("ContentPatcher/manifest.json"),
        r#"{"UniqueID":"Pathoschild.ContentPatcher"}"#,
    )
    .unwrap();
    fs::write(m.join("ContentPatcher/ContentPatcher.dll"), "x").unwrap();
    mods::install_archive(
        &mut p.store,
        &p.game,
        &d,
        &m,
        &InstallMeta::default(),
        InstallHooks::default(),
    )
    .unwrap();
    assert_eq!(mod_count(&p), 1);
}

/// Catches: a loader archive from Nexus (NXM, install <id>, collections)
/// running or deploying anything instead of waiting for consent, and the
/// download source not reaching provenance.
#[test]
fn pipeline_turns_a_loader_into_a_pending_preview() {
    let mut p = stardew();
    let zip = smapi_zip_at(p.tmp.path());
    let ui = Recorder::default();
    let game = p.game.clone();
    let d = data(&p);
    let mut cx = Cx::new(&p.paths, &mut p.store, &ui);
    let origin = DownloadOrigin::Nexus {
        domain: "stardewvalley".into(),
        mod_id: 2400,
        file_id: 9,
    };
    let meta = InstallMeta {
        nexus_mod_id: Some(2400),
        nexus_file_id: Some(9),
        source: Some("nexus".into()),
        ..Default::default()
    };
    let out =
        appinst::install_archive(&mut cx, &game, &d, &zip, &meta, false, Some(&origin)).unwrap();
    let Installed::Loader(l) = out else {
        panic!("{out:?}")
    };
    assert_eq!(
        (l.loader_key.as_str(), l.op, l.version.as_deref()),
        ("smapi", Op::Install, Some("4.5.2"))
    );
    let rec = d.join(format!("loaders/previews/{}.json", l.preview_id));
    let text = fs::read_to_string(rec).unwrap();
    assert!(
        text.contains("\"verified\"") || text.contains("\"kind\":\"verified\""),
        "{text}"
    );
    assert_eq!(mod_count(&p), 0);
    assert!(
        p.store.unfinished_loader_runs().unwrap().is_empty(),
        "nothing ran"
    );
    assert!(p
        .store
        .active_loader_install("stardewvalley", "smapi")
        .unwrap()
        .is_none());

    // Without an origin (restage, reinstall) it is an error, never a mod.
    let ui = Recorder::default();
    let mut cx = Cx::new(&p.paths, &mut p.store, &ui);
    let e = appinst::install_archive(&mut cx, &game, &d, &zip, &meta, false, None).unwrap_err();
    assert!(e.to_string().contains("mod loader"), "{e}");
}

#[test]
fn hand_made_install_gives_an_adopt_preview() {
    let mut p = stardew();
    let g = p.game.folder.clone().unwrap();
    fs::rename(g.join("StardewValley"), g.join("StardewValley-original")).unwrap();
    fs::write(
        g.join("StardewValley"),
        "#!/bin/bash\nexec ./StardewModdingAPI\n",
    )
    .unwrap();
    fs::write(g.join("StardewModdingAPI.dll"), "x").unwrap();
    let zip = smapi_zip_at(p.tmp.path());
    let ui = Recorder::default();
    let game = p.game.clone();
    let d = data(&p);
    let mut cx = Cx::new(&p.paths, &mut p.store, &ui);
    let local = DownloadOrigin::Local { path: zip.clone() };
    let out = appinst::install_archive(
        &mut cx,
        &game,
        &d,
        &zip,
        &InstallMeta::default(),
        false,
        Some(&local),
    )
    .unwrap();
    assert_eq!(out.loader().unwrap().op, Op::Adopt);
}

fn denv(p: &P) -> DeployEnv {
    let mut e = DeployEnv::new(&p.paths, &data(p), &p.game.id);
    e.probe = Arc::new(NeverRunning);
    e
}

fn deploy(p: &mut P) -> crate::deploy::DeployReport {
    let env = denv(p);
    crate::deploy::deploy(&mut p.store, &p.game, &env, &DeployOptions::default()).unwrap()
}

fn loader_owns(p: &P, path: &str) {
    p.store
        .conn()
        .execute_batch(&format!(
            "INSERT INTO loader_installs (id, game_id, game_dir, loader_key, runtime, strategy, status,
                 origin, installed_at, updated_at)
               VALUES (1, 'stardewvalley', '/g', 'smapi', 'native', 'installer', 'installed', 'nexus', 0, 0);
             INSERT INTO loader_files (install_id, path, change, kind) VALUES (1, '{path}', 'created', 'file');"
        ))
        .unwrap();
}

/// Catches: a mod deploy overwriting (and later "restoring") a loader's
/// file, and purge deleting it.
#[test]
fn deploy_skips_and_purge_keeps_loader_owned_paths() {
    let mut p = stardew();
    let d = data(&p);
    let src = p.tmp.path().join("ModA");
    fs::create_dir_all(src.join("ModA")).unwrap();
    fs::write(src.join("ModA/manifest.json"), r#"{"UniqueID":"A"}"#).unwrap();
    fs::write(src.join("ModA/a.dll"), "mod").unwrap();
    mods::install_archive(
        &mut p.store,
        &p.game,
        &d,
        &src,
        &InstallMeta::default(),
        InstallHooks::default(),
    )
    .unwrap();
    let game_dir = p.game.folder.clone().unwrap();
    // Purge side: deployed first, then a loader takes the path over.
    let r = deploy(&mut p);
    assert!(r.loader_owned.is_empty());
    let placed = game_dir.join("Mods/ModA/a.dll");
    assert!(placed.exists());
    loader_owns(&p, "Mods/ModA/a.dll");
    let env = denv(&p);
    let pr = crate::deploy::purge(&mut p.store, &p.game, &env).unwrap();
    assert!(placed.exists(), "purge left the loader's file");
    assert!(pr.kept >= 1);
    assert!(!game_dir.join("Mods/ModA/manifest.json").exists());
    // Deploy side: the colliding item is skipped and reported.
    fs::write(&placed, "loader").unwrap();
    let r = deploy(&mut p);
    assert_eq!(r.loader_owned, ["Mods/ModA/a.dll"]);
    assert_eq!(fs::read_to_string(&placed).unwrap(), "loader");
    assert!(game_dir.join("Mods/ModA/manifest.json").exists());
}

fn needs_smapi(p: &P) {
    p.store
        .conn()
        .execute(
            "INSERT INTO mods (game, uid, name, nexus_mod_id, priority, installed_at)
             VALUES ('stardewvalley', 'CP', 'Content Patcher', 1915, 0, 0)",
            [],
        )
        .unwrap();
    p.store
        .conn()
        .execute(
            "INSERT INTO requirements_cache (game, nexus_mod_id, body, available, fetched_at, checked_at)
             VALUES ('stardewvalley', 1915, ?1, 1, 0, 0)",
            [r#"[{"modId":2400,"modName":"SMAPI - Stardew Modding API","gameId":1303}]"#],
        )
        .unwrap();
}

fn smapi_row(p: &P) -> requirements::ReqRow {
    requirements::analyze(&p.store, &p.game, &Scope::default(), None)
        .unwrap()
        .into_iter()
        .find(|r| r.key == "tool:smapi")
        .unwrap()
}

/// Catches: requirements ignoring Vessel-managed loaders (or their drift).
#[test]
fn requirements_read_the_managed_loader_state() {
    let p = stardew();
    needs_smapi(&p);
    let before = smapi_row(&p);
    assert_eq!(
        before.status,
        ReqStatus::Missing,
        "no row: the old behaviour"
    );
    let g = p.game.folder.clone().unwrap();
    fs::rename(g.join("StardewValley"), g.join("StardewValley-original")).unwrap();
    fs::write(
        g.join("StardewValley"),
        "#!/bin/bash\nexec ./StardewModdingAPI\n",
    )
    .unwrap();
    fs::write(g.join("StardewModdingAPI.dll"), "x").unwrap();
    p.store
        .conn()
        .execute_batch(
            "INSERT INTO loader_installs (game_id, game_dir, loader_key, version, runtime, strategy,
                 status, origin, installed_at, updated_at)
               VALUES ('stardewvalley', '/g', 'smapi', '4.5.2', 'native', 'installer', 'installed',
                 'nexus', 0, 0);",
        )
        .unwrap();
    let row = smapi_row(&p);
    assert_eq!(row.status, ReqStatus::Ok);
    assert!(
        row.detail.contains("4.5.2") && row.detail.contains("Vessel"),
        "{}",
        row.detail
    );
    // Steam put the vanilla launcher back.
    fs::write(
        g.join("StardewValley"),
        "#!/bin/bash\n./\"Stardew Valley\" $@\n",
    )
    .unwrap();
    let row = smapi_row(&p);
    assert!(row.detail.starts_with("warning:"), "{}", row.detail);
}

/// Catches: old SMAPI-installer mod rows staying behind (and deployed).
#[test]
fn junk_rows_are_found_and_removed_with_their_files() {
    let mut p = stardew();
    let staging = p.tmp.path().join("staging/SMAPI-junk");
    let files = [
        "SMAPI 4.1.0 installer/internal/linux/SMAPI.Installer",
        "SMAPI 4.1.0 installer/internal/linux/install.dat",
        "SMAPI 4.1.0 installer/internal/windows/install.dat",
        "SMAPI 4.1.0 installer/install on Linux.sh",
    ];
    for f in files {
        let path = staging.join(f);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, f).unwrap();
    }
    p.store
        .conn()
        .execute(
            "INSERT INTO mods (game, uid, name, staging_dir, priority, file_count, installed_at)
             VALUES ('stardewvalley', 'SMAPI-4.1.0-installer', 'SMAPI 4.1.0 installer', ?1, 0, 4, 0)",
            [staging.display().to_string()],
        )
        .unwrap();
    let id = p.store.conn().last_insert_rowid();
    for f in files {
        p.store
            .conn()
            .execute(
                "INSERT INTO mod_files (mod_id, path, size, sha256) VALUES (?1, ?2, 1, 'x')",
                rusqlite::params![id, f],
            )
            .unwrap();
    }
    deploy(&mut p);
    let mods_dir = p
        .game
        .folder
        .clone()
        .unwrap()
        .join("Mods/SMAPI 4.1.0 installer");
    assert!(mods_dir.join("internal/linux/install.dat").exists());

    let ui = Recorder::default();
    let game = p.game.clone();
    let mut cx = Cx::new(&p.paths, &mut p.store, &ui);
    let junk = find_loader_junk(&cx, &game).unwrap();
    assert_eq!(junk.len(), 1);
    assert_eq!((junk[0].mod_id, junk[0].loader_key.as_str()), (id, "smapi"));
    assert_eq!(remove_loader_junk(&mut cx, &game, &[id]).unwrap(), 1);
    assert_eq!(mod_count(&p), 0);
    assert!(!mods_dir.join("internal/linux/install.dat").exists());
}

/// Catches: an nxm:// link to the SMAPI release installing a mod,
/// deploying, or starting the installer instead of stopping at a pending
/// preview (the consent dialog).
#[test]
fn nxm_link_to_smapi_stops_at_a_pending_preview() {
    use crate::app::nexus::{AUTH_BACKEND_KEY, PREMIUM_KEY, TEST_BASE_URL};
    use crate::nexus::{ApiKey, KeyStorage, ProbeOutcome};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mut p = stardew();
    let zip = fs::read(smapi_zip_at(p.tmp.path())).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(MockServer::start());
    let base = server.uri();
    let api = "/v1/games/stardewvalley/mods/2400";
    rt.block_on(async {
        let ok = |body: String| ResponseTemplate::new(200).set_body_raw(body, "application/json");
        Mock::given(method("GET"))
            .and(path(format!("{api}.json")))
            .respond_with(ok(r#"{"mod_id":2400,"name":"SMAPI","version":"4.5.2","author":"Pathoschild","status":"published","available":true}"#.into()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{api}/files.json")))
            .respond_with(ok(format!(
                r#"{{"files":[{{"file_id":1,"name":"SMAPI","version":"4.5.2","category_name":"MAIN","file_name":"SMAPI-4.5.2-installer.zip","size_in_bytes":{},"uploaded_timestamp":1000}}],"file_updates":[]}}"#,
                zip.len()
            )))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{api}/files/1/download_link.json")))
            .respond_with(ok(format!(
                r#"[{{"name":"Nexus CDN","short_name":"Nexus","URI":"{base}/cdn/SMAPI-4.5.2-installer.zip"}}]"#
            )))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/cdn/SMAPI-4.5.2-installer.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(zip.clone()))
            .mount(&server)
            .await;
    });
    TEST_BASE_URL.with(|b| *b.borrow_mut() = Some(base.clone()));
    KeyStorage::new(&p.paths, ProbeOutcome::Unavailable("test".into()))
        .save(&ApiKey::new("TestApiKey123").unwrap())
        .unwrap();
    p.store.set_setting(AUTH_BACKEND_KEY, "file").unwrap();
    p.store.set_setting(PREMIUM_KEY, "false").unwrap();

    let ui = Recorder::default();
    let mut cx = Cx::new(&p.paths, &mut p.store, &ui);
    let data_dir = cx.config().unwrap().data_dir;
    let accepted = crate::app::nxm::accept(
        cx.store,
        "nxm://stardewvalley/mods/2400/files/1?key=NxmKey&expires=4000000000&user_id=7",
    )
    .unwrap();
    let out = crate::app::nxm::process(&mut cx, &accepted);
    TEST_BASE_URL.with(|b| *b.borrow_mut() = None);
    let out = out.unwrap_or_else(|e| panic!("{e}: {:?}", ui.texts()));
    assert_eq!(out.kind, "loader");
    assert!(!out.installed && out.installed_mod.is_none());
    let pending = out.loader.expect("a pending loader preview");
    assert_eq!(
        (pending.loader_key.as_str(), pending.op),
        ("smapi", Op::Install)
    );
    let rec = data_dir.join(format!("loaders/previews/{}.json", pending.preview_id));
    assert!(rec.is_file(), "pending preview {}", rec.display());

    assert_eq!(mod_count(&p), 0, "no mod row");
    let deployed: i64 = p
        .store
        .conn()
        .query_row("SELECT COUNT(*) FROM deployments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(deployed, 0, "nothing deployed");
    assert!(p.store.unfinished_loader_runs().unwrap().is_empty());
    let runs: i64 = p
        .store
        .conn()
        .query_row("SELECT COUNT(*) FROM loader_runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(runs, 0, "no loader run");
    assert!(p
        .store
        .active_loader_install("stardewvalley", "smapi")
        .unwrap()
        .is_none());

    // The runner is never reached: a run row is written before any spawn,
    // and the game folder is untouched.
    let g = p.game.folder.clone().unwrap();
    assert!(!g.join("StardewModdingAPI.dll").exists());
    assert!(!g.join("StardewValley-original").exists());
}

/// Schedule I with the GitHub-sourced MelonLoader of [`MELON_TOML`]
/// (Nexus mod 1196) as a user definition.
fn schedule1() -> P {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let home = base.join("home");
    fs::create_dir_all(&home).unwrap();
    let paths = Paths::for_home(&home);
    fs::create_dir_all(paths.definitions_dir()).unwrap();
    fs::write(paths.definitions_dir().join("schedule1.toml"), MELON_TOML).unwrap();
    let store = Store::open_in_memory().unwrap();
    let mut game = GameConfig::custom("schedule1");
    game.nexus_domain = Some("schedule1".into());
    game.folder = Some(base.join("game"));
    game.subdir = ".".into();
    game.profile = "melonloader".into();
    game.definition = Some("schedule1".into());
    games::save_game(store.conn(), &game).unwrap();
    fs::create_dir_all(base.join("game")).unwrap();
    set_runtime_override(&store, "schedule1", Some("native")).unwrap();
    P {
        tmp,
        paths,
        store,
        game,
    }
}

fn nexus_1196() -> (InstallMeta, DownloadOrigin) {
    let meta = InstallMeta {
        nexus_mod_id: Some(1196),
        nexus_file_id: Some(7),
        source: Some("nexus".into()),
        ..Default::default()
    };
    let origin = DownloadOrigin::Nexus {
        domain: "schedule1".into(),
        mod_id: 1196,
        file_id: 7,
    };
    (meta, origin)
}

fn files_named(root: &Path, name: &str) -> Vec<PathBuf> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|e| e.file_name() == name)
        .map(|e| e.path().to_path_buf())
        .collect()
}

/// Catches: the loader's Nexus installer (.exe) being staged, deployed or
/// run as a mod, instead of pointing to the loader's GitHub release; and
/// the redirect swallowing other files from the same Nexus page.
#[test]
fn nexus_installer_redirects_to_the_github_loader() {
    let mut p = schedule1();
    let d = data(&p);
    let installer = zip_files(
        p.tmp.path(),
        "MelonLoader.Installer.zip",
        &["MelonLoader.Installer.exe", "README.txt"],
    );
    let (meta, origin) = nexus_1196();
    let game = p.game.clone();

    // Without an origin (restage, reinstall): a typed refusal, nothing kept.
    let ui = Recorder::default();
    let mut cx = Cx::new(&p.paths, &mut p.store, &ui);
    let e =
        appinst::install_archive(&mut cx, &game, &d, &installer, &meta, false, None).unwrap_err();
    assert!(
        matches!(&e, crate::app::AppError::Mods(ModsError::LoaderInstaller { loader_key }) if loader_key == "melonloader"),
        "{e}"
    );

    // With the Nexus origin: a pending preview of the GitHub release.
    let gh = FakeGithub::serve(zip_files(p.tmp.path(), "melon.zip", MELON_FILES));
    let ui = Recorder::default();
    let mut cx = Cx::new(&p.paths, &mut p.store, &ui);
    let out = appinst::install_archive(&mut cx, &game, &d, &installer, &meta, false, Some(&origin))
        .unwrap_or_else(|e| panic!("{e}: {:?}", ui.texts()));
    let Installed::Loader(l) = out else {
        panic!("{out:?}")
    };
    assert_eq!(gh.calls(), 1);
    assert_eq!(
        (l.loader_key.as_str(), l.op, l.version.as_deref()),
        ("melonloader", Op::Install, Some(MELON_TAG))
    );
    let rec =
        fs::read_to_string(d.join(format!("loaders/previews/{}.json", l.preview_id))).unwrap();
    assert!(
        rec.contains("\"github\"") && rec.contains("LavaGang/MelonLoader"),
        "{rec}"
    );

    assert_eq!(mod_count(&p), 0, "no mod row");
    let deployed: i64 = p
        .store
        .conn()
        .query_row("SELECT COUNT(*) FROM deployments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(deployed, 0);
    let runs: i64 = p
        .store
        .conn()
        .query_row("SELECT COUNT(*) FROM loader_runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(runs, 0, "nothing ran");
    // The installer exists only as the user's download.
    assert!(files_named(&d, "MelonLoader.Installer.exe").is_empty());
    assert!(files_named(p.game.folder.as_ref().unwrap(), "MelonLoader.Installer.exe").is_empty());

    // Another file from the same page (no top-level exe/msi) is a mod.
    let optional = zip_files(p.tmp.path(), "Extras.zip", &["Mods/Extras.dll"]);
    let ui = Recorder::default();
    let mut cx = Cx::new(&p.paths, &mut p.store, &ui);
    let out = appinst::install_archive(&mut cx, &game, &d, &optional, &meta, false, Some(&origin))
        .unwrap();
    assert!(out.as_mod().is_some(), "{out:?}");
    assert_eq!(mod_count(&p), 1);
    assert_eq!(gh.calls(), 1);
}

/// Inserts a mod row for `p.game` with `nexus` as its Nexus mod id. With
/// `staged`, the files are written under a staging folder and recorded in
/// `mod_files`; without it the row points at a folder that does not exist.
fn junk_candidate(p: &P, uid: &str, nexus: i64, staged: Option<&[&str]>) -> i64 {
    let dir = p.tmp.path().join("staging").join(uid);
    p.store
        .conn()
        .execute(
            "INSERT INTO mods (game, uid, name, nexus_mod_id, staging_dir, priority, installed_at)
             VALUES (?1, ?2, ?2, ?3, ?4, 0, 0)",
            rusqlite::params![p.game.id, uid, nexus, dir.display().to_string()],
        )
        .unwrap();
    let id = p.store.conn().last_insert_rowid();
    for f in staged.unwrap_or(&[]) {
        let path = dir.join(f);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, f).unwrap();
        p.store
            .conn()
            .execute(
                "INSERT INTO mod_files (mod_id, path, size, sha256) VALUES (?1, ?2, 1, 'x')",
                rusqlite::params![id, f],
            )
            .unwrap();
    }
    id
}

/// Reports junk for `p.game`, checks it is exactly `want` (by loader
/// `key`), removes every row through `remove_loader_junk`, and returns the
/// uids left.
fn junk_roundtrip(p: &mut P, ids: &[i64], want: &[i64], key: &str) -> Vec<String> {
    let ui = Recorder::default();
    let game = p.game.clone();
    let mut cx = Cx::new(&p.paths, &mut p.store, &ui);
    let junk = find_loader_junk(&cx, &game).unwrap();
    let got: Vec<(i64, &str)> = junk
        .iter()
        .map(|j| (j.mod_id, j.loader_key.as_str()))
        .collect();
    let expect: Vec<(i64, &str)> = want.iter().map(|&i| (i, key)).collect();
    assert_eq!(got, expect, "{junk:?}");
    assert_eq!(remove_loader_junk(&mut cx, &game, ids).unwrap(), want.len());
    crate::mods::list_mods(p.store.conn(), &game.id)
        .unwrap()
        .into_iter()
        .map(|m| m.uid)
        .collect()
}

/// Catches: an old MelonLoader installer row (Nexus 1196, staged as a mod
/// like the user's mod 219) not being reported; an optional file from the
/// same Nexus page (installed as a mod on purpose) being reported; a row
/// whose staging folder is gone being reported; removal touching rows that
/// were not reported.
#[test]
fn mod_rows_with_a_loaders_nexus_id_are_junk_only_when_staged_as_an_installer() {
    let mut p = schedule1();
    let installer = junk_candidate(
        &p,
        "MelonLoader-219",
        1196,
        Some(&["MelonLoader.Installer.EXE"]),
    );
    let ids = [
        installer,
        junk_candidate(&p, "Extras", 1196, Some(&["Mods/Extras.dll"])),
        junk_candidate(&p, "Gone", 1196, None),
        junk_candidate(&p, "NestedExe", 1196, Some(&["tools/Helper.exe"])),
        junk_candidate(&p, "SomeMod", 55, Some(&["Setup.msi"])),
    ];
    let left = junk_roundtrip(&mut p, &ids, &[installer], "melonloader");
    assert_eq!(left, ["Extras", "Gone", "NestedExe", "SomeMod"]);
}

/// Catches: the same rule not applying to SMAPI (Nexus 2400 on Stardew).
#[test]
fn smapi_nexus_id_rows_are_junk_only_when_staged_as_an_installer() {
    let mut p = stardew();
    let installer = junk_candidate(&p, "SMAPI-installer", 2400, Some(&["SMAPI.Installer.msi"]));
    let ids = [
        installer,
        junk_candidate(&p, "SMAPI-extra", 2400, Some(&["ExtraMod/manifest.json"])),
        junk_candidate(&p, "Other", 1915, Some(&["Other.exe"])),
    ];
    let left = junk_roundtrip(&mut p, &ids, &[installer], "smapi");
    assert_eq!(left, ["SMAPI-extra", "Other"]);
}
