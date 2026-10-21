// SPDX-License-Identifier: GPL-3.0-or-later
//! Global settings (`modman config`, legacy `cmd_config` L3965-4014) stored in
//! the `settings` table under `config.<key>`.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::paths::Paths;
use crate::store::{Store, StoreError};

/// Deploy methods accepted by `config deploy-method` and `game set method`.
/// `reflink` is new in the rewrite (tech stack §4.1 fallback chain).
pub const DEPLOY_METHODS: &[&str] = &["auto", "hardlink", "reflink", "symlink", "copy"];

/// Errors from settings handling.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    #[error("unknown setting '{0}' (keys: {keys})", keys = ConfigKey::NAMES.join(" "))]
    UnknownKey(String),
    #[error("invalid value '{value}' for {key}: expected {expected}")]
    InvalidValue {
        key: &'static str,
        value: String,
        expected: &'static str,
    },
    #[error("cannot determine the home directory ($HOME unset)")]
    NoHome,
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// The 9 documented `config` keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigKey {
    DataDir,
    DeployMethod,
    Jobs,
    Cdn,
    Adult,
    AutoDeploy,
    Fomod,
    AutoupdateCollections,
    AutoupdateInterval,
}

impl ConfigKey {
    pub const ALL: [ConfigKey; 9] = [
        ConfigKey::DataDir,
        ConfigKey::DeployMethod,
        ConfigKey::Jobs,
        ConfigKey::Cdn,
        ConfigKey::Adult,
        ConfigKey::AutoDeploy,
        ConfigKey::Fomod,
        ConfigKey::AutoupdateCollections,
        ConfigKey::AutoupdateInterval,
    ];
    pub const NAMES: [&'static str; 9] = [
        "data-dir",
        "deploy-method",
        "jobs",
        "cdn",
        "adult",
        "auto-deploy",
        "fomod",
        "autoupdate-collections",
        "autoupdate-interval",
    ];

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }

    fn setting_key(self) -> String {
        format!("config.{}", self.name())
    }

    /// One-line description shown by `config` (legacy L3984-3990).
    pub fn describe(self) -> &'static str {
        match self {
            ConfigKey::DataDir => "where downloads and per-game data live",
            ConfigKey::DeployMethod => "auto|hardlink|reflink|symlink|copy",
            ConfigKey::Jobs => "parallel downloads, Premium",
            ConfigKey::Cdn => "preferred download server",
            ConfigKey::Adult => "show adult-only collections",
            ConfigKey::AutoDeploy => "deploy after every change",
            ConfigKey::Fomod => "auto|ask|default — installer behaviour for single mods",
            ConfigKey::AutoupdateCollections => {
                "auto-update moves collections to their newest revision"
            }
            ConfigKey::AutoupdateInterval => {
                "hours between extra auto-update checks; 0 = only after system/game updates"
            }
        }
    }

    /// Default value (legacy L220-223; data-dir follows XDG instead of
    /// `~/nexus-mods`).
    pub fn default_value(self, paths: &Paths) -> String {
        match self {
            ConfigKey::DataDir => paths.data_dir.to_string_lossy().into_owned(),
            ConfigKey::DeployMethod => "auto".into(),
            ConfigKey::Jobs => "4".into(),
            ConfigKey::Cdn => String::new(),
            ConfigKey::Adult | ConfigKey::AutoDeploy => "false".into(),
            ConfigKey::Fomod => "auto".into(),
            ConfigKey::AutoupdateCollections => "true".into(),
            ConfigKey::AutoupdateInterval => "0".into(),
        }
    }

    /// Validates and normalises a value (legacy L3996-4008).
    pub fn normalize(self, value: &str, home: &Path) -> Result<String, ConfigError> {
        let invalid = |expected| ConfigError::InvalidValue {
            key: self.name(),
            value: value.to_string(),
            expected,
        };
        match self {
            ConfigKey::DataDir => {
                let v = expand_tilde(value, home);
                let v = v.trim_end_matches('/');
                if !v.starts_with('/') {
                    return Err(invalid("an absolute path"));
                }
                Ok(v.to_string())
            }
            ConfigKey::DeployMethod => DEPLOY_METHODS
                .contains(&value)
                .then(|| value.to_string())
                .ok_or_else(|| invalid("auto, hardlink, reflink, symlink or copy")),
            ConfigKey::Jobs => match value.parse::<u8>() {
                Ok(n) if (1..=16).contains(&n) => Ok(n.to_string()),
                _ => Err(invalid("an integer 1-16")),
            },
            ConfigKey::Cdn => Ok(value.to_string()),
            ConfigKey::Adult | ConfigKey::AutoDeploy | ConfigKey::AutoupdateCollections => {
                matches!(value, "true" | "false")
                    .then(|| value.to_string())
                    .ok_or_else(|| invalid("true or false"))
            }
            ConfigKey::Fomod => matches!(value, "auto" | "ask" | "default")
                .then(|| value.to_string())
                .ok_or_else(|| invalid("auto, ask or default")),
            ConfigKey::AutoupdateInterval => value
                .parse::<u32>()
                .map(|n| n.to_string())
                .map_err(|_| invalid("a number of hours (0 = off)")),
        }
    }
}

impl FromStr for ConfigKey {
    type Err = ConfigError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::NAMES
            .iter()
            .position(|n| *n == s)
            .map(|i| Self::ALL[i])
            .ok_or_else(|| ConfigError::UnknownKey(s.to_string()))
    }
}

/// Replaces a leading `~` with `home` (legacy `${val/#\~/$HOME}`).
pub fn expand_tilde(value: &str, home: &Path) -> String {
    match value.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            format!("{}{rest}", home.display())
        }
        _ => value.to_string(),
    }
}

/// Current value of a key (stored or default).
pub fn get(store: &Store, paths: &Paths, key: ConfigKey) -> Result<String, ConfigError> {
    Ok(store
        .setting(&key.setting_key())?
        .unwrap_or_else(|| key.default_value(paths)))
}

/// Validates, stores and returns the normalised value.
pub fn set(
    store: &Store,
    paths: &Paths,
    key: ConfigKey,
    value: &str,
) -> Result<String, ConfigError> {
    let v = key.normalize(value, &paths.home)?;
    store.set_setting(&key.setting_key(), &v)?;
    Ok(v)
}

/// Typed snapshot of all settings for consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub data_dir: PathBuf,
    pub deploy_method: String,
    pub jobs: u8,
    /// `None` = automatic.
    pub cdn: Option<String>,
    pub adult: bool,
    pub auto_deploy: bool,
    pub fomod: String,
    pub autoupdate_collections: bool,
    pub autoupdate_interval_hours: u32,
}

impl Config {
    pub fn load(store: &Store, paths: &Paths) -> Result<Self, ConfigError> {
        let g = |k| get(store, paths, k);
        Ok(Self {
            data_dir: PathBuf::from(g(ConfigKey::DataDir)?),
            deploy_method: g(ConfigKey::DeployMethod)?,
            jobs: g(ConfigKey::Jobs)?.parse().unwrap_or(4),
            cdn: Some(g(ConfigKey::Cdn)?).filter(|s| !s.is_empty()),
            adult: g(ConfigKey::Adult)? == "true",
            auto_deploy: g(ConfigKey::AutoDeploy)? == "true",
            fomod: g(ConfigKey::Fomod)?,
            autoupdate_collections: g(ConfigKey::AutoupdateCollections)? == "true",
            autoupdate_interval_hours: g(ConfigKey::AutoupdateInterval)?.parse().unwrap_or(0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Paths {
        Paths::for_home(Path::new("/home/u"))
    }

    #[test]
    fn accepts_exactly_the_nine_documented_keys() {
        for name in ConfigKey::NAMES {
            assert_eq!(name.parse::<ConfigKey>().unwrap().name(), name);
        }
        assert!(matches!(
            "game".parse::<ConfigKey>(),
            Err(ConfigError::UnknownKey(_))
        ));
    }

    #[test]
    fn defaults_match_legacy() {
        let store = Store::open_in_memory().unwrap();
        let c = Config::load(&store, &paths()).unwrap();
        assert_eq!(c.data_dir, PathBuf::from("/home/u/.local/share/modman"));
        assert_eq!(c.deploy_method, "auto");
        assert_eq!(c.jobs, 4);
        assert_eq!(c.cdn, None);
        assert!(!c.adult && !c.auto_deploy && c.autoupdate_collections);
        assert_eq!(c.fomod, "auto");
        assert_eq!(c.autoupdate_interval_hours, 0);
    }

    #[test]
    fn validation_per_key() {
        let h = Path::new("/home/u");
        let ok = |k: ConfigKey, v: &str| k.normalize(v, h).unwrap();
        let bad = |k: ConfigKey, v: &str| assert!(k.normalize(v, h).is_err(), "{k:?}={v}");
        assert_eq!(ok(ConfigKey::DataDir, "~/mods/"), "/home/u/mods");
        bad(ConfigKey::DataDir, "relative/dir");
        ok(ConfigKey::DeployMethod, "symlink");
        bad(ConfigKey::DeployMethod, "move");
        ok(ConfigKey::Jobs, "16");
        bad(ConfigKey::Jobs, "0");
        bad(ConfigKey::Jobs, "17");
        ok(ConfigKey::Adult, "true");
        bad(ConfigKey::AutoDeploy, "yes");
        bad(ConfigKey::Fomod, "none");
        ok(ConfigKey::AutoupdateInterval, "12");
        bad(ConfigKey::AutoupdateInterval, "-1");
        ok(ConfigKey::Cdn, "Amsterdam");
    }

    #[test]
    fn set_persists_normalised_value() {
        let store = Store::open_in_memory().unwrap();
        let p = paths();
        set(&store, &p, ConfigKey::Jobs, "08").unwrap();
        assert_eq!(get(&store, &p, ConfigKey::Jobs).unwrap(), "8");
    }

    #[test]
    fn tilde_expansion_only_at_start() {
        let h = Path::new("/h");
        assert_eq!(expand_tilde("~", h), "/h");
        assert_eq!(expand_tilde("~/x", h), "/h/x");
        assert_eq!(expand_tilde("~user/x", h), "~user/x");
        assert_eq!(expand_tilde("/a/~", h), "/a/~");
    }
}
