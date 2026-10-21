// SPDX-License-Identifier: GPL-3.0-or-later
//! M3: resumable downloads and the nxm:// intake queue.

use std::path::PathBuf;

use crate::store::StoreError;

mod fetch;
mod nxm;
mod queue;

pub use fetch::{file_name_from_uri, DownloadEvent, FetchRequest, Fetched, Fetcher};
pub use nxm::{NxmLink, NxmModLink, MAX_NXM_LEN};
pub use queue::{
    enqueue, enqueue_nxm, get, list, mark_done, mark_failed, set_status, update_progress, Download,
    DownloadSource, DownloadStatus, NewDownload,
};

/// Errors from this module. Messages never contain nxm keys.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DownloadError {
    #[error("not a valid nxm:// link: {0}")]
    InvalidNxm(&'static str),
    #[error("download failed (HTTP {status})")]
    Http { status: u16 },
    #[error("network error while downloading: {0}")]
    Network(String),
    #[error("checksum mismatch for {path}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("server sent {got} bytes, expected {expected}")]
    SizeMismatch { got: u64, expected: u64 },
    #[error("invalid download URL: {0}")]
    BadUrl(String),
    #[error("download {0} not found")]
    NotFound(i64),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for DownloadError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(StoreError::Sqlite(e))
    }
}

impl DownloadError {
    pub(crate) fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.into();
        move |source| Self::Io { path, source }
    }
}
