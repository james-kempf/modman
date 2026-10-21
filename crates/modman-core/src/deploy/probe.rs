// SPDX-License-Identifier: GPL-3.0-or-later
//! "Is the game running?" (legacy `au_game_running`, L4458-4463). Legacy only
//! asked this before autoupdate; the rewrite refuses deploy and purge too.

use std::path::PathBuf;

use crate::games::GameConfig;

/// Detects a running game. Mocked in tests.
pub trait GameProbe: Send + Sync {
    fn is_running(&self, game: &GameConfig) -> bool;
}

/// Never reports a running game.
#[derive(Debug, Clone, Copy, Default)]
pub struct NeverRunning;

impl GameProbe for NeverRunning {
    fn is_running(&self, _: &GameConfig) -> bool {
        false
    }
}

/// Scans `<proc_root>/<pid>/cmdline`. A process belongs to the game when one
/// argument is exactly `AppId=<appid>` (Steam/Proton launch wrappers), or the
/// command line contains `<folder>/` or its Windows spelling `\..\folder\`.
#[derive(Debug, Clone)]
pub struct ProcProbe {
    pub proc_root: PathBuf,
}

impl Default for ProcProbe {
    fn default() -> Self {
        Self {
            proc_root: PathBuf::from("/proc"),
        }
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

impl GameProbe for ProcProbe {
    fn is_running(&self, game: &GameConfig) -> bool {
        let appid = game.appid.map(|id| format!("AppId={id}").into_bytes());
        let folders: Vec<Vec<u8>> = game
            .folder
            .as_ref()
            .map(|f| {
                let unix = f.to_string_lossy().trim_end_matches('/').to_string();
                let win = unix.replace('/', "\\");
                vec![
                    format!("{unix}/").into_bytes(),
                    format!("{win}\\").into_bytes(),
                ]
            })
            .unwrap_or_default();
        if appid.is_none() && folders.is_empty() {
            return false;
        }
        let own = std::process::id().to_string();
        let Ok(dir) = std::fs::read_dir(&self.proc_root) else {
            return false;
        };
        dir.flatten().any(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) || name == own {
                return false;
            }
            let Ok(cmdline) = std::fs::read(e.path().join("cmdline")) else {
                return false;
            };
            appid
                .as_deref()
                .is_some_and(|a| cmdline.split(|b| *b == 0).any(|arg| arg == a))
                || folders.iter().any(|f| contains(&cmdline, f))
        })
    }
}
