// SPDX-License-Identifier: GPL-3.0-or-later
//! The "Add to Steam" shortcut: modman as a non-Steam game, written to
//! Steam's per-account binary `userdata/<id>/config/shortcuts.vdf`.
//!
//! Steam reads the file at start-up, so a running Steam only shows a change
//! after a restart. It may also rewrite the file from memory when it exits;
//! the status reports whether it is running so the UI can say so.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{fail, R};
use crate::games::steam::STEAM_ROOT_CANDIDATES;

/// The shortcut's name in Steam, which also identifies our entry.
const APP_NAME: &str = "modman";

const T_OBJ: u8 = 0;
const T_STR: u8 = 1;
const T_INT: u8 = 2;
const T_END: u8 = 8;

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Obj(Vec<(String, Node)>),
    Str(String),
    Int(u32),
}

/// What Settings shows.
#[derive(Debug, Clone, Serialize)]
pub struct ShortcutStatus {
    /// Steam accounts found on this machine (shortcuts are per account).
    pub accounts: usize,
    /// Accounts that already have the modman shortcut.
    pub added: usize,
    /// The Steam client is running: a change shows after Steam restarts.
    pub steam_running: bool,
    /// What the shortcut starts.
    pub exe: PathBuf,
}

fn bad(file: &Path, what: &str) -> super::AppError {
    super::AppError::Msg(format!("cannot read {}: {what}", file.display()))
}

fn cstr(b: &[u8], i: &mut usize, file: &Path) -> R<String> {
    let end = b[*i..]
        .iter()
        .position(|&c| c == 0)
        .ok_or_else(|| bad(file, "unterminated string"))?;
    let s = String::from_utf8_lossy(&b[*i..*i + end]).into_owned();
    *i += end + 1;
    Ok(s)
}

/// The entries of an object up to its end marker.
fn parse_obj(b: &[u8], i: &mut usize, file: &Path) -> R<Vec<(String, Node)>> {
    let mut out = Vec::new();
    loop {
        let t = *b.get(*i).ok_or_else(|| bad(file, "unexpected end"))?;
        *i += 1;
        if t == T_END {
            return Ok(out);
        }
        let key = cstr(b, i, file)?;
        let node = match t {
            T_OBJ => Node::Obj(parse_obj(b, i, file)?),
            T_STR => Node::Str(cstr(b, i, file)?),
            T_INT => {
                let v = b
                    .get(*i..*i + 4)
                    .ok_or_else(|| bad(file, "unexpected end"))?;
                *i += 4;
                Node::Int(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
            }
            // Never rewrite a file holding values this does not understand.
            t => return Err(bad(file, &format!("unsupported value type {t}"))),
        };
        out.push((key, node));
    }
}

fn parse(b: &[u8], file: &Path) -> R<Vec<(String, Node)>> {
    // Steam writes a 13-byte skeleton for an account with no shortcuts; an
    // empty file is treated the same.
    if b.is_empty() {
        return Ok(Vec::new());
    }
    let mut i = 0;
    if b.first() != Some(&T_OBJ) {
        return Err(bad(file, "not a shortcuts file"));
    }
    i += 1;
    if cstr(b, &mut i, file)? != "shortcuts" {
        return Err(bad(file, "not a shortcuts file"));
    }
    parse_obj(b, &mut i, file)
}

fn write_obj(out: &mut Vec<u8>, entries: &[(String, Node)]) {
    for (k, n) in entries {
        match n {
            Node::Obj(e) => {
                out.push(T_OBJ);
                out.extend(k.as_bytes());
                out.push(0);
                write_obj(out, e);
            }
            Node::Str(s) => {
                out.push(T_STR);
                out.extend(k.as_bytes());
                out.push(0);
                out.extend(s.as_bytes());
                out.push(0);
            }
            Node::Int(v) => {
                out.push(T_INT);
                out.extend(k.as_bytes());
                out.push(0);
                out.extend(v.to_le_bytes());
            }
        }
    }
    out.push(T_END);
}

fn serialize(entries: &[(String, Node)]) -> Vec<u8> {
    let mut out = vec![T_OBJ];
    out.extend(b"shortcuts\0");
    write_obj(&mut out, entries);
    out.push(T_END);
    out
}

/// CRC-32 (IEEE), as Steam uses for a shortcut's id.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn is_ours(n: &Node) -> bool {
    matches!(n, Node::Obj(f) if f.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("appname") && matches!(v, Node::Str(s) if s == APP_NAME)
    }))
}

fn entry(exe: &Path) -> Node {
    let quoted = format!("\"{}\"", exe.display());
    let dir = exe.parent().unwrap_or(Path::new("/"));
    let id = crc32(format!("{quoted}{APP_NAME}").as_bytes()) | 0x8000_0000;
    let s = |k: &str, v: String| (k.to_string(), Node::Str(v));
    let n = |k: &str, v: u32| (k.to_string(), Node::Int(v));
    Node::Obj(vec![
        n("appid", id),
        s("AppName", APP_NAME.into()),
        s("Exe", quoted),
        s("StartDir", format!("\"{}\"", dir.display())),
        s("icon", String::new()),
        s("ShortcutPath", String::new()),
        s("LaunchOptions", String::new()),
        n("IsHidden", 0),
        n("AllowDesktopConfig", 1),
        n("AllowOverlay", 1),
        n("OpenVR", 0),
        n("Devkit", 0),
        s("DevkitGameID", String::new()),
        n("DevkitOverrideAppID", 0),
        n("LastPlayTime", 0),
        s("FlatpakAppID", String::new()),
        ("tags".into(), Node::Obj(Vec::new())),
    ])
}

/// `userdata/<account>/config/shortcuts.vdf` of every Steam account.
fn shortcut_files(home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in STEAM_ROOT_CANDIDATES.iter().map(|c| home.join(c)) {
        let Ok(rd) = std::fs::read_dir(root.join("userdata")) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            // "0" is a placeholder Steam keeps; accounts are numeric ids.
            if name == "0" || name.is_empty() || !name.bytes().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let f = e.path().join("config/shortcuts.vdf");
            let Ok(canon) = e.path().canonicalize() else {
                continue;
            };
            if !out.iter().any(|(c, _): &(PathBuf, PathBuf)| *c == canon) {
                out.push((canon, f));
            }
        }
    }
    out.sort();
    out.into_iter().map(|(_, f)| f).collect()
}

/// Whether the Steam client is running (a process named `steam`).
fn steam_running() -> bool {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| c.trim() == "steam"))
}

/// What the shortcut should start: the AppImage when running from one, else
/// this program.
pub fn shortcut_exe(exe: &Path) -> PathBuf {
    super::actions::running_appimage(exe).unwrap_or_else(|| exe.to_path_buf())
}

pub fn status(home: &Path, exe: &Path) -> R<ShortcutStatus> {
    status_with(home, exe, steam_running())
}

fn status_with(home: &Path, exe: &Path, running: bool) -> R<ShortcutStatus> {
    let files = shortcut_files(home);
    let mut added = 0;
    for f in &files {
        let entries = match std::fs::read(f) {
            Ok(b) => parse(&b, f)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(bad(f, &e.to_string())),
        };
        if entries.iter().any(|(_, n)| is_ours(n)) {
            added += 1;
        }
    }
    Ok(ShortcutStatus {
        accounts: files.len(),
        added,
        steam_running: running,
        exe: shortcut_exe(exe),
    })
}

/// Adds the shortcut to (or removes it from) every Steam account.
pub fn set(home: &Path, exe: &Path, enabled: bool) -> R<ShortcutStatus> {
    set_with(home, exe, enabled, steam_running())
}

fn set_with(home: &Path, exe: &Path, enabled: bool, running: bool) -> R<ShortcutStatus> {
    let files = shortcut_files(home);
    if files.is_empty() {
        fail!("No Steam account was found: start Steam and sign in once first.");
    }
    let target = shortcut_exe(exe);
    // Parse everything before writing anything.
    let mut plans = Vec::new();
    for f in &files {
        let mut entries = match std::fs::read(f) {
            Ok(b) => parse(&b, f)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(bad(f, &e.to_string())),
        };
        entries.retain(|(_, n)| !is_ours(n));
        if enabled {
            entries.push((String::new(), entry(&target)));
        }
        // Entries are keyed "0", "1", ... in order.
        for (i, (k, _)) in entries.iter_mut().enumerate() {
            *k = i.to_string();
        }
        plans.push((f, entries));
    }
    for (f, entries) in plans {
        if let Some(dir) = f.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Keep the first original once, in case something goes wrong later.
        let bak = f.with_extension("vdf.modman-bak");
        if f.is_file() && !bak.exists() {
            std::fs::copy(f, &bak)?;
        }
        let tmp = f.with_extension("vdf.modman-tmp");
        std::fs::write(&tmp, serialize(&entries))?;
        std::fs::rename(&tmp, f)?;
    }
    status_with(home, exe, running)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(home: &Path, id: &str) -> PathBuf {
        let d = home
            .join(".local/share/Steam/userdata")
            .join(id)
            .join("config");
        std::fs::create_dir_all(&d).unwrap();
        d.join("shortcuts.vdf")
    }

    // Fails when a file written by Steam is not reproduced byte for byte.
    #[test]
    fn round_trips_a_steam_written_file() {
        let mut b = vec![T_OBJ];
        b.extend(b"shortcuts\0");
        let mut e = Vec::new();
        e.push(T_OBJ);
        e.extend(b"0\0");
        e.push(T_INT);
        e.extend(b"appid\0");
        e.extend(0x8000_0001u32.to_le_bytes());
        e.push(T_STR);
        e.extend(b"AppName\0Game\0");
        e.push(T_OBJ);
        e.extend(b"tags\0");
        e.push(T_STR);
        e.extend(b"0\0fav\0");
        e.push(T_END);
        e.push(T_END);
        b.extend(e);
        b.push(T_END);
        b.push(T_END);
        let p = Path::new("x");
        assert_eq!(serialize(&parse(&b, p).unwrap()), b);
    }

    #[test]
    fn crc32_matches_the_reference_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    // Fails when adding is not idempotent, drops another shortcut, or does
    // not remove cleanly.
    #[test]
    fn adds_once_keeps_others_and_removes() {
        let home = tempfile::tempdir().unwrap();
        let f = account(home.path(), "123");
        std::fs::write(
            &f,
            serialize(&[("0".into(), entry(Path::new("/opt/other")))]),
        )
        .unwrap();
        // Rename the pre-existing one so it is not ours.
        let mut other = parse(&std::fs::read(&f).unwrap(), &f).unwrap();
        if let Node::Obj(fields) = &mut other[0].1 {
            fields[1].1 = Node::Str("Other".into());
        }
        std::fs::write(&f, serialize(&other)).unwrap();

        let exe = Path::new("/usr/bin/modman-desktop");
        let s = set_with(home.path(), exe, true, false).unwrap();
        assert_eq!((s.accounts, s.added), (1, 1));
        set_with(home.path(), exe, true, false).unwrap();
        let entries = parse(&std::fs::read(&f).unwrap(), &f).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].0, "1");
        assert!(f.with_extension("vdf.modman-bak").is_file());

        let s = set_with(home.path(), exe, false, false).unwrap();
        assert_eq!(s.added, 0);
        let entries = parse(&std::fs::read(&f).unwrap(), &f).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!is_ours(&entries[0].1));
    }

    // Fails when there is no account to write to, or a running Steam blocks the change.
    #[test]
    fn needs_an_account_but_not_a_closed_steam() {
        let home = tempfile::tempdir().unwrap();
        let exe = Path::new("/usr/bin/modman-desktop");
        let e = set_with(home.path(), exe, true, false).unwrap_err();
        assert!(e.to_string().contains("No Steam account"), "{e}");
        let f = account(home.path(), "123");
        let s = set_with(home.path(), exe, true, true).unwrap();
        assert!((s.added, s.steam_running) == (1, true));
        assert!(f.exists());
    }

    // Fails when a file with unknown value types would be rewritten.
    #[test]
    fn refuses_files_it_cannot_round_trip() {
        let home = tempfile::tempdir().unwrap();
        let f = account(home.path(), "123");
        let mut b = vec![T_OBJ];
        b.extend(b"shortcuts\0");
        b.push(7);
        b.extend(b"k\0");
        std::fs::write(&f, &b).unwrap();
        let e = set_with(home.path(), Path::new("/x"), true, false).unwrap_err();
        assert!(e.to_string().contains("unsupported"), "{e}");
        assert_eq!(std::fs::read(&f).unwrap(), b);
    }
}
