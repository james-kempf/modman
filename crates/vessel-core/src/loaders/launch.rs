// SPDX-License-Identifier: GPL-3.0-or-later
//! Steam launch options that loaders need (`"<game>\StardewModdingAPI.exe"
//! %command%`, `WINEDLLOVERRIDES=...`).
//!
//! By default Vessel only shows the string for the user to copy, and the
//! user acknowledges it ([`Store::ack_launch_options`]). [`apply`] writes it
//! into the account's text `userdata/<id>/config/localconfig.vdf`, but only
//! on an explicit call, only while Steam is closed (Steam rewrites the file
//! from memory on exit), and edits only
//! `UserLocalConfigStore > Software > Valve > Steam > apps > <appid> >
//! LaunchOptions`; every other byte of the file is kept.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::{LaunchChange, Runtime};
use crate::games::steam::STEAM_ROOT_CANDIDATES;
use crate::store::{Store, StoreError};

/// Errors of launch-option composition and [`apply`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LaunchError {
    #[error("launch options conflict: {0}")]
    Conflict(String),
    #[error("invalid launch option: {0}")]
    Invalid(String),
    #[error("Steam is running: close Steam first, it rewrites localconfig.vdf when it exits")]
    SteamRunning,
    #[error("{0} changed while Vessel was editing it; nothing was written")]
    Changed(PathBuf),
    #[error("No Steam account was found: start Steam and sign in once first.")]
    NoAccount,
    #[error("unknown Steam account {0}")]
    UnknownAccount(u32),
    #[error("game '{0}' has no launch options to apply")]
    NothingToApply(String),
    #[error("unknown game '{0}'")]
    UnknownGame(String),
    #[error("cannot read {path}: {message}")]
    Vdf { path: PathBuf, message: String },
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl From<rusqlite::Error> for LaunchError {
    fn from(e: rusqlite::Error) -> Self {
        LaunchError::Store(e.into())
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> LaunchError + '_ {
    move |source| LaunchError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn vdf(path: &Path, message: impl Into<String>) -> LaunchError {
    LaunchError::Vdf {
        path: path.to_path_buf(),
        message: message.into(),
    }
}

// ---------------------------------------------------------------- compose

/// Adds `winhttp` (as `winhttp=n,b`) or a full `name=mode` override once.
fn add_dll(d: &str, dlls: &mut Vec<String>) {
    let d = d.trim();
    if d.is_empty() {
        return;
    }
    let d = if d.contains('=') {
        d.to_string()
    } else {
        format!("{d}=n,b")
    };
    if !dlls.contains(&d) {
        dlls.push(d);
    }
}

/// `path` as Wine sees it through the `Z:` drive (`Z:\home\...`).
pub fn wine_path(path: &Path) -> String {
    format!("Z:{}", path.display().to_string().replace('/', "\\"))
}

fn plain_env_name(name: &str) -> bool {
    let mut b = name.bytes();
    b.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && b.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// `s` as one POSIX shell word, double-quoted. Inside double quotes a
/// backslash is literal unless it precedes `$`, `` ` ``, `"`, `\` or the
/// closing quote, so only those are escaped: Wine paths keep single
/// backslashes, as Steam users type them.
fn dquote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '"' | '$' | '`' => {
                out.push('\\');
                out.push(c);
            }
            '\\' => {
                let next = chars.get(i + 1);
                if next.is_none_or(|n| matches!(n, '"' | '$' | '`' | '\\')) {
                    out.push('\\');
                }
                out.push('\\');
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `s` as one shell word: bare when it has only safe characters.
fn word(s: &str) -> String {
    let safe = !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-./,:=+@".contains(&b));
    if safe {
        s.to_string()
    } else {
        dquote(s)
    }
}

fn check_value(what: &str, v: &str) -> Result<(), LaunchError> {
    if v.contains('\n') || v.contains('\r') || v.contains('\0') || v.contains("%command%") {
        return Err(LaunchError::Invalid(format!(
            "{what} contains a newline, NUL or %command%"
        )));
    }
    Ok(())
}

/// The launch-options string for `changes` under `runtime`, or `None` when
/// they change nothing. Shape: `[NAME=value ...] ["<exe>"] %command% [args]`.
/// Env vars are merged (one `WINEDLLOVERRIDES` for every DLL override); two
/// different exes, or two values for one variable, are a conflict. A
/// literal `steam_launch_option` with `%command%` takes the command slot;
/// one without is appended as game arguments.
pub fn compose(
    changes: &[LaunchChange],
    runtime: &Runtime,
    game_dir: &Path,
) -> Result<Option<String>, LaunchError> {
    let mut exe: Option<String> = None;
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    let mut dlls: Vec<String> = Vec::new();
    let mut command: Option<String> = None;
    let mut args: Vec<String> = Vec::new();
    for c in changes {
        let c = c.for_runtime(runtime);
        if let Some(e) = &c.exe {
            check_value("exe", e)?;
            let p = match runtime {
                Runtime::Proton(_) => format!("{}\\{}", wine_path(game_dir), e.replace('/', "\\")),
                _ => game_dir.join(e).display().to_string(),
            };
            match &exe {
                Some(old) if *old != p => {
                    return Err(LaunchError::Conflict(format!(
                        "two programs: {old} and {p}"
                    )))
                }
                _ => exe = Some(p),
            }
        }
        for (k, v) in &c.env {
            if !plain_env_name(k) {
                return Err(LaunchError::Invalid(format!("env name '{k}'")));
            }
            check_value(k, v)?;
            if k == "WINEDLLOVERRIDES" {
                v.split(';').for_each(|d| add_dll(d, &mut dlls));
                continue;
            }
            match env.get(k) {
                Some(old) if old != v => {
                    return Err(LaunchError::Conflict(format!("{k}={old} and {k}={v}")))
                }
                _ => {
                    env.insert(k.clone(), v.clone());
                }
            }
        }
        c.dll_overrides.iter().for_each(|d| add_dll(d, &mut dlls));
        if let Some(lit) = c.steam_launch_option.as_deref().map(str::trim) {
            if lit.contains('\n') || lit.contains('\r') || lit.contains('\0') {
                return Err(LaunchError::Invalid(
                    "launch option contains a newline".into(),
                ));
            }
            if lit.contains("%command%") {
                match &command {
                    Some(old) if old != lit => {
                        return Err(LaunchError::Conflict(format!("'{old}' and '{lit}'")))
                    }
                    _ => command = Some(lit.to_string()),
                }
            } else if !lit.is_empty() && !args.iter().any(|a| a == lit) {
                args.push(lit.to_string());
            }
        }
    }
    if !dlls.is_empty() {
        env.insert("WINEDLLOVERRIDES".into(), dlls.join(";"));
    }
    if exe.is_none() && env.is_empty() && command.is_none() && args.is_empty() {
        return Ok(None);
    }
    let mut parts: Vec<String> = env
        .iter()
        .map(|(k, v)| format!("{k}={}", word(v)))
        .collect();
    match (exe, command) {
        (Some(e), Some(c)) => {
            return Err(LaunchError::Conflict(format!("program {e} and '{c}'")));
        }
        (Some(e), None) => {
            parts.push(dquote(&e));
            parts.push("%command%".into());
        }
        (None, Some(c)) => parts.push(c),
        (None, None) => parts.push("%command%".into()),
    }
    parts.extend(args);
    Ok(Some(parts.join(" ")))
}

// ------------------------------------------------------------ persistence

/// The game's launch options as stored in `games`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LaunchState {
    /// What the active loaders need.
    pub desired: Option<String>,
    /// The string the user confirmed setting by hand.
    pub acked: Option<String>,
    /// The string Vessel last wrote to localconfig.vdf.
    pub applied: Option<String>,
}

impl LaunchState {
    /// The desired string when neither acknowledged nor applied as is.
    pub fn mismatch(&self) -> Option<&str> {
        let d = self.desired.as_deref()?;
        (self.acked.as_deref() != Some(d) && self.applied.as_deref() != Some(d)).then_some(d)
    }
}

impl Store {
    pub fn launch_state(&self, game_id: &str) -> Result<LaunchState, LaunchError> {
        self.conn()
            .query_row(
                "SELECT launch_options_desired, launch_options_acked, launch_options_applied
                 FROM games WHERE id = ?1",
                [game_id],
                |r| {
                    Ok(LaunchState {
                        desired: r.get(0)?,
                        acked: r.get(1)?,
                        applied: r.get(2)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| LaunchError::UnknownGame(game_id.into()))
    }

    fn set_launch_column(
        &self,
        game_id: &str,
        sql: &str,
        v: Option<&str>,
    ) -> Result<(), LaunchError> {
        match self.conn().execute(sql, params![v, game_id])? {
            0 => Err(LaunchError::UnknownGame(game_id.into())),
            _ => Ok(()),
        }
    }

    pub fn set_launch_desired(&self, game_id: &str, v: Option<&str>) -> Result<(), LaunchError> {
        self.set_launch_column(
            game_id,
            "UPDATE games SET launch_options_desired = ?1 WHERE id = ?2",
            v,
        )
    }

    pub fn set_launch_applied(&self, game_id: &str, v: Option<&str>) -> Result<(), LaunchError> {
        self.set_launch_column(
            game_id,
            "UPDATE games SET launch_options_applied = ?1 WHERE id = ?2",
            v,
        )
    }

    /// Records that the user set the current desired string by hand;
    /// returns it.
    pub fn ack_launch_options(&self, game_id: &str) -> Result<Option<String>, LaunchError> {
        let desired = self.launch_state(game_id)?.desired;
        self.set_launch_column(
            game_id,
            "UPDATE games SET launch_options_acked = ?1 WHERE id = ?2",
            desired.as_deref(),
        )?;
        Ok(desired)
    }
}

// ------------------------------------------------------------- text VDF

/// A text VDF string with its byte span (quotes included).
#[derive(Debug)]
struct Tok {
    start: usize,
    end: usize,
    kind: TokKind,
}

#[derive(Debug, PartialEq)]
enum TokKind {
    Open,
    Close,
    Str(String),
}

/// Decodes the VDF escapes `\"`, `\\`, `\n`, `\t`; any other backslash pair
/// is kept as written.
fn unescape(raw: &[u8]) -> String {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\\' && i + 1 < raw.len() {
            match raw[i + 1] {
                b'"' => out.push(b'"'),
                b'\\' => out.push(b'\\'),
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                o => out.extend([b'\\', o]),
            }
            i += 2;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `s` as a quoted VDF string; [`unescape`] reverses it.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn tokenize(b: &[u8], path: &Path) -> Result<Vec<Tok>, LaunchError> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' | b'}' => {
                let kind = if c == b'{' {
                    TokKind::Open
                } else {
                    TokKind::Close
                };
                out.push(Tok {
                    start: i,
                    end: i + 1,
                    kind,
                });
                i += 1;
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                if i >= b.len() {
                    return Err(vdf(path, "unterminated string"));
                }
                i += 1;
                let text = unescape(&b[start + 1..i - 1]);
                out.push(Tok {
                    start,
                    end: i,
                    kind: TokKind::Str(text),
                });
            }
            _ => {
                let start = i;
                while i < b.len() && !b" \t\r\n{}\"".contains(&b[i]) {
                    i += 1;
                }
                // `[$WIN32]`-style conditionals are not keys or values.
                if b[start] != b'[' {
                    let text = unescape(&b[start..i]);
                    out.push(Tok {
                        start,
                        end: i,
                        kind: TokKind::Str(text),
                    });
                }
            }
        }
    }
    Ok(out)
}

#[derive(Debug)]
enum Val {
    Str {
        start: usize,
        end: usize,
        text: String,
    },
    /// Byte offset of the closing `}`.
    Obj { close: usize, entries: Vec<Entry> },
}

#[derive(Debug)]
struct Entry {
    key: String,
    val: Val,
}

fn parse_entries(
    toks: &[Tok],
    i: &mut usize,
    top: bool,
    path: &Path,
) -> Result<(Vec<Entry>, usize), LaunchError> {
    let mut out = Vec::new();
    loop {
        let Some(t) = toks.get(*i) else {
            return if top {
                Ok((out, usize::MAX))
            } else {
                Err(vdf(path, "unexpected end of file"))
            };
        };
        *i += 1;
        let key = match &t.kind {
            TokKind::Close if !top => return Ok((out, t.start)),
            TokKind::Str(k) => k.clone(),
            _ => return Err(vdf(path, format!("unexpected brace at byte {}", t.start))),
        };
        let v = toks
            .get(*i)
            .ok_or_else(|| vdf(path, "unexpected end of file"))?;
        *i += 1;
        let val = match &v.kind {
            TokKind::Str(s) => Val::Str {
                start: v.start,
                end: v.end,
                text: s.clone(),
            },
            TokKind::Open => {
                let (entries, close) = parse_entries(toks, i, false, path)?;
                Val::Obj { close, entries }
            }
            TokKind::Close => return Err(vdf(path, format!("'{key}' has no value"))),
        };
        out.push(Entry { key, val });
    }
}

fn parse_vdf(b: &[u8], path: &Path) -> Result<Vec<Entry>, LaunchError> {
    let toks = tokenize(b, path)?;
    Ok(parse_entries(&toks, &mut 0, true, path)?.0)
}

fn find<'a>(entries: &'a [Entry], key: &str) -> Option<&'a Entry> {
    entries.iter().find(|e| e.key.eq_ignore_ascii_case(key))
}

fn obj<'a>(entries: &'a [Entry], key: &str) -> Option<(&'a [Entry], usize)> {
    match find(entries, key).map(|e| &e.val) {
        Some(Val::Obj { close, entries }) => Some((entries, *close)),
        _ => None,
    }
}

/// The `apps` block of `UserLocalConfigStore > Software > Valve > Steam`
/// (not the other top-level `apps` section).
const APPS_PATH: [&str; 5] = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"];

fn apps_block<'a>(root: &'a [Entry], path: &Path) -> Result<(&'a [Entry], usize), LaunchError> {
    let mut cur = (root, usize::MAX);
    for k in APPS_PATH {
        cur = obj(cur.0, k)
            .ok_or_else(|| vdf(path, format!("no {} section", APPS_PATH.join(" > "))))?;
    }
    Ok(cur)
}

/// `LaunchOptions` of `appid` in a text `localconfig.vdf`.
pub fn read_launch_options(path: &Path, appid: u32) -> Result<Option<String>, LaunchError> {
    let b = std::fs::read(path).map_err(io(path))?;
    let root = parse_vdf(&b, path)?;
    let (apps, _) = apps_block(&root, path)?;
    let Some((app, _)) = obj(apps, &appid.to_string()) else {
        return Ok(None);
    };
    Ok(match find(app, "LaunchOptions").map(|e| &e.val) {
        Some(Val::Str { text, .. }) => Some(text.clone()),
        _ => None,
    })
}

/// Where to insert a new block line before the `}` at `close`, and the
/// indent of the lines inside that block.
fn insert_point(b: &[u8], close: usize) -> (usize, String, bool) {
    let line_start = b[..close]
        .iter()
        .rposition(|&c| c == b'\n')
        .map_or(0, |p| p + 1);
    let lead = &b[line_start..close];
    if lead.iter().all(|c| *c == b' ' || *c == b'\t') {
        let mut indent = String::from_utf8_lossy(lead).into_owned();
        indent.push('\t');
        (line_start, indent, true)
    } else {
        (close, "\t".into(), false)
    }
}

/// `b` with `appid`'s `LaunchOptions` set to `value`; only that value's
/// bytes change (or the missing key/app entry is inserted).
fn set_launch_options(
    b: &[u8],
    appid: u32,
    value: &str,
    path: &Path,
) -> Result<Vec<u8>, LaunchError> {
    let root = parse_vdf(b, path)?;
    let (apps, apps_close) = apps_block(&root, path)?;
    let quoted = escape(value);
    let splice = |at: usize, end: usize, text: &str| {
        let mut out = Vec::with_capacity(b.len() + text.len());
        out.extend_from_slice(&b[..at]);
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(&b[end..]);
        out
    };
    let id = appid.to_string();
    let Some(app) = find(apps, &id) else {
        let (at, i, at_line) = insert_point(b, apps_close);
        let nl = if at_line { "" } else { "\n" };
        let text = format!("{nl}{i}\"{id}\"\n{i}{{\n{i}\t\"LaunchOptions\"\t\t{quoted}\n{i}}}\n");
        return Ok(splice(at, at, &text));
    };
    let Val::Obj { close, entries } = &app.val else {
        return Err(vdf(path, format!("apps > {id} is not a section")));
    };
    match find(entries, "LaunchOptions").map(|e| &e.val) {
        Some(Val::Str { start, end, .. }) => Ok(splice(*start, *end, &quoted)),
        Some(Val::Obj { .. }) => Err(vdf(path, "LaunchOptions is a section")),
        None => {
            let (at, i, at_line) = insert_point(b, *close);
            let nl = if at_line { "" } else { "\n" };
            Ok(splice(
                at,
                at,
                &format!("{nl}{i}\"LaunchOptions\"\t\t{quoted}\n"),
            ))
        }
    }
}

// --------------------------------------------------------------- accounts

/// A Steam account from `config/loginusers.vdf`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SteamAccount {
    /// The `userdata/<account_id>` folder name.
    pub account_id: u32,
    /// SteamID64, as a string (it does not fit a JS number).
    pub steam_id: String,
    pub account_name: String,
    pub persona_name: String,
    pub timestamp: Option<u64>,
    #[serde(skip)]
    most_recent: bool,
    pub localconfig: PathBuf,
}

const STEAM_ID64_BASE: u64 = 76_561_197_960_265_728;

/// Accounts of every Steam root under `home` that have a userdata folder.
pub fn accounts(home: &Path) -> Vec<SteamAccount> {
    let mut seen_roots = Vec::new();
    let mut out: Vec<SteamAccount> = Vec::new();
    for root in STEAM_ROOT_CANDIDATES.iter().map(|c| home.join(c)) {
        let Ok(canon) = root.canonicalize() else {
            continue;
        };
        if seen_roots.contains(&canon) {
            continue;
        }
        seen_roots.push(canon.clone());
        let f = canon.join("config/loginusers.vdf");
        let Ok(b) = std::fs::read(&f) else { continue };
        let root_entries = match parse_vdf(&b, &f) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("skipping {}: {e}", f.display());
                continue;
            }
        };
        let Some((users, _)) = obj(&root_entries, "users") else {
            continue;
        };
        for u in users {
            let Val::Obj { entries, .. } = &u.val else {
                continue;
            };
            let Some(account_id) = u
                .key
                .parse::<u64>()
                .ok()
                .and_then(|id| id.checked_sub(STEAM_ID64_BASE))
                .and_then(|id| u32::try_from(id).ok())
            else {
                continue;
            };
            let dir = canon.join("userdata").join(account_id.to_string());
            if !dir.is_dir() || out.iter().any(|a| a.account_id == account_id) {
                continue;
            }
            let s = |k: &str| match find(entries, k).map(|e| &e.val) {
                Some(Val::Str { text, .. }) => text.clone(),
                _ => String::new(),
            };
            out.push(SteamAccount {
                account_id,
                steam_id: u.key.clone(),
                account_name: s("AccountName"),
                persona_name: s("PersonaName"),
                timestamp: s("timestamp").parse().ok(),
                most_recent: s("MostRecent") == "1",
                localconfig: dir.join("config/localconfig.vdf"),
            });
        }
    }
    out
}

/// The account to preselect: `MostRecent` when Steam still writes it, else
/// the newest `timestamp`, else the only account.
pub fn default_account(accounts: &[SteamAccount]) -> Option<u32> {
    if let Some(a) = accounts.iter().find(|a| a.most_recent) {
        return Some(a.account_id);
    }
    if let Some(a) = accounts
        .iter()
        .filter(|a| a.timestamp.is_some())
        .max_by_key(|a| a.timestamp)
    {
        return Some(a.account_id);
    }
    match accounts {
        [a] => Some(a.account_id),
        _ => None,
    }
}

// ------------------------------------------------------------------ apply

/// What [`apply`] did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ApplyOutcome {
    /// `value` is now the app's LaunchOptions in `path`.
    Applied {
        account_id: u32,
        path: PathBuf,
        value: String,
        /// The pre-edit copy (`None` when the file already had the value).
        backup: Option<PathBuf>,
    },
    /// Several accounts and none chosen: nothing was written. `default` is
    /// the one to preselect.
    ChooseAccount {
        accounts: Vec<SteamAccount>,
        default: Option<u32>,
    },
}

/// Writes the game's desired launch options to `account`'s localconfig.vdf
/// (Steam app `appid`). Without `account` it uses the only account, or
/// returns the list to choose from. With no desired string it clears a
/// value Vessel applied earlier and refuses anything else.
pub fn apply(
    store: &Store,
    home: &Path,
    game_id: &str,
    appid: u32,
    account: Option<u32>,
) -> Result<ApplyOutcome, LaunchError> {
    apply_with(
        store,
        home,
        game_id,
        appid,
        account,
        &mut crate::app::shortcut::steam_running,
    )
}

fn apply_with(
    store: &Store,
    home: &Path,
    game_id: &str,
    appid: u32,
    account: Option<u32>,
    running: &mut dyn FnMut() -> bool,
) -> Result<ApplyOutcome, LaunchError> {
    let state = store.launch_state(game_id)?;
    let all = accounts(home);
    let acct = match account {
        Some(id) => all
            .iter()
            .find(|a| a.account_id == id)
            .ok_or(LaunchError::UnknownAccount(id))?,
        None => match all.as_slice() {
            [] => return Err(LaunchError::NoAccount),
            [a] => a,
            _ => {
                let default = default_account(&all);
                return Ok(ApplyOutcome::ChooseAccount {
                    accounts: all,
                    default,
                });
            }
        },
    };
    if running() {
        return Err(LaunchError::SteamRunning);
    }
    let path = acct.localconfig.clone();
    let before = std::fs::read(&path).map_err(io(&path))?;
    let value = match &state.desired {
        Some(d) => d.clone(),
        None => {
            let current = read_launch_options(&path, appid)?;
            if state.applied.is_none() || current != state.applied {
                return Err(LaunchError::NothingToApply(game_id.into()));
            }
            String::new()
        }
    };
    let after = set_launch_options(&before, appid, &value, &path)?;
    let backup = if after == before {
        None
    } else {
        Some(write_atomic(&path, &before, &after, running)?)
    };
    store.set_launch_applied(game_id, state.desired.as_deref())?;
    Ok(ApplyOutcome::Applied {
        account_id: acct.account_id,
        path,
        value,
        backup,
    })
}

/// Removes the temp file unless defused.
struct TempGuard<'a>(Option<&'a Path>);

impl Drop for TempGuard<'_> {
    fn drop(&mut self) {
        if let Some(p) = self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Backs `path` up, writes `after` to a synced temp file with `path`'s mode,
/// and renames it over `path` only if Steam is still closed and the file
/// still holds `before`. Returns the backup path.
fn write_atomic(
    path: &Path,
    before: &[u8],
    after: &[u8],
    running: &mut dyn FnMut() -> bool,
) -> Result<PathBuf, LaunchError> {
    let backup = path.with_extension("vdf.vessel-bak");
    std::fs::write(&backup, before).map_err(io(&backup))?;
    let perms = std::fs::metadata(path).map_err(io(path))?.permissions();
    let tmp = path.with_extension("vdf.vessel-tmp");
    let mut guard = TempGuard(Some(&tmp));
    {
        let mut f = std::fs::File::create(&tmp).map_err(io(&tmp))?;
        f.write_all(after).map_err(io(&tmp))?;
        f.set_permissions(perms).map_err(io(&tmp))?;
        f.sync_all().map_err(io(&tmp))?;
    }
    // Steam may have started (or written the file) since the first check.
    if running() {
        return Err(LaunchError::SteamRunning);
    }
    if std::fs::read(path).map_err(io(path))? != before {
        return Err(LaunchError::Changed(path.to_path_buf()));
    }
    std::fs::rename(&tmp, path).map_err(io(path))?;
    guard.0 = None;
    if let Some(dir) = path.parent() {
        std::fs::File::open(dir)
            .and_then(|d| d.sync_all())
            .map_err(io(dir))?;
    }
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::ProtonEnv;
    use std::cell::Cell;
    use std::os::unix::fs::PermissionsExt;

    fn proton() -> Runtime {
        Runtime::Proton(ProtonEnv {
            proton: "/p/proton".into(),
            compat_data: "/c".into(),
            client_root: "/s".into(),
            app_id: 413_150,
            slr: None,
        })
    }

    fn exe(e: &str) -> LaunchChange {
        LaunchChange {
            exe: Some(e.into()),
            ..Default::default()
        }
    }

    fn dll(d: &str) -> LaunchChange {
        LaunchChange {
            dll_overrides: vec![d.into()],
            ..Default::default()
        }
    }

    // Fails when the SMAPI Proton option is not the documented form.
    #[test]
    fn smapi_proton_option_is_the_quoted_wine_exe_then_command() {
        let s = compose(
            &[exe("StardewModdingAPI.exe")],
            &proton(),
            Path::new("/home/u/.steam/steamapps/common/Stardew Valley"),
        )
        .unwrap();
        assert_eq!(
            s.as_deref(),
            Some(
                r#""Z:\home\u\.steam\steamapps\common\Stardew Valley\StardewModdingAPI.exe" %command%"#
            )
        );
    }

    // Fails when two loaders' DLL overrides yield two WINEDLLOVERRIDES
    // (the shell keeps only the last) or the `;` is left unquoted.
    #[test]
    fn dll_overrides_merge_into_one_variable() {
        let mut env = LaunchChange::default();
        env.env
            .insert("WINEDLLOVERRIDES".into(), "dinput8=n,b".into());
        let s = compose(
            &[dll("winhttp"), dll("version"), dll("winhttp"), env],
            &proton(),
            Path::new("/g"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            s,
            r#"WINEDLLOVERRIDES="winhttp=n,b;version=n,b;dinput8=n,b" %command%"#
        );
        assert_eq!(s.matches("WINEDLLOVERRIDES").count(), 1);
        // Native: DLL overrides do not apply.
        assert_eq!(
            compose(&[dll("winhttp")], &Runtime::Native, Path::new("/g")).unwrap(),
            None
        );
    }

    // Fails when spaces, quotes, `$` or a trailing backslash break out of
    // the shell word.
    #[test]
    fn quoting_survives_spaces_quotes_and_backslashes() {
        let mut c = exe("bin/Run \"it\".sh");
        c.env.insert("A_B".into(), "x y $HOME `id` \\".into());
        c.env.insert("PLAIN".into(), "1".into());
        let s = compose(&[c], &Runtime::Native, Path::new("/g dir"))
            .unwrap()
            .unwrap();
        assert_eq!(
            s,
            r#"A_B="x y \$HOME \`id\` \\" PLAIN=1 "/g dir/bin/Run \"it\".sh" %command%"#
        );
        // Only the backslashes before `\` and the closing quote are doubled.
        assert_eq!(dquote(r"a\b\\c\"), r#""a\b\\\c\\""#);
    }

    // Fails when conflicting changes are silently resolved or bad env names
    // pass through.
    #[test]
    fn conflicts_and_invalid_names_are_rejected() {
        let g = Path::new("/g");
        assert!(matches!(
            compose(&[exe("a.exe"), exe("b.exe")], &proton(), g),
            Err(LaunchError::Conflict(_))
        ));
        let mut bad = LaunchChange::default();
        bad.env.insert("A;rm".into(), "1".into());
        assert!(matches!(
            compose(&[bad], &proton(), g),
            Err(LaunchError::Invalid(_))
        ));
        let lit = LaunchChange {
            steam_launch_option: Some("-console".into()),
            ..Default::default()
        };
        assert_eq!(
            compose(&[exe("a.exe"), lit], &Runtime::Native, g)
                .unwrap()
                .unwrap(),
            r#""/g/a.exe" %command% -console"#
        );
        assert_eq!(compose(&[], &proton(), g).unwrap(), None);
    }

    const LOCALCONFIG: &str = concat!(
        "\"UserLocalConfigStore\"\n{\n",
        "\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n",
        "\t\t\t\t\"apps\"\n\t\t\t\t{\n",
        "\t\t\t\t\t\"413150\"\n\t\t\t\t\t{\n",
        "\t\t\t\t\t\t\"LastPlayed\"\t\t\"1700000000\"\n",
        "\t\t\t\t\t\t\"LaunchOptions\"\t\t\"old\"\n",
        "\t\t\t\t\t}\n",
        "\t\t\t\t\t\"570\"\n\t\t\t\t\t{\n",
        "\t\t\t\t\t\t\"LaunchOptions\"\t\t\"PROTON_LOG=1 \\\"C:\\\\x y\\\" %command%\"\n",
        "\t\t\t\t\t}\n",
        "\t\t\t\t\t\"620\"\n\t\t\t\t\t{\n",
        "\t\t\t\t\t\t\"playtime\"\t\t\"5\"\n",
        "\t\t\t\t\t}\n",
        "\t\t\t\t}\n",
        "\t\t\t}\n\t\t}\n\t}\n",
        "\t\"apps\"\n\t{\n\t\t\"999\"\n\t\t{\n\t\t\t\"LaunchOptions\"\t\t\"other\"\n\t\t}\n\t}\n",
        "}\n",
    );

    fn edit(b: &str, appid: u32, v: &str) -> String {
        String::from_utf8(set_launch_options(b.as_bytes(), appid, v, Path::new("x")).unwrap())
            .unwrap()
    }

    fn read(b: &str, appid: u32) -> Option<String> {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("localconfig.vdf");
        std::fs::write(&p, b).unwrap();
        read_launch_options(&p, appid).unwrap()
    }

    // Fails when escaped values are misdecoded, or an edit rewrites any
    // byte outside the one LaunchOptions value.
    #[test]
    fn edits_only_the_value_and_round_trips_escapes() {
        assert_eq!(
            read(LOCALCONFIG, 570).as_deref(),
            Some(r#"PROTON_LOG=1 "C:\x y" %command%"#)
        );
        let v = r#""Z:\g\SMAPI.exe" %command%"#;
        let out = edit(LOCALCONFIG, 413_150, v);
        assert_eq!(read(&out, 413_150).as_deref(), Some(v));
        let want = LOCALCONFIG.replace(
            "\"LaunchOptions\"\t\t\"old\"",
            "\"LaunchOptions\"\t\t\"\\\"Z:\\\\g\\\\SMAPI.exe\\\" %command%\"",
        );
        assert_eq!(out, want);
        // Rewriting 570 with its own decoded value reproduces the bytes.
        let same = read(LOCALCONFIG, 570).unwrap();
        assert_eq!(edit(LOCALCONFIG, 570, &same), LOCALCONFIG);
        for s in ["a\\b", "q\"", "tab\tnl\n", "\\\\\""] {
            let e = escape(s);
            assert_eq!(unescape(&e.as_bytes()[1..e.len() - 1]), s);
        }
    }

    // Fails when the key or app entry is not created, keys are matched
    // case-sensitively, or the second top-level apps block is touched.
    #[test]
    fn creates_missing_entries_only_under_software_valve_steam() {
        let out = edit(LOCALCONFIG, 620, "X");
        assert_eq!(read(&out, 620).as_deref(), Some("X"));
        assert!(out.contains("\t\t\t\t\t\t\"playtime\"\t\t\"5\"\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"X\"\n\t\t\t\t\t}"));
        let out = edit(LOCALCONFIG, 999, "Y");
        assert_eq!(read(&out, 999).as_deref(), Some("Y"));
        assert!(out.contains("\t\t\"999\"\n\t\t{\n\t\t\t\"LaunchOptions\"\t\t\"other\"\n"));
        assert!(out.contains("\t\t\t\t\t\"999\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"Y\"\n\t\t\t\t\t}\n\t\t\t\t}"));
        let upper = LOCALCONFIG
            .replace("\"apps\"\n\t\t\t\t{", "\"Apps\"\n\t\t\t\t{")
            .replace(
                "\"LaunchOptions\"\t\t\"old\"",
                "\"launchoptions\"\t\t\"old\"",
            );
        assert_eq!(read(&upper, 413_150).as_deref(), Some("old"));
        assert_eq!(
            read(&edit(&upper, 413_150, "Z"), 413_150).as_deref(),
            Some("Z")
        );
        assert!(edit(&upper, 413_150, "Z").contains("\"launchoptions\"\t\t\"Z\""));
    }

    // ---- apply

    const ID64: u64 = STEAM_ID64_BASE + 1000;

    fn steam(home: &Path, users: &[(u64, Option<u64>)]) -> Vec<PathBuf> {
        let root = home.join(".local/share/Steam");
        std::fs::create_dir_all(root.join("config")).unwrap();
        let mut lu = String::from("\"users\"\n{\n");
        let mut files = Vec::new();
        for (id, ts) in users {
            lu.push_str(&format!(
                "\t\"{id}\"\n\t{{\n\t\t\"AccountName\"\t\t\"a{id}\"\n"
            ));
            if let Some(t) = ts {
                lu.push_str(&format!("\t\t\"timestamp\"\t\t\"{t}\"\n"));
            }
            lu.push_str("\t}\n");
            let cfg = root.join(format!("userdata/{}/config", id - STEAM_ID64_BASE));
            std::fs::create_dir_all(&cfg).unwrap();
            let f = cfg.join("localconfig.vdf");
            std::fs::write(&f, LOCALCONFIG).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
            files.push(f);
        }
        lu.push_str("}\n");
        std::fs::write(root.join("config/loginusers.vdf"), lu).unwrap();
        files
    }

    fn store_with(desired: Option<&str>) -> Store {
        let s = Store::open_in_memory().unwrap();
        let g = crate::games::GameConfig::custom("sv");
        crate::games::save_game(s.conn(), &g).unwrap();
        s.set_launch_desired("sv", desired).unwrap();
        s
    }

    fn dir_names(d: &Path) -> Vec<String> {
        let mut v: Vec<_> = std::fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    // Fails when Apply writes without a backup, loses the file mode, or
    // does not record what it applied.
    #[test]
    fn apply_backs_up_writes_and_records() {
        let home = tempfile::tempdir().unwrap();
        let f = steam(home.path(), &[(ID64, Some(5))]).remove(0);
        let store = store_with(Some("NEW %command%"));
        let r = apply_with(&store, home.path(), "sv", 413_150, None, &mut || false).unwrap();
        let ApplyOutcome::Applied {
            backup: Some(bak),
            account_id,
            ..
        } = r
        else {
            panic!("{r:?}")
        };
        assert_eq!(account_id, 1000);
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), LOCALCONFIG);
        assert_eq!(
            read_launch_options(&f, 413_150).unwrap().as_deref(),
            Some("NEW %command%")
        );
        assert_eq!(
            std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            store.launch_state("sv").unwrap().applied.as_deref(),
            Some("NEW %command%")
        );
        assert_eq!(
            dir_names(f.parent().unwrap()),
            ["localconfig.vdf", "localconfig.vdf.vessel-bak"]
        );
    }

    // Fails when Steam running at the start or at the pre-rename re-check
    // lets the write through or leaves a temp file behind.
    #[test]
    fn steam_running_before_or_at_recheck_aborts_cleanly() {
        let home = tempfile::tempdir().unwrap();
        let f = steam(home.path(), &[(ID64, None)]).remove(0);
        let store = store_with(Some("NEW"));
        for start_running in [true, false] {
            let calls = Cell::new(0);
            let mut running = || {
                calls.set(calls.get() + 1);
                start_running || calls.get() > 1
            };
            let e = apply_with(&store, home.path(), "sv", 413_150, None, &mut running).unwrap_err();
            assert!(matches!(e, LaunchError::SteamRunning), "{e}");
            assert_eq!(calls.get(), if start_running { 1 } else { 2 });
            assert_eq!(std::fs::read_to_string(&f).unwrap(), LOCALCONFIG);
            assert!(!f.with_extension("vdf.vessel-tmp").exists());
            assert_eq!(store.launch_state("sv").unwrap().applied, None);
        }
    }

    // Fails when several accounts lead to a silent pick (or a write)
    // instead of the list with the newest-timestamp account preselected.
    #[test]
    fn several_accounts_without_a_choice_return_the_list() {
        let home = tempfile::tempdir().unwrap();
        let files = steam(home.path(), &[(ID64, Some(10)), (ID64 + 1, Some(20))]);
        let store = store_with(Some("NEW"));
        let r = apply_with(&store, home.path(), "sv", 413_150, None, &mut || false).unwrap();
        let ApplyOutcome::ChooseAccount { accounts, default } = r else {
            panic!("{r:?}")
        };
        assert_eq!(accounts.len(), 2);
        assert_eq!(default, Some(1001));
        for f in &files {
            assert_eq!(std::fs::read_to_string(f).unwrap(), LOCALCONFIG);
        }
        apply_with(&store, home.path(), "sv", 413_150, Some(1001), &mut || {
            false
        })
        .unwrap();
        assert_eq!(
            read_launch_options(&files[1], 413_150).unwrap().as_deref(),
            Some("NEW")
        );
        assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), LOCALCONFIG);
        assert!(matches!(
            apply_with(&store, home.path(), "sv", 413_150, Some(7), &mut || false),
            Err(LaunchError::UnknownAccount(7))
        ));
    }

    // Fails when Apply with nothing desired clears a user's own options.
    #[test]
    fn apply_without_desired_only_clears_what_vessel_applied() {
        let home = tempfile::tempdir().unwrap();
        let f = steam(home.path(), &[(ID64, None)]).remove(0);
        let store = store_with(None);
        assert!(matches!(
            apply_with(&store, home.path(), "sv", 413_150, None, &mut || false),
            Err(LaunchError::NothingToApply(_))
        ));
        store.set_launch_applied("sv", Some("old")).unwrap();
        apply_with(&store, home.path(), "sv", 413_150, None, &mut || false).unwrap();
        assert_eq!(
            read_launch_options(&f, 413_150).unwrap().as_deref(),
            Some("")
        );
        assert_eq!(store.launch_state("sv").unwrap().applied, None);
    }

    // Fails when ack does not store the acked string or a mismatch is
    // reported after the user acknowledged or Vessel applied it.
    #[test]
    fn ack_and_apply_clear_the_mismatch() {
        let store = store_with(Some("A"));
        assert_eq!(store.launch_state("sv").unwrap().mismatch(), Some("A"));
        assert_eq!(
            store.ack_launch_options("sv").unwrap().as_deref(),
            Some("A")
        );
        assert_eq!(
            store.launch_state("sv").unwrap().acked.as_deref(),
            Some("A")
        );
        assert_eq!(store.launch_state("sv").unwrap().mismatch(), None);
        store.set_launch_desired("sv", Some("B")).unwrap();
        assert_eq!(store.launch_state("sv").unwrap().mismatch(), Some("B"));
        store.set_launch_applied("sv", Some("B")).unwrap();
        assert_eq!(store.launch_state("sv").unwrap().mismatch(), None);
        assert!(matches!(
            store.launch_state("nope"),
            Err(LaunchError::UnknownGame(_))
        ));
    }
}
