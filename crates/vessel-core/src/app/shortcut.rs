// SPDX-License-Identifier: GPL-3.0-or-later
//! The "Add to Steam" shortcut: vessel as a non-Steam game, written to
//! Steam's per-account binary `userdata/<id>/config/shortcuts.vdf`.
//!
//! Steam reads the file at start-up, so a running Steam only shows a change
//! after a restart. It may also rewrite the file from memory when it exits;
//! the status reports whether it is running so the UI can say so.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{fail, R};
use crate::games::steam::STEAM_ROOT_CANDIDATES;
use crate::legacy::LEGACY_STEAM_APP_NAME;

/// The shortcut's name in Steam, which also identifies our entry (as does
/// the legacy name, R14).
const APP_NAME: &str = "Vessel";

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
    /// Accounts that already have the vessel shortcut.
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

fn app_name_is(n: &Node, name: &str) -> bool {
    matches!(n, Node::Obj(f) if f.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("appname") && matches!(v, Node::Str(s) if s == name)
    }))
}

fn is_ours(n: &Node) -> bool {
    app_name_is(n, APP_NAME) || app_name_is(n, LEGACY_STEAM_APP_NAME)
}

fn quoted_exe(exe: &Path) -> (String, String) {
    let dir = exe.parent().unwrap_or(Path::new("/"));
    (
        format!("\"{}\"", exe.display()),
        format!("\"{}\"", dir.display()),
    )
}

fn entry(exe: &Path) -> Node {
    let (quoted, start_dir) = quoted_exe(exe);
    let id = crc32(format!("{quoted}{APP_NAME}").as_bytes()) | 0x8000_0000;
    let s = |k: &str, v: String| (k.to_string(), Node::Str(v));
    let n = |k: &str, v: u32| (k.to_string(), Node::Int(v));
    Node::Obj(vec![
        n("appid", id),
        s("AppName", APP_NAME.into()),
        s("Exe", quoted),
        s("StartDir", start_dir),
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

/// Points an existing entry at `exe` under our name, in place: its appid
/// (and with it playtime, controller config and art) is kept (R14).
fn update_in_place(n: &mut Node, exe: &Path) {
    let Node::Obj(fields) = n else { return };
    let (quoted, start_dir) = quoted_exe(exe);
    for (key, value) in [
        ("AppName", APP_NAME.to_string()),
        ("Exe", quoted),
        ("StartDir", start_dir),
    ] {
        match fields.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
            Some((_, v)) => *v = Node::Str(value),
            None => fields.push((key.to_string(), Node::Str(value))),
        }
    }
}

/// Our shortcut in `entries`, exactly once: the first existing one (Vessel
/// or legacy name) updated in place, else a new entry; other copies dropped.
fn upsert(entries: &mut Vec<(String, Node)>, exe: &Path) {
    let mut found = false;
    entries.retain_mut(|(_, n)| {
        if !is_ours(n) {
            return true;
        }
        if found {
            return false;
        }
        found = true;
        update_in_place(n, exe);
        true
    });
    if !found {
        entries.push((String::new(), entry(exe)));
    }
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
        if enabled {
            upsert(&mut entries, &target);
        } else {
            entries.retain(|(_, n)| !is_ours(n));
        }
        plans.push((f.clone(), entries));
    }
    write_all(plans)?;
    status_with(home, exe, running)
}

/// Writes each file (keys renumbered), backing up its first original once.
fn write_all(plans: Vec<(PathBuf, Vec<(String, Node)>)>) -> R<()> {
    for (f, mut entries) in plans {
        // Entries are keyed "0", "1", ... in order.
        for (i, (k, _)) in entries.iter_mut().enumerate() {
            *k = i.to_string();
        }
        if let Some(dir) = f.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Keep the first original once, in case something goes wrong later.
        let bak = f.with_extension("vdf.vessel-bak");
        if f.is_file() && !bak.exists() {
            std::fs::copy(&f, &bak)?;
        }
        let tmp = f.with_extension("vdf.vessel-tmp");
        std::fs::write(&tmp, serialize(&entries))?;
        std::fs::rename(&tmp, &f)?;
    }
    Ok(())
}

/// Result of [`migrate_legacy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyMigrated {
    /// Accounts whose legacy entry was renamed.
    pub accounts: usize,
    /// Accounts whose legacy entry was left as is: no Vessel app to point
    /// it at.
    pub left: usize,
    /// Steam was running: it shows the change after a restart.
    pub steam_running: bool,
}

/// Renames the legacy shortcut (`AppName` [`LEGACY_STEAM_APP_NAME`]) of
/// every account to ours in place, keeping its appid and art (R14), and
/// points it at `exe`. Accounts without one are left alone. Without `exe`
/// (the app is not installed) legacy entries are counted in `left`.
pub fn migrate_legacy(home: &Path, exe: Option<&Path>) -> R<LegacyMigrated> {
    migrate_legacy_with(home, exe, steam_running)
}

fn migrate_legacy_with(
    home: &Path,
    exe: Option<&Path>,
    running: fn() -> bool,
) -> R<LegacyMigrated> {
    let mut plans = Vec::new();
    let mut left = 0;
    for f in shortcut_files(home) {
        let mut entries = match std::fs::read(&f) {
            Ok(b) => parse(&b, &f)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(bad(&f, &e.to_string())),
        };
        if !entries
            .iter()
            .any(|(_, n)| app_name_is(n, LEGACY_STEAM_APP_NAME))
        {
            continue;
        }
        let Some(exe) = exe else {
            left += 1;
            continue;
        };
        upsert(&mut entries, &shortcut_exe(exe));
        plans.push((f, entries));
    }
    let accounts = plans.len();
    write_all(plans)?;
    Ok(LegacyMigrated {
        accounts,
        left,
        steam_running: accounts > 0 && running(),
    })
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

        let exe = Path::new("/usr/bin/vessel-desktop");
        let s = set_with(home.path(), exe, true, false).unwrap();
        assert_eq!((s.accounts, s.added), (1, 1));
        set_with(home.path(), exe, true, false).unwrap();
        let entries = parse(&std::fs::read(&f).unwrap(), &f).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].0, "1");
        assert!(f.with_extension("vdf.vessel-bak").is_file());

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
        let exe = Path::new("/usr/bin/vessel-desktop");
        let e = set_with(home.path(), exe, true, false).unwrap_err();
        assert!(e.to_string().contains("No Steam account"), "{e}");
        let f = account(home.path(), "123");
        let s = set_with(home.path(), exe, true, true).unwrap();
        assert!((s.added, s.steam_running) == (1, true));
        assert!(f.exists());
    }

    fn legacy_entry(appid: u32) -> Node {
        let mut n = entry(Path::new("/old/legacy-desktop"));
        if let Node::Obj(f) = &mut n {
            f[0].1 = Node::Int(appid);
            f[1].1 = Node::Str(LEGACY_STEAM_APP_NAME.into());
        }
        n
    }

    /// `(appid, AppName, Exe)` of every entry.
    fn summary(f: &Path) -> Vec<(u32, String, String)> {
        let field = |n: &Node, key: &str| match n {
            Node::Obj(fs) => fs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.clone()),
            _ => None,
        };
        parse(&std::fs::read(f).unwrap(), f)
            .unwrap()
            .iter()
            .map(|(_, n)| {
                let Some(Node::Int(id)) = field(n, "appid") else {
                    panic!("no appid")
                };
                let s = |k| match field(n, k) {
                    Some(Node::Str(s)) => s,
                    _ => String::new(),
                };
                (id, s("AppName"), s("Exe"))
            })
            .collect()
    }

    const LEGACY_ID: u32 = 0x8000_1234;

    /// A legacy shortcut next to someone else's, plus its grid art.
    fn legacy_account(home: &Path) -> (PathBuf, PathBuf) {
        let f = account(home, "123");
        let mut other = entry(Path::new("/opt/other"));
        if let Node::Obj(fields) = &mut other {
            fields[1].1 = Node::Str("Other".into());
        }
        std::fs::write(
            &f,
            serialize(&[("0".into(), other), ("1".into(), legacy_entry(LEGACY_ID))]),
        )
        .unwrap();
        let art = f.with_file_name(format!("grid/{LEGACY_ID}p.png"));
        std::fs::create_dir_all(art.parent().unwrap()).unwrap();
        std::fs::write(&art, b"png").unwrap();
        (f, art)
    }

    // Fails when the legacy shortcut gets a new appid (losing playtime,
    // controller config and art), is duplicated, or a rerun rewrites it.
    #[test]
    fn legacy_shortcut_is_renamed_in_place_keeping_its_appid() {
        let home = tempfile::tempdir().unwrap();
        let (f, art) = legacy_account(home.path());
        let exe = Path::new("/usr/bin/vessel-desktop");
        let r = migrate_legacy_with(home.path(), Some(exe), || false).unwrap();
        assert_eq!(r.accounts, 1);
        let s = summary(&f);
        assert_eq!(s.len(), 2);
        assert_eq!(
            s[1],
            (
                LEGACY_ID,
                APP_NAME.into(),
                "\"/usr/bin/vessel-desktop\"".into()
            )
        );
        assert_eq!(s[0].1, "Other");
        assert_eq!(std::fs::read(&art).unwrap(), b"png");
        assert!(f.with_extension("vdf.vessel-bak").is_file());

        let before = std::fs::read(&f).unwrap();
        let r = migrate_legacy_with(home.path(), Some(exe), || false).unwrap();
        assert_eq!(r.accounts, 0);
        assert_eq!(std::fs::read(&f).unwrap(), before);
    }

    // Fails when "Add to Steam" adds a second entry next to a legacy (or
    // migrated) one instead of reusing it and its appid.
    #[test]
    fn adding_the_shortcut_reuses_the_legacy_entry() {
        let home = tempfile::tempdir().unwrap();
        let (f, _) = legacy_account(home.path());
        let exe = Path::new("/usr/bin/vessel-desktop");
        let st = set_with(home.path(), exe, true, false).unwrap();
        assert_eq!((st.accounts, st.added), (1, 1));
        set_with(home.path(), exe, true, false).unwrap();
        migrate_legacy_with(home.path(), Some(exe), || false).unwrap();
        let ours: Vec<_> = summary(&f)
            .into_iter()
            .filter(|(_, n, _)| n != "Other")
            .collect();
        assert_eq!(ours.len(), 1, "{ours:?}");
        assert_eq!((ours[0].0, ours[0].1.as_str()), (LEGACY_ID, APP_NAME));
    }

    // Fails when a fresh shortcut does not use the Vessel-derived appid.
    #[test]
    fn a_fresh_shortcut_uses_the_vessel_appid() {
        let home = tempfile::tempdir().unwrap();
        let f = account(home.path(), "123");
        let exe = Path::new("/usr/bin/vessel-desktop");
        set_with(home.path(), exe, true, false).unwrap();
        let want = crc32(format!("\"{}\"{APP_NAME}", exe.display()).as_bytes()) | 0x8000_0000;
        assert_eq!(summary(&f)[0].0, want);
    }

    // Fails when an unknown GUI path blocks the migration or writes a
    // legacy entry without a target instead of leaving it for the user.
    #[test]
    fn migration_needs_the_gui_path_only_for_legacy_entries() {
        let home = tempfile::tempdir().unwrap();
        let f = account(home.path(), "1");
        std::fs::write(&f, serialize(&[("0".into(), entry(Path::new("/x")))])).unwrap();
        assert_eq!(
            migrate_legacy_with(home.path(), None, || false)
                .unwrap()
                .accounts,
            0
        );
        let (g, _) = legacy_account(home.path());
        let before = std::fs::read(&g).unwrap();
        let r = migrate_legacy_with(home.path(), None, || false).unwrap();
        assert_eq!((r.accounts, r.left), (0, 1));
        assert_eq!(std::fs::read(&g).unwrap(), before);
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
