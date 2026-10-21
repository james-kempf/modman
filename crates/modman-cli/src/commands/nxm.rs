// SPDX-License-Identifier: GPL-3.0-or-later
//! nxm:// handling: register, unregister, status, <url> (legacy
//! `nxm_register` L4230, `nxm_handle` L4276; routing per spikes.md §5).
//!
//! `nxm <url>` parses the link with the core parser first, then forwards it
//! to a running GUI over D-Bus (5 s timeout). Without a GUI the link is
//! queued in `downloads` and processed inline.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use anyhow::{anyhow, bail, Result};
use clap::Args;
use modman_core::download::{self, DownloadStatus, NxmLink};
use modman_core::games;
use modman_core::ipc;

use super::install::install_nexus;
use super::mods::which;
use crate::context::Ctx;
use crate::nexus as nx;

#[derive(Args, Debug)]
pub struct NxmArgs {
    /// register, unregister, status (default) or an nxm:// link.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

pub fn run(ctx: &mut Ctx, args: NxmArgs) -> Result<()> {
    match args.args.first().map(String::as_str) {
        None | Some("status") => status(ctx),
        Some("register") => register(ctx),
        Some("unregister") => unregister(ctx),
        Some(u) if u.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("nxm://")) => handle(ctx, u),
        Some(_) => bail!("Usage: modman nxm register | unregister | status | <nxm://link>"),
    }
}

fn xdg_dir(var: &str, home: &Path, default: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(default))
}

fn applications_dir(ctx: &Ctx) -> PathBuf {
    xdg_dir("XDG_DATA_HOME", &ctx.paths.home, ".local/share").join("applications")
}

fn mimeapps_path(ctx: &Ctx) -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", &ctx.paths.home, ".config").join("mimeapps.list")
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

/// Default handler of `x-scheme-handler/nxm` in mimeapps.list.
fn default_handler(text: &str) -> Option<String> {
    let mut in_sec = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_sec = t == "[Default Applications]";
        } else if in_sec {
            if let Some((k, v)) = t.split_once('=') {
                if k.trim() == ipc::NXM_SCHEMES[0] {
                    return v.split(';').next().map(|s| s.trim().to_string());
                }
            }
        }
    }
    None
}

fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent", path.display()))?;
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("modman.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn refresh_desktop_db(dir: &Path) {
    if let Some(u) = which("update-desktop-database") {
        let _ = std::process::Command::new(u)
            .arg(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// `$APPIMAGE` under AppImage (current_exe points into a temporary mount).
fn program_path() -> Result<String> {
    if let Some(a) = std::env::var_os("APPIMAGE").filter(|a| !a.is_empty()) {
        return Ok(PathBuf::from(a).display().to_string());
    }
    let exe = std::env::current_exe()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    exe.to_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("the modman path is not valid UTF-8"))
}

/// The desktop entry for `program` (spikes.md §5).
pub fn desktop_entry(program: &str) -> Result<String> {
    let exec = ipc::nxm_exec_line(program)
        .ok_or_else(|| anyhow!("the modman path contains control characters"))?;
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=modman (Nexus Mods)\n\
         Comment=Handle Nexus Mods \"Mod manager download\" links\n{exec}\n\
         Icon=applications-games\nTerminal=false\nNoDisplay=true\n\
         MimeType={};\n",
        ipc::NXM_SCHEMES.join(";")
    ))
}

fn register(ctx: &mut Ctx) -> Result<()> {
    let dir = applications_dir(ctx);
    let file = dir.join(ipc::NXM_DESKTOP_FILE);
    write_atomic(&file, &desktop_entry(&program_path()?)?)?;
    let mime = mimeapps_path(ctx);
    let mut text = std::fs::read_to_string(&mime).unwrap_or_default();
    for scheme in ipc::NXM_SCHEMES {
        text = ini_set(
            &text,
            "Default Applications",
            scheme,
            &format!("{};", ipc::NXM_DESKTOP_FILE),
        );
    }
    write_atomic(&mime, &text)?;
    refresh_desktop_db(&dir);
    ctx.out.success(&format!(
        "modman is now the handler for nxm:// links ({}).",
        file.display()
    ));
    ctx.out.info(
        "\"Mod manager download\" / \"Download with Vortex\" buttons on nexusmods.com now open modman.",
    );
    Ok(())
}

fn unregister(ctx: &mut Ctx) -> Result<()> {
    let dir = applications_dir(ctx);
    match std::fs::remove_file(dir.join(ipc::NXM_DESKTOP_FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    let mime = mimeapps_path(ctx);
    if let Ok(text) = std::fs::read_to_string(&mime) {
        let new = ini_remove_value(&text, ipc::NXM_DESKTOP_FILE);
        if new != text {
            write_atomic(&mime, &new)?;
        }
    }
    refresh_desktop_db(&dir);
    ctx.out.success("nxm:// handler removed.");
    Ok(())
}

/// The handler file exists and is the default for nxm://.
pub fn registered(ctx: &Ctx) -> bool {
    applications_dir(ctx).join(ipc::NXM_DESKTOP_FILE).is_file()
        && std::fs::read_to_string(mimeapps_path(ctx))
            .ok()
            .and_then(|t| default_handler(&t))
            .as_deref()
            == Some(ipc::NXM_DESKTOP_FILE)
}

fn status(ctx: &mut Ctx) -> Result<()> {
    if registered(ctx) {
        ctx.out.success("modman handles nxm:// links.");
    } else {
        ctx.out
            .warn("modman is not the nxm:// handler. Run: modman nxm register");
    }
    Ok(())
}

/// Desktop notification (legacy `notify`, L157-160); best effort.
fn notify(msg: &str) {
    tracing::info!("NOTIFY {msg}");
    if let Some(n) = which("notify-send") {
        let _ = std::process::Command::new(n)
            .args(["-a", "modman", "-i", "applications-games", "modman", msg])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// Hands the link to a running GUI. `Err` = no GUI accepted it.
fn forward_to_gui(url: &str) -> std::result::Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let url = url.to_string();
    let spawned = std::thread::Builder::new()
        .name("nxm-forward".into())
        .spawn(move || {
            let r = (|| -> zbus::Result<()> {
                let conn = zbus::blocking::connection::Builder::session()?
                    .method_timeout(ipc::FORWARD_TIMEOUT)
                    .build()?;
                let argv = vec![ipc::GUI_ARGV0.to_string(), "nxm".to_string(), url];
                let cwd = std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                conn.call_method(
                    Some(ipc::SINGLE_INSTANCE_BUS),
                    ipc::SINGLE_INSTANCE_PATH,
                    Some(ipc::SINGLE_INSTANCE_INTERFACE),
                    ipc::SINGLE_INSTANCE_METHOD,
                    &(argv, cwd),
                )?;
                Ok(())
            })();
            let _ = tx.send(r.map_err(|e| e.to_string()));
        });
    if spawned.is_err() {
        return Err("cannot start the forwarding thread".into());
    }
    rx.recv_timeout(ipc::FORWARD_TIMEOUT + std::time::Duration::from_secs(1))
        .unwrap_or_else(|_| Err("the GUI did not answer in time".into()))
}

fn handle(ctx: &mut Ctx, url: &str) -> Result<()> {
    let link = match NxmLink::parse(url) {
        Ok(l) => l,
        Err(e) => {
            notify("Unrecognised nxm link");
            // The query may hold key/expires: never print it.
            bail!(
                "Unrecognised nxm link {}: {e}",
                url.split('?').next().unwrap_or("")
            );
        }
    };
    tracing::info!("nxm link received: {}", link.redacted());
    match forward_to_gui(url) {
        Ok(()) => {
            ctx.out.success("Link handed to the running modman window.");
            return Ok(());
        }
        Err(e) => tracing::debug!("no GUI to forward to: {e}"),
    }
    let row = download::enqueue_nxm(&ctx.store, &link)?;
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let m = match &link {
        NxmLink::Collection { game, slug, .. } => {
            let msg = format!("Collection {slug} ({game}) queued; open modman to install it (collections arrive with milestone M5).");
            if !interactive {
                notify(&msg);
            }
            ctx.out.info(&msg);
            return Ok(());
        }
        NxmLink::Mod(m) => m.clone(),
    };
    if !interactive {
        ctx.yes = true;
        notify(&format!("Installing mod {} ({})…", m.mod_id, m.game));
    }
    let res = (|| -> Result<bool> {
        let game =
            games::require_game(ctx.store.conn(), &m.game).map_err(crate::context::games_hint)?;
        let nexus = nx::require(ctx)?;
        let done = install_nexus(
            ctx,
            &nexus,
            &game,
            m.mod_id,
            Some(m.file_id),
            m.key.clone(),
            None,
            Some(row),
        )?;
        if done.is_some() {
            super::install::after_change(ctx, &game)?;
        }
        Ok(done.is_some())
    })();
    match res {
        Ok(installed) => {
            if !interactive && installed {
                notify(&format!(
                    "Installed mod {}. Run 'modman deploy' to apply it.",
                    m.mod_id
                ));
            }
            Ok(())
        }
        Err(e) => {
            let still_open = download::get(&ctx.store, row)
                .is_ok_and(|d| !matches!(d.status, DownloadStatus::Done | DownloadStatus::Failed));
            if still_open {
                let _ = download::mark_failed(&ctx.store, row, &format!("{e:#}"));
            }
            if !interactive {
                notify(&format!(
                    "Installing mod {} failed — see: modman log",
                    m.mod_id
                ));
            }
            Err(e)
        }
    }
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
            "[D]\na=modman-nxm.desktop;\nb=other.desktop;modman-nxm.desktop;\nc=x;\n",
            "modman-nxm.desktop",
        );
        assert_eq!(t, "[D]\nb=other.desktop;\nc=x;\n");
    }

    #[test]
    fn desktop_entry_matches_the_packaged_one() {
        let e = desktop_entry("/opt/mod man/modman").unwrap();
        assert!(e.contains("\nExec=\"/opt/mod man/modman\" nxm %u\n"), "{e}");
        assert!(e.contains("\nMimeType=x-scheme-handler/nxm;x-scheme-handler/nxm-protocol;\n"));
        assert!(e.contains("\nNoDisplay=true\n") && e.contains("\nTerminal=false\n"));
    }
}
