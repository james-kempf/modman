// SPDX-License-Identifier: GPL-3.0-or-later
//! XDG locations. The database and log always live in the XDG dirs; the
//! `data-dir` setting only moves downloads and per-game data.

use std::path::{Path, PathBuf};

use crate::config::ConfigError;

/// Legacy bash tool (modman v3) config dir, relative to `$HOME` (legacy L23).
pub const LEGACY_CONFIG_DIR: &str = ".config/nexus-mod-manager";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub home: PathBuf,
    /// `$XDG_CONFIG_HOME/vessel`
    pub config_dir: PathBuf,
    /// `$XDG_DATA_HOME/vessel`
    pub data_dir: PathBuf,
    /// `$XDG_STATE_HOME/vessel`
    pub state_dir: PathBuf,
}

impl Paths {
    /// Resolves from `$HOME` and the `XDG_*` variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        let base = directories::BaseDirs::new().ok_or(ConfigError::NoHome)?;
        let proj = directories::ProjectDirs::from("", "", "vessel").ok_or(ConfigError::NoHome)?;
        let data_dir = proj.data_dir().to_path_buf();
        Ok(Self {
            home: base.home_dir().to_path_buf(),
            config_dir: proj.config_dir().to_path_buf(),
            state_dir: proj
                .state_dir()
                .map_or_else(|| data_dir.clone(), Path::to_path_buf),
            data_dir,
        })
    }

    /// XDG defaults under `home`, ignoring the environment (tests).
    pub fn for_home(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            config_dir: home.join(".config/vessel"),
            data_dir: home.join(".local/share/vessel"),
            state_dir: home.join(".local/state/vessel"),
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("vessel.db")
    }

    pub fn log_path(&self) -> PathBuf {
        self.state_dir.join("vessel.log")
    }

    pub fn deploy_lock_path(&self) -> PathBuf {
        self.data_dir.join("deploy.lock")
    }

    /// User game definitions that override or extend the embedded ones.
    pub fn definitions_dir(&self) -> PathBuf {
        self.config_dir.join("games")
    }

    /// Legacy bash tool state, if present. The rewrite never reads it; the
    /// caller warns the user to purge with the legacy tool first.
    pub fn legacy_state_dir(&self) -> Option<PathBuf> {
        let dir = self.home.join(LEGACY_CONFIG_DIR);
        dir.is_dir().then_some(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_state_detected_only_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Paths::for_home(tmp.path());
        assert_eq!(p.legacy_state_dir(), None);
        std::fs::create_dir_all(tmp.path().join(LEGACY_CONFIG_DIR)).unwrap();
        assert_eq!(
            p.legacy_state_dir(),
            Some(tmp.path().join(LEGACY_CONFIG_DIR))
        );
    }

    #[test]
    fn files_live_in_xdg_dirs() {
        let p = Paths::for_home(Path::new("/h"));
        assert_eq!(
            p.db_path(),
            PathBuf::from("/h/.local/share/vessel/vessel.db")
        );
        assert_eq!(
            p.log_path(),
            PathBuf::from("/h/.local/state/vessel/vessel.log")
        );
        assert_eq!(
            p.definitions_dir(),
            PathBuf::from("/h/.config/vessel/games")
        );
    }
}
