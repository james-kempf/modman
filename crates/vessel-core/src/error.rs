// SPDX-License-Identifier: GPL-3.0-or-later
//! Top-level error type. Each module owns its error enum; this wraps them.

use crate::archive::ArchiveError;
use crate::config::ConfigError;
use crate::deploy::DeployError;
use crate::download::DownloadError;
use crate::fomod::FomodError;
use crate::games::GamesError;
use crate::loaders::LoaderError;
use crate::loadorder::LoadOrderError;
use crate::nexus::NexusError;
use crate::staging::StagingError;
use crate::store::StoreError;

/// Error type for the core crate.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    #[error(transparent)]
    Staging(#[from] StagingError),
    #[error(transparent)]
    Deploy(#[from] DeployError),
    #[error(transparent)]
    Nexus(#[from] NexusError),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    Fomod(#[from] FomodError),
    #[error(transparent)]
    LoadOrder(#[from] LoadOrderError),
    #[error(transparent)]
    Loader(#[from] LoaderError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Games(#[from] GamesError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    /// Stable code for the UI to map to a title and an action; only
    /// loader errors carry one so far.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Error::Loader(e) => Some(e.code()),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
