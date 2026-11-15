// SPDX-License-Identifier: GPL-3.0-or-later
//! CLI tests for `mods restage`. Temp HOME, archives and game folders only.

mod support;

use predicates::str::contains;
use support::Env;

const G: &str = "testgame";

/// Fails if `mods restage` loses staged edits or enabled state, restages
/// while a confirmed poisoned backup exists, or lets --ignore-poisoned move
/// a mod away from the path of its own poisoned backup.
#[test]
fn restage_keeps_edits_and_blocks_on_poisoned_backups() {
    let env = Env::new();
    env.add_game(G);
    let z = env.zip(
        "Alpha.zip",
        &[("Data/alpha.esp", b"a"), ("Data/textures/a.dds", b"t")],
    );
    env.run(&["install", z.to_str().unwrap()]).success();
    env.run(&["disable", "Alpha"]).success();
    let st = env.paths().data_dir.join(G).join("staging/Alpha");
    std::fs::write(st.join("alpha.esp"), "edited").unwrap();

    // A custom game has no UE4SS layout: nothing is outdated.
    env.run(&["mods", "restage"])
        .success()
        .stdout(contains("Nothing to restage"));
    env.run(&["mods", "restage", "--all", "Alpha"]).failure();
    env.run(&["mods", "restage", "Alpha"])
        .success()
        .stdout(contains("Restaged 1 mod(s)"))
        .stdout(contains("kept your edit"));
    assert_eq!(
        std::fs::read_to_string(st.join("alpha.esp")).unwrap(),
        "edited"
    );
    assert!(st.join("textures/a.dds").is_file());
    assert_eq!(env.enabled(), [("Alpha".into(), false)]);

    // Poison: an earlier deploy whose tracking was lost, then a pre-guard
    // deploy that backed the placed mod file up as the "original".
    std::fs::write(st.join("alpha.esp"), "a").unwrap();
    std::fs::remove_file(st.join("textures/a.dds")).unwrap();
    std::fs::remove_dir(st.join("textures")).unwrap();
    env.store()
        .conn()
        .execute_batch("DELETE FROM mod_files WHERE path = 'textures/a.dds';")
        .unwrap();
    env.run(&["enable", "Alpha"]).success();
    env.run(&["deploy"]).success();
    env.store()
        .conn()
        .execute_batch(&format!(
            "DELETE FROM deployments; DELETE FROM deployment_dirs;
             INSERT INTO backups (game, target, rel_path, sha256, store_path, size, mode, created_at)
             SELECT '{G}', 'game_dir', 'Data/alpha.esp', sha256, 'x', 1, 33188, 0
             FROM mod_files WHERE path = 'alpha.esp';"
        ))
        .unwrap();
    let held = || env.query_i64("SELECT COUNT(*) FROM backups WHERE restored_at IS NULL");
    assert_eq!(held(), 1, "poisoned fixture");

    env.run(&["mods", "restage", "--all"])
        .failure()
        .stderr(contains("vessel deploy clean-backups"));
    assert!(
        !st.join("textures").exists(),
        "blocked restage changed staging"
    );
    // Alpha provides the poisoned path: --ignore-poisoned does not cover it.
    env.run(&["mods", "restage", "--all", "--ignore-poisoned"])
        .failure()
        .stderr(contains("clean-backups --apply first"));
    assert!(
        !st.join("textures").exists(),
        "refused restage changed staging"
    );
    assert_eq!(held(), 1);
}
