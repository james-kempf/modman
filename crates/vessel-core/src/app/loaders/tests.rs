// SPDX-License-Identifier: GPL-3.0-or-later
//! Orchestrator tests: FakeRunner only, stub archives, fixture game folders.

use std::io::Write;
use std::os::unix::process::CommandExt;

use super::*;
use crate::loaders::declarative::tests::{BEPINEX_TOML, SKSE_TOML};
use crate::loaders::records::FileKind;
use crate::loaders::runner::{FakeOp, FakeRunner};
use crate::loaders::smapi::tests::release;
use crate::loaders::ProtonEnv;

const V1: &str = "#!/bin/bash\n./\"Stardew Valley\" $@\n";
const V2: &str = "#!/bin/bash\n# 1.6.16\n./\"Stardew Valley\" $@\n";
const LAUNCHER: &str = "#!/bin/bash\nexec ./StardewModdingAPI \"$@\"\n";

struct Fx {
    tmp: tempfile::TempDir,
    game: PathBuf,
    store: Store,
    env: LoaderEnv,
}

fn w(path: &Path, body: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

/// A game row plus folders; `definition` picks the loaders.
fn fx(id: &str, definition: &str, runtime: Runtime) -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let game = base.join("game");
    for d in ["game", "data", "home", "defs", "proc", "root"] {
        fs::create_dir_all(base.join(d)).unwrap();
    }
    let store = Store::open_in_memory().unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO games (id, name, folder, definition, nexus_domain, added_at)
             VALUES (?1, ?1, ?2, ?3, ?1, 0)",
            rusqlite::params![id, game.display().to_string(), definition],
        )
        .unwrap();
    let env = LoaderEnv {
        data_dir: base.join("data"),
        lock_path: base.join("data/deploy.lock"),
        backup_dir: base.join("data/backups"),
        home: base.join("home"),
        definitions_dir: Some(base.join("defs")),
        steam: SteamEnv::default(),
        runner: Arc::new(FakeRunner::default()),
        proc_root: base.join("proc"),
        fs_root: base.join("root"),
        timeout: Duration::from_secs(20),
        preview_ttl: PREVIEW_TTL,
        forced_runtime: Some(runtime),
        crash_at: None,
    };
    Fx {
        tmp,
        game,
        store,
        env,
    }
}

fn base(f: &Fx) -> PathBuf {
    f.tmp.path().canonicalize().unwrap()
}

fn with_runner(f: &mut Fx, r: FakeRunner) -> Arc<FakeRunner> {
    let r = Arc::new(r);
    f.env.runner = r.clone();
    r
}

fn zip_dir(dir: &Path, out: &Path) {
    let mut z = zip::ZipWriter::new(fs::File::create(out).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    for e in walkdir::WalkDir::new(dir).min_depth(1).sort_by_file_name() {
        let e = e.unwrap();
        let rel = e
            .path()
            .strip_prefix(dir)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        if e.file_type().is_dir() {
            z.add_directory(rel, opts).unwrap();
        } else {
            z.start_file(rel, opts).unwrap();
            z.write_all(&fs::read(e.path()).unwrap()).unwrap();
        }
    }
    z.finish().unwrap();
}

fn smapi_zip(f: &Fx) -> PathBuf {
    smapi_zip_at(&base(f))
}

/// A stub SMAPI release zipped into `dir`.
pub(super) fn smapi_zip_at(dir: &Path) -> PathBuf {
    let r = release(true);
    let out = dir.join("SMAPI-4.5.2-installer.zip");
    zip_dir(r.path(), &out);
    out
}

fn vanilla(f: &Fx) {
    vanilla_at(&f.game);
}

/// A vanilla native Stardew folder.
pub(super) fn vanilla_at(game: &Path) {
    w(&game.join("StardewValley"), V1.as_bytes());
    w(&game.join("Stardew Valley"), b"\x7fELF\x02stub");
    w(&game.join("steam_appid.txt"), b"413150");
    fs::create_dir_all(game.join("Mods")).unwrap();
}

fn nexus() -> DownloadOrigin {
    DownloadOrigin::Nexus {
        domain: "stardewvalley".into(),
        mod_id: 2400,
        file_id: 1,
    }
}

fn native_scripts() -> Vec<(String, Vec<FakeOp>)> {
    let wr = |p: &str, b: &str| FakeOp::Write {
        path: p.into(),
        contents: b.as_bytes().to_vec(),
    };
    let mv = |a: &str, b: &str| FakeOp::Rename {
        from: a.into(),
        to: b.into(),
    };
    vec![
        (
            "--install".into(),
            vec![
                mv("StardewValley-original", "StardewValley"),
                mv("StardewValley", "StardewValley-original"),
                wr("StardewValley", LAUNCHER),
                wr("StardewModdingAPI.dll", "dll"),
                wr("StardewModdingAPI", "apphost"),
                wr("smapi-internal/config.json", "{}"),
                FakeOp::Remove {
                    path: "steam_appid.txt".into(),
                },
            ],
        ),
        (
            "--uninstall".into(),
            vec![
                FakeOp::Remove {
                    path: "StardewModdingAPI.dll".into(),
                },
                FakeOp::Remove {
                    path: "StardewModdingAPI".into(),
                },
                FakeOp::RemoveAll {
                    path: "smapi-internal".into(),
                },
                mv("StardewValley-original", "StardewValley"),
            ],
        ),
    ]
}

fn smapi_fx() -> (Fx, Arc<FakeRunner>) {
    let mut f = fx("stardewvalley", "stardewvalley", Runtime::Native);
    vanilla(&f);
    let r = with_runner(
        &mut f,
        FakeRunner {
            scripts: native_scripts(),
            ..FakeRunner::default()
        },
    );
    (f, r)
}

fn req(op: Op, archive: Option<PathBuf>, origin: DownloadOrigin) -> PreviewRequest {
    PreviewRequest {
        game_id: "stardewvalley".into(),
        op,
        loader_key: Some("smapi".into()),
        archive,
        origin,
        unverified_ack: false,
    }
}

fn do_op(f: &mut Fx, r: PreviewRequest) -> RunReport {
    let p = preview(&f.env, &f.store, &r).unwrap();
    run(&f.env, &mut f.store, &p.preview_id, &p.plan_hash).unwrap()
}

fn sha(p: &Path) -> String {
    sha256_file(p).unwrap()
}

fn rows(f: &Fx, id: i64) -> BTreeMap<String, LoaderFile> {
    f.store
        .loader_files(id)
        .unwrap()
        .into_iter()
        .map(|r| (r.path.clone(), r))
        .collect()
}

fn loader_err(e: &super::super::AppError) -> Option<&LoaderError> {
    match e {
        super::super::AppError::Core(crate::Error::Loader(l)) => Some(l),
        _ => None,
    }
}

/// Catches: the install not tracked (or tracked wrongly), detect not seeing
/// it, or uninstall leaving the game anything but vanilla.
#[test]
fn native_install_and_uninstall_end_to_end() {
    let (mut f, runner) = smapi_fx();
    let zip = smapi_zip(&f);
    let p = preview(&f.env, &f.store, &req(Op::Install, Some(zip), nexus())).unwrap();
    assert!(p.provenance.as_ref().unwrap().is_verified());
    assert_eq!(p.version.as_deref(), Some("4.5.2"));
    assert_eq!(p.commands.len(), 1);
    assert!(p.commands[0]
        .program
        .ends_with("internal/linux/SMAPI.Installer"));
    assert_eq!(p.commands[0].args[..2], ["--install", "--no-prompt"]);
    let target = PathBuf::from(&p.commands[0].program);
    assert_ne!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o111,
        0,
        "exec bit set"
    );
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o222,
        0,
        "scratch is read-only"
    );
    let rep = run(&f.env, &mut f.store, &p.preview_id, &p.plan_hash).unwrap();
    assert_eq!(rep.outcome, RunOutcome::Succeeded, "{:?}", rep.warnings);
    assert_eq!(runner.calls(), 1);
    let r = rows(&f, rep.install_id);
    assert_eq!(r["StardewValley"].change, FileChange::Modified);
    assert_eq!(r["StardewValley-original"].change, FileChange::Created);
    assert_eq!(r["smapi-internal/config.json"].change, FileChange::Created);
    assert_eq!(r["steam_appid.txt"].change, FileChange::Deleted);
    assert!(r["StardewValley"].backup_ref.is_some());
    assert_eq!(
        status(&f.env, &f.store, "stardewvalley", "smapi").unwrap(),
        LoaderStatus::Installed {
            install_id: rep.install_id,
            version: Some("4.5.2".into())
        }
    );
    // Mode fixups ran.
    assert_eq!(
        fs::metadata(f.game.join("StardewValley"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert!(f.store.unfinished_loader_runs().unwrap().is_empty());
    assert!(
        f.store.loader_backup_refs().unwrap().iter().all(|r| {
            r.len() > 64
                && rows(&f, rep.install_id)
                    .values()
                    .any(|x| x.backup_ref.as_deref() == Some(r))
        }),
        "run backups cleared once loader_files hold the refs"
    );

    let rep = do_op(&mut f, req(Op::Uninstall, None, nexus()));
    assert_eq!(rep.outcome, RunOutcome::Succeeded, "{:?}", rep.warnings);
    assert_eq!(runner.calls(), 2);
    assert_eq!(
        fs::read_to_string(f.game.join("StardewValley")).unwrap(),
        V1
    );
    assert_eq!(
        fs::read_to_string(f.game.join("steam_appid.txt")).unwrap(),
        "413150"
    );
    for gone in [
        "StardewValley-original",
        "StardewModdingAPI.dll",
        "smapi-internal",
    ] {
        assert!(!f.game.join(gone).exists(), "{gone}");
    }
    assert_eq!(
        status(&f.env, &f.store, "stardewvalley", "smapi").unwrap(),
        LoaderStatus::NotInstalled
    );
}

/// Catches: an installer that "succeeds" (exit 0 is advisory) but did not
/// install being recorded as installed, and its partial changes kept.
#[test]
fn failed_install_rolls_back_and_keeps_the_log() {
    let mut f = fx("stardewvalley", "stardewvalley", Runtime::Native);
    vanilla(&f);
    with_runner(
        &mut f,
        FakeRunner {
            ops: vec![FakeOp::Write {
                path: "StardewModdingAPI.dll".into(),
                contents: b"half".to_vec(),
            }],
            exit_code: 1,
            ..FakeRunner::default()
        },
    );
    let zip = smapi_zip(&f);
    let rep = do_op(&mut f, req(Op::Install, Some(zip), nexus()));
    assert!(matches!(rep.outcome, RunOutcome::Failed { .. }));
    assert_eq!(rep.status, "failed");
    assert!(!f.game.join("StardewModdingAPI.dll").exists());
    assert_eq!(
        fs::read_to_string(f.game.join("StardewValley")).unwrap(),
        V1
    );
    assert!(rep.log_dir.join("step-1.log").exists());
    assert_eq!(
        status(&f.env, &f.store, "stardewvalley", "smapi").unwrap(),
        LoaderStatus::NotInstalled
    );
}

/// Catches: any way to run without the exact, fresh, consented plan.
#[test]
fn bad_tokens_never_spawn() {
    let (mut f, runner) = smapi_fx();
    let zip = smapi_zip(&f);
    let p = preview(
        &f.env,
        &f.store,
        &req(Op::Install, Some(zip.clone()), nexus()),
    )
    .unwrap();
    let consent = |e: super::super::AppError| {
        assert!(
            matches!(loader_err(&e), Some(LoaderError::Consent(_))),
            "{e}"
        );
    };
    consent(run(&f.env, &mut f.store, "deadbeef", &p.plan_hash).unwrap_err());
    consent(run(&f.env, &mut f.store, &p.preview_id, "0000").unwrap_err());
    let mut expired = f.env.clone();
    expired.preview_ttl = Duration::ZERO;
    consent(run(&expired, &mut f.store, &p.preview_id, &p.plan_hash).unwrap_err());
    // Tampered scratch file.
    let target = PathBuf::from(&p.commands[0].program);
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(&target, "evil").unwrap();
    consent(run(&f.env, &mut f.store, &p.preview_id, &p.plan_hash).unwrap_err());
    // Unverified source without the acknowledgement.
    let local = DownloadOrigin::Local { path: zip.clone() };
    let p = preview(
        &f.env,
        &f.store,
        &req(Op::Install, Some(zip.clone()), local.clone()),
    )
    .unwrap();
    assert!(!p.provenance.as_ref().unwrap().is_verified());
    consent(run(&f.env, &mut f.store, &p.preview_id, &p.plan_hash).unwrap_err());
    assert_eq!(runner.calls(), 0);
    // With it, the same archive may run.
    let mut acked = req(Op::Install, Some(zip), local);
    acked.unverified_ack = true;
    let rep = do_op(&mut f, acked);
    assert_eq!(rep.outcome, RunOutcome::Succeeded);
    assert_eq!(runner.calls(), 1);

    // A changed cached archive blocks repair and uninstall.
    for op in [Op::Repair, Op::Uninstall] {
        let p = preview(&f.env, &f.store, &req(op, None, nexus())).unwrap();
        let inst = f.store.loader_install(rep.install_id).unwrap().unwrap();
        let cached = PathBuf::from(inst.archive_path.unwrap());
        let orig = fs::read(&cached).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(&cached)
            .unwrap()
            .write_all(b"x")
            .unwrap();
        consent(run(&f.env, &mut f.store, &p.preview_id, &p.plan_hash).unwrap_err());
        fs::write(&cached, orig).unwrap();
    }
    assert_eq!(runner.calls(), 1);
}

/// Catches: a Steam update's new vanilla being overwritten by the old one
/// on repair or uninstall, and upgrade replacing the original backup.
#[test]
fn steam_update_then_repair_and_uninstall_keep_the_new_vanilla() {
    let (mut f, _) = smapi_fx();
    let zip = smapi_zip(&f);
    let rep = do_op(&mut f, req(Op::Install, Some(zip.clone()), nexus()));
    let v1 = sha(&f.game.join("StardewValley-original"));

    // Upgrade keeps the original vanilla.
    let up = do_op(&mut f, req(Op::Upgrade, Some(zip), nexus()));
    assert_eq!(up.outcome, RunOutcome::Succeeded, "{:?}", up.warnings);
    assert_eq!(
        rows(&f, rep.install_id)["StardewValley"]
            .pre_sha256
            .as_deref(),
        Some(v1.as_str())
    );
    let inst = f.store.loader_install(rep.install_id).unwrap().unwrap();
    assert_eq!(inst.vanilla_sha256.as_deref(), Some(v1.as_str()));

    // Steam updates the game: StardewValley becomes vanilla V2.
    fs::write(f.game.join("StardewValley"), V2).unwrap();
    let rp = do_op(&mut f, req(Op::Repair, None, nexus()));
    assert_eq!(rp.outcome, RunOutcome::Succeeded, "{:?}", rp.warnings);
    assert!(rp.warnings.iter().any(|w| w.contains("Steam updated")));
    assert_eq!(
        fs::read_to_string(f.game.join("StardewValley")).unwrap(),
        LAUNCHER
    );
    assert_eq!(
        fs::read_to_string(f.game.join("StardewValley-original")).unwrap(),
        V2
    );

    let un = do_op(&mut f, req(Op::Uninstall, None, nexus()));
    assert_eq!(un.outcome, RunOutcome::Succeeded, "{:?}", un.warnings);
    assert_eq!(
        fs::read_to_string(f.game.join("StardewValley")).unwrap(),
        V2
    );
    assert_eq!(
        sha(&f.game.join("StardewValley")),
        hex(&Sha256::digest(V2.as_bytes()))
    );
}

fn last_run_id(f: &Fx) -> i64 {
    f.store
        .conn()
        .query_row("SELECT MAX(id) FROM loader_runs", [], |r| r.get(0))
        .unwrap()
}

fn crash_run(f: &mut Fx, at: RunPhase) {
    let zip = smapi_zip(f);
    let p = preview(&f.env, &f.store, &req(Op::Install, Some(zip), nexus())).unwrap();
    let mut env = f.env.clone();
    env.crash_at = Some(at);
    assert!(run(&env, &mut f.store, &p.preview_id, &p.plan_hash).is_err());
}

/// Catches: recovery rolling back a run that never touched the game.
#[test]
fn crash_while_backing_up_is_discarded() {
    let (mut f, runner) = smapi_fx();
    crash_run(&mut f, RunPhase::BackingUp);
    assert_eq!(runner.calls(), 0);
    let out = recover(&f.env, &mut f.store).unwrap();
    assert_eq!(out[0].action, RecoveryAction::Discarded);
    assert_eq!(
        fs::read_to_string(f.game.join("StardewValley")).unwrap(),
        V1
    );
    assert!(f.store.loader_backup_refs().unwrap().is_empty());
    assert!(f.store.unfinished_loader_runs().unwrap().is_empty());
}

/// Catches: recovery killing a reused pid, and an interrupted run's
/// changes not being recorded for the offered rollback.
#[test]
fn crash_after_spawn_offers_rollback_and_spares_a_reused_pid() {
    let (mut f, _) = smapi_fx();
    crash_run(&mut f, RunPhase::Spawned);
    let run_id = last_run_id(&f);
    let me = std::process::id() as i64;
    let start = runner::proc_start_time(Path::new("/proc"), me as u32).unwrap() as i64;
    // Our own pid with another start time: a reused pid, must not be killed.
    f.store
        .set_loader_run_process(run_id, me, me, start + 1)
        .unwrap();
    let out = recover(&f.env, &mut f.store).unwrap();
    assert_eq!(
        out[0].action,
        RecoveryAction::RollbackOffered { killed: false }
    );
    let id = out[0].install_id;
    assert_eq!(
        f.store.loader_install(id).unwrap().unwrap().status,
        "interrupted"
    );
    assert!(rows(&f, id).contains_key("StardewValley-original"));
    assert!(f
        .store
        .loader_backup_refs()
        .unwrap()
        .iter()
        .any(|r| rows(&f, id)["StardewValley"].backup_ref.as_deref() == Some(r)));
    let lost = rollback(&f.env, &mut f.store, id).unwrap();
    assert!(lost.is_empty(), "{lost:?}");
    assert_eq!(
        fs::read_to_string(f.game.join("StardewValley")).unwrap(),
        V1
    );
    assert!(!f.game.join("StardewModdingAPI.dll").exists());
    assert!(f.game.join("steam_appid.txt").exists());
}

/// Catches: a still-running installer group surviving recovery.
#[test]
fn crash_after_spawn_kills_the_live_group() {
    let (mut f, _) = smapi_fx();
    crash_run(&mut f, RunPhase::Spawned);
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = child.id();
    let start = runner::proc_start_time(Path::new("/proc"), pid).unwrap();
    f.store
        .set_loader_run_process(last_run_id(&f), pid as i64, pid as i64, start as i64)
        .unwrap();
    let out = recover(&f.env, &mut f.store).unwrap();
    assert_eq!(
        out[0].action,
        RecoveryAction::RollbackOffered { killed: true }
    );
    let status = child.wait().unwrap();
    assert!(!status.success(), "killed, not finished");
}

#[test]
fn crash_while_diffing_finishes_the_diff_and_done_needs_nothing() {
    let (mut f, _) = smapi_fx();
    crash_run(&mut f, RunPhase::Diffing);
    let out = recover(&f.env, &mut f.store).unwrap();
    assert_eq!(
        out[0].action,
        RecoveryAction::RollbackOffered { killed: false }
    );
    assert_eq!(
        rows(&f, out[0].install_id)["StardewValley"].change,
        FileChange::Modified
    );
    decline_rollback(&f.env, &f.store, out[0].install_id).unwrap();
    assert!(
        recover(&f.env, &mut f.store).unwrap().is_empty(),
        "done: no action"
    );
}

fn trusted_proton(f: &Fx, client_root: PathBuf) -> Runtime {
    let lib = base(f).join("lib/steamapps");
    let proton = lib.join("common/Proton 10.0/proton");
    w(&proton, b"");
    fs::create_dir_all(lib.join("compatdata/413150/pfx")).unwrap();
    for e in walkdir::WalkDir::new(base(f).join("lib"))
        .into_iter()
        .flatten()
    {
        fs::set_permissions(e.path(), fs::Permissions::from_mode(0o755)).unwrap();
    }
    Runtime::Proton(ProtonEnv {
        proton,
        compat_data: lib.join("compatdata/413150"),
        client_root,
        app_id: 413150,
        slr: None,
    })
}

/// Catches: the Proton launch option not reaching the game's desired
/// launch options (or being applied/acked behind the user's back).
#[test]
fn proton_install_sets_desired_launch_options_only() {
    let mut f = fx("stardewvalley", "stardewvalley", Runtime::Native);
    f.env.forced_runtime = Some(trusted_proton(&f, "/steam".into()));
    w(&f.game.join("Stardew Valley.exe"), b"MZ");
    let wr = |p: &str| FakeOp::Write {
        path: p.into(),
        contents: b"x".to_vec(),
    };
    with_runner(
        &mut f,
        FakeRunner {
            ops: vec![
                wr("StardewModdingAPI.dll"),
                wr("StardewModdingAPI.exe"),
                wr("smapi-internal/a.json"),
            ],
            ..FakeRunner::default()
        },
    );
    let zip = smapi_zip(&f);
    let rep = do_op(&mut f, req(Op::Install, Some(zip), nexus()));
    assert_eq!(rep.outcome, RunOutcome::Succeeded, "{:?}", rep.warnings);
    let st = f.store.launch_state("stardewvalley").unwrap();
    let desired = st.desired.unwrap();
    assert!(
        desired.ends_with("\\StardewModdingAPI.exe\" %command%"),
        "{desired}"
    );
    assert!(desired.starts_with("\"Z:"), "{desired}");
    assert_eq!((st.acked, st.applied), (None, None));
}

#[test]
fn flatpak_steam_and_unknown_runtime_are_typed_errors() {
    let mut f = fx("stardewvalley", "stardewvalley", Runtime::Unknown);
    vanilla(&f);
    let zip = smapi_zip(&f);
    let e = preview(
        &f.env,
        &f.store,
        &req(Op::Install, Some(zip.clone()), nexus()),
    )
    .unwrap_err();
    assert!(
        matches!(loader_err(&e), Some(LoaderError::UnknownRuntime(_))),
        "{e}"
    );
    f.env.forced_runtime = Some(trusted_proton(
        &f,
        "/home/u/.var/app/com.valvesoftware.Steam/.local/share/Steam".into(),
    ));
    let e = preview(&f.env, &f.store, &req(Op::Install, Some(zip), nexus())).unwrap_err();
    assert!(
        matches!(loader_err(&e), Some(LoaderError::FlatpakSteamUnsupported)),
        "{e}"
    );
}

fn manual_smapi(f: &Fx) {
    fs::rename(
        f.game.join("StardewValley"),
        f.game.join("StardewValley-original"),
    )
    .unwrap();
    w(&f.game.join("StardewValley"), LAUNCHER.as_bytes());
    w(&f.game.join("StardewModdingAPI.dll"), b"old");
    w(&f.game.join("smapi-internal/config.json"), b"{}");
}

/// Catches: a hand-made install silently overwritten, adopt skipping the
/// vendor uninstall, or adopting onto a game that is not vanilla.
#[test]
fn adopt_takes_over_a_manual_install() {
    let (mut f, runner) = smapi_fx();
    manual_smapi(&f);
    let zip = smapi_zip(&f);
    let e = preview(
        &f.env,
        &f.store,
        &req(Op::Install, Some(zip.clone()), nexus()),
    )
    .unwrap_err();
    assert!(
        matches!(loader_err(&e), Some(LoaderError::ManualInstallDetected(_))),
        "{e}"
    );
    let rep = do_op(&mut f, req(Op::Adopt, Some(zip), nexus()));
    assert_eq!(rep.outcome, RunOutcome::Succeeded, "{:?}", rep.warnings);
    assert_eq!(runner.calls(), 2, "vendor uninstall, then install");
    assert!(matches!(
        status(&f.env, &f.store, "stardewvalley", "smapi").unwrap(),
        LoaderStatus::Installed { .. }
    ));
    // Tracked against the verified vanilla: uninstall gives V1 back.
    let un = do_op(&mut f, req(Op::Uninstall, None, nexus()));
    assert_eq!(un.outcome, RunOutcome::Succeeded, "{:?}", un.warnings);
    assert_eq!(
        fs::read_to_string(f.game.join("StardewValley")).unwrap(),
        V1
    );
}

#[test]
fn adopt_is_blocked_when_the_game_is_not_vanilla_after_uninstall() {
    let (mut f, _) = smapi_fx();
    manual_smapi(&f);
    // The ELF is gone (broken game files).
    fs::write(f.game.join("Stardew Valley"), "#!/bin/bash\n").unwrap();
    let zip = smapi_zip(&f);
    let rep = do_op(&mut f, req(Op::Adopt, Some(zip), nexus()));
    let RunOutcome::Failed { reason } = &rep.outcome else {
        panic!("{rep:?}")
    };
    assert!(
        reason.contains("verify the game files in Steam"),
        "{reason}"
    );
    assert_eq!(rep.status, "failed");
    // Rolled back to the hand-made install.
    assert_eq!(
        fs::read_to_string(f.game.join("StardewValley")).unwrap(),
        LAUNCHER
    );
    assert!(f.game.join("StardewModdingAPI.dll").exists());
}

fn declarative_fx(id: &str, toml: &str, files: &[&str]) -> (Fx, PathBuf) {
    let mut f = fx(id, id, Runtime::Native);
    fs::write(base(&f).join(format!("defs/{id}.toml")), toml).unwrap();
    f.env.forced_runtime = Some(trusted_proton(&f, "/steam".into()));
    let src = base(&f).join("src");
    for p in files {
        w(&src.join(p), p.as_bytes());
    }
    let zip = base(&f).join(format!("{id}-loader.zip"));
    zip_dir(&src, &zip);
    with_runner(&mut f, FakeRunner::default());
    (f, zip)
}

fn dreq(id: &str, op: Op, archive: Option<PathBuf>) -> PreviewRequest {
    PreviewRequest {
        game_id: id.into(),
        op,
        loader_key: None,
        archive,
        origin: DownloadOrigin::Nexus {
            domain: id.into(),
            mod_id: 30379,
            file_id: 1,
        },
        unverified_ack: true,
    }
}

/// Catches: declarative loaders needing loader-specific core code, their
/// files or launch change not applied, or uninstall leaving them behind.
#[test]
fn declarative_bepinex_and_skse_end_to_end() {
    let (mut f, zip) = declarative_fx(
        "valheim",
        BEPINEX_TOML,
        &[
            "winhttp.dll",
            "doorstop_config.ini",
            "BepInEx/core/BepInEx.Preloader.dll",
        ],
    );
    let rep = do_op(&mut f, dreq("valheim", Op::Install, Some(zip)));
    assert_eq!(rep.outcome, RunOutcome::Succeeded, "{:?}", rep.warnings);
    assert!(f.game.join("BepInEx/core/BepInEx.Preloader.dll").is_file());
    assert!(
        rep.launch_desired
            .as_deref()
            .unwrap()
            .contains("WINEDLLOVERRIDES=\"winhttp=n,b\"")
            || rep
                .launch_desired
                .as_deref()
                .unwrap()
                .contains("WINEDLLOVERRIDES=winhttp=n,b"),
        "{:?}",
        rep.launch_desired
    );
    let mut un = dreq("valheim", Op::Uninstall, None);
    un.loader_key = Some("bepinex".into());
    let rep = do_op(&mut f, un);
    assert_eq!(rep.outcome, RunOutcome::Succeeded, "{:?}", rep.warnings);
    assert!(!f.game.join("winhttp.dll").exists() && !f.game.join("BepInEx").exists());
    assert_eq!(rep.launch_desired, None);
    assert_eq!(f.store.launch_state("valheim").unwrap().desired, None);

    let (mut f, zip) = declarative_fx(
        "skyrimspecialedition",
        SKSE_TOML,
        &[
            "skse64_2_02_06/skse64_loader.exe",
            "skse64_2_02_06/skse64_1_6_1170.dll",
            "skse64_2_02_06/Data/Scripts/Actor.pex",
        ],
    );
    w(&f.game.join("SkyrimSE.exe"), b"MZ");
    let rep = do_op(&mut f, dreq("skyrimspecialedition", Op::Install, Some(zip)));
    assert_eq!(rep.outcome, RunOutcome::Succeeded, "{:?}", rep.warnings);
    assert!(f.game.join("Data/Scripts/Actor.pex").is_file());
    assert!(rep
        .launch_desired
        .unwrap()
        .contains("skse64_loader.exe\" %command%"));
    let mut un = dreq("skyrimspecialedition", Op::Uninstall, None);
    un.loader_key = Some("skse".into());
    let rep = do_op(&mut f, un);
    assert_eq!(rep.outcome, RunOutcome::Succeeded, "{:?}", rep.warnings);
    assert!(!f.game.join("skse64_loader.exe").exists() && !f.game.join("Data").exists());
    assert!(f.game.join("SkyrimSE.exe").exists());
}

/// Catches: merge losing the first backup or keeping rows for paths back
/// at their original state.
#[test]
fn merge_keeps_first_backup_and_drops_round_trips() {
    let row = |p: &str, c: FileChange, pre: Option<&str>, post: Option<&str>, b: Option<&str>| {
        LoaderFile {
            path: p.into(),
            change: c,
            kind: FileKind::File,
            pre_sha256: pre.map(Into::into),
            post_sha256: post.map(Into::into),
            backup_ref: b.map(Into::into),
            out_of_scope: false,
        }
    };
    let old = [
        row(
            "a",
            FileChange::Modified,
            Some("v1"),
            Some("s1"),
            Some("b1"),
        ),
        row("n", FileChange::Created, None, Some("x"), None),
        row("d", FileChange::Deleted, Some("d1"), None, Some("bd")),
    ];
    let new = [
        row(
            "a",
            FileChange::Modified,
            Some("s1"),
            Some("s2"),
            Some("b2"),
        ),
        row("n", FileChange::Deleted, Some("x"), None, Some("bx")),
        row("d", FileChange::Created, None, Some("d1"), None),
    ];
    let m = merge_files(&old, &new);
    assert_eq!(m.len(), 1, "{m:?}");
    assert_eq!(
        (
            m[0].pre_sha256.as_deref(),
            m[0].post_sha256.as_deref(),
            m[0].backup_ref.as_deref()
        ),
        (Some("v1"), Some("s2"), Some("b1"))
    );
}

/// Catches: the unverified acknowledgement not reaching the hashed plan on
/// a re-request (the old token would still be refused), the stale preview
/// staying usable, or the reopened preview losing the archive/origin.
#[test]
fn reopen_preview_carries_the_ack_into_a_fresh_token() {
    let (mut f, runner) = smapi_fx();
    let zip = smapi_zip(&f);
    let local = DownloadOrigin::Local { path: zip.clone() };
    let p = preview(&f.env, &f.store, &req(Op::Install, Some(zip), local)).unwrap();
    // Shown in full again (no ack): same plan, new token.
    let shown = reopen_preview(&f.env, &f.store, &p.preview_id, false).unwrap();
    assert_ne!(shown.preview_id, p.preview_id);
    // Same plan; scratch paths name the new preview.
    assert_eq!(shown.commands[0].args, p.commands[0].args);
    assert!(shown.commands[0].program.contains(&shown.preview_id));
    assert!(!shown.provenance.as_ref().unwrap().is_verified());
    assert!(run(&f.env, &mut f.store, &p.preview_id, &p.plan_hash).is_err());
    let acked = reopen_preview(&f.env, &f.store, &shown.preview_id, true).unwrap();
    assert_ne!(acked.plan_hash, shown.plan_hash);
    assert!(run(&f.env, &mut f.store, &shown.preview_id, &shown.plan_hash).is_err());
    assert_eq!(runner.calls(), 0);
    let rep = run(&f.env, &mut f.store, &acked.preview_id, &acked.plan_hash).unwrap();
    assert_eq!(rep.outcome, RunOutcome::Succeeded);
    assert_eq!(
        latest_run(&f.store, rep.install_id).unwrap(),
        Some(rep.run_id)
    );
}

/// Catches: an arbitrary string stored as the runtime override (detection
/// would then fail for good) and a silent no-op for an unknown game.
#[test]
fn runtime_override_accepts_only_native_proton_or_none() {
    let f = fx("stardewvalley", "stardewvalley", Runtime::Native);
    set_runtime_override(&f.store, "stardewvalley", Some("proton")).unwrap();
    assert_eq!(
        runtime_override(&f.store, "stardewvalley")
            .unwrap()
            .as_deref(),
        Some("proton")
    );
    assert!(set_runtime_override(&f.store, "stardewvalley", Some("wine")).is_err());
    set_runtime_override(&f.store, "stardewvalley", None).unwrap();
    assert_eq!(runtime_override(&f.store, "stardewvalley").unwrap(), None);
    assert!(set_runtime_override(&f.store, "nope", Some("native")).is_err());
}

fn consent_err(e: &super::super::AppError) {
    assert!(
        matches!(loader_err(e), Some(LoaderError::Consent(_))),
        "{e}"
    );
}

/// Catches: a consented plan running against another game folder (the
/// game was moved or re-pointed after the preview).
#[test]
fn run_refuses_a_preview_for_another_game_folder() {
    let (mut f, runner) = smapi_fx();
    let zip = smapi_zip(&f);
    let p = preview(&f.env, &f.store, &req(Op::Install, Some(zip), nexus())).unwrap();
    let other = base(&f).join("moved");
    vanilla_at(&other);
    f.store
        .conn()
        .execute(
            "UPDATE games SET folder = ?1 WHERE id = 'stardewvalley'",
            [other.display().to_string()],
        )
        .unwrap();
    consent_err(&run(&f.env, &mut f.store, &p.preview_id, &p.plan_hash).unwrap_err());
    assert_eq!(runner.calls(), 0);
    assert!(f.store.unfinished_loader_runs().unwrap().is_empty());
    // The folder is part of the hash: editing the stored record to match
    // breaks the token instead.
    let p = preview(
        &f.env,
        &f.store,
        &req(Op::Install, Some(smapi_zip(&f)), nexus()),
    )
    .unwrap();
    let mut rec = load_record(&f.env, &p.preview_id).unwrap();
    rec.game_dir = f.game.clone();
    write_private(
        &f.env.dir("previews").join(format!("{}.json", p.preview_id)),
        &serde_json::to_vec(&rec).unwrap(),
    )
    .unwrap();
    consent_err(&run(&f.env, &mut f.store, &p.preview_id, &p.plan_hash).unwrap_err());
    assert_eq!(runner.calls(), 0);
}

/// Catches: the installer swapped after the token check (while backups
/// run) still being started.
#[test]
fn installer_changed_after_the_token_check_never_spawns() {
    let (f, runner) = smapi_fx();
    let zip = smapi_zip(&f);
    let p = preview(&f.env, &f.store, &req(Op::Install, Some(zip), nexus())).unwrap();
    let rec = load_record(&f.env, &p.preview_id).unwrap();
    check_token(&f.env, &f.store, &rec, &p.plan_hash).unwrap();
    let target = PathBuf::from(&p.commands[0].program);
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(&target, "evil").unwrap();

    let game = crate::games::require_game(f.store.conn(), "stardewvalley").unwrap();
    let registry = registry_for(&f.env, &game).unwrap();
    let plugin = registry.get("smapi").unwrap();
    let lock = lock(&f.env).unwrap();
    let dirs = run_dirs(&f.env, &p.preview_id);
    mkdir_private(&dirs.dir).unwrap();
    let mut ex = Exec {
        env: &f.env,
        rec: &rec,
        plugin,
        ctx: game_ctx(&f.store, &game, "smapi").unwrap(),
        scope: rec.plan.touch_scope.clone(),
        run_id: 1,
        install_id: 1,
        dirs,
        pre: Manifest::default(),
        refs: BTreeMap::new(),
        exit_code: None,
        timed_out: false,
        launch: None,
        warnings: Vec::new(),
    };
    let r = ex.steps(&f.store, &lock).unwrap();
    assert_eq!(
        r,
        Err("the extracted installer changed after the preview".into())
    );
    assert_eq!(runner.calls(), 0);
    assert!(!installer_tmp(&f.env, &p.preview_id).exists());
}

/// Catches: the consent dialog showing a TMPDIR or PATH other than the
/// ones the installer is started with.
#[test]
fn preview_env_is_the_env_the_run_uses() {
    let (mut f, runner) = smapi_fx();
    let zip = smapi_zip(&f);
    let p = preview(&f.env, &f.store, &req(Op::Install, Some(zip), nexus())).unwrap();
    let shown = p.commands[0].env.clone();
    for k in ["TMPDIR", "PATH"] {
        assert!(shown.iter().any(|(n, _)| n == k), "{k} shown: {shown:?}");
    }
    let rep = run(&f.env, &mut f.store, &p.preview_id, &p.plan_hash).unwrap();
    assert_eq!(rep.outcome, RunOutcome::Succeeded, "{:?}", rep.warnings);
    assert_eq!(runner.calls(), 1);
    let ran: Vec<(String, String)> = runner
        .last_command()
        .unwrap()
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.to_string_lossy().into_owned()))
        .collect();
    assert_eq!(ran, shown);
    assert!(
        !installer_tmp(&f.env, &p.preview_id).exists(),
        "run tmp removed"
    );
}

/// Catches: a reopened preview giving the old Nexus origin (verified
/// provenance) to an archive file that changed after it was hashed.
#[test]
fn reopen_preview_refuses_a_changed_archive() {
    let (f, runner) = smapi_fx();
    let zip = smapi_zip(&f);
    let p = preview(&f.env, &f.store, &req(Op::Install, Some(zip), nexus())).unwrap();
    assert!(p.provenance.as_ref().unwrap().is_verified());
    let rec = load_record(&f.env, &p.preview_id).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(rec.archive_path.as_ref().unwrap())
        .unwrap()
        .write_all(b"x")
        .unwrap();
    consent_err(&reopen_preview(&f.env, &f.store, &p.preview_id, true).unwrap_err());
    assert!(
        load_record(&f.env, &p.preview_id).is_err(),
        "stale preview dropped"
    );
    assert_eq!(runner.calls(), 0);
}

/// Catches: the reopen TOCTOU, where the archive was re-hashed and then
/// cached (and given the stored origin) under whatever bytes were present
/// at a second, later hash. cache_archive must hash the bytes it caches
/// and refuse, before any origin is written, when they are not the
/// expected ones.
#[test]
fn cache_archive_refuses_bytes_other_than_the_expected_sha() {
    let (f, runner) = smapi_fx();
    let zip = smapi_zip(&f);
    let real = sha256_file(&zip).unwrap();
    let stale = "0".repeat(64);
    consent_err(&cache_archive(&f.env, &zip, &nexus(), Some(&stale)).unwrap_err());
    let archives = f.env.dir("archives");
    let left: Vec<_> = fs::read_dir(&archives).unwrap().collect();
    assert!(left.is_empty(), "nothing cached, no tmp left: {left:?}");
    // The same refusal through a reopened preview's path.
    let r = req(Op::Install, Some(zip.clone()), nexus());
    consent_err(&preview_expecting(&f.env, &f.store, &r, Some(&stale)).unwrap_err());
    assert!(!archives.join(&real).join("origin.json").exists());
    let (sha, dest) = cache_archive(&f.env, &zip, &nexus(), Some(&real)).unwrap();
    assert_eq!(sha, real);
    assert_eq!(sha256_file(&dest).unwrap(), real);
    assert!(matches!(cached_origin(&dest), DownloadOrigin::Nexus { .. }));
    assert_eq!(runner.calls(), 0);
}
