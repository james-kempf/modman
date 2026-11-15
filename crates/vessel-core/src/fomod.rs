// SPDX-License-Identifier: GPL-3.0-or-later
//! M5: FOMOD installer (legacy `fomod_py` L941-1335, mode resolution in
//! `install_archive` L1383-1391; legacy_spec.md §8).
//!
//! [`load_module_config`] parses `fomod/ModuleConfig.xml`, [`build_plan`]
//! picks options (defaults, recorded [`FomodChoices`] or a
//! [`FomodResolver`]) and lists the files to copy, and [`apply_plan`] copies
//! them out of the extracted tree. [`run`] does all three. The install
//! pipeline (`mods::install_archive`) calls [`resolve_mode`] and [`run`];
//! collections pass [`FomodMode::Collection`] with their recorded choices.
//!
//! Config key `fomod` (legacy ambiguity A2: saved but never read): the CLI
//! uses it as the default mode of `install` when `--fomod` is not given.
//! `auto` (the default) keeps legacy behaviour.

mod apply;
mod model;
mod plan;
mod xml;

use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use apply::{apply_plan, ApplyReport};
pub use model::{
    Condition, Dependency, FileEntry, FileState, Group, GroupType, ModuleConfig, Operator, Plugin,
    PluginType, Step, TypeDescriptor,
};
pub use plan::{
    build_plan, default_selection, validate_selection, FomodResolver, GroupPrompt, InstallPlan,
    PlannedFile, Selection,
};

/// Errors from this module. Messages are user-facing.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FomodError {
    #[error("fomod/ModuleConfig.xml not found in {0}")]
    NotFound(PathBuf),
    #[error("cannot parse ModuleConfig.xml: {0}")]
    Parse(String),
    #[error("FOMOD installer cancelled")]
    Cancelled,
    #[error("no valid selection for FOMOD group '{group}': {reason}")]
    InvalidSelection { group: String, reason: String },
    #[error("invalid FOMOD mode '{0}': use ask, default, reuse, collection or none")]
    InvalidMode(String),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Requested installer mode (legacy meta `fomodMode`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FomodMode {
    /// `""` / `auto`: ask when interactive, else defaults.
    #[default]
    Auto,
    Ask,
    Default,
    /// Recorded choices, else ask when interactive, else defaults.
    Reuse,
    /// Recorded choices, else defaults.
    Collection,
    /// Skip the installer; plain layout rules.
    None,
}

impl FomodMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ask => "ask",
            Self::Default => "default",
            Self::Reuse => "reuse",
            Self::Collection => "collection",
            Self::None => "none",
        }
    }
}

impl FromStr for FomodMode {
    type Err = FomodError;
    fn from_str(s: &str) -> Result<Self, FomodError> {
        Ok(match s {
            "" | "auto" => Self::Auto,
            "ask" => Self::Ask,
            "default" => Self::Default,
            "reuse" => Self::Reuse,
            "collection" => Self::Collection,
            "none" => Self::None,
            other => return Err(FomodError::InvalidMode(other.to_string())),
        })
    }
}

/// How options were (or will be) picked; stored in `fomod_choices.mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResolvedMode {
    Ask,
    Default,
    /// Recorded choices replayed.
    Choices,
}

impl ResolvedMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Default => "default",
            Self::Choices => "choices",
        }
    }
}

/// Legacy mode table (L1383-1391). `None` = skip the installer.
/// `interactive` = a [`FomodResolver`] is available.
pub fn resolve_mode(
    mode: FomodMode,
    have_choices: bool,
    interactive: bool,
) -> Option<ResolvedMode> {
    let ask_or_default = if interactive {
        ResolvedMode::Ask
    } else {
        ResolvedMode::Default
    };
    Some(match mode {
        FomodMode::None => return None,
        FomodMode::Collection | FomodMode::Reuse if have_choices => ResolvedMode::Choices,
        FomodMode::Collection => ResolvedMode::Default,
        FomodMode::Default => ResolvedMode::Default,
        FomodMode::Reuse | FomodMode::Ask | FomodMode::Auto => ask_or_default,
    })
}

/// Recorded selection, Vortex/collection format:
/// `{"type":"fomod","options":[{"name":step,"groups":[{"name":group,"choices":[{"name","idx"}]}]}]}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FomodChoices {
    #[serde(rename = "type")]
    pub kind: String,
    pub options: Vec<StepChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepChoice {
    pub name: String,
    pub groups: Vec<GroupChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupChoice {
    pub name: String,
    pub choices: Vec<OptionChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionChoice {
    pub name: String,
    /// Position in the group's display order.
    pub idx: Option<i64>,
}

fn str_of(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

impl FomodChoices {
    pub fn new(options: Vec<StepChoice>) -> Self {
        FomodChoices {
            kind: "fomod".into(),
            options,
        }
    }

    /// Lenient read of recorded or collection-supplied choices (legacy reads
    /// whatever `.options` holds). `None` when there is no `options` array.
    pub fn from_value(v: &Value) -> Option<Self> {
        let opts = v.get("options")?.as_array()?;
        Some(Self::new(
            opts.iter()
                .map(|s| StepChoice {
                    name: str_of(s, "name"),
                    groups: arr(s, "groups")
                        .iter()
                        .map(|g| GroupChoice {
                            name: str_of(g, "name"),
                            choices: arr(g, "choices")
                                .iter()
                                .map(|c| OptionChoice {
                                    name: str_of(c, "name"),
                                    idx: c.get("idx").and_then(Value::as_i64),
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
        ))
    }

    /// [`Self::from_value`] over JSON text.
    pub fn from_json(s: &str) -> Option<Self> {
        Self::from_value(&serde_json::from_str(s).ok()?)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("plain data serialises")
    }

    /// `(step, chosen option names)` per step, for `info` (legacy L3467).
    pub fn summary(&self) -> Vec<(String, Vec<String>)> {
        self.options
            .iter()
            .map(|s| {
                (
                    s.name.clone(),
                    s.groups
                        .iter()
                        .flat_map(|g| g.choices.iter().map(|c| c.name.clone()))
                        .collect(),
                )
            })
            .collect()
    }
}

/// Reads and parses `<root>/fomod/ModuleConfig.xml` (case-insensitive).
pub fn load_module_config(root: &Path) -> Result<ModuleConfig, FomodError> {
    let path = apply::lookup(root, "fomod/ModuleConfig.xml", false)
        .filter(|p| p.is_file())
        .ok_or_else(|| FomodError::NotFound(root.to_path_buf()))?;
    let bytes = std::fs::read(&path).map_err(|source| FomodError::Io { path, source })?;
    parse_module_config(&bytes)
}

/// Parses ModuleConfig.xml bytes (UTF-8, UTF-16 or Windows-1252).
pub fn parse_module_config(bytes: &[u8]) -> Result<ModuleConfig, FomodError> {
    let root = xml::parse(&xml::decode(bytes))?;
    if root.tag != "config" {
        return Err(FomodError::Parse(format!(
            "root element is <{}>, expected <config>",
            root.tag
        )));
    }
    Ok(ModuleConfig::from_root(&root))
}

/// Outcome of [`run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FomodRun {
    pub module_name: String,
    pub mode: ResolvedMode,
    pub choices: FomodChoices,
    pub report: ApplyReport,
}

/// Loads the installer in `root`, builds the plan and places it into `out`.
pub fn run(
    root: &Path,
    out: &Path,
    how: Selection<'_>,
    deploy_dir: Option<&Path>,
) -> Result<FomodRun, FomodError> {
    let cfg = load_module_config(root)?;
    let plan = build_plan(&cfg, how, deploy_dir)?;
    let report = apply_plan(root, &plan, out)?;
    Ok(FomodRun {
        module_name: cfg.module_name,
        mode: plan.mode,
        choices: plan.choices,
        report,
    })
}

#[cfg(test)]
pub(crate) mod tests;
