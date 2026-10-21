// SPDX-License-Identifier: GPL-3.0-or-later
//! M6 plugin stage: plugins.txt in a temp Proton prefix, LOOT sorting on
//! fixture plugins, collection plugin rules, masterlist fetch (mocked HTTP),
//! conflict data. Temp dirs only; no network.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use modman_core::deploy::{self, DeployEnv, DeployOptions, Faults, NeverRunning};
use modman_core::games::{self, GameConfig, PluginsFormat};
use modman_core::loadorder::{
    self, conflict_data, fetch_masterlist, sort_with_loot, MasterlistSource, PluginStage,
    PluginsFiles, PluginsReport, SkipReason, SortMethod,
};
use modman_core::store::Store;

const GAME: &str = "skyrimspecialedition";
const APPDATA: &str = "Skyrim Special Edition";

fn sub(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(ty);
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
}

/// Minimal SSE plugin: a TES4 header with HEDR and MAST/DATA pairs.
fn plugin_bytes(master: bool, masters: &[&str]) -> Vec<u8> {
    let mut body = Vec::new();
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.71f32.to_le_bytes());
    hedr.extend_from_slice(&0u32.to_le_bytes());
    hedr.extend_from_slice(&0x800u32.to_le_bytes());
    sub(&mut body, b"HEDR", &hedr);
    for m in masters {
        let mut z = m.as_bytes().to_vec();
        z.push(0);
        sub(&mut body, b"MAST", &z);
        sub(&mut body, b"DATA", &0u64.to_le_bytes());
    }
    let mut rec = Vec::new();
    rec.extend_from_slice(b"TES4");
    rec.extend_from_slice(&(body.len() as u32).to_le_bytes());
    rec.extend_from_slice(&u32::from(master).to_le_bytes());
    rec.extend_from_slice(&0u32.to_le_bytes());
    rec.extend_from_slice(&0u32.to_le_bytes());
    rec.extend_from_slice(&44u16.to_le_bytes());
    rec.extend_from_slice(&0u16.to_le_bytes());
    rec.extend_from_slice(&body);
    rec
}

fn write(path: &Path, data: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
}

struct Fx {
    tmp: tempfile::TempDir,
    store: Store,
    game: GameConfig,
    env: DeployEnv,
}

impl Fx {
    /// Skyrim SE (bethesda, star) with a temp game folder holding
    /// `Skyrim.esm`, and a prefix whose AppData folder exists.
    fn sse() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let game_dir = tmp.path().join("game");
        write(&game_dir.join("Data/Skyrim.esm"), &plugin_bytes(true, &[]));
        let prefix = tmp.path().join("prefix");
        std::fs::create_dir_all(
            prefix.join(format!("drive_c/users/steamuser/AppData/Local/{APPDATA}")),
        )
        .unwrap();
        let store = Store::open(&tmp.path().join("data/modman.db")).unwrap();
        let mut game = GameConfig::custom(GAME);
        game.definition = Some(GAME.into());
        game.profile = "bethesda".into();
        game.subdir = "Data".into();
        game.plugins_format = Some(PluginsFormat::Star);
        game.appdata_folder = Some(APPDATA.into());
        game.folder = Some(game_dir);
        game.prefix = Some(prefix);
        games::save_game(store.conn(), &game).unwrap();
        let data = tmp.path().join("data");
        let env = DeployEnv {
            lock_path: data.join("deploy.lock"),
            backup_dir: data.join("backups"),
            overwrite_dir: data.join(GAME).join("overwrite"),
            probe: Arc::new(NeverRunning),
            faults: Faults::default(),
        };
        Self {
            tmp,
            store,
            game,
            env,
        }
    }

    fn files(&self) -> PluginsFiles {
        PluginsFiles::new(self.env.game_data_dir())
    }

    fn plugins_txt(&self) -> PathBuf {
        self.game.plugins_txt().unwrap()
    }

    fn save_game(&self) {
        games::save_game(self.store.conn(), &self.game).unwrap();
    }

    /// Stages a mod whose files go to the deploy folder (`Data`).
    fn add_mod(&self, uid: &str, priority: i64, files: &[(&str, Vec<u8>)]) -> i64 {
        let dir = self.tmp.path().join("staging").join(uid);
        for (p, data) in files {
            write(&dir.join(p), data);
        }
        self.store
            .conn()
            .execute(
                "INSERT INTO mods (game, uid, name, priority, staging_dir, installed_at)
                 VALUES (?1, ?2, ?2, ?3, ?4, 0)",
                rusqlite::params![self.game.domain, uid, priority, dir.to_string_lossy()],
            )
            .unwrap();
        self.store.conn().last_insert_rowid()
    }

    fn deploy(&mut self) -> deploy::DeployReport {
        deploy::deploy(
            &mut self.store,
            &self.game,
            &self.env,
            &DeployOptions::default(),
        )
        .unwrap()
    }

    fn purge(&mut self) -> deploy::PurgeReport {
        deploy::purge(&mut self.store, &self.game, &self.env).unwrap()
    }

    fn add_collection_rules(&self, rules: serde_json::Value) {
        let manifest = serde_json::json!({ "pluginRules": { "plugins": rules } });
        self.store
            .conn()
            .execute(
                "INSERT INTO collections (game, slug, revision, manifest, installed_at)
                 VALUES (?1, 'abc123', 1, ?2, 0)",
                rusqlite::params![self.game.domain, manifest.to_string()],
            )
            .unwrap();
    }
}

fn updated(stage: &Option<PluginStage>) -> &PluginsReport {
    match stage {
        Some(PluginStage::Updated(r)) => r,
        other => panic!("expected Updated, got {other:?}"),
    }
}

fn esp(masters: &[&str]) -> Vec<u8> {
    plugin_bytes(false, masters)
}

fn esm() -> Vec<u8> {
    plugin_bytes(true, &["Skyrim.esm"])
}

/// Catches: wrong path (must match legacy detect_plugins_file), missing star
/// prefix, LF endings, esp before esm, user lines lost or reordered, no
/// plugins.txt.orig backup, managed list wrong.
#[test]
fn deploy_writes_star_plugins_txt_crlf_masters_first_and_backs_up_original() {
    let mut fx = Fx::sse();
    let original = b"# old comment\r\n*User.esp\r\nDisabled.esp\r\n".to_vec();
    let pf = fx.plugins_txt();
    assert_eq!(
        pf,
        fx.tmp.path().join(format!(
            "prefix/drive_c/users/steamuser/AppData/Local/{APPDATA}/plugins.txt"
        ))
    );
    write(&pf, &original);
    fx.add_mod("one", 0, &[("ModA.esp", esp(&["Skyrim.esm"]))]);
    fx.add_mod(
        "two",
        1,
        &[("ModMaster.esm", esm()), ("readme.txt", b"x".to_vec())],
    );
    let r = fx.deploy();
    let rep = updated(&r.plugins);
    assert_eq!(rep.provided, 2);
    assert_eq!(rep.added, ["ModMaster.esm", "ModA.esp"]);

    let text = std::fs::read_to_string(&pf).unwrap();
    let lines: Vec<&str> = text.split("\r\n").collect();
    assert!(text.ends_with("\r\n") && !text.replace("\r\n", "").contains('\n'));
    assert!(lines[0].starts_with("# This file is used by the game"));
    assert!(lines[1].starts_with("# Please do not modify"));
    assert_eq!(
        &lines[2..6],
        ["*User.esp", "Disabled.esp", "*ModMaster.esm", "*ModA.esp"]
    );
    assert_eq!(std::fs::read(fx.files().orig_path()).unwrap(), original);
    assert_eq!(
        std::fs::read_to_string(fx.files().managed_path()).unwrap(),
        "ModMaster.esm\nModA.esp\n"
    );

    // Redeploy: no duplicates, orig never overwritten.
    let r = fx.deploy();
    assert_eq!(updated(&r.plugins).added, ["ModMaster.esm", "ModA.esp"]);
    let again = std::fs::read_to_string(&pf).unwrap();
    assert_eq!(again, text);
    assert_eq!(std::fs::read(fx.files().orig_path()).unwrap(), original);
}

/// Catches: plain-format games getting `*` markers.
#[test]
fn plain_format_writes_bare_names() {
    let mut fx = Fx::sse();
    fx.game.plugins_format = Some(PluginsFormat::Plain);
    fx.game.definition = Some("skyrim".into());
    fx.save_game();
    fx.add_mod("one", 0, &[("ModA.esp", esp(&["Skyrim.esm"]))]);
    let r = fx.deploy();
    assert_eq!(updated(&r.plugins).added, ["ModA.esp"]);
    let text = std::fs::read_to_string(fx.plugins_txt()).unwrap();
    assert!(text.ends_with("\r\nModA.esp\r\n"), "{text:?}");
    assert!(!text.contains('*'));
}

/// Catches: purge leaving modman entries behind, or removing user entries
/// and comments; managed list not emptied.
#[test]
fn purge_removes_only_managed_entries() {
    let mut fx = Fx::sse();
    let pf = fx.plugins_txt();
    write(&pf, b"*User.esp\r\n");
    fx.add_mod("one", 0, &[("ModA.esp", esp(&["Skyrim.esm"]))]);
    fx.deploy();
    // The user adds a line and a comment after deploy.
    let mut t = std::fs::read(&pf).unwrap();
    t.extend_from_slice(b"# mine\r\n*Later.esp\r\n");
    write(&pf, &t);

    let r = fx.purge();
    match &r.plugins {
        Some(PluginStage::Updated(rep)) => assert_eq!(rep.removed, ["ModA.esp"]),
        other => panic!("{other:?}"),
    }
    let text = std::fs::read_to_string(&pf).unwrap();
    assert!(!text.to_lowercase().contains("moda.esp"));
    for keep in [
        "*User.esp\r\n",
        "# mine\r\n",
        "*Later.esp\r\n",
        "# This file",
    ] {
        assert!(text.contains(keep), "{keep:?} missing in {text:?}");
    }
    assert_eq!(
        std::fs::read_to_string(fx.files().managed_path()).unwrap(),
        ""
    );
}

/// Catches: LOOT not consulted (mod priority order kept), or masters placed
/// after their dependents.
#[test]
fn loot_sorts_masters_before_dependents_on_deploy() {
    let mut fx = Fx::sse();
    // Mod priority puts the dependent first.
    fx.add_mod("b", 0, &[("B.esp", esp(&["Skyrim.esm", "A.esp"]))]);
    fx.add_mod("a", 1, &[("A.esp", esp(&["Skyrim.esm"]))]);
    let r = fx.deploy();
    let rep = updated(&r.plugins);
    assert_eq!(rep.sorted_by, Some(SortMethod::Loot { masterlist: false }));
    assert_eq!(rep.added, ["A.esp", "B.esp"]);
}

/// Catches: the libloot integration itself (spike fixture).
#[test]
fn sort_with_loot_orders_fixture_masters_first() {
    let tmp = tempfile::tempdir().unwrap();
    let game = tmp.path().join("game");
    let local = tmp.path().join("local");
    std::fs::create_dir_all(&local).unwrap();
    write(&game.join("Data/Skyrim.esm"), &plugin_bytes(true, &[]));
    write(&game.join("Data/A.esp"), &esp(&["Skyrim.esm"]));
    write(&game.join("Data/B.esp"), &esp(&["Skyrim.esm", "A.esp"]));
    let names: Vec<String> = ["B.esp", "A.esp", "Skyrim.esm"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let out = sort_with_loot(
        libloot::GameType::SkyrimSE,
        &game,
        &local,
        &names,
        None,
        &[],
    )
    .unwrap();
    assert_eq!(out, ["Skyrim.esm", "A.esp", "B.esp"]);
}

/// Catches: collection `after` rules not merged into the final order.
#[test]
fn collection_after_rule_is_respected() {
    let mut fx = Fx::sse();
    fx.add_mod("x", 0, &[("X.esp", esp(&["Skyrim.esm"]))]);
    fx.add_mod("y", 1, &[("Y.esp", esp(&["Skyrim.esm"]))]);
    fx.add_collection_rules(serde_json::json!([{ "name": "X.esp", "after": ["Y.esp"] }]));
    let r = fx.deploy();
    let rep = updated(&r.plugins);
    assert!(matches!(rep.sorted_by, Some(SortMethod::Loot { .. })));
    assert_eq!(rep.added, ["Y.esp", "X.esp"]);
}

/// Catches: rules ignored when LOOT cannot read the plugins (fallback path).
#[test]
fn collection_rule_applies_when_loot_cannot_read_plugins() {
    let mut fx = Fx::sse();
    fx.add_mod("x", 0, &[("X.esp", b"not a plugin".to_vec())]);
    fx.add_mod("y", 1, &[("Y.esp", b"not a plugin".to_vec())]);
    fx.add_collection_rules(serde_json::json!([{ "name": "x.esp", "after": ["y.esp"] }]));
    let r = fx.deploy();
    let rep = updated(&r.plugins);
    assert_eq!(rep.sorted_by, Some(SortMethod::Rules));
    assert!(!rep.warnings.is_empty());
    assert_eq!(rep.added, ["Y.esp", "X.esp"]);
}

/// Catches: the plugin stage running for non-bethesda games.
#[test]
fn non_bethesda_profile_skips_plugin_stage() {
    let mut fx = Fx::sse();
    fx.game.profile = "generic".into();
    fx.game.plugins_format = None;
    fx.save_game();
    fx.add_mod("one", 0, &[("ModA.esp", esp(&["Skyrim.esm"]))]);
    let r = fx.deploy();
    assert_eq!(
        r.plugins,
        Some(PluginStage::Skipped(SkipReason::NotBethesda))
    );
    let pf = fx.tmp.path().join(format!(
        "prefix/drive_c/users/steamuser/AppData/Local/{APPDATA}/plugins.txt"
    ));
    assert!(!pf.exists());
    assert!(!fx.files().managed_path().exists());
}

/// Catches: creating the AppData folder (legacy warns "launch the game
/// once") or failing the deploy.
#[test]
fn missing_appdata_folder_skips_without_failing_deploy() {
    let mut fx = Fx::sse();
    fx.game.appdata_folder = Some("Not There".into());
    fx.save_game();
    fx.add_mod("one", 0, &[("ModA.esp", esp(&["Skyrim.esm"]))]);
    let r = fx.deploy();
    assert!(r.placed > 0);
    assert!(matches!(
        r.plugins,
        Some(PluginStage::Skipped(SkipReason::FolderMissing(_)))
    ));
    assert!(!fx.plugins_txt().parent().unwrap().exists());
}

/// Catches: following a planted plugins.txt symlink out of the prefix.
#[test]
fn symlinked_plugins_txt_is_refused_and_target_untouched() {
    let mut fx = Fx::sse();
    let victim = fx.tmp.path().join("victim.txt");
    std::fs::write(&victim, "keep").unwrap();
    std::os::unix::fs::symlink(&victim, fx.plugins_txt()).unwrap();
    fx.add_mod("one", 0, &[("ModA.esp", esp(&["Skyrim.esm"]))]);
    let r = fx.deploy();
    assert!(
        matches!(r.plugins, Some(PluginStage::Failed(_))),
        "{:?}",
        r.plugins
    );
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
    assert!(std::fs::symlink_metadata(fx.plugins_txt())
        .unwrap()
        .is_symlink());
}

/// Catches: a crash between the plugins.txt rename and the managed-list
/// update turning modman entries into user entries.
#[test]
fn stale_managed_entries_are_dropped_on_next_deploy() {
    let mut fx = Fx::sse();
    fx.add_mod("one", 0, &[("ModA.esp", esp(&["Skyrim.esm"]))]);
    fx.deploy();
    // Simulate: the mod is gone, and the previous purge never ran.
    fx.store
        .conn()
        .execute("UPDATE mods SET enabled = 0", [])
        .unwrap();
    let r = fx.deploy();
    assert_eq!(updated(&r.plugins).removed, ["ModA.esp"]);
    let text = std::fs::read_to_string(fx.plugins_txt()).unwrap();
    assert!(!text.contains("ModA.esp"));
}

/// Catches: the masterlist fetch touching the real network, not caching, or
/// deploy ignoring the cached masterlist.
#[test]
fn masterlist_fetch_is_cached_and_used_by_deploy() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mut fx = Fx::sse();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(MockServer::start());
    let masterlist = "plugins:\n  - name: 'P.esp'\n    after: ['Q.esp']\n";
    rt.block_on(
        Mock::given(method("GET"))
            .and(path("/skyrimse/v0.26/masterlist.yaml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(masterlist))
            .expect(1)
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("GET"))
            .and(path("/prelude/v0.26/prelude.yaml"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server),
    );
    let src = MasterlistSource::for_game(&fx.game, &server.uri()).unwrap();
    let ml = rt
        .block_on(fetch_masterlist(&src, &fx.env.game_data_dir()))
        .unwrap();
    assert_eq!(std::fs::read_to_string(&ml).unwrap(), masterlist);

    fx.add_mod("p", 0, &[("P.esp", esp(&["Skyrim.esm"]))]);
    fx.add_mod("q", 1, &[("Q.esp", esp(&["Skyrim.esm"]))]);
    let r = fx.deploy();
    let rep = updated(&r.plugins);
    assert_eq!(rep.sorted_by, Some(SortMethod::Loot { masterlist: true }));
    assert_eq!(rep.added, ["Q.esp", "P.esp"]);
}

/// Catches: a failed download replacing a good cached masterlist.
#[test]
fn failed_masterlist_fetch_keeps_cache() {
    use wiremock::{matchers::any, Mock, MockServer, ResponseTemplate};
    let fx = Fx::sse();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(MockServer::start());
    rt.block_on(
        Mock::given(any())
            .respond_with(ResponseTemplate::new(500))
            .mount(&server),
    );
    let cache = fx.env.game_data_dir().join("cache/loot/masterlist.yaml");
    write(&cache, b"plugins: []\n");
    let src = MasterlistSource::for_game(&fx.game, &server.uri()).unwrap();
    let e = rt
        .block_on(fetch_masterlist(&src, &fx.env.game_data_dir()))
        .unwrap_err();
    assert!(
        matches!(e, loadorder::LoadOrderError::Masterlist { .. }),
        "{e}"
    );
    assert_eq!(std::fs::read_to_string(&cache).unwrap(), "plugins: []\n");
}

/// Catches: conflict data missing plugin-level info (provider, overridden
/// mods, load index, managed flag) or mis-flagging non-plugin files.
#[test]
fn conflict_data_reports_file_and_plugin_conflicts() {
    let mut fx = Fx::sse();
    write(&fx.plugins_txt(), b"*User.esp\r\n");
    let low = fx.add_mod(
        "low",
        0,
        &[
            ("Shared.esp", esp(&["Skyrim.esm"])),
            ("textures/sky.dds", b"low".to_vec()),
        ],
    );
    let high = fx.add_mod(
        "high",
        1,
        &[
            ("Shared.esp", esp(&["Skyrim.esm"])),
            ("textures/sky.dds", b"high".to_vec()),
            ("Only.esp", esp(&["Skyrim.esm"])),
        ],
    );
    fx.deploy();
    let d = conflict_data(fx.store.conn(), &fx.game, &fx.files()).unwrap();
    assert_eq!(d.files.len(), 2);
    for c in &d.files {
        assert_eq!(c.winner.id, high);
        assert_eq!(c.losers.iter().map(|m| m.id).collect::<Vec<_>>(), [low]);
        assert_eq!(c.plugin, c.rel_path.ends_with(".esp"), "{}", c.rel_path);
    }
    let names: Vec<&str> = d.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names[0], "User.esp");
    assert!(d.plugins[0].provider.is_none() && !d.plugins[0].managed);
    let shared = d.plugins.iter().find(|p| p.name == "Shared.esp").unwrap();
    assert_eq!(shared.provider.as_ref().unwrap().id, high);
    assert_eq!(
        shared.overridden.iter().map(|m| m.id).collect::<Vec<_>>(),
        [low]
    );
    assert!(shared.managed && shared.active && shared.load_index.is_some());
    assert_eq!(d.plugins.len(), 3);
}
