// SPDX-License-Identifier: GPL-3.0-or-later
//! nxm:// handling: register, unregister, status, <url> (legacy
//! `nxm_register` L4230, `nxm_handle` L4276; routing per spikes.md §5).
//!
//! `nxm <url>` parses the link with the core parser first, then forwards it
//! to a running GUI over D-Bus (5 s timeout). Without a GUI the link is
//! queued in `downloads` and processed inline: a mod link installs the
//! file, a collection link installs the (pinned) collection revision.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use anyhow::{anyhow, bail, Result};
use clap::Args;
use modman_core::app::actions;
use modman_core::app::nxm as app;
use modman_core::download::NxmLink;
use modman_core::ipc;

use super::mods::which;
use crate::context::Ctx;

#[derive(Args, Debug)]
pub struct NxmArgs {
    /// register, unregister, status (default) or an nxm:// link.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

pub fn run(ctx: &mut Ctx, args: NxmArgs) -> Result<()> {
    let sub = args.args.first().map(String::as_str);
    // Trailing args swallow `--help`; a subcommand must not run with it.
    if matches!(sub, Some("register" | "unregister" | "status"))
        && args.args[1..].iter().any(|a| a == "--help" || a == "-h")
    {
        return print_help();
    }
    match sub {
        None | Some("status") => status(ctx),
        Some("register") => register(ctx),
        Some("unregister") => unregister(ctx),
        Some(u) if u.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("nxm://")) => handle(ctx, u),
        Some(_) => bail!("Usage: modman nxm register | unregister | status | <nxm://link>"),
    }
}

fn print_help() -> Result<()> {
    use clap::CommandFactory;
    match crate::cli::Cli::command().try_get_matches_from(["modman", "nxm", "--help"]) {
        Err(e) => e.print()?,
        Ok(_) => bail!("Usage: modman nxm register | unregister | status | <nxm://link>"),
    }
    Ok(())
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
fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let path = &link_target(path);
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

/// `$APPIMAGE` when this process runs from the AppImage mount
/// (current_exe points into a temporary mount), else this binary.
fn program_path() -> Result<String> {
    let exe = std::env::current_exe()?;
    if let Some(a) = actions::running_appimage(&exe) {
        return Ok(a.display().to_string());
    }
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
    if let Err(e) = app::parse(url) {
        notify("Unrecognised nxm link");
        // The message never holds the query (key/expires).
        return Err(e.into());
    }
    match forward_to_gui(url) {
        Ok(()) => {
            ctx.out.success("Link handed to the running modman window.");
            return Ok(());
        }
        Err(e) => tracing::debug!("no GUI to forward to: {e}"),
    }
    let accepted = app::accept(&ctx.store, url)?;
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if !interactive {
        // Background run: no prompts (optional mods default to none).
        ctx.yes = true;
    }
    let (start, ok, failed) = match &accepted.link {
        NxmLink::Collection { game, slug, .. } => (
            format!("Installing collection {slug} ({game})…"),
            format!("Installed collection {slug}. Run 'modman deploy' to apply it."),
            format!("Installing collection {slug} failed — see: modman log"),
        ),
        NxmLink::Mod(m) => (
            format!("Installing mod {} ({})…", m.mod_id, m.game),
            format!(
                "Installed mod {}. Run 'modman deploy' to apply it.",
                m.mod_id
            ),
            format!("Installing mod {} failed — see: modman log", m.mod_id),
        ),
    };
    if !interactive {
        notify(&start);
    }
    let ui = ctx.ui();
    let res = app::process(&mut ctx.cx(&ui), &accepted);
    if !interactive {
        match &res {
            // A collection link notifies on every outcome (legacy).
            Ok(p) if p.installed || p.kind == "collection" => notify(&ok),
            Ok(_) => {}
            Err(_) => notify(&failed),
        }
    }
    res.map(|_| ()).map_err(Into::into)
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
        let e = desktop_entry("/opt/mod man/modman").unwrap();
        assert!(e.contains("\nExec=\"/opt/mod man/modman\" nxm %u\n"), "{e}");
        assert!(e.contains("\nMimeType=x-scheme-handler/nxm;x-scheme-handler/nxm-protocol;\n"));
        assert!(e.contains("\nNoDisplay=true\n") && e.contains("\nTerminal=false\n"));
    }
}
