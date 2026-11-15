// SPDX-License-Identifier: GPL-3.0-or-later
//! User-level `nxm://` handler registration: the desktop entry in
//! `$XDG_DATA_HOME/applications` and the default in
//! `$XDG_CONFIG_HOME/mimeapps.list` (legacy `nxm_register`, L4230; spikes.md
//! §5). Shared by `vessel nxm register|unregister|status` and the migration.

use std::io;
use std::path::{Path, PathBuf};

use crate::ipc::{nxm_exec_line, NXM_DESKTOP_FILE, NXM_SCHEMES};
use crate::paths::Paths;

const DEFAULTS: &str = "Default Applications";

/// `$XDG_DATA_HOME/applications` (the parent of the vessel data dir).
pub fn applications_dir(paths: &Paths) -> PathBuf {
    paths
        .data_dir
        .parent()
        .map_or_else(|| paths.home.join(".local/share"), Path::to_path_buf)
        .join("applications")
}

/// `$XDG_CONFIG_HOME/mimeapps.list` (next to the vessel config dir).
pub fn mimeapps_path(paths: &Paths) -> PathBuf {
    paths
        .config_dir
        .parent()
        .map_or_else(|| paths.home.join(".config"), Path::to_path_buf)
        .join("mimeapps.list")
}

/// Sets `key=value` in `[section]`, replacing an existing entry.
pub fn ini_set(text: &str, section: &str, key: &str, value: &str) -> String {
    let header = format!("[{section}]");
    let mut out: Vec<String> = Vec::new();
    let mut in_sec = false;
    let mut seen_sec = false;
    let mut done = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            if in_sec && !done {
                out.push(format!("{key}={value}"));
                done = true;
            }
            in_sec = t == header;
            seen_sec |= in_sec;
        } else if in_sec && t.split_once('=').is_some_and(|(k, _)| k.trim() == key) {
            if !done {
                out.push(format!("{key}={value}"));
                done = true;
            }
            continue;
        }
        out.push(line.to_string());
    }
    if !done {
        if !seen_sec {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(header);
        }
        out.push(format!("{key}={value}"));
    }
    out.join("\n") + "\n"
}

/// Removes `value` from every `key=a;b;` list, dropping emptied entries.
pub fn ini_remove_value(text: &str, value: &str) -> String {
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some((k, v)) = line
            .split_once('=')
            .filter(|_| !line.trim_start().starts_with(['#', '[']))
        {
            if v.split(';').any(|x| x.trim() == value) {
                let rest: Vec<&str> = v
                    .split(';')
                    .map(str::trim)
                    .filter(|x| !x.is_empty() && *x != value)
                    .collect();
                if !rest.is_empty() {
                    out.push(format!("{k}={};", rest.join(";")));
                }
                continue;
            }
        }
        out.push(line.to_string());
    }
    out.join("\n") + "\n"
}

/// Default handler of `scheme` in mimeapps.list text.
pub fn default_handler(text: &str, scheme: &str) -> Option<String> {
    let header = format!("[{DEFAULTS}]");
    let mut in_sec = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_sec = t == header;
        } else if in_sec {
            if let Some((k, v)) = t.split_once('=') {
                if k.trim() == scheme {
                    return v.split(';').next().map(|s| s.trim().to_string());
                }
            }
        }
    }
    None
}

/// `path`, or the file a symlink at `path` points to (so writing keeps a
/// dotfiles-managed link intact).
fn link_target(path: &Path) -> PathBuf {
    let mut p = path.to_path_buf();
    // Bounded: a symlink loop leaves the last link in place.
    for _ in 0..40 {
        match std::fs::read_link(&p) {
            Ok(t) => {
                p = match p.parent() {
                    Some(dir) if t.is_relative() => dir.join(t),
                    _ => t,
                }
            }
            Err(_) => break,
        }
    }
    p
}

/// Atomic replace of the file `path` names, through symlinks.
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let path = &link_target(path);
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other(format!("{} has no parent", path.display())))?;
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("vessel.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Best effort `update-desktop-database <dir>`.
pub fn refresh_desktop_db(dir: &Path) {
    if let Some(u) = crate::app::which("update-desktop-database") {
        let _ = std::process::Command::new(u)
            .arg(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// The desktop entry for `program` (spikes.md §5).
pub fn desktop_entry(program: &str) -> io::Result<String> {
    let exec = nxm_exec_line(program).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the vessel path contains control characters",
        )
    })?;
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=vessel (Nexus Mods)\n\
         Comment=Handle Nexus Mods \"Mod manager download\" links\n{exec}\n\
         Icon=applications-games\nTerminal=false\nNoDisplay=true\n\
         MimeType={};\n",
        NXM_SCHEMES.join(";")
    ))
}

fn read_mimeapps(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Writes the desktop entry for `program` and makes it the default for
/// every nxm scheme. Returns the entry's path.
pub fn register(paths: &Paths, program: &str) -> io::Result<PathBuf> {
    let dir = applications_dir(paths);
    let file = dir.join(NXM_DESKTOP_FILE);
    write_atomic(&file, &desktop_entry(program)?)?;
    let mime = mimeapps_path(paths);
    let mut text = read_mimeapps(&mime);
    for scheme in NXM_SCHEMES {
        text = ini_set(&text, DEFAULTS, scheme, &format!("{NXM_DESKTOP_FILE};"));
    }
    write_atomic(&mime, &text)?;
    refresh_desktop_db(&dir);
    Ok(file)
}

/// Removes the desktop entry and every mimeapps.list reference to it.
pub fn unregister(paths: &Paths) -> io::Result<()> {
    let dir = applications_dir(paths);
    match std::fs::remove_file(dir.join(NXM_DESKTOP_FILE)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let mime = mimeapps_path(paths);
    if let Ok(text) = std::fs::read_to_string(&mime) {
        let new = ini_remove_value(&text, NXM_DESKTOP_FILE);
        if new != text {
            write_atomic(&mime, &new)?;
        }
    }
    refresh_desktop_db(&dir);
    Ok(())
}

/// The handler file exists and is the default for nxm://.
pub fn registered(paths: &Paths) -> bool {
    applications_dir(paths).join(NXM_DESKTOP_FILE).is_file()
        && default_handler(&read_mimeapps(&mimeapps_path(paths)), NXM_SCHEMES[0]).as_deref()
            == Some(NXM_DESKTOP_FILE)
}

/// Replaces a user-level registration of the desktop file `legacy` with
/// ours for `program` (R13). A scheme's default changes only where it is
/// `legacy`; other handlers' defaults stay. `false` = `legacy` was not
/// registered (nothing done). Rerunnable: the old entry goes last.
pub fn replace_legacy(paths: &Paths, legacy: &str, program: &str) -> io::Result<bool> {
    let dir = applications_dir(paths);
    let old = dir.join(legacy);
    if !old.is_file() {
        return Ok(false);
    }
    write_atomic(&dir.join(NXM_DESKTOP_FILE), &desktop_entry(program)?)?;
    let mime = mimeapps_path(paths);
    if let Ok(text) = std::fs::read_to_string(&mime) {
        let mut new = text.clone();
        for scheme in NXM_SCHEMES {
            if default_handler(&new, scheme).as_deref() == Some(legacy) {
                new = ini_set(&new, DEFAULTS, scheme, &format!("{NXM_DESKTOP_FILE};"));
            }
        }
        new = ini_remove_value(&new, legacy);
        if new != text {
            write_atomic(&mime, &new)?;
        }
    }
    std::fs::remove_file(&old)?;
    refresh_desktop_db(&dir);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_set_adds_section_and_replaces_entries() {
        let t = ini_set("", "Default Applications", "a", "x.desktop;");
        assert_eq!(t, "[Default Applications]\na=x.desktop;\n");
        let t = ini_set(
            "[Added Associations]\nb=y;\n[Default Applications]\na=old;\nc=z;\n",
            "Default Applications",
            "a",
            "new;",
        );
        assert_eq!(
            t,
            "[Added Associations]\nb=y;\n[Default Applications]\na=new;\nc=z;\n"
        );
        let t = ini_set(
            "[Default Applications]\nc=z;\n[Other]\n",
            "Default Applications",
            "a",
            "n;",
        );
        assert_eq!(t, "[Default Applications]\nc=z;\na=n;\n[Other]\n");
    }

    #[test]
    fn ini_remove_value_keeps_other_handlers() {
        let t = ini_remove_value(
            "[D]\na=vessel-nxm.desktop;\nb=other.desktop;vessel-nxm.desktop;\nc=x;\n",
            "vessel-nxm.desktop",
        );
        assert_eq!(t, "[D]\nb=other.desktop;\nc=x;\n");
    }

    #[test]
    fn writing_through_a_symlinked_mimeapps_keeps_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles/mimeapps.list");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "[Default Applications]\n").unwrap();
        let link = dir.path().join("mimeapps.list");
        std::os::unix::fs::symlink("dotfiles/mimeapps.list", &link).unwrap();
        write_atomic(&link, "new\n").unwrap();
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new\n");
        let plain = dir.path().join("plain.list");
        write_atomic(&plain, "x\n").unwrap();
        assert_eq!(std::fs::read_to_string(&plain).unwrap(), "x\n");
    }

    #[test]
    fn desktop_entry_matches_the_packaged_one() {
        let e = desktop_entry("/opt/mod man/vessel").unwrap();
        assert!(e.contains("\nExec=\"/opt/mod man/vessel\" nxm %u\n"), "{e}");
        assert!(e.contains("\nMimeType=x-scheme-handler/nxm;x-scheme-handler/nxm-protocol;\n"));
        assert!(e.contains("\nNoDisplay=true\n") && e.contains("\nTerminal=false\n"));
    }

    // Catches: a per-scheme default being read from the wrong key.
    #[test]
    fn default_handler_reads_the_named_scheme() {
        let t = "[Default Applications]\nx-scheme-handler/nxm=a.desktop;\n\
                 x-scheme-handler/nxm-protocol=b.desktop;c.desktop;\n";
        assert_eq!(
            default_handler(t, NXM_SCHEMES[1]).as_deref(),
            Some("b.desktop")
        );
        assert_eq!(default_handler("[Other]\nx=y;\n", NXM_SCHEMES[0]), None);
    }
}
