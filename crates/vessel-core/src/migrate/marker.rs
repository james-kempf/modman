// SPDX-License-Identifier: GPL-3.0-or-later
//! `<config>/migration.json`: per-step and per-library progress, written
//! atomically (temp + rename) after every step.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::MigrateError;

pub const MARKER_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepState {
    Done,
    Pending,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LibraryState {
    Done,
    Offline,
    Conflict,
    /// A move was started by us; a target found on resume is our own.
    Pending,
}

/// Legacy locations fixed when the marker is created, so a resume uses the
/// same sources even if the environment changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sources {
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub version: u32,
    pub sources: Sources,
    #[serde(default)]
    pub steps: BTreeMap<String, StepState>,
    #[serde(default)]
    pub libraries: BTreeMap<PathBuf, LibraryState>,
    /// SHA-256 of each copied legacy file (absolute source path), checked
    /// before the old dirs are removed.
    #[serde(default)]
    pub copied: BTreeMap<String, String>,
    /// Whether the legacy auto-update timer was enabled, recorded before
    /// it is disabled so the Vessel timer can match it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_timer_enabled: Option<bool>,
    /// The keyring was unreachable during migration: the Nexus key lookup
    /// still tries the legacy keyring entry and moves it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub legacy_key_pending: bool,
    pub started_at: i64,
    #[serde(default)]
    pub finished_at: Option<i64>,
}

impl Marker {
    pub fn new(sources: Sources) -> Self {
        Self {
            version: MARKER_VERSION,
            sources,
            steps: BTreeMap::new(),
            libraries: BTreeMap::new(),
            copied: BTreeMap::new(),
            legacy_timer_enabled: None,
            legacy_key_pending: false,
            started_at: crate::store::now(),
            finished_at: None,
        }
    }

    pub fn step(&self, name: &str) -> StepState {
        self.steps.get(name).copied().unwrap_or(StepState::Pending)
    }

    /// Libraries still waiting (offline or conflicting); retried every run.
    pub fn has_waiting_libraries(&self) -> bool {
        self.libraries
            .values()
            .any(|s| matches!(s, LibraryState::Offline | LibraryState::Conflict))
    }

    pub fn load(path: &Path) -> Result<Option<Self>, MigrateError> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| MigrateError::Marker(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(MigrateError::io(path, e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), MigrateError> {
        let dir = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(|e| MigrateError::io(dir, e))?;
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(self).expect("marker serializes");
        super::fsops::write_synced(&tmp, &body)?;
        std::fs::rename(&tmp, path).map_err(|e| MigrateError::io(path, e))?;
        super::fsops::sync_dir(dir);
        Ok(())
    }
}
