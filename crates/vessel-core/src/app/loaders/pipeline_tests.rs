// SPDX-License-Identifier: GPL-3.0-or-later
//! T5: loader archives in the install pipeline, deploy/purge exclusion,
//! requirements status and junk cleanup.

use super::tests::{smapi_zip_at, vanilla_at};
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
