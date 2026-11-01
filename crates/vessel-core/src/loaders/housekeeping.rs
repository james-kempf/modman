// SPDX-License-Identifier: GPL-3.0-or-later
//! Loader housekeeping: log retention, scratch/tmp/preview/archive cleanup,
//! leftover SMAPI installer dirs in Proton prefixes, and log redaction for
//! display or export.
//!
//! [`run`] is meant for startup (after recovery). It holds the deploy lock,
//! so no loader run is in progress; files a concurrent (unlocked) preview
//! may still be writing are protected by an age grace of one preview TTL.
//! Removal never follows symlinks (the link is removed, not its target) and
//! never leaves `<data>/loaders/`, except for the exact Proton temp glob.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use super::{io_err, LoaderError};
use crate::app::loaders::LoaderEnv;
use crate::games::steam;
use crate::store::{now, DeployLock, Store, StoreError};

/// Runs per install whose logs are kept regardless of age.
pub const KEEP_RUNS: usize = 20;
/// Logs older than this are deleted (unless protected).
pub const MAX_LOG_AGE: Duration = Duration::from_secs(90 * 24 * 3600);
/// Where SMAPI's Windows installer leaves its temp copies, under a prefix.
const PROTON_TEMP: &str = "drive_c/users/steamuser/AppData/Local/Temp";
const PROTON_GLOB_PREFIX: &str = "SMAPI-installer-";
/// Upper bound of Proton temp entries removed per prefix and pass.
const PROTON_MAX: usize = 64;
/// Bytes read per step log by [`read_log`].
const MAX_LOG_BYTES: u64 = 4 << 20;
const REDACTED: &str = "[redacted]";

/// What a housekeeping pass did. Removal failures are collected, not fatal.
#[derive(Debug, Default)]
pub struct Report {
    pub removed: Vec<PathBuf>,
    pub errors: Vec<String>,
    /// Set when the pass did nothing (e.g. the deploy lock was busy).
    pub skipped: Option<String>,
}

impl Report {
    fn remove(&mut self, base: &Path, path: &Path) {
        match remove_within(base, path) {
            Ok(()) => self.removed.push(path.to_path_buf()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => self.errors.push(format!("{}: {e}", path.display())),
        }
    }
}

/// One housekeeping pass. Only store errors are returned.
pub fn run(env: &LoaderEnv, store: &Store) -> Result<Report, LoaderError> {
    run_at(env, store, now())
}

fn run_at(env: &LoaderEnv, store: &Store, now: i64) -> Result<Report, LoaderError> {
    let mut rep = Report::default();
    let Some(_lock) = acquire(&env.lock_path)? else {
        rep.skipped = Some("another Vessel process holds the deploy lock".into());
        return Ok(rep);
    };
    let grace = env.preview_ttl.as_secs() as i64;
    let root = env.data_dir.join("loaders");
    if real_dir(&root) {
        let live = previews(&root, &mut rep, env.preview_ttl.as_secs() as i64, now);
        scratch(&root, &mut rep, &live, grace, now);
        tmp(&root, &mut rep, &live, grace, now);
        runs(&root, store, &mut rep, now)?;
        archives(&root, store, &mut rep, &live, grace, now)?;
    } else if root.symlink_metadata().is_ok() {
        rep.errors
            .push(format!("{} is not a plain folder; skipped", root.display()));
    }
    proton_temp(env, store, &mut rep, grace, now)?;
    Ok(rep)
}

/// The deploy lock, retried briefly (a forked child can hold a copy of the
/// lock's file description until it execs). `None` when it stays busy.
fn acquire(path: &Path) -> Result<Option<DeployLock>, LoaderError> {
    for _ in 0..50 {
        match DeployLock::try_acquire(path) {
            Ok(l) => return Ok(Some(l)),
            Err(StoreError::LockBusy(_)) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(None)
}

/// A directory reached without following a symlink.
fn real_dir(p: &Path) -> bool {
    p.symlink_metadata().is_ok_and(|m| m.is_dir())
}

/// Entries of a plain sub-folder of `root` (empty if missing or a link).
fn entries(dir: &Path) -> Vec<(String, PathBuf)> {
    if !real_dir(dir) {
        return Vec::new();
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
        .collect()
}

fn mtime(p: &Path) -> Option<i64> {
    let m = p.symlink_metadata().ok()?.modified().ok()?;
    Some(m.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// Unknown mtimes count as old: nothing can still be writing them.
fn older_than(p: &Path, secs: i64, now: i64) -> bool {
    mtime(p).is_none_or(|t| t + secs <= now)
}

fn is_hex_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Live previews (id -> archive sha). Expired ones are removed with their
/// scratch and tmp dirs.
fn previews(root: &Path, rep: &mut Report, ttl: i64, now: i64) -> BTreeMap<String, Option<String>> {
    let mut live = BTreeMap::new();
    for (name, path) in entries(&root.join("previews")) {
        let Some(id) = name.strip_suffix(".json").filter(|s| is_hex_id(s)) else {
            continue;
        };
        let rec: Option<serde_json::Value> = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        let created = rec
            .as_ref()
            .and_then(|r| r.get("created_at"))
            .and_then(|v| v.as_i64());
        let expired = match created {
            Some(c) => now >= c + ttl,
            None => older_than(&path, ttl, now),
        };
        if expired {
            rep.remove(root, &path);
            rep.remove(root, &root.join("scratch").join(id));
            rep.remove(root, &root.join("tmp").join(format!("preview-{id}")));
        } else {
            let sha = rec
                .as_ref()
                .and_then(|r| r.get("archive_sha256"))
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            live.insert(id.to_string(), sha);
        }
    }
    live
}

/// Scratch dirs without a live preview (finished, failed or crashed runs).
fn scratch(
    root: &Path,
    rep: &mut Report,
    live: &BTreeMap<String, Option<String>>,
    grace: i64,
    now: i64,
) {
    for (name, path) in entries(&root.join("scratch")) {
        if !live.contains_key(&name) && older_than(&path, grace, now) {
            rep.remove(root, &path);
        }
    }
}

/// Per-run (`<run_id>`) and per-preview (`preview-<id>`) tmp dirs.
fn tmp(
    root: &Path,
    rep: &mut Report,
    live: &BTreeMap<String, Option<String>>,
    grace: i64,
    now: i64,
) {
    for (name, path) in entries(&root.join("tmp")) {
        let of_live = name
            .strip_prefix("preview-")
            .is_some_and(|id| live.contains_key(id));
        if !of_live && older_than(&path, grace, now) {
            rep.remove(root, &path);
        }
    }
}

struct RunRow {
    id: i64,
    install_id: i64,
    started_at: i64,
    log_dir: Option<String>,
    first: bool,
    protected: bool,
}

/// Log retention over `<loaders>/runs/<preview_id>/`.
fn runs(root: &Path, store: &Store, rep: &mut Report, now: i64) -> Result<(), LoaderError> {
    let runs_dir = root.join("runs");
    let mut stmt = store.conn().prepare(
        "SELECT r.id, r.install_id, r.phase, r.started_at, r.exit_code, r.timed_out, r.log_path,
                i.status,
                (SELECT MIN(id) FROM loader_runs WHERE install_id = r.install_id),
                (SELECT MAX(id) FROM loader_runs WHERE install_id = r.install_id)
         FROM loader_runs r JOIN loader_installs i ON i.id = r.install_id
         ORDER BY r.install_id, r.id DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let id: i64 = r.get(0)?;
            let phase: String = r.get(2)?;
            let exit: Option<i64> = r.get(4)?;
            let timed_out: bool = r.get(5)?;
            let log: Option<String> = r.get(6)?;
            let status: String = r.get(7)?;
            let first: i64 = r.get(8)?;
            let last: i64 = r.get(9)?;
            // Failed or interrupted: unfinished, timed out, non-zero exit,
            // or the latest run of an install left failed/interrupted.
            let protected = phase != "done"
                || timed_out
                || exit.is_some_and(|c| c != 0)
                || (id == last && matches!(status.as_str(), "failed" | "interrupted"));
            Ok(RunRow {
                id,
                install_id: r.get(1)?,
                started_at: r.get(3)?,
                // Only dirs directly under runs/ are ours to touch.
                log_dir: log
                    .filter(|l| Path::new(l).parent() == Some(runs_dir.as_path()))
                    .and_then(|l| Path::new(&l).file_name()?.to_str().map(str::to_owned)),
                first: id == first,
                protected,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let referenced: BTreeSet<String> = rows.iter().filter_map(|r| r.log_dir.clone()).collect();
    let cutoff = now - MAX_LOG_AGE.as_secs() as i64;
    let mut rank = 0usize;
    let mut cur = None;
    for row in &rows {
        if cur != Some(row.install_id) {
            cur = Some(row.install_id);
            rank = 0;
        }
        rank += 1;
        let Some(name) = &row.log_dir else { continue };
        if row.protected || (rank <= KEEP_RUNS && row.started_at >= cutoff) {
            continue;
        }
        let dir = runs_dir.join(name);
        if row.first {
            // The first run's pre-manifest is the install's vanilla
            // baseline for rollbacks: drop only the logs.
            for (f, path) in entries(&dir) {
                if f.ends_with(".log") {
                    rep.remove(root, &path);
                }
            }
        } else {
            tracing::debug!(run = row.id, "pruning loader run logs");
            rep.remove(root, &dir);
        }
    }
    for (name, path) in entries(&runs_dir) {
        if !referenced.contains(&name) && older_than(&path, MAX_LOG_AGE.as_secs() as i64, now) {
            rep.remove(root, &path);
        }
    }
    Ok(())
}

/// Cached archives no install row and no live preview references.
fn archives(
    root: &Path,
    store: &Store,
    rep: &mut Report,
    live: &BTreeMap<String, Option<String>>,
    grace: i64,
    now: i64,
) -> Result<(), LoaderError> {
    let archives_dir = root.join("archives");
    let mut keep: BTreeSet<String> = live.values().flatten().cloned().collect();
    let mut stmt = store
        .conn()
        .prepare("SELECT archive_sha256, archive_path FROM loader_installs")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<String>>(1)?,
        ))
    })? {
        let (sha, path) = row?;
        keep.extend(sha);
        if let Some(p) = path {
            let p = Path::new(&p);
            if let Some(d) = p
                .parent()
                .filter(|d| d.parent() == Some(archives_dir.as_path()))
            {
                keep.extend(d.file_name().and_then(|n| n.to_str()).map(str::to_owned));
            }
        }
    }
    drop(stmt);
    for (name, path) in entries(&archives_dir) {
        if !keep.contains(&name) && older_than(&path, grace, now) {
            rep.remove(root, &path);
        }
    }
    Ok(())
}

/// `SMAPI-installer-*` leftovers in the Proton prefixes of games with a
/// loader install (and prefixes loader runs recorded).
fn proton_temp(
    env: &LoaderEnv,
    store: &Store,
    rep: &mut Report,
    grace: i64,
    now: i64,
) -> Result<(), LoaderError> {
    let conn = store.conn();
    let mut prefixes = BTreeSet::new();
    let mut stmt = conn.prepare(
        "SELECT DISTINCT r.wineprefix FROM loader_runs r
         JOIN loader_installs i ON i.id = r.install_id WHERE r.wineprefix IS NOT NULL",
    )?;
    for p in stmt.query_map([], |r| r.get::<_, String>(0))? {
        prefixes.insert(PathBuf::from(p?));
    }
    drop(stmt);
    let mut stmt =
        conn.prepare("SELECT DISTINCT game_id FROM loader_installs WHERE game_id IS NOT NULL")?;
    let games = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    for id in games {
        let appid = crate::games::require_game(conn, &id)
            .ok()
            .and_then(|g| g.appid);
        if let Some(pfx) = appid
            .and_then(|a| steam::find_app(&env.steam.libraries, a))
            .and_then(|a| a.prefix)
        {
            prefixes.insert(pfx);
        }
    }
    for pfx in prefixes {
        let Some(temp) = proton_temp_dir(&pfx) else {
            continue;
        };
        let mut n = 0;
        for (name, path) in entries(&temp) {
            if n >= PROTON_MAX {
                break;
            }
            let Some(rest) = name.strip_prefix(PROTON_GLOB_PREFIX) else {
                continue;
            };
            // Links are removed as links; dirs get the grace in case an
            // installer outside Vessel is still writing them.
            let Ok(ty) = path.symlink_metadata().map(|m| m.file_type()) else {
                continue;
            };
            let ours = ty.is_symlink() || (ty.is_dir() && older_than(&path, grace, now));
            if !rest.is_empty() && ours {
                rep.remove(&temp, &path);
                n += 1;
            }
        }
    }
    Ok(())
}

/// `<pfx>/drive_c/users/steamuser/AppData/Local/Temp` when every component
/// below the prefix is a plain folder.
fn proton_temp_dir(pfx: &Path) -> Option<PathBuf> {
    if !pfx.is_absolute() || pfx.components().any(|c| c == Component::ParentDir) {
        return None;
    }
    let mut p = pfx.to_path_buf();
    if !p.is_dir() {
        return None;
    }
    for part in PROTON_TEMP.split('/') {
        p.push(part);
        if !real_dir(&p) {
            return None;
        }
    }
    Some(p)
}

/// Removes `path` (a strict descendant of `base`) and everything under it
/// without following symlinks. Read-only dirs are made writable first.
fn remove_within(base: &Path, path: &Path) -> io::Result<()> {
    let inside = path != base
        && path.starts_with(base)
        && !path.components().any(|c| c == Component::ParentDir);
    if !inside {
        return Err(io::Error::other(format!(
            "refusing to remove {} outside {}",
            path.display(),
            base.display()
        )));
    }
    remove_tree(path)
}

fn remove_tree(path: &Path) -> io::Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() {
        return fs::remove_file(path);
    }
    let mode = m.permissions().mode();
    if mode & 0o700 != 0o700 {
        // lstat said this is a real dir, so chmod hits it, not a link target.
        fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o700))?;
    }
    for e in fs::read_dir(path)? {
        remove_tree(&e?.path())?;
    }
    fs::remove_dir(path)
}

/// A run's step logs (`step-N.log`, in step order), redacted for display
/// or export. `nexus_key` is the configured Nexus API key, if known.
pub fn read_log(
    env: &LoaderEnv,
    store: &Store,
    run_id: i64,
    nexus_key: Option<&str>,
) -> Result<String, LoaderError> {
    let runs_dir = env.data_dir.join("loaders").join("runs");
    let not_found = |p: &Path| LoaderError::Io {
        path: p.to_path_buf(),
        source: io::Error::new(io::ErrorKind::NotFound, format!("no logs for run {run_id}")),
    };
    let run = store
        .loader_run(run_id)?
        .ok_or_else(|| not_found(&runs_dir))?;
    let dir = run
        .log_path
        .map(PathBuf::from)
        .filter(|d| d.parent() == Some(runs_dir.as_path()) && real_dir(d))
        .ok_or_else(|| not_found(&runs_dir))?;
    let mut steps: Vec<(u32, PathBuf)> = entries(&dir)
        .into_iter()
        .filter_map(|(name, path)| {
            let n = name
                .strip_prefix("step-")?
                .strip_suffix(".log")?
                .parse()
                .ok()?;
            path.symlink_metadata().ok()?.is_file().then_some((n, path))
        })
        .collect();
    steps.sort();
    let mut out = String::new();
    for (n, path) in &steps {
        let mut buf = Vec::new();
        fs::File::open(path)
            .and_then(|f| f.take(MAX_LOG_BYTES).read_to_end(&mut buf))
            .map_err(io_err(path))?;
        if steps.len() > 1 {
            out.push_str(&format!("== step {n} ==\n"));
        }
        out.push_str(&String::from_utf8_lossy(&buf));
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    let secrets: Vec<&str> = nexus_key.into_iter().collect();
    Ok(redact_with(&out, &env.home, &secrets))
}

/// Redacts API keys and tokens and replaces `home` with `~`.
pub fn redact(text: &str, home: &Path) -> String {
    redact_with(text, home, &[])
}

/// [`redact`], also replacing each exact `secrets` value (8+ chars).
pub fn redact_with(text: &str, home: &Path, secrets: &[&str]) -> String {
    let mut s = text.to_string();
    for sec in secrets.iter().map(|s| s.trim()).filter(|s| s.len() >= 8) {
        s = s.replace(sec, REDACTED);
    }
    s = redact_patterns(&s);
    let h = home.to_string_lossy();
    let h = h.trim_end_matches('/');
    if !h.is_empty() {
        s = replace_home(&s, h);
    }
    s
}

/// Replaces `home` when followed by `/` or a non-path character.
fn replace_home(s: &str, home: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(home) {
        let after = &rest[i + home.len()..];
        let boundary = after
            .chars()
            .next()
            .is_none_or(|c| c == '/' || !(c.is_alphanumeric() || "._-".contains(c)));
        out.push_str(&rest[..i]);
        out.push_str(if boundary { "~" } else { home });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Keys whose `key: value` / `key=value` value is a secret.
const SECRET_KEYS: &[&str] = &[
    "apikey",
    "api_key",
    "api-key",
    "token",
    "secret",
    "password",
    "authorization",
];

fn is_token_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'_' | b'-')
}

/// End of a `key=value` value starting at `i`.
fn value_end(b: &[u8], i: usize) -> usize {
    let mut j = i;
    while j < b.len()
        && !b[j].is_ascii_whitespace()
        && !matches!(
            b[j],
            b'"' | b'\'' | b',' | b';' | b'&' | b')' | b'}' | b']' | b'>'
        )
    {
        j += 1;
    }
    j
}

fn skip(b: &[u8], mut i: usize, f: impl Fn(u8) -> bool) -> usize {
    while i < b.len() && f(b[i]) {
        i += 1;
    }
    i
}

/// Byte ranges of secret values, then the text with them replaced.
fn redact_patterns(s: &str) -> String {
    let b = s.as_bytes();
    let lower = s.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let mut ranges: Vec<(usize, usize)> = Vec::new();

    // key: value / key = value (quoted keys and values too).
    for key in SECRET_KEYS {
        for (k, _) in lower.match_indices(key) {
            let mut i = skip(b, k + key.len(), |c| matches!(c, b'"' | b'\''));
            i = skip(b, i, |c| c == b' ' || c == b'\t');
            if i >= b.len() || !matches!(b[i], b':' | b'=') {
                continue;
            }
            i = skip(b, i + 1, |c| matches!(c, b' ' | b'\t' | b'"' | b'\''));
            // `Authorization: Bearer x` keeps the scheme word.
            let end = value_end(b, i);
            if matches!(&lb[i..end], b"bearer" | b"basic" | b"token") {
                i = skip(b, end, |c| c == b' ' || c == b'\t');
            }
            let end = value_end(b, i);
            if end > i {
                ranges.push((i, end));
            }
        }
    }
    // Bearer <token>
    for (k, _) in lower.match_indices("bearer") {
        let i = skip(b, k + 6, |c| c == b' ' || c == b'\t');
        if i == k + 6 {
            continue;
        }
        let end = skip(b, i, is_token_char);
        if end > i {
            ranges.push((i, end));
        }
    }
    // Long hex or base64-ish runs.
    let mut i = 0;
    while i < b.len() {
        if !is_token_char(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        i = skip(b, i, is_token_char);
        if looks_like_token(&s[start..i]) {
            ranges.push((start, i));
        }
    }

    ranges.sort();
    let mut out = String::with_capacity(s.len());
    let mut pos = 0;
    for (a, z) in ranges {
        if z <= pos {
            continue;
        }
        let a = a.max(pos);
        out.push_str(&s[pos..a]);
        out.push_str(REDACTED);
        pos = z;
    }
    out.push_str(&s[pos..]);
    out
}

fn looks_like_token(t: &str) -> bool {
    let t = t.trim_matches(|c| c == '-' || c == '_' || c == '/');
    if t.len() >= 32 && t.bytes().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    // Mixed-case alphanumerics with long '/'-free segments: base64 keys,
    // not paths (whose segments are short and mostly lowercase).
    t.len() >= 32
        && t.bytes().any(|c| c.is_ascii_uppercase())
        && t.bytes().any(|c| c.is_ascii_lowercase())
        && t.bytes().any(|c| c.is_ascii_digit())
        && t.split('/').map(str::len).max().unwrap_or(0) >= 20
}

#[cfg(test)]
#[path = "housekeeping_tests.rs"]
mod tests;
