// SPDX-License-Identifier: GPL-3.0-or-later
//! vessel-owned UE4SS mods.txt: merge on deploy, policy-driven restore on
//! purge, crash recovery, and exclusion from conflicts and placement. Temp
//! dirs only; nothing touches a real game install, Steam folder or vessel
//! data.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use vessel_core::deploy::{self, DeployEnv, DeployError, DeployOptions, Faults, NeverRunning};
use vessel_core::games::{self, GameConfig};
use vessel_core::loadorder::{conflict_data, PluginsFiles};
use vessel_core::store::Store;

const GAME: &str = "palworld";
const MODS: &str = "Pal/Binaries/Win64/ue4ss/Mods";
const LEGACY: &str = "Pal/Binaries/Win64/Mods";
const PROBE: &str = "Pal/Binaries/Win64/ue4ss/UE4SS.dll";

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
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let game_dir = tmp.path().join("game");
        write(
            &game_dir.join("Pal/Binaries/Win64/Palworld-Win64-Shipping.exe"),
            b"exe",
        );
        let data = tmp.path().join("data");
        let store = Store::open(&data.join("vessel.db")).unwrap();
        let mut game = GameConfig::custom(GAME);
        game.profile = "ue".into();
        game.definition = Some(GAME.into());
        game.folder = Some(game_dir);
        games::save_game(store.conn(), &game).unwrap();
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

    fn game_dir(&self) -> PathBuf {
        self.game.folder.clone().unwrap()
    }

    fn put(&self, rel: &str, data: &[u8]) {
        write(&self.game_dir().join(rel), data);
    }

    fn bytes(&self, rel: &str) -> Option<Vec<u8>> {
        std::fs::read(self.game_dir().join(rel)).ok()
    }

    fn text(&self, rel: &str) -> String {
        String::from_utf8(self.bytes(rel).expect("file exists")).unwrap()
    }

    fn add_mod(&self, uid: &str, priority: i64, files: &[(&str, &str)]) {
        let dir = self.tmp.path().join("staging").join(uid);
        self.store
            .conn()
            .execute(
                "INSERT INTO mods (game, uid, name, priority, staging_dir, installed_at)
                 VALUES (?1, ?2, ?2, ?3, ?4, 0)",
                rusqlite::params![GAME, uid, priority, dir.to_string_lossy()],
            )
            .unwrap();
        for (p, data) in files {
            write(&dir.join(p), data.as_bytes());
        }
    }

    /// A UE4SS Lua mod staged as `<dir>/<folder>/Scripts/main.lua`.
    fn lua_mod(&self, dir: &str, folder: &str, priority: i64) {
        let main = format!("{dir}/{folder}/Scripts/main.lua");
        self.add_mod(folder, priority, &[(&main, folder)]);
    }

    fn set_enabled(&self, uid: &str, on: bool) {
        self.store
            .conn()
            .execute(
                "UPDATE mods SET enabled = ?2 WHERE uid = ?1",
                rusqlite::params![uid, on],
            )
            .unwrap();
    }

    fn deploy(&mut self) -> Result<deploy::DeployReport, DeployError> {
        deploy::deploy(
            &mut self.store,
            &self.game,
            &self.env,
            &DeployOptions::default(),
        )
    }

    fn purge(&mut self) {
        deploy::purge(&mut self.store, &self.game, &self.env).unwrap();
    }
}

fn txt(dir: &str) -> String {
    format!("{dir}/mods.txt")
}

/// Catches: mods.txt rewritten wholesale (CRLF, missing final newline or
/// user lines lost), Keybinds no longer last, and purge leaving a re-encoded
/// file instead of the original bytes.
#[test]
fn deploy_merges_before_keybinds_and_purge_restores_original_bytes() {
    let mut fx = Fx::new();
    let original: &[u8] = b"; user comment\r\nBPModLoaderMod : 1\r\nUserMod : 0\r\nKeybinds : 1";
    fx.put(PROBE, b"dll");
    fx.put(&txt(MODS), original);
    fx.lua_mod(MODS, "AlphaMod", 1);
    fx.lua_mod(MODS, "BetaMod", 2);

    fx.deploy().unwrap();
    assert_eq!(
        fx.text(&txt(MODS)),
        "; user comment\r\nBPModLoaderMod : 1\r\nUserMod : 0\r\nAlphaMod : 1\r\nBetaMod : 1\r\nKeybinds : 1"
    );

    fx.purge();
    assert_eq!(fx.bytes(&txt(MODS)).unwrap(), original);
}

/// Catches: a disabled mod's line surviving a redeploy, an enabled mod being
/// listed twice, and user edits made after a deploy being lost.
#[test]
fn disable_removes_entry_and_user_edits_survive_redeploy() {
    let mut fx = Fx::new();
    fx.put(PROBE, b"dll");
    fx.put(&txt(MODS), b"Keybinds : 1\n");
    fx.lua_mod(MODS, "AlphaMod", 1);
    fx.lua_mod(MODS, "BetaMod", 2);
    fx.deploy().unwrap();
    assert_eq!(
        fx.text(&txt(MODS)),
        "AlphaMod : 1\nBetaMod : 1\nKeybinds : 1\n"
    );

    // The user adds an entry after the deploy.
    fx.put(
        &txt(MODS),
        b"UserMod : 1\nAlphaMod : 1\nBetaMod : 1\nKeybinds : 1\n",
    );
    fx.set_enabled("BetaMod", false);
    fx.deploy().unwrap();
    assert_eq!(
        fx.text(&txt(MODS)),
        "UserMod : 1\nAlphaMod : 1\nKeybinds : 1\n"
    );
}

/// Catches: purge restoring the pre-deploy original over user edits (losing
/// them), or leaving vessel's lines behind.
#[test]
fn purge_after_user_edit_removes_only_vessel_lines() {
    let mut fx = Fx::new();
    fx.put(PROBE, b"dll");
    fx.put(&txt(MODS), b"Keybinds : 1\n");
    fx.lua_mod(MODS, "AlphaMod", 1);
    fx.deploy().unwrap();
    fx.put(&txt(MODS), b"AlphaMod : 1\nUserMod : 1\nKeybinds : 1\n");

    fx.purge();
    assert_eq!(fx.text(&txt(MODS)), "UserMod : 1\nKeybinds : 1\n");
}

/// Catches: no mods.txt written when the game has none, and purge leaving a
/// file (and folder) vessel created.
#[test]
fn missing_mods_txt_is_created_and_purge_removes_it() {
    let mut fx = Fx::new();
    fx.lua_mod(MODS, "AlphaMod", 1);
    fx.deploy().unwrap();
    assert_eq!(fx.text(&txt(MODS)), "AlphaMod : 1\n");

    fx.purge();
    assert!(fx.bytes(&txt(MODS)).is_none());
    assert!(!fx.game_dir().join(MODS).exists(), "created folder removed");
}

/// Catches: mods.txt shipped by several mods reported as a file conflict, or
/// a mod's copy placed over the merged file.
#[test]
fn mods_shipping_mods_txt_neither_conflict_nor_overwrite() {
    let mut fx = Fx::new();
    fx.put(PROBE, b"dll");
    fx.put(&txt(MODS), b"Keybinds : 1\n");
    let a_main = format!("{MODS}/AlphaMod/Scripts/main.lua");
    let b_main = format!("{MODS}/BetaMod/Scripts/main.lua");
    let t = txt(MODS);
    fx.add_mod("a", 1, &[(&a_main, "a"), (&t, "AlphaMod : 1\n")]);
    fx.add_mod("b", 2, &[(&b_main, "b"), (&t, "BetaMod : 0\n")]);

    let plan = deploy::build_plan(fx.store.conn(), &fx.game).unwrap();
    assert!(plan.conflicts().is_empty(), "{:?}", plan.conflicts());
    assert!(!plan
        .items
        .iter()
        .any(|i| i.rel_path.eq_ignore_ascii_case(&t)));
    let data = conflict_data(
        fx.store.conn(),
        &fx.game,
        &PluginsFiles::new(fx.env.game_data_dir()),
    )
    .unwrap();
    assert!(data.files.is_empty(), "{:?}", data.files);

    fx.deploy().unwrap();
    assert_eq!(fx.text(&t), "AlphaMod : 1\nBetaMod : 1\nKeybinds : 1\n");
    let rows: i64 = fx
        .store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM deployments WHERE lower(rel_path) LIKE '%/mods.txt'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 0, "mods.txt is merged, never placed");
}

/// Catches: a UE4SS install shipped as a mod losing its mods.txt (defaults
/// such as Keybinds) when the game has none.
#[test]
fn shipped_mods_txt_is_the_base_when_the_game_has_none() {
    let mut fx = Fx::new();
    let t = txt(MODS);
    let keybinds = format!("{MODS}/Keybinds/Scripts/main.lua");
    fx.add_mod(
        "ue4ss",
        1,
        &[(PROBE, "dll"), (&t, "Keybinds : 1\n"), (&keybinds, "k")],
    );
    fx.lua_mod(MODS, "AlphaMod", 2);
    fx.deploy().unwrap();
    assert_eq!(fx.text(&t), "AlphaMod : 1\nKeybinds : 1\n");

    fx.purge();
    assert!(fx.bytes(&t).is_none());
}

/// Catches: mods.txt written to the current folder while an older UE4SS
/// reads the legacy one (or the reverse).
#[test]
fn mods_txt_path_follows_the_resolved_mods_dir() {
    // Legacy only: no probe, legacy folder present.
    let mut fx = Fx::new();
    fx.put(&txt(LEGACY), b"Keybinds : 1\n");
    fx.lua_mod(LEGACY, "AlphaMod", 1);
    fx.deploy().unwrap();
    assert_eq!(fx.text(&txt(LEGACY)), "AlphaMod : 1\nKeybinds : 1\n");
    assert!(fx.bytes(&txt(MODS)).is_none());
    fx.purge();
    assert_eq!(fx.text(&txt(LEGACY)), "Keybinds : 1\n");

    // Probe present: the current folder wins even with a legacy folder.
    let mut fx = Fx::new();
    fx.put(PROBE, b"dll");
    fx.put(&txt(LEGACY), b"Keybinds : 1\n");
    fx.put(&txt(MODS), b"Keybinds : 1\n");
    fx.lua_mod(MODS, "AlphaMod", 1);
    fx.deploy().unwrap();
    assert_eq!(fx.text(&txt(MODS)), "AlphaMod : 1\nKeybinds : 1\n");
    assert_eq!(fx.text(&txt(LEGACY)), "Keybinds : 1\n");
}

/// Catches: a crash after mods.txt was written but before the deploy
/// journal committed leaving vessel's lines in the file for good.
#[test]
fn crash_after_mods_txt_write_is_recovered_to_original_bytes() {
    let original: &[u8] = b"BPModLoaderMod : 1\r\nKeybinds : 1\r\n";
    for next in ["recover", "purge", "deploy"] {
        let mut fx = Fx::new();
        fx.put(PROBE, b"dll");
        fx.put(&txt(MODS), original);
        fx.lua_mod(MODS, "AlphaMod", 1);
        // One staged file: the crash point after the last placement is
        // after the mods.txt write and before the journal commit.
        fx.env.faults.crash_after_files = Some(1);
        assert!(matches!(fx.deploy(), Err(DeployError::InjectedCrash)));
        assert_ne!(fx.bytes(&txt(MODS)).unwrap(), original, "file was written");
        fx.env.faults = Faults::default();

        match next {
            "recover" => {
                deploy::recover(&mut fx.store, &fx.game, &fx.env).unwrap();
                assert_eq!(fx.bytes(&txt(MODS)).unwrap(), original, "{next}");
            }
            "purge" => {
                fx.purge();
                assert_eq!(fx.bytes(&txt(MODS)).unwrap(), original, "{next}");
            }
            _ => {
                fx.deploy().unwrap();
                assert_eq!(
                    fx.text(&txt(MODS)),
                    "BPModLoaderMod : 1\r\nAlphaMod : 1\r\nKeybinds : 1\r\n"
                );
                fx.purge();
                assert_eq!(fx.bytes(&txt(MODS)).unwrap(), original, "{next}");
            }
        }
    }
}
