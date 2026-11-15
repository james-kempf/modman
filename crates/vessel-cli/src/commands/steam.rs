// SPDX-License-Identifier: GPL-3.0-or-later
//! `steam add | remove | status`: vessel as a non-Steam game, the same
//! shortcut as the Settings toggle (`vessel_core::app::shortcut`).
//!
//! The shortcut starts the desktop app: the AppImage when this runs from
//! one, else `vessel-desktop` next to this program or on `PATH`, else a
//! Vessel AppImage in the usual folders (a standalone CLI next to an
//! AppImage, as on the Deck).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};
use vessel_core::app::actions;
use vessel_core::app::shortcut::{self, ShortcutStatus};
use vessel_core::migrate::Programs;

use crate::context::Ctx;

/// Folders searched for a Vessel AppImage, relative to home.
const APPIMAGE_DIRS: &[&str] = &[
    "Applications",
    "Documents/Applications",
    "Downloads",
    "Desktop",
    ".local/bin",
    "bin",
];

#[derive(Args, Debug)]
pub struct SteamArgs {
    #[command(subcommand)]
    pub cmd: Option<Sub>,
}

#[derive(Subcommand, Debug)]
pub enum Sub {
    /// Add vessel to every Steam account as a non-Steam game.
    #[command(visible_alias = "on")]
    Add {
        /// Program the shortcut starts, as the host sees it (default: the
        /// AppImage or vessel-desktop). A Flatpak passes its exported
        /// launcher, since the sandbox's own path is not on the host.
        #[arg(long, value_name = "PATH")]
        exe: Option<PathBuf>,
    },
    /// Remove the shortcut from every Steam account.
    #[command(visible_aliases = ["off", "rm"])]
    Remove,
    /// Whether the shortcut is in Steam (default).
    Status,
}

pub fn run(ctx: &mut Ctx, args: SteamArgs) -> Result<()> {
    let home = ctx.paths.home.clone();
    match args.cmd.unwrap_or(Sub::Status) {
        Sub::Status => {
            // Only the reported target needs the desktop app.
            let gui = gui(&home);
            let s = shortcut::status(&home, &or_self(gui.as_deref())?)?;
            print_status(ctx, &s, gui.is_some());
        }
        Sub::Add { exe } => {
            let exe = match exe {
                Some(p) => std::path::absolute(p)?,
                None => gui(&home).ok_or_else(|| {
                    anyhow!(
                        "the desktop app was not found (vessel-desktop on PATH or a vessel*.AppImage in ~/Applications, \
                         ~/Documents/Applications, ~/Downloads, ...); pass it with --exe <path>"
                    )
                })?,
            };
            let s = shortcut::set(&home, &exe, true)?;
            ctx.out
                .success(&format!("Added vessel to Steam ({}).", s.exe.display()));
            restart_hint(ctx, &s);
        }
        Sub::Remove => {
            // Removing never writes a target.
            let s = shortcut::set(&home, &or_self(None)?, false)?;
            ctx.out.success("Removed vessel from Steam.");
            restart_hint(ctx, &s);
        }
    }
    Ok(())
}

/// What the shortcut should start. Inside the AppImage this program is
/// enough: the core maps it to `$APPIMAGE`, which starts the GUI.
fn gui(home: &Path) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if actions::running_appimage(&exe).is_some() {
        return Some(exe);
    }
    Programs::current().gui.or_else(|| find_appimage(home))
}

fn or_self(gui: Option<&Path>) -> Result<PathBuf> {
    match gui {
        Some(p) => Ok(p.to_path_buf()),
        None => Ok(std::env::current_exe()?),
    }
}

/// The newest `vessel*.AppImage` (any case) directly in [`APPIMAGE_DIRS`].
fn find_appimage(home: &Path) -> Option<PathBuf> {
    APPIMAGE_DIRS
        .iter()
        .filter_map(|d| std::fs::read_dir(home.join(d)).ok())
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            name.starts_with("vessel") && name.ends_with(".appimage")
        })
        .filter_map(|e| {
            let meta = std::fs::metadata(e.path()).ok().filter(|m| m.is_file())?;
            Some((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), e.path()))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, p)| p.canonicalize().unwrap_or(p))
}

fn print_status(ctx: &Ctx, s: &ShortcutStatus, known_target: bool) {
    let out = ctx.out;
    if s.accounts == 0 {
        out.warn("No Steam account was found: start Steam and sign in once first.");
    } else if s.added == 0 {
        out.info("vessel is not in Steam. Run: vessel steam add");
    } else if known_target {
        out.success(&format!(
            "vessel is in Steam for {} of {} account(s) ({}).",
            s.added,
            s.accounts,
            s.exe.display()
        ));
    } else {
        out.success(&format!(
            "vessel is in Steam for {} of {} account(s).",
            s.added, s.accounts
        ));
    }
}

fn restart_hint(ctx: &Ctx, s: &ShortcutStatus) {
    if s.steam_running {
        ctx.out
            .warn("Steam is running: restart Steam for the change to show up.");
    } else {
        ctx.out.info("The change shows the next time Steam starts.");
    }
}
