// SPDX-License-Identifier: GPL-3.0-or-later
//! M3: Nexus REST v1 and GraphQL v2 client, auth and rate limits.
//!
//! Secrets (API key, nxm download keys) never reach `Display`/`Debug` output,
//! error strings or tracing events: [`ApiKey`] and [`NxmKey`] redact
//! themselves, request URLs are logged without their query, and reqwest
//! errors are stripped of their URL before they are wrapped.

use std::path::PathBuf;

use crate::store::StoreError;

mod auth;
mod client;
mod deps;
mod graphql;
mod keystore;
pub(crate) use keystore::kr_err;
mod ratelimit;
mod updates;

pub use auth::{ApiKey, ApiKeyAuth, AuthProvider, NxmKey};
pub(crate) use client::network_message as client_network_message;
pub use client::{
    install_crypto_provider, Account, CdnLink, FileInfo, FileUpdate, GameInfo, Md5Match, ModFiles,
    ModInfo, NexusClient, NexusConfig, RetryPolicy, SharedStore, UpdatedMod, DEFAULT_GRAPHQL_URL,
    DEFAULT_REST_BASE,
};
pub use deps::{
    cached_requirements, refresh_requirements, CachedRequirements, Requirement, BATCH_SIZE,
    REQUIREMENTS_TTL,
};
pub use graphql::{SearchMod, SearchResults, REQUIREMENT_FIELDS};
pub use keystore::{
    keyring_opener, probe_keyring, Backend, KeyStorage, KeyringOpener, ProbeOutcome, API_KEY_ENV,
    CREDENTIALS_FILE, KEYRING_SERVICE, KEYRING_USER, PROBE_TIMEOUT,
};
pub use ratelimit::{RateLimit, BULK_RESERVE};
pub use updates::{check_updates, latest_file, UpdateCheck};

/// Errors from this module. Messages never contain secrets.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NexusError {
    #[error("not logged in to Nexus Mods (no API key); run `vessel login`")]
    NotLoggedIn,
    #[error("the API key is empty or contains invalid characters")]
    InvalidKey,
    #[error("Nexus Mods rejected the API key (HTTP 401); run `vessel login` again")]
    Unauthorized,
    #[error("{0} was not found on Nexus Mods")]
    NotFound(String),
    #[error("Nexus Mods API error (HTTP {status}): {message}")]
    Http { status: u16, message: String },
    #[error("Nexus Mods API rate limit reached ({hourly} hourly / {daily} daily requests left, hourly reset {reset}); try again later")]
    RateLimited {
        hourly: String,
        daily: String,
        reset: String,
    },
    #[error("hourly Nexus Mods API budget is low ({remaining} left, {reserve} kept in reserve); re-run later")]
    RateLimitLow { remaining: u32, reserve: u32 },
    #[error(
        "free accounts need an nxm:// link for this file: click \"Slow download\" on the website"
    )]
    NxmKeyRequired,
    #[error("the nxm:// download link has expired; click \"Slow download\" again")]
    NxmKeyExpired,
    #[error("Nexus Mods returned no download link")]
    NoDownloadLink,
    #[error("Nexus Mods GraphQL error: {}", messages.join("; "))]
    GraphQl {
        messages: Vec<String>,
        /// First path element of each error (the failing alias), if any.
        paths: Vec<String>,
    },
    #[error("network error talking to Nexus Mods: {0}")]
    Network(String),
    #[error("unexpected response from Nexus Mods: {0}")]
    Decode(String),
    #[error("invalid Nexus Mods URL: {0}")]
    BadUrl(String),
    #[error("keyring error: {0}")]
    Keyring(String),
    #[error("credentials file {path} is accessible by other users (mode {mode:o}); run: chmod 600 {path}")]
    InsecureCredentials { path: PathBuf, mode: u32 },
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for NexusError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(StoreError::Sqlite(e))
    }
}

impl NexusError {
    pub(crate) fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.into();
        move |source| Self::Io { path, source }
    }
}
