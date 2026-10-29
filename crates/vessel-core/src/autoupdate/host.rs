// SPDX-License-Identifier: GPL-3.0-or-later
//! What auto-update reads from the machine: the system fingerprint, a busy
//! package manager, Steam's view of a game and whether the game runs
//! (legacy `au_system_paths`/`au_path_mtime`/`au_system_fp`/
//! `au_package_manager_busy`/`au_steam_state`/`au_game_running`,
//! L4403-4463). Everything is read-only; roots are configurable for tests.

use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use serde::Serialize;

use crate::deploy::{GameProbe, ProcProbe};
use crate::games::{steam, GameConfig};

/// Process names (`/proc/<pid>/comm`) of package managers and system
/// updaters (legacy L4440).
pub const PACKAGE_MANAGERS: [&str; 15] = [
    "pacman",
    "dnf",
    "dnf5",
    "yum",
    "rpm",
    "apt",
    "apt-get",
    "dpkg",
    "zypper",
    "rpm-ostree",
    "apk",
    "xbps-install",
    "emerge",
    "steamos-atomupd",
    "rauc",
];

/// Paths (relative to the root) whose change means "the system was
/// updated" (legacy L4403-4412). `ostree/deploy/*/deploy` is expanded
/// separately.
const SYSTEM_PATHS: [&str; 13] = [
    "etc/os-release",
    "usr/lib/os-release",
    "var/lib/pacman/local",
    "var/lib/rpm",
    "usr/lib/sysimage/rpm",
    "var/lib/dpkg/status",
    "lib/apk/db/installed",
    "var/db/xbps",
    "var/db/pkg",
    "nix/var/nix/profiles/system",
    "run/current-system",
    "etc/steamos-release",
    "usr/lib/steamos-release",
];

/// os-release keys that identify an image or release (legacy L4426).
const OS_RELEASE_KEYS: [&str; 6] = [
    "ID",
    "VERSION_ID",
    "BUILD_ID",
    "IMAGE_ID",
    "IMAGE_VERSION",
    "VARIANT_ID",
];

/// System fingerprint and the newest change among the watched paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemSnapshot {
    /// 16 hex chars of an md5 over os-release ids, path mtimes and the kernel.
    pub fingerprint: String,
    /// Epoch seconds of the newest change; 0 when nothing was found.
    pub newest_change: i64,
}

/// Steam's view of a game (`appmanifest_<appid>.acf`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SteamState {
    pub buildid: Option<String>,
    /// 4 = fully installed; anything else = Steam is updating.
    pub state_flags: Option<u32>,
}

/// The machine as auto-update sees it. Mocked in tests.
pub trait Host {
    /// Epoch seconds.
    fn now(&self) -> i64;
    fn system_snapshot(&self) -> SystemSnapshot;
    /// Name of the busy package manager, if any.
    fn package_manager_busy(&self) -> Option<String>;
    fn steam_state(&self, game: &GameConfig) -> Option<SteamState>;
    fn game_running(&self, game: &GameConfig) -> bool;
}

/// The real machine. `root` and `proc_root` are `/` and `/proc` outside tests;
/// `home` locates the Steam libraries.
#[derive(Debug, Clone)]
pub struct LiveHost {
    pub root: PathBuf,
    pub proc_root: PathBuf,
    pub home: PathBuf,
}

impl LiveHost {
    pub fn new(home: &Path) -> Self {
        Self {
            root: PathBuf::from("/"),
            proc_root: PathBuf::from("/proc"),
            home: home.to_path_buf(),
        }
    }
}

/// Lock and shared-memory files change on every database read (legacy
/// L4416-4419).
fn ignored(name: &str) -> bool {
    name.ends_with("-shm")
        || name.ends_with(".lock")
        || name.ends_with(".lck")
        || name == "lock"
        || name == "lock-frontend"
}

fn mtime_secs(p: &Path) -> Option<i64> {
    let m = std::fs::metadata(p)
        .or_else(|_| std::fs::symlink_metadata(p))
        .ok()?;
    let t = m.modified().ok()?;
    let d = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(d.as_secs()).ok()
}

/// Newest mtime of a file, or of a folder and its direct entries.
pub(crate) fn path_mtime(p: &Path) -> Option<i64> {
    let own = p
        .file_name()
        .is_none_or(|n| !ignored(&n.to_string_lossy()))
        .then(|| mtime_secs(p))
        .flatten();
    let entries = std::fs::read_dir(p).into_iter().flatten().flatten();
    entries
        .filter(|e| !ignored(&e.file_name().to_string_lossy()))
        .filter_map(|e| mtime_secs(&e.path()))
        .chain(own)
        .max()
}

impl LiveHost {
    fn system_paths(&self) -> Vec<(String, PathBuf)> {
        let mut v: Vec<(String, PathBuf)> = SYSTEM_PATHS
            .iter()
            .map(|rel| (format!("/{rel}"), self.root.join(rel)))
            .collect();
        let ostree = self.root.join("ostree/deploy");
        let mut stateroots: Vec<_> = std::fs::read_dir(&ostree)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        stateroots.sort();
        // Legacy order: ostree comes before the SteamOS release files.
        let at = v.len() - 2;
        for (i, s) in stateroots.into_iter().enumerate() {
            let rel = format!("ostree/deploy/{s}/deploy");
            v.insert(at + i, (format!("/{rel}"), self.root.join(rel)));
        }
        v.retain(|(_, p)| p.exists());
        v
    }
}

impl Host for LiveHost {
    fn now(&self) -> i64 {
        crate::store::now()
    }

    fn system_snapshot(&self) -> SystemSnapshot {
        let mut ids = String::new();
        for rel in ["etc/os-release", "usr/lib/os-release"] {
            let Ok(text) = std::fs::read_to_string(self.root.join(rel)) else {
                continue;
            };
            let mut lines: Vec<&str> = text
                .lines()
                .filter(|l| {
                    l.split_once('=')
                        .is_some_and(|(k, _)| OS_RELEASE_KEYS.contains(&k))
                })
                .collect();
            lines.sort_unstable();
            for l in lines {
                ids.push_str(l);
                ids.push(' ');
            }
        }
        let mut stamps = String::new();
        let mut newest = 0;
        for (name, p) in self.system_paths() {
            let Some(mt) = path_mtime(&p) else { continue };
            stamps.push_str(&format!("{name}={mt} "));
            newest = newest.max(mt);
        }
        if let Ok(target) = std::fs::read_link(self.root.join("run/current-system")) {
            stamps.push_str(&format!("nixos={} ", target.display()));
        }
        let kernel = std::fs::read_to_string(self.proc_root.join("sys/kernel/osrelease"))
            .unwrap_or_default();
        let digest = Md5::digest(format!("{ids}|{stamps}|{}", kernel.trim()).as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        SystemSnapshot {
            fingerprint: hex[..16].to_string(),
            newest_change: newest,
        }
    }

    fn package_manager_busy(&self) -> Option<String> {
        if self.root.join("var/lib/pacman/db.lck").exists() {
            return Some("pacman".into());
        }
        let dir = std::fs::read_dir(&self.proc_root).ok()?;
        dir.flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .bytes()
                    .all(|b| b.is_ascii_digit())
            })
            .filter_map(|e| std::fs::read_to_string(e.path().join("comm")).ok())
            .map(|c| c.trim_end().to_string())
            .find(|c| PACKAGE_MANAGERS.contains(&c.as_str()))
    }

    fn steam_state(&self, game: &GameConfig) -> Option<SteamState> {
        let appid = game.appid?;
        let libs = game
            .library
            .iter()
            .cloned()
            .chain(steam::steam_libraries(&self.home));
        steam_state_in(libs, appid)
    }

    fn game_running(&self, game: &GameConfig) -> bool {
        ProcProbe {
            proc_root: self.proc_root.clone(),
        }
        .is_running(game)
    }
}

/// Reads `appmanifest_<appid>.acf` from the first library that has one.
pub fn steam_state_in(
    libraries: impl IntoIterator<Item = PathBuf>,
    appid: u32,
) -> Option<SteamState> {
    for lib in libraries {
        let acf = lib.join(format!("steamapps/appmanifest_{appid}.acf"));
        let Ok(text) = std::fs::read_to_string(&acf) else {
            continue;
        };
        match steam::parse_app_manifest(&text, &acf.display().to_string()) {
            Ok(m) => {
                return Some(SteamState {
                    buildid: m.buildid.filter(|b| !b.is_empty()),
                    state_flags: m.state_flags,
                })
            }
            Err(e) => tracing::warn!("skipping {}: {e}", acf.display()),
        }
    }
    None
}
