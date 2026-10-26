// SPDX-License-Identifier: GPL-3.0-or-later
//! Provenance backup guard and poisoned-backup quarantine. Temp dirs only;
//! nothing touches a real game install, Steam folder or modman data.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use modman_core::deploy::{
    self, DeployEnv, DeployError, DeployOptions, Faults, NeverRunning, PoisonClass, PoisonedBackup,
    QuarantineOrigin,
};
use modman_core::games::{self, GameConfig};
use modman_core::store::Store;
use sha2::{Digest, Sha256};

const GAME: &str = "testgame";

fn sha_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// path -> (kind, sha256 or link target, permission bits)
type Snapshot = BTreeMap<String, (char, String, u32)>;

fn snapshot(dir: &Path) -> Snapshot {
    let mut out = BTreeMap::new();
    if !dir.exists() {
        return out;
    }
    for e in walkdir::WalkDir::new(dir).min_depth(1).sort_by_file_name() {
        let e = e.unwrap();
        let rel = e.path().strip_prefix(dir).unwrap().display().to_string();
        let meta = std::fs::symlink_metadata(e.path()).unwrap();
        let mode = meta.permissions().mode() & 0o7777;
        let v = if meta.is_symlink() {
            let t = std::fs::read_link(e.path()).unwrap();
            ('l', t.display().to_string(), 0)
        } else if meta.is_dir() {
            ('d', String::new(), mode)
        } else {
            ('f', sha_hex(&std::fs::read(e.path()).unwrap()), mode)
        };
        out.insert(rel, v);
    }
    out
}

fn write(path: &Path, data: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
}

/// Rows of the `backups` table that matter for a byte-identical restore.
type BackupRows = Vec<(
    String,
    String,
    String,
    i64,
    i64,
    Option<i64>,
    i64,
    Option<i64>,
)>;

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
        write(&game_dir.join("game.exe"), "exe");
        let data = tmp.path().join("data");
        let store = Store::open(&data.join("modman.db")).unwrap();
        let mut game = GameConfig::custom(GAME);
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

    fn quarantine_dir(&self) -> PathBuf {
        self.env.game_data_dir().join("quarantine")
    }

    /// Stages a mod with its `mod_files` hashes (as the installer records).
    fn add_mod(&self, uid: &str, priority: i64, files: &[(&str, &str)]) -> i64 {
        let dir = self.tmp.path().join("staging").join(uid);
        let c = self.store.conn();
        c.execute(
            "INSERT INTO mods (game, uid, name, priority, staging_dir, installed_at)
             VALUES (?1, ?2, ?2, ?3, ?4, 0)",
            rusqlite::params![GAME, uid, priority, dir.to_string_lossy()],
        )
        .unwrap();
        let id = c.last_insert_rowid();
        for (p, data) in files {
            write(&dir.join(p), data);
            c.execute(
                "INSERT INTO mod_files (mod_id, path, size, sha256) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, p, data.len() as i64, sha_hex(data.as_bytes())],
            )
            .unwrap();
        }
        id
    }

    fn set_game(&mut self, f: impl FnOnce(&mut GameConfig)) {
        f(&mut self.game);
        games::save_game(self.store.conn(), &self.game).unwrap();
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

    fn exec(&self, sql: &str) {
        self.store.conn().execute_batch(sql).unwrap();
    }

    /// The deployment records are lost (the journal survives), as in the
    /// untracked Palworld deploy.
    fn lose_tracking(&self) {
        self.exec("DELETE FROM deployments; DELETE FROM deployment_dirs;");
    }

    fn count(&self, sql: &str) -> i64 {
        self.store.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn held(&self) -> i64 {
        self.count("SELECT COUNT(*) FROM backups WHERE restored_at IS NULL")
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.game_dir().join(rel)).ok()
    }

    fn backup_rows(&self) -> BackupRows {
        let mut stmt = self
            .store
            .conn()
            .prepare(
                "SELECT target, rel_path, sha256, size, mode, mtime_ns, created_at, \
                 (SELECT d.id FROM deployments d WHERE d.backup_id = b.id) \
                 FROM backups b WHERE restored_at IS NULL ORDER BY rel_path",
            )
            .unwrap();
        stmt.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    }

    fn poisoned(&self) -> Vec<PoisonedBackup> {
        deploy::poisoned_backups(self.store.conn(), &self.game).unwrap()
    }

    fn state(&self) -> (Snapshot, Snapshot, BackupRows, Snapshot) {
        (
            snapshot(&self.game_dir()),
            snapshot(&self.env.backup_dir),
            self.backup_rows(),
            snapshot(&self.quarantine_dir()),
        )
    }

    /// Poisoned state like the live Palworld one:
    /// - `Mods/A/main.lua`: placed by an earlier deploy whose tracking was
    ///   lost, then backed up as an "original"; `Mods` is the game's
    ///   mod-only paks folder (confirmed);
    /// - UE4SS's own files, placed and backed up the same way (ambiguous);
    /// - `Data/b.txt`: the game already had the mod's bytes (ambiguous);
    /// - `Data/c.txt`: a real original (not poisoned).
    fn poisoned_setup(&mut self) {
        self.set_game(|g| g.paks_dir = Some("Mods".into()));
        write(&self.game_dir().join("Data/b.txt"), "b");
        write(&self.game_dir().join("Data/c.txt"), "c-orig");
        self.add_mod("a", 0, &[("Mods/A/main.lua", "a-lua")]);
        self.add_mod(
            "ue4ss",
            1,
            &[
                ("ue4ss/UE4SS.dll", "dll"),
                ("ue4ss/Mods/Keybinds/Scripts/main.lua", "kb"),
            ],
        );
        self.deploy();
        self.lose_tracking();
        // The pre-guard engine backed such files up; a purge that ran
        // without the lost records reproduces that here.
        self.exec(
            "INSERT INTO deploy_journal (game, op, state, started_at) VALUES ('testgame', 'purge', 'committed', 0)",
        );
        self.add_mod("b", 2, &[("Data/b.txt", "b")]);
        self.add_mod("c", 3, &[("Data/c.txt", "c-mod")]);
        let r = self.deploy();
        assert_eq!(r.backed_up, 5, "poisoned fixture: 4 poisoned + 1 original");
    }
}

// ── guard ──────────────────────────────────────────────────

/// Fails if the guard compared bytes instead of provenance: a mod shipping
/// the vanilla bytes would leave the original without a backup.
#[test]
fn guard_backs_up_vanilla_identical_file_without_modman_record() {
    let mut fx = Fx::new();
    write(&fx.game_dir().join("Data/v.txt"), "vanilla");
    std::fs::set_permissions(
        fx.game_dir().join("Data/v.txt"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let before = snapshot(&fx.game_dir());
    fx.add_mod("m", 0, &[("Data/v.txt", "vanilla")]);
    assert_eq!(fx.deploy().backed_up, 1);
    assert_eq!(fx.held(), 1);
    fx.purge();
    assert_eq!(snapshot(&fx.game_dir()), before);
}

/// Fails if a file modman placed (tracking lost) is backed up as the
/// original: purge would then "restore" the mod file forever.
#[test]
fn guard_skips_file_placed_by_an_untracked_earlier_deploy() {
    let mut fx = Fx::new();
    fx.add_mod("m", 0, &[("Mods/M/main.lua", "m-lua")]);
    fx.deploy();
    fx.lose_tracking();
    let dry = deploy::deploy(
        &mut fx.store,
        &fx.game,
        &fx.env,
        &DeployOptions {
            dry_run: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(dry.backed_up, 0, "dry run agrees with the guard");
    assert_eq!(fx.deploy().backed_up, 0);
    assert_eq!(fx.held(), 0);
    fx.purge();
    // The folders' records were lost with the tracking, so they stay.
    assert_eq!(fx.read("Mods/M/main.lua"), None, "purge removes the file");
}

/// Fails if provenance alone (without the placed bytes) decides: a file
/// replaced since the untracked placement must be kept as an original.
#[test]
fn guard_backs_up_file_changed_since_the_untracked_placement() {
    let mut fx = Fx::new();
    fx.add_mod("m", 0, &[("Data/x.txt", "mod")]);
    fx.deploy();
    fx.lose_tracking();
    write(&fx.game_dir().join("Data/x.txt"), "user");
    assert_eq!(fx.deploy().backed_up, 1);
    fx.purge();
    assert_eq!(fx.read("Data/x.txt").as_deref(), Some("user"));
}

/// Fails if placements undone by a purge still count as modman's: the
/// restored original would then be deleted by the next purge.
#[test]
fn guard_ignores_placements_a_purge_already_undid() {
    let mut fx = Fx::new();
    write(&fx.game_dir().join("Data/o.txt"), "orig");
    fx.add_mod("m", 0, &[("Data/o.txt", "mod")]);
    fx.deploy();
    fx.purge();
    // Records that do not say what was placed, so only the purge decides.
    fx.exec(
        "UPDATE deploy_journal_steps SET detail = json_remove(detail, '$.sha256');
         DELETE FROM mod_files;",
    );
    assert_eq!(fx.deploy().backed_up, 1);
    fx.purge();
    assert_eq!(fx.read("Data/o.txt").as_deref(), Some("orig"));
}

// ── dry run ────────────────────────────────────────────────

#[test]
fn dry_run_classifies_poisoned_backups_without_side_effects() {
    let mut fx = Fx::new();
    fx.poisoned_setup();
    let before = fx.state();
    let entries = fx.poisoned();
    assert_eq!(fx.state(), before, "dry run changed something");

    let by_path: BTreeMap<&str, &PoisonedBackup> =
        entries.iter().map(|e| (e.path.as_str(), e)).collect();
    assert_eq!(
        by_path.keys().copied().collect::<Vec<_>>(),
        [
            "Data/b.txt",
            "Mods/A/main.lua",
            "ue4ss/Mods/Keybinds/Scripts/main.lua",
            "ue4ss/UE4SS.dll"
        ],
        "the real original Data/c.txt is not listed"
    );
    let a = by_path["Mods/A/main.lua"];
    assert_eq!(a.class, PoisonClass::Confirmed, "{}", a.reason);
    assert!(a.deployed_by_enabled_mod);
    assert_eq!(a.mods, ["a"]);
    // No earlier deploy record.
    assert_eq!(by_path["Data/b.txt"].class, PoisonClass::Ambiguous);
    assert!(by_path["Data/b.txt"]
        .reason
        .contains("no earlier modman deploy"));
    // UE4SS files: a built-in mod, and a file of the UE4SS mod itself.
    for p in ["ue4ss/Mods/Keybinds/Scripts/main.lua", "ue4ss/UE4SS.dll"] {
        assert_eq!(by_path[p].class, PoisonClass::Ambiguous, "{p}");
        assert!(by_path[p].reason.contains("UE4SS"), "{p}");
    }
}

/// Fails if a placement a purge undid still confirms a later backup: the
/// purge restored the vanilla file, and a mod shipping the same bytes makes
/// the next backup look like mod bytes although it is the real original.
#[test]
fn vanilla_identical_backup_after_a_purge_is_ambiguous() {
    let mut fx = Fx::new();
    write(&fx.game_dir().join("Data/v.txt"), "vanilla");
    fx.add_mod("m", 0, &[("Data/v.txt", "vanilla")]);
    assert_eq!(fx.deploy().backed_up, 1);
    fx.purge();
    assert_eq!(fx.deploy().backed_up, 1);
    let e = fx.poisoned();
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].class, PoisonClass::Ambiguous, "{}", e[0].reason);
}

/// Fails if a backup in a mod-only folder (UE4SS mods, paks) is not
/// confirmed when no deploy record exists: the live Palworld files held
/// mod bytes before modman's first deploy.
#[test]
fn backups_in_mod_only_folders_are_confirmed_without_a_record() {
    let mut fx = Fx::new();
    fx.set_game(|g| {
        g.definition = Some("palworld".into());
        g.paks_dir = Some("Pal/Content/Paks/~mods".into());
    });
    let lua = "Pal/Binaries/Win64/ue4ss/Mods/MyMod/Scripts/main.lua";
    let pak = "Pal/Content/Paks/~mods/x.pak";
    let other = "Pal/Content/Paks/Pal-Windows.pak";
    for p in [lua, pak, other] {
        write(&fx.game_dir().join(p), "pre");
    }
    fx.add_mod("m", 0, &[(lua, "pre"), (pak, "pre"), (other, "pre")]);
    assert_eq!(fx.deploy().backed_up, 3);
    let class: BTreeMap<String, PoisonClass> = fx
        .poisoned()
        .into_iter()
        .map(|e| (e.path, e.class))
        .collect();
    assert_eq!(class[lua], PoisonClass::Confirmed);
    assert_eq!(class[pak], PoisonClass::Confirmed);
    assert_eq!(class[other], PoisonClass::Ambiguous, "vanilla folder");
}

/// Fails if a placement still in effect (tracking lost, no purge) when a
/// pre-guard deploy took the backup no longer confirms it.
#[test]
fn placement_not_undone_before_the_backup_is_confirmed() {
    let mut fx = Fx::new();
    fx.add_mod("m", 0, &[("Data/p.txt", "p")]);
    fx.deploy();
    fx.lose_tracking();
    fx.exec(&format!(
        "INSERT INTO backups (game, target, rel_path, sha256, store_path, size, mode, created_at)
         VALUES ('testgame', 'game_dir', 'Data/p.txt', '{}', 'x', 1, 33188, 0);",
        sha_hex(b"p")
    ));
    let e = fx.poisoned();
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].class, PoisonClass::Confirmed, "{}", e[0].reason);
}

// ── apply ──────────────────────────────────────────────────

#[test]
fn apply_without_confirmation_changes_nothing() {
    let mut fx = Fx::new();
    fx.poisoned_setup();
    let before = fx.state();
    let e = deploy::quarantine_poisoned(&mut fx.store, &fx.game, &fx.env, true, false);
    assert!(matches!(e, Err(DeployError::NotConfirmed)), "{e:?}");
    assert_eq!(fx.state(), before);
}

/// Fails if ambiguous entries are quarantined by default, or if a deployed
/// file of an enabled mod is taken out of the game.
#[test]
fn apply_takes_only_confirmed_entries_and_purge_then_removes_the_file() {
    let mut fx = Fx::new();
    fx.poisoned_setup();
    let r = deploy::quarantine_poisoned(&mut fx.store, &fx.game, &fx.env, false, true).unwrap();
    assert_eq!((r.backups, r.game_files, r.skipped_ambiguous), (1, 0, 3));
    assert_eq!(fx.read("Mods/A/main.lua").as_deref(), Some("a-lua"));
    let left: Vec<String> = fx.poisoned().into_iter().map(|e| e.path).collect();
    assert_eq!(left.len(), 3);
    assert!(!left.contains(&"Mods/A/main.lua".to_string()));

    let qdir = r.dir.unwrap();
    let m = &deploy::list_quarantines(&fx.env).unwrap()[0];
    assert_eq!(Some(&m.id), r.id.as_ref());
    assert_eq!(m.items.len(), 1);
    assert_eq!(m.items[0].path, "Mods/A/main.lua");
    assert_eq!(m.items[0].origin, QuarantineOrigin::Backup);
    assert_eq!(m.items[0].sha256, sha_hex(b"a-lua"));
    assert_eq!(
        std::fs::read(qdir.join("blobs").join(sha_hex(b"a-lua"))).unwrap(),
        b"a-lua"
    );

    fx.purge();
    assert_eq!(
        fx.read("Mods/A/main.lua"),
        None,
        "purge must not restore mod bytes"
    );
    assert_eq!(fx.read("Data/c.txt").as_deref(), Some("c-orig"));
}

/// Fails if the round trip loses a blob, a row, a deployment link or an
/// untracked game file.
#[test]
fn apply_with_ambiguous_moves_untracked_files_and_restore_roundtrips() {
    let mut fx = Fx::new();
    fx.poisoned_setup();
    // Tracking of the poisoned A placement is lost again: its game file is
    // now an untracked copy of the mod bytes.
    fx.exec("DELETE FROM deployments WHERE rel_path = 'Mods/A/main.lua'");
    let before = fx.state();

    let r = deploy::quarantine_poisoned(&mut fx.store, &fx.game, &fx.env, true, true).unwrap();
    assert_eq!((r.backups, r.game_files, r.skipped_ambiguous), (4, 1, 0));
    let id = r.id.unwrap();
    let qdir = r.dir.unwrap();
    assert_eq!(fx.read("Mods/A/main.lua"), None);
    assert_eq!(
        std::fs::read_to_string(qdir.join("game/game_dir/Mods/A/main.lua")).unwrap(),
        "a-lua"
    );
    assert_eq!(
        fx.read("Data/b.txt").as_deref(),
        Some("b"),
        "deployed file stays"
    );
    assert_eq!(fx.held(), 1, "only the real original is still held");
    assert!(fx.poisoned().is_empty());
    let m = &deploy::list_quarantines(&fx.env).unwrap()[0];
    assert!(m
        .items
        .iter()
        .any(|i| i.origin == QuarantineOrigin::GameFile && i.path == "Mods/A/main.lua"));

    let e = deploy::restore_quarantine(&mut fx.store, &fx.game, &fx.env, &id, false);
    assert!(matches!(e, Err(DeployError::NotConfirmed)), "{e:?}");
    let rr = deploy::restore_quarantine(&mut fx.store, &fx.game, &fx.env, &id, true).unwrap();
    assert_eq!((rr.backups, rr.game_files), (4, 1));
    let after = fx.state();
    assert_eq!(after.0, before.0, "game folder");
    assert_eq!(after.1, before.1, "backup store");
    assert_eq!(after.2, before.2, "backup rows and deployment links");
    assert!(deploy::restore_quarantine(&mut fx.store, &fx.game, &fx.env, &id, true).is_err());
}

/// Fails if a quarantine move follows a symlinked folder out of the game
/// or accepts a `..` path.
#[test]
fn apply_refuses_paths_escaping_the_game_folder() {
    for (rel, outside_rel) in [("Link/x.txt", "outside/x.txt"), ("../x.txt", "x.txt")] {
        let mut fx = Fx::new();
        let outside = fx.tmp.path().join(outside_rel);
        write(&outside, "evil");
        std::fs::create_dir_all(fx.tmp.path().join("outside")).unwrap();
        std::os::unix::fs::symlink(fx.tmp.path().join("outside"), fx.game_dir().join("Link"))
            .unwrap();
        let sha = sha_hex(b"evil");
        let blob = fx.env.backup_dir.join(&sha[..2]).join(&sha);
        write(&blob, "evil");
        fx.add_mod("d", 0, &[]);
        fx.exec(&format!(
            "INSERT INTO mod_files (mod_id, path, size, sha256) VALUES (1, '{rel}', 4, '{sha}');
             INSERT INTO backups (game, target, rel_path, sha256, store_path, size, mode, created_at)
             VALUES ('testgame', 'game_dir', '{rel}', '{sha}', '{}', 4, 33188, 0);",
            blob.display()
        ));
        assert_eq!(fx.poisoned().len(), 1, "{rel}");
        let before = fx.state();
        let e = deploy::quarantine_poisoned(&mut fx.store, &fx.game, &fx.env, true, true);
        assert!(
            matches!(e, Err(DeployError::PathConflict { .. })),
            "{rel}: {e:?}"
        );
        assert_eq!(fx.state(), before, "{rel}");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "evil");
    }
}
