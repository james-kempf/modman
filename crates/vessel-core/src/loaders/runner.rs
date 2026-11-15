// SPDX-License-Identifier: GPL-3.0-or-later
//! Runs a vendor installer. Only this module turns an [`InstallerSpec`]
//! into a process, and it does so with argv only: no shell ever parses a
//! path or argument.
//!
//! - The target is resolved inside the preview scratch dir, refusing any
//!   symlinked component, and the process runs from the target's folder
//!   (vendor scripts start their installer relative to the cwd).
//! - Native: `BashScript` runs as `/bin/bash <script> args...`, `Exe`
//!   directly. Proton: `[<SLR run> --] <proton> run <exe> args...`, and
//!   `BatScript` as `... run cmd.exe /c call <bat> args...`. Every bat
//!   argument is checked for cmd metacharacters first.
//! - The env is cleared down to an allowlist; Vessel secrets never reach
//!   the child.
//! - The child leads its own process group and dies with Vessel
//!   (PDEATHSIG). On timeout or cancel the whole group is killed, and for
//!   Proton `wineserver -k` cleans up the prefix.
//! - pid, pgid, start time and the Proton cleanup target reach the caller
//!   before the wait, so crash recovery can kill a stale group (and its
//!   wineserver) without hitting a reused pid.
//! - After a normal exit the group is killed too, so nothing the installer
//!   left in the background keeps changing the game folder after the
//!   post-snapshot.
//! - Contract: PDEATHSIG fires when the *thread* that forked exits, so
//!   [`ProcessRunner::run`] must be called from a thread that stays alive
//!   until it returns (it blocks until then; the `Child` never leaves it).
//! - The runner makes no assumption about how a vendor installer is
//!   driven: the plugin chooses target, kind and args.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::types::{Arg, Host, InstallerSpec, ProtonEnv, TargetKind};
use super::{io_err, LoaderError};
use crate::deploy::confine;

/// Default installer timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
/// Log size cap; output beyond it is drained and dropped (installers can
/// loop printing the same prompt until the timeout).
pub const LOG_CAP: u64 = 8 * 1024 * 1024;
/// PATH given to every child.
pub const BASE_PATH: &str = "/usr/local/bin:/usr/bin:/bin";
/// Env names the child may see. Everything else is cleared.
pub const ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "LANG",
    "STEAM_COMPAT_DATA_PATH",
    "STEAM_COMPAT_CLIENT_INSTALL_PATH",
    "TMPDIR",
];
/// Characters cmd.exe interprets, even inside quotes for some of them.
pub const CMD_METACHARS: &[char] = &['&', '|', '<', '>', '^', '%', '!', '"', '(', ')', '\r', '\n'];

/// One installer run.
#[derive(Debug, Clone, Copy)]
pub struct RunRequest<'a> {
    pub spec: &'a InstallerSpec,
    /// Preview scratch dir the target must be inside.
    pub scratch: &'a Path,
    pub game_dir: &'a Path,
    /// `HOME` for the child.
    pub home: &'a Path,
    /// `LANG` for the child (`C.UTF-8` when `None`).
    pub lang: Option<&'a str>,
    /// The caller's PATH, searched for `python3` (Proton needs it).
    pub caller_path: Option<&'a OsStr>,
    pub timeout: Duration,
    pub log_path: &'a Path,
    pub cancel: Option<&'a AtomicBool>,
    /// Per-run temp dir (absolute), required when the spec asks for
    /// `private_tmp`; becomes `TMPDIR`.
    pub tmp_dir: Option<&'a Path>,
}

/// Identity of the spawned process, for persistence before the wait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spawned {
    pub pid: u32,
    /// Equals `pid` (the child leads its own group).
    pub pgid: u32,
    /// `/proc/<pid>/stat` starttime, in clock ticks since boot.
    pub proc_start: u64,
    /// Proton: `(wineserver, WINEPREFIX)`, to persist for recovery
    /// (`Store::set_loader_run_wine`).
    pub wine: Option<(PathBuf, PathBuf)>,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// `None` when killed by a signal.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    /// From the spec: when `false`, `exit_code` is advisory only.
    pub exit_code_reliable: bool,
    pub log_truncated: bool,
}

impl RunOutcome {
    /// `Some(success)` when the exit code decides; `None` when the plugin's
    /// post-snapshot check must decide.
    pub fn exit_verdict(&self) -> Option<bool> {
        if self.timed_out || self.cancelled {
            return Some(false);
        }
        self.exit_code_reliable.then_some(self.exit_code == Some(0))
    }
}

/// The exact process a spec turns into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(String, OsString)>,
    pub cwd: PathBuf,
    /// `(wineserver, WINEPREFIX)` for Proton cleanup.
    pub wineserver: Option<(PathBuf, PathBuf)>,
    /// What cmd.exe sees after `/c` (BatScript only), for display.
    pub cmd_line: Option<String>,
}

/// Starts processes for loader ops.
pub trait ProcessRunner: Send + Sync {
    /// Runs `req`. `on_spawn` is called once the process exists and before
    /// waiting; an error from it kills the process and aborts the run.
    fn run(
        &self,
        req: &RunRequest<'_>,
        on_spawn: &mut dyn FnMut(&Spawned) -> Result<(), LoaderError>,
    ) -> Result<RunOutcome, LoaderError>;
}

/// Resolves `rel` inside `scratch`: no `.`/`..`, no symlinked component,
/// a regular file, and still inside after canonicalisation.
pub fn resolve_target(scratch: &Path, rel: &str) -> Result<PathBuf, LoaderError> {
    let unsafe_path = |m: String| LoaderError::UnsafePath {
        path: rel.to_string(),
        message: m,
    };
    let base = scratch
        .canonicalize()
        .map_err(|e| unsafe_path(format!("scratch dir: {e}")))?;
    let comps = confine::components(rel).map_err(|m| unsafe_path(m.into()))?;
    if comps.is_empty() {
        return Err(unsafe_path("empty target".into()));
    }
    let mut p = base.clone();
    for c in &comps {
        p.push(c);
        let m = fs::symlink_metadata(&p).map_err(|e| unsafe_path(e.to_string()))?;
        if m.file_type().is_symlink() {
            return Err(unsafe_path(format!("'{c}' is a symbolic link")));
        }
    }
    if !fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()) {
        return Err(unsafe_path("not a regular file".into()));
    }
    let real = p.canonicalize().map_err(|e| unsafe_path(e.to_string()))?;
    if !real.starts_with(&base) {
        return Err(unsafe_path("resolves outside the scratch dir".into()));
    }
    Ok(real)
}

/// `Z:\...` spelling of an absolute host path (Proton maps `Z:` to `/`).
/// No trailing backslash, which would escape a closing quote.
pub fn win_path(p: &Path) -> Result<String, LoaderError> {
    let s = p
        .to_str()
        .filter(|s| s.starts_with('/'))
        .ok_or_else(|| LoaderError::UnsafePath {
            path: p.display().to_string(),
            message: "needs an absolute UTF-8 path".into(),
        })?;
    Ok(format!("Z:{}", s.trim_end_matches('/').replace('/', "\\")))
}

/// Refuses a bat argument cmd.exe could interpret.
pub fn check_cmd_safe(arg: &str) -> Result<(), LoaderError> {
    if arg.contains(CMD_METACHARS) {
        return Err(LoaderError::UnsafeForCmd(arg.to_string()));
    }
    Ok(())
}

/// The Windows command line Wine builds from argv elements that passed
/// [`check_cmd_safe`]: elements with a space or tab, or empty ones, are
/// double-quoted.
pub fn windows_command_line(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.is_empty() || a.contains([' ', '\t']) {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `wineserver` of a Proton install (`files/bin`, or `files/bin-wow64`).
pub fn wineserver_of(proton: &Path) -> Option<PathBuf> {
    let dir = proton.parent()?;
    ["files/bin/wineserver", "files/bin-wow64/wineserver"]
        .iter()
        .map(|p| dir.join(p))
        .find(|p| p.is_file())
}

/// Folder holding `python3` when it is not in [`BASE_PATH`]: only absolute
/// entries of the caller's PATH count (a relative one would resolve
/// against the installer's cwd).
fn python3_dir(caller_path: Option<&OsStr>) -> Option<PathBuf> {
    if std::env::split_paths(BASE_PATH).any(|d| d.join("python3").is_file()) {
        return None;
    }
    caller_path
        .into_iter()
        .flat_map(std::env::split_paths)
        .filter(|d| d.is_absolute())
        .find(|d| d.join("python3").is_file())
}

/// Builds the process for `req` without starting it.
pub fn build_command(req: &RunRequest<'_>) -> Result<CommandPlan, LoaderError> {
    let spec = req.spec;
    let target = resolve_target(req.scratch, &spec.target.rel_path)?;
    let cwd = target.parent().expect("a file has a parent").to_path_buf();
    let mut args: Vec<String> = Vec::new();
    let mut os_args: Vec<OsString> = Vec::new();
    for a in &spec.args {
        match a {
            Arg::Lit(s) => {
                args.push(s.clone());
                os_args.push(s.into());
            }
            Arg::GamePath => {
                args.push(req.game_dir.to_string_lossy().into_owned());
                os_args.push(req.game_dir.as_os_str().to_owned());
            }
            Arg::GamePathWin => {
                let w = win_path(req.game_dir)?;
                args.push(w.clone());
                os_args.push(w.into());
            }
        }
    }
    let mut env: Vec<(String, OsString)> = vec![
        ("HOME".into(), req.home.as_os_str().to_owned()),
        ("LANG".into(), req.lang.unwrap_or("C.UTF-8").into()),
    ];
    if spec.private_tmp {
        let dir = req
            .tmp_dir
            .filter(|d| d.is_absolute() && d.is_dir())
            .ok_or_else(|| LoaderError::UnsafePath {
                path: "TMPDIR".into(),
                message: "the installer needs an existing private temp dir".into(),
            })?;
        env.push(("TMPDIR".into(), dir.as_os_str().to_owned()));
    }
    let kind_name = match spec.target.kind {
        TargetKind::BashScript => "bash script",
        TargetKind::BatScript => "batch file",
        TargetKind::Exe => "executable",
    };
    let (program, argv, wineserver, cmd_line, path) = match (&spec.host, spec.target.kind) {
        (Host::Native, TargetKind::BashScript) => {
            let mut v: Vec<OsString> = vec![target.into_os_string()];
            v.extend(os_args);
            (
                PathBuf::from("/bin/bash"),
                v,
                None,
                None,
                BASE_PATH.to_string(),
            )
        }
        (Host::Native, TargetKind::Exe) => (target, os_args, None, None, BASE_PATH.to_string()),
        (Host::Native, TargetKind::BatScript) => {
            return Err(LoaderError::HostMismatch {
                kind: kind_name,
                host: "native",
            })
        }
        (Host::Proton(_), TargetKind::BashScript) => {
            return Err(LoaderError::HostMismatch {
                kind: kind_name,
                host: "Proton",
            })
        }
        (Host::Proton(p), kind) => {
            let (program, mut v) = proton_prefix(p);
            let mut cmd_line = None;
            if kind == TargetKind::BatScript {
                let bat = win_path(&target)?;
                check_cmd_safe(&bat)?;
                for a in &args {
                    check_cmd_safe(a)?;
                }
                // `call` keeps the line from starting with a quote, so cmd's
                // /c rule never strips the first and last quote characters.
                let mut line = vec!["call".to_string(), bat];
                line.extend(args.iter().cloned());
                cmd_line = Some(windows_command_line(&line));
                v.extend(["cmd.exe", "/c"].map(OsString::from));
                v.extend(line.into_iter().map(OsString::from));
            } else {
                v.push(target.into_os_string());
                v.extend(os_args);
            }
            env.push((
                "STEAM_COMPAT_DATA_PATH".into(),
                p.compat_data.as_os_str().to_owned(),
            ));
            env.push((
                "STEAM_COMPAT_CLIENT_INSTALL_PATH".into(),
                p.client_root.as_os_str().to_owned(),
            ));
            // Appended, so it never shadows a system binary.
            let path = match python3_dir(req.caller_path) {
                Some(d) => format!("{BASE_PATH}:{}", d.display()),
                None => BASE_PATH.to_string(),
            };
            // Detection validated these; re-check right before they run.
            super::runtime::recheck_trusted(&p.proton)?;
            if let Some(slr) = &p.slr {
                super::runtime::recheck_trusted(slr)?;
            }
            let ws = wineserver_of(&p.proton).map(|w| (w, p.compat_data.join("pfx")));
            (program, v, ws, cmd_line, path)
        }
    };
    env.push(("PATH".into(), path.into()));
    assert!(
        env.iter().all(|(k, _)| ENV_ALLOWLIST.contains(&k.as_str())),
        "installer env outside the allowlist"
    );
    Ok(CommandPlan {
        program,
        args: argv,
        env,
        cwd,
        wineserver,
        cmd_line,
    })
}

fn proton_prefix(p: &ProtonEnv) -> (PathBuf, Vec<OsString>) {
    let run = [p.proton.as_os_str().to_owned(), "run".into()];
    match &p.slr {
        Some(slr) => {
            let mut v: Vec<OsString> = vec!["--".into()];
            v.extend(run);
            (slr.clone(), v)
        }
        None => (p.proton.clone(), vec!["run".into()]),
    }
}

/// `/proc/<pid>/stat` starttime (field 22).
pub fn proc_start_time(proc_root: &Path, pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
    // comm (field 2) may contain spaces and ')'; fields resume after the last ')'.
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(19)?.parse().ok()
}

/// `/proc/<pid>/stat` process group (field 5).
fn proc_pgrp(proc_root: &Path, pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(2)?.parse().ok()
}

/// True when group `pgid` of a run whose leader `pid` started at
/// `proc_start` still has a member: the leader itself, or (leader gone)
/// any process in the group that started no earlier than it.
///
/// Reuse guard: Linux does not hand out a pid while it is still in use as
/// a process group id, so while any member of the old group lives, no new
/// process can have pid `pgid`. A live process with that pid but another
/// start time therefore means the old group is gone and the id was reused.
pub fn group_alive(proc_root: &Path, pid: u32, pgid: u32, proc_start: u64) -> bool {
    match proc_start_time(proc_root, pid) {
        Some(t) if t == proc_start => return true,
        Some(_) if pid == pgid => return false,
        _ => {}
    }
    let Ok(dir) = fs::read_dir(proc_root) else {
        return false;
    };
    dir.flatten().any(|e| {
        let Some(p) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            return false;
        };
        proc_pgrp(proc_root, p) == Some(pgid)
            && proc_start_time(proc_root, p).is_some_and(|t| t >= proc_start)
    })
}

/// Startup recovery for a run left in phase `spawned`: kills group `pgid`
/// if [`group_alive`], then runs `wineserver -k` for a Proton run (its
/// wineserver is not in the group). `true` when a group kill was sent.
pub fn kill_stale(pid: u32, pgid: u32, proc_start: u64, wine: Option<(&Path, &Path)>) -> bool {
    let alive = group_alive(Path::new("/proc"), pid, pgid, proc_start);
    if alive {
        kill_group(pgid);
    }
    if let Some((ws, pfx)) = wine {
        wineserver_kill(ws, pfx);
    }
    alive
}

/// [`kill_stale`] for a stored run.
pub fn kill_stale_run(run: &super::records::LoaderRun) -> bool {
    let (Some(pid), Some(pgid), Some(start)) = (run.pid, run.pgid, run.proc_start) else {
        return false;
    };
    let wine = run
        .wineserver
        .as_deref()
        .zip(run.wineprefix.as_deref())
        .map(|(w, p)| (Path::new(w), Path::new(p)));
    kill_stale(pid as u32, pgid as u32, start as u64, wine)
}

fn kill_group(pgid: u32) {
    if let Some(p) = rustix::process::Pid::from_raw(pgid as i32) {
        let _ = rustix::process::kill_process_group(p, rustix::process::Signal::KILL);
    }
}

/// Bound for `wineserver -k`.
pub const WINESERVER_KILL_TIMEOUT: Duration = Duration::from_secs(10);
/// Bound for `wineserver -w` after a normal Proton exit.
pub const WINESERVER_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs `wineserver <flag>` for the prefix with the minimal env, killing it
/// after `timeout`. `false` on failure or timeout.
fn wineserver(wineserver: &Path, prefix: &Path, flag: &str, timeout: Duration) -> bool {
    let child = Command::new(wineserver)
        .arg(flag)
        .env_clear()
        .env("PATH", BASE_PATH)
        .env("WINEPREFIX", prefix)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(wineserver = %wineserver.display(), error = %e, "wineserver {flag} failed");
            return false;
        }
    };
    let until = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                tracing::warn!(wineserver = %wineserver.display(), "wineserver {flag} timed out");
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// `wineserver -k` for the prefix (bounded by [`WINESERVER_KILL_TIMEOUT`]).
pub fn wineserver_kill(ws: &Path, prefix: &Path) -> bool {
    wineserver(ws, prefix, "-k", WINESERVER_KILL_TIMEOUT)
}

/// Spawns real processes.
#[derive(Debug, Clone)]
pub struct RealRunner {
    /// Filesystem root for the Flatpak check (`/`).
    pub fs_root: PathBuf,
}

impl Default for RealRunner {
    fn default() -> Self {
        Self {
            fs_root: PathBuf::from("/"),
        }
    }
}

struct Log {
    file: Mutex<fs::File>,
    written: AtomicU64,
    truncated: AtomicBool,
}

impl Log {
    fn pump(self: Arc<Self>, mut from: impl Read + Send + 'static) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = from.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let before = self.written.fetch_add(n as u64, Ordering::Relaxed);
                let room = LOG_CAP.saturating_sub(before).min(n as u64) as usize;
                if room < n {
                    self.truncated.store(true, Ordering::Relaxed);
                }
                if room > 0 {
                    if let Ok(mut f) = self.file.lock() {
                        let _ = f.write_all(&buf[..room]);
                    }
                }
            }
        })
    }
}

/// Waits for the log pumps until `until`; stops early on `cancel`.
fn wait_threads(
    threads: &[std::thread::JoinHandle<()>],
    until: Instant,
    cancel: Option<&AtomicBool>,
) -> bool {
    while Instant::now() < until {
        if threads.iter().all(|t| t.is_finished()) {
            return true;
        }
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    threads.iter().all(|t| t.is_finished())
}

impl RealRunner {
    fn spawn(&self, plan: &CommandPlan) -> Result<Child, LoaderError> {
        let parent = rustix::process::getpid();
        let mut cmd = Command::new(&plan.program);
        cmd.args(&plan.args)
            .env_clear()
            .envs(plan.env.iter().map(|(k, v)| (k, v)))
            .current_dir(&plan.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        // SAFETY: the closure only makes async-signal-safe syscalls
        // (prctl, getppid) and allocates nothing.
        unsafe {
            cmd.pre_exec(move || {
                rustix::process::set_parent_process_death_signal(Some(
                    rustix::process::Signal::KILL,
                ))?;
                // Vessel died between fork and prctl: the signal never comes.
                if rustix::process::getppid() != Some(parent) {
                    return Err(rustix::io::Errno::SRCH.into());
                }
                Ok(())
            });
        }
        cmd.spawn().map_err(|source| LoaderError::Spawn {
            program: plan.program.clone(),
            source,
        })
    }
}

impl ProcessRunner for RealRunner {
    fn run(
        &self,
        req: &RunRequest<'_>,
        on_spawn: &mut dyn FnMut(&Spawned) -> Result<(), LoaderError>,
    ) -> Result<RunOutcome, LoaderError> {
        if matches!(req.spec.host, Host::Proton(_)) && crate::autoupdate::in_flatpak(&self.fs_root)
        {
            return Err(LoaderError::FlatpakUnsupported);
        }
        let plan = build_command(req)?;
        if let Some(dir) = req.log_path.parent() {
            fs::create_dir_all(dir).map_err(io_err(dir))?;
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(req.log_path)
            .map_err(io_err(req.log_path))?;
        let mut child = self.spawn(&plan)?;
        let pid = child.id();
        let kill = |child: &mut Child| {
            kill_group(pid);
            let _ = child.wait();
            if let Some((ws, pfx)) = &plan.wineserver {
                wineserver_kill(ws, pfx);
            }
        };
        // The leader is not reaped yet, so its stat exists even if it exited.
        let Some(proc_start) = proc_start_time(Path::new("/proc"), pid) else {
            kill(&mut child);
            return Err(LoaderError::Spawn {
                program: plan.program.clone(),
                source: std::io::Error::other("cannot read the process start time"),
            });
        };
        let spawned = Spawned {
            pid,
            pgid: pid,
            proc_start,
            wine: plan.wineserver.clone(),
        };
        if let Err(e) = on_spawn(&spawned) {
            kill(&mut child);
            return Err(e);
        }
        let log = Arc::new(Log {
            file: Mutex::new(file),
            written: AtomicU64::new(0),
            truncated: AtomicBool::new(false),
        });
        let mut threads = Vec::new();
        if let Some(out) = child.stdout.take() {
            threads.push(log.clone().pump(out));
        }
        if let Some(err) = child.stderr.take() {
            threads.push(log.clone().pump(err));
        }
        let deadline = Instant::now() + req.timeout;
        let is_cancelled = || req.cancel.is_some_and(|c| c.load(Ordering::Relaxed));
        let (mut timed_out, mut cancelled) = (false, false);
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) => {}
                Err(e) => {
                    kill(&mut child);
                    return Err(io_err(&plan.program)(e));
                }
            }
            if Instant::now() >= deadline {
                timed_out = true;
            } else if is_cancelled() {
                cancelled = true;
            }
            if timed_out || cancelled {
                kill(&mut child);
                break None;
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        // Group members that outlive the leader may still hold the pipes;
        // give them the rest of the timeout.
        if !wait_threads(&threads, deadline, req.cancel) {
            if is_cancelled() {
                cancelled = true;
            } else {
                timed_out = true;
            }
        }
        // Let Wine finish on a clean exit, then end everything the run left.
        if let Some((ws, pfx)) = &plan.wineserver {
            if status.is_some() && !timed_out && !cancelled {
                let left = deadline.saturating_duration_since(Instant::now());
                wineserver(ws, pfx, "-w", left.min(WINESERVER_WAIT_TIMEOUT));
            }
        }
        // The leader is reaped by now: once its group is empty the id can be
        // reused, so only signal a group that still has a member of this run.
        if group_alive(Path::new("/proc"), pid, pid, proc_start) {
            kill_group(pid);
        }
        if let Some((ws, pfx)) = &plan.wineserver {
            wineserver_kill(ws, pfx);
        }
        // A process that left the group may still hold a pipe; do not
        // block on its reader thread.
        wait_threads(&threads, Instant::now() + Duration::from_secs(2), None);
        Ok(RunOutcome {
            exit_code: status.and_then(|s| s.code()),
            timed_out,
            cancelled,
            exit_code_reliable: req.spec.exit_code_reliable,
            log_truncated: log.truncated.load(Ordering::Relaxed),
        })
    }
}

/// A scripted change a [`FakeRunner`] applies to the game folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeOp {
    Write {
        path: String,
        contents: Vec<u8>,
    },
    /// Missing files are ignored.
    Remove {
        path: String,
    },
    /// Removes a folder tree; missing is ignored.
    RemoveAll {
        path: String,
    },
    /// Renames when `from` exists.
    Rename {
        from: String,
        to: String,
    },
    Symlink {
        path: String,
        target: String,
    },
    Chmod {
        path: String,
        mode: u32,
    },
}

fn ignore_missing(r: std::io::Result<()>) -> std::io::Result<()> {
    match r {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

/// Test double: validates the command like the real runner, then applies
/// scripted file changes instead of starting anything.
#[derive(Debug, Default)]
pub struct FakeRunner {
    pub ops: Vec<FakeOp>,
    /// Ops used instead of `ops` when the first literal argument matches
    /// (`--install`, `--uninstall`).
    pub scripts: Vec<(String, Vec<FakeOp>)>,
    pub exit_code: i32,
    pub time_out: bool,
    pub(crate) calls: AtomicUsize,
    pub(crate) last: Mutex<Option<CommandPlan>>,
}

impl FakeRunner {
    pub fn new(ops: Vec<FakeOp>) -> Self {
        Self {
            ops,
            ..Self::default()
        }
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }

    pub fn last_command(&self) -> Option<CommandPlan> {
        self.last.lock().ok().and_then(|g| g.clone())
    }

    fn apply(&self, spec: &InstallerSpec, game_dir: &Path) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let first = spec.args.iter().find_map(|a| match a {
            Arg::Lit(s) => Some(s.as_str()),
            _ => None,
        });
        let ops = self
            .scripts
            .iter()
            .find(|(k, _)| Some(k.as_str()) == first)
            .map_or(&self.ops, |(_, ops)| ops);
        for op in ops {
            match op {
                FakeOp::Write { path, contents } => {
                    let p = game_dir.join(path);
                    if let Some(d) = p.parent() {
                        fs::create_dir_all(d)?;
                    }
                    fs::write(p, contents)?;
                }
                FakeOp::Remove { path } => ignore_missing(fs::remove_file(game_dir.join(path)))?,
                FakeOp::RemoveAll { path } => {
                    ignore_missing(fs::remove_dir_all(game_dir.join(path)))?
                }
                FakeOp::Rename { from, to } => {
                    ignore_missing(fs::rename(game_dir.join(from), game_dir.join(to)))?
                }
                FakeOp::Symlink { path, target } => {
                    std::os::unix::fs::symlink(target, game_dir.join(path))?
                }
                FakeOp::Chmod { path, mode } => {
                    fs::set_permissions(game_dir.join(path), fs::Permissions::from_mode(*mode))?
                }
            }
        }
        Ok(())
    }
}

impl ProcessRunner for FakeRunner {
    fn run(
        &self,
        req: &RunRequest<'_>,
        on_spawn: &mut dyn FnMut(&Spawned) -> Result<(), LoaderError>,
    ) -> Result<RunOutcome, LoaderError> {
        let plan = build_command(req)?;
        self.calls.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut g) = self.last.lock() {
            *g = Some(plan);
        }
        on_spawn(&Spawned {
            pid: 4242,
            pgid: 4242,
            proc_start: 1,
            wine: None,
        })?;
        self.apply(req.spec, req.game_dir)
            .map_err(io_err(req.game_dir))?;
        if let Some(d) = req.log_path.parent() {
            fs::create_dir_all(d).map_err(io_err(d))?;
        }
        fs::write(req.log_path, b"fake run\n").map_err(io_err(req.log_path))?;
        Ok(RunOutcome {
            exit_code: (!self.time_out).then_some(self.exit_code),
            timed_out: self.time_out,
            cancelled: false,
            exit_code_reliable: req.spec.exit_code_reliable,
            log_truncated: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::types::InstallerTarget;
    use std::os::unix::fs::PermissionsExt;

    struct Env {
        _tmp: tempfile::TempDir,
        scratch: PathBuf,
        game: PathBuf,
        home: PathBuf,
        log: PathBuf,
    }

    fn env() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let scratch = base.join("scratch");
        let game = base.join("game dir $(touch pwned); `id` 'q' \"dq\"");
        let home = base.join("home");
        for d in [&scratch, &game, &home] {
            fs::create_dir_all(d).unwrap();
        }
        Env {
            log: base.join("logs/run.log"),
            _tmp: tmp,
            scratch,
            game,
            home,
        }
    }

    fn spec(rel: &str, kind: TargetKind, args: Vec<Arg>, host: Host) -> InstallerSpec {
        InstallerSpec {
            target: InstallerTarget {
                rel_path: rel.into(),
                kind,
            },
            args,
            host,
            exit_code_reliable: true,
            private_tmp: false,
        }
    }

    fn req<'a>(e: &'a Env, s: &'a InstallerSpec, timeout: Duration) -> RunRequest<'a> {
        RunRequest {
            spec: s,
            scratch: &e.scratch,
            game_dir: &e.game,
            home: &e.home,
            lang: None,
            caller_path: None,
            timeout,
            log_path: &e.log,
            cancel: None,
            tmp_dir: None,
        }
    }

    fn script(e: &Env, name: &str, body: &str) {
        let p = e.scratch.join(name);
        fs::write(&p, body).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A Proton host whose proton and SLR files exist and pass the trust
    /// re-check (`<base>/lib/steamapps/...`).
    fn proton_host(e: &Env) -> Host {
        let lib = e.home.parent().unwrap().join("lib");
        let proton = lib.join("steamapps/common/Proton 10.0/proton");
        let slr = lib.join("steamapps/common/SteamLinuxRuntime_sniper/run");
        for f in [&proton, &slr] {
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, "").unwrap();
        }
        for d in walkdir::WalkDir::new(&lib).into_iter().flatten() {
            fs::set_permissions(d.path(), fs::Permissions::from_mode(0o755)).unwrap();
        }
        Host::Proton(ProtonEnv {
            proton,
            compat_data: lib.join("steamapps/compatdata/413150"),
            client_root: "/home/u/.local/share/Steam".into(),
            app_id: 413150,
            slr: Some(slr),
        })
    }

    fn run_ok(e: &Env, s: &InstallerSpec) -> (RunOutcome, Option<Spawned>) {
        let mut seen = None;
        let out = RealRunner::default()
            .run(&req(e, s, Duration::from_secs(20)), &mut |sp| {
                seen = Some(sp.clone());
                Ok(())
            })
            .unwrap();
        (out, seen)
    }

    /// Catches: a game path with shell syntax being split or evaluated
    /// instead of passed as one argv element.
    #[test]
    fn hostile_game_path_is_one_argv_element() {
        let e = env();
        script(
            &e,
            "args.sh",
            "#!/bin/bash\nprintf '%s\\0' \"$@\" > args.out\n",
        );
        let s = spec(
            "args.sh",
            TargetKind::BashScript,
            vec![Arg::Lit("--game-path".into()), Arg::GamePath],
            Host::Native,
        );
        let (out, _) = run_ok(&e, &s);
        assert_eq!(out.exit_code, Some(0));
        let got = fs::read(e.scratch.join("args.out")).unwrap();
        let parts: Vec<&[u8]> = got.split(|b| *b == 0).filter(|p| !p.is_empty()).collect();
        assert_eq!(
            parts,
            [
                b"--game-path".as_slice(),
                e.game.to_str().unwrap().as_bytes()
            ]
        );
        assert!(!e.scratch.join("pwned").exists() && !e.game.join("pwned").exists());
    }

    /// Catches: cmd metacharacters reaching cmd.exe, and the cmd line
    /// changing shape for a safe path.
    #[test]
    fn bat_args_are_checked_and_cmd_line_is_exact() {
        let e = env();
        fs::create_dir_all(e.scratch.join("SMAPI 4.5.2 installer")).unwrap();
        fs::write(
            e.scratch
                .join("SMAPI 4.5.2 installer/install on Windows.bat"),
            "",
        )
        .unwrap();
        let rel = "SMAPI 4.5.2 installer/install on Windows.bat";
        for bad in [
            "a&b", "a|b", "a<b", "a>b", "a^b", "%PATH%", "a!b", "a\"b", "(a)", "a\rb", "a\nb",
        ] {
            let s = spec(
                rel,
                TargetKind::BatScript,
                vec![Arg::Lit(bad.into())],
                proton_host(&e),
            );
            assert!(
                matches!(
                    build_command(&req(&e, &s, DEFAULT_TIMEOUT)),
                    Err(LoaderError::UnsafeForCmd(_))
                ),
                "{bad:?}"
            );
            let fake = FakeRunner::default();
            assert!(fake
                .run(&req(&e, &s, DEFAULT_TIMEOUT), &mut |_| Ok(()))
                .is_err());
            assert_eq!(fake.calls(), 0, "rejected before spawn");
        }
        // The hostile game dir holds quotes and backticks: rejected as well.
        let s = spec(
            rel,
            TargetKind::BatScript,
            vec![Arg::GamePathWin],
            proton_host(&e),
        );
        assert!(matches!(
            build_command(&req(&e, &s, DEFAULT_TIMEOUT)),
            Err(LoaderError::UnsafeForCmd(_))
        ));

        let safe_game = e.home.join("Stardew Valley");
        fs::create_dir_all(&safe_game).unwrap();
        let s = spec(
            rel,
            TargetKind::BatScript,
            vec![
                Arg::Lit("--install".into()),
                Arg::Lit("--game-path".into()),
                Arg::GamePathWin,
            ],
            proton_host(&e),
        );
        let Host::Proton(host) = &s.host else {
            unreachable!()
        };
        let (proton, slr) = (
            host.proton.to_str().unwrap().to_string(),
            host.slr.clone().unwrap(),
        );
        let mut r = req(&e, &s, DEFAULT_TIMEOUT);
        r.game_dir = &safe_game;
        let plan = build_command(&r).unwrap();
        let bat = win_path(&e.scratch.join(rel)).unwrap();
        let game = win_path(&safe_game).unwrap();
        assert_eq!(
            plan.cmd_line.as_deref().unwrap(),
            format!("call \"{bat}\" --install --game-path \"{game}\"")
        );
        assert_eq!(plan.program, slr);
        let argv: Vec<String> = plan
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            argv,
            [
                "--",
                proton.as_str(),
                "run",
                "cmd.exe",
                "/c",
                "call",
                bat.as_str(),
                "--install",
                "--game-path",
                game.as_str()
            ]
        );
        assert_eq!(plan.cwd, e.scratch.join("SMAPI 4.5.2 installer"));
        let keys: Vec<&str> = plan.env.iter().map(|(k, _)| k.as_str()).collect();
        assert!(
            keys.contains(&"STEAM_COMPAT_DATA_PATH")
                && keys.contains(&"STEAM_COMPAT_CLIENT_INSTALL_PATH")
        );
        assert!(!keys
            .iter()
            .any(|k| k.starts_with("SteamAppId") || k.starts_with("SteamGameId")));
    }

    /// Catches: a target outside the scratch dir, or reached through a
    /// symlinked folder or file, being executed.
    #[test]
    fn target_must_be_a_real_file_inside_scratch() {
        let e = env();
        script(&e, "ok.sh", "#!/bin/bash\n");
        fs::create_dir_all(e.home.join("real")).unwrap();
        fs::write(e.home.join("real/x.sh"), "").unwrap();
        std::os::unix::fs::symlink(e.home.join("real"), e.scratch.join("linkdir")).unwrap();
        std::os::unix::fs::symlink(e.scratch.join("ok.sh"), e.scratch.join("link.sh")).unwrap();
        assert!(resolve_target(&e.scratch, "ok.sh").is_ok());
        for bad in [
            "../home/real/x.sh",
            "/etc/passwd",
            "linkdir/x.sh",
            "link.sh",
            "missing.sh",
            "",
        ] {
            assert!(
                matches!(
                    resolve_target(&e.scratch, bad),
                    Err(LoaderError::UnsafePath { .. })
                ),
                "{bad}"
            );
        }
    }

    /// Catches: the child inheriting Vessel's env (API keys, tokens). The
    /// secret is planted by re-running this test binary with it set, so
    /// no test mutates the shared process env.
    #[test]
    fn child_env_is_the_allowlist_only() {
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "loaders::runner::tests::env_probe_inner",
                "--ignored",
                "--test-threads=1",
            ])
            .env("NEXUS_API_KEY", "secret")
            .env("VESSEL_ENV_PROBE", "1")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && text.contains("1 passed"), "{text}");
    }

    #[test]
    #[ignore = "run by child_env_is_the_allowlist_only with a planted secret"]
    fn env_probe_inner() {
        if std::env::var_os("VESSEL_ENV_PROBE").is_none() {
            return;
        }
        assert_eq!(std::env::var("NEXUS_API_KEY").as_deref(), Ok("secret"));
        let e = env();
        fs::copy("/usr/bin/env", e.scratch.join("env")).unwrap();
        let mut s = spec(
            "env",
            TargetKind::Exe,
            vec![Arg::Lit("-0".into())],
            Host::Native,
        );
        s.private_tmp = true;
        assert!(
            build_command(&req(&e, &s, DEFAULT_TIMEOUT)).is_err(),
            "private_tmp without a dir is refused"
        );
        let tmp = e.home.join("run-tmp");
        fs::create_dir_all(&tmp).unwrap();
        let mut r = req(&e, &s, Duration::from_secs(20));
        r.tmp_dir = Some(&tmp);
        let out = RealRunner::default().run(&r, &mut |_| Ok(())).unwrap();
        assert_eq!(out.exit_code, Some(0));
        let log = fs::read(&e.log).unwrap();
        let tmp_line = format!("TMPDIR={}", tmp.display());
        assert!(log.split(|b| *b == 0).any(|v| v == tmp_line.as_bytes()));
        let names: Vec<String> = log
            .split(|b| *b == 0)
            .filter(|v| !v.is_empty())
            .map(|v| String::from_utf8_lossy(v.split(|b| *b == b'=').next().unwrap()).into_owned())
            .collect();
        assert!(!names.is_empty());
        for n in &names {
            assert!(ENV_ALLOWLIST.contains(&n.as_str()), "unexpected env {n}");
        }
        assert!(!String::from_utf8_lossy(&log).contains("secret"));
    }

    /// Catches: recovery missing a group whose leader is already dead
    /// while a grandchild keeps running (F1).
    #[test]
    fn kill_stale_reaps_a_group_whose_leader_died() {
        let e = env();
        script(
            &e,
            "grp.sh",
            "#!/bin/bash\nsleep 60 &\necho $! > gc.pid\nsleep 60\n",
        );
        let mut leader = Command::new("/bin/bash")
            .arg(e.scratch.join("grp.sh"))
            .current_dir(&e.scratch)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = leader.id();
        let start = proc_start_time(Path::new("/proc"), pid).unwrap();
        let pidfile = e.scratch.join("gc.pid");
        let gc = (0..250)
            .find_map(|_| {
                let v = fs::read_to_string(&pidfile)
                    .ok()
                    .and_then(|t| t.trim().parse::<u32>().ok());
                if v.is_none() {
                    std::thread::sleep(Duration::from_millis(20));
                }
                v
            })
            .expect("grandchild pid");
        leader.kill().unwrap();
        leader.wait().unwrap();
        assert!(group_alive(Path::new("/proc"), pid, pid, start));
        assert!(kill_stale(pid, pid, start, None));
        assert!(dead_soon(gc), "grandchild {gc} survived recovery");
        assert!(!group_alive(Path::new("/proc"), pid, pid, start));
        assert!(!kill_stale(pid, pid, start, None), "nothing left to kill");
    }

    /// Catches: a reused pgid (new process with the old leader's pid) being
    /// killed by recovery.
    #[test]
    fn group_alive_ignores_a_reused_leader_pid() {
        let me = std::process::id();
        let start = proc_start_time(Path::new("/proc"), me).unwrap();
        assert!(group_alive(Path::new("/proc"), me, me, start));
        assert!(!group_alive(Path::new("/proc"), me, me, start + 1));
    }

    /// Catches: lost stdout or stderr, and the spawn callback arriving
    /// after the process already finished.
    #[test]
    fn logs_capture_both_streams_and_spawn_is_reported_first() {
        let e = env();
        script(
            &e,
            "out.sh",
            "#!/bin/bash\nwhile [ ! -e go ]; do :; done\necho to-stdout\necho to-stderr >&2\nexit 3\n",
        );
        let s = spec("out.sh", TargetKind::BashScript, vec![], Host::Native);
        let go = e.scratch.join("go");
        let mut seen = None;
        let out = RealRunner::default()
            .run(&req(&e, &s, Duration::from_secs(20)), &mut |sp| {
                // The script waits for this file, so it is still running.
                assert!(Path::new(&format!("/proc/{}", sp.pid)).exists());
                assert_eq!(
                    proc_start_time(Path::new("/proc"), sp.pid),
                    Some(sp.proc_start)
                );
                seen = Some(sp.clone());
                fs::write(&go, "").unwrap();
                Ok(())
            })
            .unwrap();
        let sp = seen.unwrap();
        assert_eq!(sp.pgid, sp.pid);
        assert_eq!(out.exit_code, Some(3));
        assert_eq!(out.exit_verdict(), Some(false));
        let log = fs::read_to_string(&e.log).unwrap();
        assert!(
            log.contains("to-stdout") && log.contains("to-stderr"),
            "{log}"
        );
    }

    /// Catches: a timeout leaving grandchildren alive.
    #[test]
    fn timeout_kills_the_whole_group() {
        let e = env();
        script(
            &e,
            "hang.sh",
            "#!/bin/bash\nsleep 60 &\necho $! > child.pid\nsleep 60\n",
        );
        let s = spec("hang.sh", TargetKind::BashScript, vec![], Host::Native);
        let started = Instant::now();
        let out = RealRunner::default()
            .run(&req(&e, &s, Duration::from_millis(500)), &mut |_| Ok(()))
            .unwrap();
        assert!(out.timed_out && out.exit_code.is_none());
        assert!(started.elapsed() < Duration::from_secs(10));
        let gc: u32 = fs::read_to_string(e.scratch.join("child.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let gone = (0..100).any(|_| {
            let stat = fs::read_to_string(format!("/proc/{gc}/stat")).unwrap_or_default();
            let dead = stat.is_empty()
                || stat
                    .rsplit(')')
                    .next()
                    .is_some_and(|r| r.trim_start().starts_with('Z'));
            if !dead {
                std::thread::sleep(Duration::from_millis(20));
            }
            dead
        });
        assert!(gone, "grandchild {gc} survived the timeout");
    }

    /// Catches: an advisory exit code being treated as success.
    #[test]
    fn unreliable_exit_code_defers_to_the_plugin() {
        let mut s = spec("x", TargetKind::Exe, vec![], Host::Native);
        s.exit_code_reliable = false;
        let e = env();
        script(&e, "x", "#!/bin/bash\n");
        let out = FakeRunner::default()
            .run(&req(&e, &s, DEFAULT_TIMEOUT), &mut |_| Ok(()))
            .unwrap();
        assert_eq!((out.exit_code, out.exit_verdict()), (Some(0), None));
        let json = r#"{"target":{"rel_path":"a","kind":"exe"},"args":[],"host":{"kind":"native"}}"#;
        let parsed: InstallerSpec = serde_json::from_str(json).unwrap();
        assert!(parsed.exit_code_reliable, "defaults to reliable");
    }

    /// Catches: bash scripts on Proton or bat files on the native host
    /// being started at all.
    #[test]
    fn host_and_kind_must_fit() {
        let e = env();
        script(&e, "a", "");
        for (kind, host) in [
            (TargetKind::BatScript, Host::Native),
            (TargetKind::BashScript, proton_host(&e)),
        ] {
            let s = spec("a", kind, vec![], host);
            assert!(matches!(
                build_command(&req(&e, &s, DEFAULT_TIMEOUT)),
                Err(LoaderError::HostMismatch { .. })
            ));
        }
    }

    #[test]
    fn flatpak_vessel_refuses_proton_runs() {
        let e = env();
        script(&e, "a.exe", "");
        let root = e.home.join("root");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(".flatpak-info"), "").unwrap();
        let s = spec("a.exe", TargetKind::Exe, vec![], proton_host(&e));
        let r = RealRunner { fs_root: root };
        assert!(matches!(
            r.run(&req(&e, &s, DEFAULT_TIMEOUT), &mut |_| Ok(())),
            Err(LoaderError::FlatpakUnsupported)
        ));
    }

    #[test]
    fn wineserver_is_found_in_bin_or_bin_wow64() {
        let tmp = tempfile::tempdir().unwrap();
        let proton = tmp.path().join("proton");
        assert_eq!(wineserver_of(&proton), None);
        fs::create_dir_all(tmp.path().join("files/bin-wow64")).unwrap();
        fs::write(tmp.path().join("files/bin-wow64/wineserver"), "").unwrap();
        assert_eq!(
            wineserver_of(&proton),
            Some(tmp.path().join("files/bin-wow64/wineserver"))
        );
        fs::create_dir_all(tmp.path().join("files/bin")).unwrap();
        fs::write(tmp.path().join("files/bin/wineserver"), "").unwrap();
        assert_eq!(
            wineserver_of(&proton),
            Some(tmp.path().join("files/bin/wineserver"))
        );
    }

    /// Dead or zombie within 2 s.
    fn dead_soon(pid: u32) -> bool {
        (0..100).any(|_| {
            let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            let dead = stat.is_empty()
                || stat
                    .rsplit(')')
                    .next()
                    .is_some_and(|r| r.trim_start().starts_with('Z'));
            if !dead {
                std::thread::sleep(Duration::from_millis(20));
            }
            dead
        })
    }

    /// Catches: a background process the installer leaves behind writing
    /// into the game folder after run() returned (F2).
    #[test]
    fn normal_exit_kills_leftover_group_members() {
        let e = env();
        script(
            &e,
            "bg.sh",
            "#!/bin/bash\n(sleep 0.5; echo late > late.txt) </dev/null >/dev/null 2>&1 &\necho $! > bg.pid\nexit 0\n",
        );
        let s = spec("bg.sh", TargetKind::BashScript, vec![], Host::Native);
        let (out, _) = run_ok(&e, &s);
        assert_eq!((out.exit_code, out.timed_out), (Some(0), false));
        let bg: u32 = fs::read_to_string(e.scratch.join("bg.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(dead_soon(bg), "background member {bg} survived");
        std::thread::sleep(Duration::from_millis(800));
        assert!(!e.scratch.join("late.txt").exists());
    }

    /// Catches: cmd metacharacters in the scratch path itself reaching
    /// cmd.exe (M1), and a symlinked folder inside scratch (M3).
    #[test]
    fn scratch_path_metachars_and_inner_alias_are_rejected() {
        let e = env();
        let odd = e.scratch.join("scr&a%b");
        fs::create_dir_all(&odd).unwrap();
        fs::write(odd.join("install.bat"), "").unwrap();
        let s = spec(
            "scr&a%b/install.bat",
            TargetKind::BatScript,
            vec![],
            proton_host(&e),
        );
        assert!(matches!(
            build_command(&req(&e, &s, DEFAULT_TIMEOUT)),
            Err(LoaderError::UnsafeForCmd(_))
        ));
        fs::create_dir_all(e.scratch.join("real")).unwrap();
        fs::write(e.scratch.join("real/x.bat"), "").unwrap();
        std::os::unix::fs::symlink(e.scratch.join("real"), e.scratch.join("alias")).unwrap();
        assert!(resolve_target(&e.scratch, "real/x.bat").is_ok());
        assert!(matches!(
            resolve_target(&e.scratch, "alias/x.bat"),
            Err(LoaderError::UnsafePath { .. })
        ));
    }

    /// Catches: python3 from a relative or caller PATH entry shadowing
    /// system binaries.
    #[test]
    fn python3_dir_is_absolute_and_appended() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("python3"), "").unwrap();
        let has_system = std::env::split_paths(BASE_PATH).any(|d| d.join("python3").is_file());
        let caller = std::env::join_paths(["rel/bin".into(), tmp.path().to_path_buf()]).unwrap();
        let got = python3_dir(Some(&caller));
        if has_system {
            assert_eq!(got, None);
        } else {
            assert_eq!(got.as_deref(), Some(tmp.path()));
        }
        assert_eq!(python3_dir(Some(OsStr::new("relative"))), None);
    }

    /// Catches: shell-string invocations creeping into the loaders module.
    #[test]
    fn no_shell_string_invocation_in_loaders() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/loaders");
        let needles = [
            concat!("sh", " -c"),
            concat!("\"-", "c\""),
            concat!("bash", " -c"),
        ];
        for f in fs::read_dir(&dir).unwrap().flatten() {
            // Only sources; an editor's temp file can vanish mid-scan.
            if f.path().extension().is_none_or(|x| x != "rs") {
                continue;
            }
            let Ok(text) = fs::read_to_string(f.path()) else {
                continue;
            };
            for n in needles {
                assert!(!text.contains(n), "{} contains {n:?}", f.path().display());
            }
        }
    }
}
