// SPDX-License-Identifier: GPL-3.0-or-later

use std::os::unix::fs::symlink;
use std::sync::Arc;
use std::time::SystemTime;

use super::*;
use crate::app::loaders::PREVIEW_TTL;
use crate::loaders::records::{InstallStatus, NewLoaderInstall, NewLoaderRun, Strategy};
use crate::loaders::runner::FakeRunner;
use crate::loaders::runtime::SteamEnv;
use crate::loaders::Op;

struct Fx {
    _tmp: tempfile::TempDir,
    base: PathBuf,
    store: Store,
    env: LoaderEnv,
}

impl Fx {
    fn root(&self) -> PathBuf {
        self.env.data_dir.join("loaders")
    }

    fn install(&self, status: InstallStatus, sha: Option<&str>) -> i64 {
        // One live install per game and loader: a fresh key each time.
        let n: i64 = self
            .store
            .conn()
            .query_row("SELECT COUNT(*) FROM loader_installs", [], |r| r.get(0))
            .unwrap();
        self.store
            .insert_loader_install(&NewLoaderInstall {
                game_id: "g".into(),
                game_dir: self.base.join("game").display().to_string(),
                loader_key: format!("l{n}"),
                version: None,
                runtime: "native".into(),
                strategy: Strategy::Installer,
                status,
                nexus_mod_id: None,
                nexus_file_id: None,
                origin: "local".into(),
                archive_sha256: sha.map(Into::into),
                archive_path: None,
                vanilla_sha256: None,
                game_build: None,
            })
            .unwrap()
    }

    /// A run with `runs/<name>/{pre.json,step-1.log}`.
    fn run(&self, install: i64, name: &str, started_at: i64, exit: Option<i64>, done: bool) -> i64 {
        let dir = self.root().join("runs").join(name);
        w(&dir.join("pre.json"), b"{}");
        w(&dir.join("step-1.log"), b"log\n");
        let id = self
            .store
            .insert_loader_run(&NewLoaderRun {
                install_id: install,
                op: Op::Install,
                consent_hash: "h".into(),
                pre_manifest_path: Some(dir.join("pre.json").display().to_string()),
                log_path: Some(dir.display().to_string()),
            })
            .unwrap();
        if done {
            self.store.finish_loader_run(id, exit, false).unwrap();
        }
        self.store
            .conn()
            .execute(
                "UPDATE loader_runs SET started_at = ?2 WHERE id = ?1",
                rusqlite::params![id, started_at],
            )
            .unwrap();
        id
    }
}

fn fx() -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    fs::create_dir_all(base.join("data/loaders")).unwrap();
    let store = Store::open_in_memory().unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO games (id, name, folder, definition, nexus_domain, added_at)
             VALUES ('g', 'g', ?1, 'stardewvalley', 'g', 0)",
            [base.join("game").display().to_string()],
        )
        .unwrap();
    let env = LoaderEnv {
        data_dir: base.join("data"),
        lock_path: base.join("data/deploy.lock"),
        backup_dir: base.join("data/backups"),
        home: PathBuf::from("/home/tester"),
        definitions_dir: None,
        steam: SteamEnv::default(),
        runner: Arc::new(FakeRunner::default()),
        proc_root: base.join("proc"),
        fs_root: base.join("root"),
        timeout: Duration::from_secs(20),
        preview_ttl: PREVIEW_TTL,
        forced_runtime: None,
        crash_at: None,
    };
    Fx {
        _tmp: tmp,
        base,
        store,
        env,
    }
}

fn w(path: &Path, body: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

/// Backdates `path`'s mtime by `secs` (after its contents are written).
fn age(path: &Path, secs: u64) {
    let t = SystemTime::now() - Duration::from_secs(secs);
    fs::File::open(path).unwrap().set_modified(t).unwrap();
}

fn read_only(dir: &Path) {
    for e in walkdir::WalkDir::new(dir).contents_first(true) {
        let e = e.unwrap();
        if !e.path_is_symlink() {
            fs::set_permissions(e.path(), fs::Permissions::from_mode(0o555)).unwrap();
        }
    }
}

const DAY: i64 = 24 * 3600;

// Catches: retention deleting failed/interrupted logs, the first run's
// baseline pre-manifest, or keeping more than KEEP_RUNS / older than 90 days.
#[test]
fn retention_prunes_old_runs_but_keeps_failed_and_interrupted() {
    let f = fx();
    let t = now();
    let inst = f.install(InstallStatus::Installed, None);
    for i in 1..=25 {
        let exit = if i == 3 { Some(1) } else { Some(0) };
        let started = if i == 24 || i == 3 {
            t - 100 * DAY
        } else {
            t - 25 + i
        };
        f.run(inst, &format!("r{i}"), started, exit, true);
    }
    // Interrupted install: its last run (no exit code, old) is protected.
    let broken = f.install(InstallStatus::Interrupted, None);
    f.run(broken, "x1", t - 200 * DAY, None, true);
    // An unfinished run is never pruned.
    let other = f.install(InstallStatus::Installed, None);
    f.run(other, "u1", t - 200 * DAY, None, false);
    f.run(other, "u2", t, Some(0), true);
    // Unreferenced dirs: old ones go, fresh ones stay.
    let runs = f.root().join("runs");
    w(&runs.join("stale/step-1.log"), b"x");
    age(&runs.join("stale"), 100 * DAY as u64);
    w(&runs.join("fresh/step-1.log"), b"x");

    let rep = run(&f.env, &f.store).unwrap();
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);

    for i in (6..=25).filter(|i| *i != 24) {
        assert!(runs.join(format!("r{i}/step-1.log")).exists(), "r{i}");
    }
    for i in [2, 4, 5, 24] {
        assert!(!runs.join(format!("r{i}")).exists(), "r{i} kept");
    }
    assert!(runs.join("r3/step-1.log").exists(), "failed run log pruned");
    assert!(
        runs.join("r1/pre.json").exists(),
        "baseline pre-manifest removed"
    );
    assert!(!runs.join("r1/step-1.log").exists());
    assert!(
        runs.join("x1/step-1.log").exists(),
        "interrupted run log pruned"
    );
    assert!(
        runs.join("u1/step-1.log").exists(),
        "unfinished run log pruned"
    );
    assert!(!runs.join("stale").exists());
    assert!(runs.join("fresh").exists());
}

// Catches: a key, Bearer token or home path leaking into a shown log, and
// over-redaction that mangles ordinary paths.
#[test]
fn redaction_removes_keys_tokens_and_home() {
    let home = Path::new("/home/tester");
    let key = "Zm9vYmFyYmF6cXV4--c2VjcmV0a2V5MTIzNDU2Nzg5MA==--q1w2e3r4t5";
    let text = format!(
        "GET https://api.nexusmods.com/v1/users/validate.json apikey: {key}\n\
         Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.abc.def\n\
         url?api_key=shortsecret&x=1\n\
         sha 0123456789abcdef0123456789abcdef0123456789abcdef\n\
         Installing to /home/tester/.local/share/Steam/steamapps/common/Stardew Valley\n\
         lib /usr/lib/x86_64-linux-gnu/libSDL2-2.0.so.0 and /home/testerx stays\n"
    );
    let out = redact_with(&text, home, &["unrelated-configured-key-123"]);
    assert!(!out.contains(key), "{out}");
    assert!(!out.contains("eyJhbGciOiJIUzI1NiJ9"), "{out}");
    assert!(!out.contains("shortsecret"), "{out}");
    assert!(!out.contains("0123456789abcdef0123"), "{out}");
    assert!(out.contains("Bearer [redacted]"), "{out}");
    assert!(
        out.contains("~/.local/share/Steam/steamapps/common/Stardew Valley"),
        "{out}"
    );
    assert!(!out.contains("/home/tester/"), "{out}");
    assert!(out.contains("/home/testerx stays"), "{out}");
    assert!(
        out.contains("/usr/lib/x86_64-linux-gnu/libSDL2-2.0.so.0"),
        "{out}"
    );
    assert_eq!(redact("plain text", home), "plain text");
}

// Catches: read_log returning raw text, or missing the exact configured
// key when it does not look like a token.
#[test]
fn read_log_redacts_configured_key_and_home() {
    let mut f = fx();
    f.env.home = f.base.join("home");
    let inst = f.install(InstallStatus::Installed, None);
    let id = f.run(inst, "r1", now(), Some(0), true);
    let dir = f.root().join("runs/r1");
    let home = f.env.home.display().to_string();
    w(
        &dir.join("step-1.log"),
        format!("cwd {home}/x\n").as_bytes(),
    );
    w(&dir.join("step-2.log"), b"key short-key-value!\n");
    let out = read_log(&f.env, &f.store, id, Some("short-key-value!")).unwrap();
    assert!(out.contains("cwd ~/x"), "{out}");
    assert!(out.contains("key [redacted]"), "{out}");
    assert!(out.find("== step 1 ==") < out.find("== step 2 =="));
    assert!(read_log(&f.env, &f.store, 999, None).is_err());
}

// Catches: read-only scratch failing to delete, and removal following a
// symlink out of the scratch dir.
#[test]
fn orphan_read_only_scratch_is_removed_without_following_links() {
    let f = fx();
    let outside = f.base.join("outside");
    w(&outside.join("keep.txt"), b"keep");
    let s = f.root().join("scratch/abc123");
    w(&s.join("internal/linux/SMAPI.Installer"), b"bin");
    symlink(&outside, s.join("link")).unwrap();
    age(&s, 3600);
    read_only(&s);

    let rep = run(&f.env, &f.store).unwrap();
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert!(!s.exists());
    assert!(outside.join("keep.txt").exists(), "followed the symlink");
}

// Catches: expired previews lingering with their scratch, or a live
// preview's scratch/tmp being removed; stale per-run tmp dirs lingering.
#[test]
fn expired_preview_and_scratch_go_live_one_survives() {
    let f = fx();
    let r = f.root();
    let t = now();
    w(
        &r.join("previews/aaaa.json"),
        format!(r#"{{"created_at":{}}}"#, t - 3600).as_bytes(),
    );
    w(&r.join("scratch/aaaa/f"), b"x");
    read_only(&r.join("scratch/aaaa"));
    w(
        &r.join("previews/bbbb.json"),
        format!(r#"{{"created_at":{t}}}"#).as_bytes(),
    );
    w(&r.join("scratch/bbbb/f"), b"x");
    age(&r.join("scratch/bbbb"), 3600);
    read_only(&r.join("scratch/bbbb"));
    w(&r.join("tmp/preview-bbbb/x"), b"x");
    age(&r.join("tmp/preview-bbbb"), 3600);
    w(&r.join("tmp/42/x"), b"x");
    age(&r.join("tmp/42"), 3600);

    let rep = run(&f.env, &f.store).unwrap();
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert!(!r.join("previews/aaaa.json").exists());
    assert!(!r.join("scratch/aaaa").exists());
    assert!(r.join("previews/bbbb.json").exists());
    assert!(r.join("scratch/bbbb/f").exists());
    assert!(r.join("tmp/preview-bbbb").exists());
    assert!(!r.join("tmp/42").exists());
}

// Catches: deleting an archive an install or a live preview still needs.
#[test]
fn referenced_archives_survive_unreferenced_go() {
    let f = fx();
    let r = f.root();
    f.install(InstallStatus::Uninstalled, Some("aa11"));
    w(
        &r.join("previews/cccc.json"),
        format!(r#"{{"created_at":{},"archive_sha256":"bb22"}}"#, now()).as_bytes(),
    );
    for sha in ["aa11", "bb22", "cc33"] {
        w(&r.join(format!("archives/{sha}/SMAPI.zip")), b"zip");
        age(&r.join(format!("archives/{sha}")), 3600);
    }
    w(&r.join("archives/dd44/SMAPI.zip"), b"zip"); // fresh: grace

    let rep = run(&f.env, &f.store).unwrap();
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert!(r.join("archives/aa11/SMAPI.zip").exists());
    assert!(r.join("archives/bb22/SMAPI.zip").exists());
    assert!(!r.join("archives/cc33").exists());
    assert!(r.join("archives/dd44").exists());
}

// Catches: the Proton temp cleanup touching anything but SMAPI-installer-*
// dirs, or following a link named like one.
#[test]
fn proton_temp_cleanup_touches_only_smapi_installer_dirs() {
    let f = fx();
    let pfx = f.base.join("compatdata/413150/pfx");
    let temp = pfx.join(PROTON_TEMP);
    let target = f.base.join("target");
    w(&target.join("keep.txt"), b"keep");
    w(&temp.join("SMAPI-installer-1a2b/internal/x.dll"), b"x");
    age(&temp.join("SMAPI-installer-1a2b"), 3600);
    read_only(&temp.join("SMAPI-installer-1a2b"));
    symlink(&target, temp.join("SMAPI-installer-link")).unwrap();
    w(&temp.join("SMAPI-installer-file"), b"f");
    w(&temp.join("other/x"), b"x");
    age(&temp.join("other"), 3600);
    w(&temp.join("SMAPI-installerX/x"), b"x");
    age(&temp.join("SMAPI-installerX"), 3600);
    let inst = f.install(InstallStatus::Installed, None);
    let id = f.run(inst, "p1", now(), Some(0), true);
    f.store
        .set_loader_run_wine(id, "/ws", &pfx.display().to_string())
        .unwrap();
    let link = temp.join("SMAPI-installer-link");

    let rep = run(&f.env, &f.store).unwrap();
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert!(!temp.join("SMAPI-installer-1a2b").exists());
    assert!(link.symlink_metadata().is_err(), "link not removed");
    assert!(target.join("keep.txt").exists(), "followed the link");
    assert!(temp.join("SMAPI-installer-file").exists());
    assert!(temp.join("other/x").exists());
    assert!(temp.join("SMAPI-installerX/x").exists());
}

// Catches: removal escaping its base through `..` or an outside path.
#[test]
fn remove_within_refuses_outside_paths() {
    let f = fx();
    let r = f.root();
    let outside = f.base.join("outside");
    w(&outside.join("x"), b"x");
    assert!(remove_within(&r, &outside).is_err());
    assert!(remove_within(&r, &r.join("../../outside")).is_err());
    assert!(remove_within(&r, &r).is_err());
    assert!(outside.join("x").exists());
}
