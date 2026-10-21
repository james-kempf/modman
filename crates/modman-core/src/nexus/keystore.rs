// SPDX-License-Identifier: GPL-3.0-or-later
//! API key storage: Secret Service keyring, falling back to a 0600 file in a
//! 0700 directory when no keyring is reachable (Deck Game Mode). Decision and
//! fallback rules: spikes.md §3.
//!
//! All calls block; wrap them in `spawn_blocking` from async code.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use keyring_core::{CredentialStore, Entry, Error as KrError};
use serde::{Deserialize, Serialize};

use super::{ApiKey, NexusError};
use crate::paths::Paths;

/// Overrides the stored key for one run; never written (legacy L230-233).
pub const API_KEY_ENV: &str = "NEXUS_API_KEY";
pub const KEYRING_SERVICE: &str = "modman";
pub const KEYRING_USER: &str = "nexus-api-key";
/// File backend, inside `Paths::config_dir`.
pub const CREDENTIALS_FILE: &str = "credentials.toml";
/// Secret Service activation can hang indefinitely (spikes.md §3).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Creates the keyring store. Injected so tests never touch the real keyring.
pub type KeyringOpener = Box<dyn FnOnce() -> keyring_core::Result<Arc<CredentialStore>> + Send>;

/// Opener for the real Secret Service store.
pub fn keyring_opener() -> KeyringOpener {
    Box::new(|| {
        let store: Arc<CredentialStore> = zbus_secret_service_keyring_store::Store::new()?;
        Ok(store)
    })
}

/// Result of [`probe_keyring`].
#[derive(Debug)]
pub enum ProbeOutcome {
    Available(Entry),
    /// Use the file backend; the reason is safe to show.
    Unavailable(String),
}

/// Where the key lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Keyring,
    File,
}

impl Backend {
    /// Value for the `auth.backend` setting.
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Keyring => "keyring",
            Backend::File => "file",
        }
    }
}

/// Errors meaning "no usable keyring here" rather than a real failure.
fn is_unavailable(e: &KrError) -> bool {
    matches!(
        e,
        KrError::PlatformFailure(_)
            | KrError::NoStorageAccess(_)
            | KrError::NoDefaultStore
            | KrError::NotSupportedByStore(_)
    )
}

fn kr_err(e: &KrError) -> NexusError {
    // Variants carrying secret bytes are summarised, never echoed.
    let msg = match e {
        KrError::BadEncoding(_) => "stored secret is not valid UTF-8".to_owned(),
        KrError::BadDataFormat(..) => "stored secret has an unexpected format".to_owned(),
        KrError::Ambiguous(v) => format!("{} matching keyring entries", v.len()),
        other => other.to_string(),
    };
    NexusError::Keyring(msg)
}

/// Opens the store on a worker thread and reads the entry once, giving up
/// after `timeout`. A hung probe thread is abandoned.
pub fn probe_keyring(opener: KeyringOpener, timeout: Duration) -> Result<ProbeOutcome, NexusError> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("keyring-probe".into())
        .spawn(move || {
            let res = (|| {
                let store = match opener() {
                    Ok(s) => s,
                    Err(e) => return Ok(ProbeOutcome::Unavailable(format!("keyring: {e}"))),
                };
                let entry = match store.build(KEYRING_SERVICE, KEYRING_USER, None) {
                    Ok(e) => e,
                    Err(e) if is_unavailable(&e) => {
                        return Ok(ProbeOutcome::Unavailable(format!("keyring: {e}")))
                    }
                    Err(e) => return Err(kr_err(&e)),
                };
                match entry.get_password() {
                    Ok(_) | Err(KrError::NoEntry) => Ok(ProbeOutcome::Available(entry)),
                    Err(e) if is_unavailable(&e) => {
                        Ok(ProbeOutcome::Unavailable(format!("keyring: {e}")))
                    }
                    Err(e) => Err(kr_err(&e)),
                }
            })();
            let _ = tx.send(res);
        })
        .map_err(|e| NexusError::Keyring(format!("cannot start keyring probe: {e}")))?;
    match rx.recv_timeout(timeout) {
        Ok(res) => res,
        Err(_) => Ok(ProbeOutcome::Unavailable(format!(
            "keyring did not answer within {} s",
            timeout.as_secs_f32()
        ))),
    }
}

#[derive(Serialize, Deserialize)]
struct CredentialsFile {
    nexus_api_key: String,
}

/// API key storage for one user.
#[derive(Debug)]
pub struct KeyStorage {
    dir: PathBuf,
    file: PathBuf,
    keyring: Option<Entry>,
    unavailable_reason: Option<String>,
}

impl KeyStorage {
    pub fn new(paths: &Paths, probe: ProbeOutcome) -> Self {
        let (keyring, unavailable_reason) = match probe {
            ProbeOutcome::Available(e) => (Some(e), None),
            ProbeOutcome::Unavailable(r) => (None, Some(r)),
        };
        Self {
            dir: paths.config_dir.clone(),
            file: paths.config_dir.join(CREDENTIALS_FILE),
            keyring,
            unavailable_reason,
        }
    }

    /// Probes the keyring (with timeout) and builds the storage.
    pub fn open(
        paths: &Paths,
        opener: KeyringOpener,
        timeout: Duration,
    ) -> Result<Self, NexusError> {
        let probe = probe_keyring(opener, timeout)?;
        if let ProbeOutcome::Unavailable(reason) = &probe {
            tracing::info!(%reason, "Secret Service unavailable; API key uses the file backend");
        }
        Ok(Self::new(paths, probe))
    }

    pub fn file_path(&self) -> &Path {
        &self.file
    }

    /// Why the keyring is not used, when it is not.
    pub fn unavailable_reason(&self) -> Option<&str> {
        self.unavailable_reason.as_deref()
    }

    /// Where [`load`](Self::load) reads from: an existing file copy wins
    /// until the next successful keyring [`save`](Self::save).
    pub fn backend(&self) -> Backend {
        if self.keyring.is_none() || self.file.exists() {
            Backend::File
        } else {
            Backend::Keyring
        }
    }

    /// The key from `env_override` (pass `std::env::var(API_KEY_ENV).ok()`),
    /// else from the active backend.
    pub fn load(&self, env_override: Option<&str>) -> Result<Option<ApiKey>, NexusError> {
        if let Some(raw) = env_override.filter(|v| !v.trim().is_empty()) {
            return ApiKey::new(raw).map(Some);
        }
        match (self.backend(), &self.keyring) {
            (Backend::Keyring, Some(entry)) => match entry.get_password() {
                Ok(p) => ApiKey::new(&p).map(Some),
                Err(KrError::NoEntry) => Ok(None),
                Err(e) => Err(kr_err(&e)),
            },
            _ => self.read_file(),
        }
    }

    /// Stores the key, preferring the keyring. A successful keyring write
    /// removes any file copy.
    pub fn save(&self, key: &ApiKey) -> Result<Backend, NexusError> {
        if let Some(entry) = &self.keyring {
            match entry.set_password(key.expose()) {
                Ok(()) => {
                    self.remove_file()?;
                    return Ok(Backend::Keyring);
                }
                Err(e) if is_unavailable(&e) => {
                    tracing::info!("keyring write failed; storing the API key in the file backend");
                }
                Err(e) => return Err(kr_err(&e)),
            }
        }
        self.write_file(key)?;
        Ok(Backend::File)
    }

    /// Removes the key from both backends.
    pub fn clear(&self) -> Result<(), NexusError> {
        if let Some(entry) = &self.keyring {
            match entry.delete_credential() {
                Ok(()) | Err(KrError::NoEntry) => {}
                Err(e) if is_unavailable(&e) => {}
                Err(e) => return Err(kr_err(&e)),
            }
        }
        self.remove_file()
    }

    fn remove_file(&self) -> Result<(), NexusError> {
        match fs::remove_file(&self.file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(NexusError::io(&self.file)(e))
            }
            _ => Ok(()),
        }
    }

    fn ensure_private_dir(&self) -> Result<(), NexusError> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)
            .map_err(NexusError::io(&self.dir))?;
        // An existing dir keeps its old mode; tighten it.
        let mode = fs::metadata(&self.dir)
            .map_err(NexusError::io(&self.dir))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))
                .map_err(NexusError::io(&self.dir))?;
        }
        Ok(())
    }

    fn write_file(&self, key: &ApiKey) -> Result<(), NexusError> {
        self.ensure_private_dir()?;
        let body = toml::to_string(&CredentialsFile {
            nexus_api_key: key.expose().to_owned(),
        })
        .map_err(|_| NexusError::Keyring("cannot encode credentials".into()))?;
        let tmp = self
            .dir
            .join(format!(".{CREDENTIALS_FILE}.{}.tmp", std::process::id()));
        let _ = fs::remove_file(&tmp);
        let write = || -> std::io::Result<()> {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            // The umask can only clear bits, but be explicit.
            f.set_permissions(fs::Permissions::from_mode(0o600))?;
            f.write_all(body.as_bytes())?;
            f.sync_all()?;
            fs::rename(&tmp, &self.file)
        };
        write().map_err(|e| {
            let _ = fs::remove_file(&tmp);
            NexusError::io(&self.file)(e)
        })
    }

    fn read_file(&self) -> Result<Option<ApiKey>, NexusError> {
        // O_NOFOLLOW, then checks on the opened file itself (no TOCTOU): a
        // symlink or a file owned by someone else could feed us a key.
        let mut f = match OpenOptions::new()
            .read(true)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(&self.file)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) if e.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) => {
                return Err(NexusError::Keyring(format!(
                    "{} is a symbolic link; refusing to read credentials through it",
                    self.file.display()
                )))
            }
            Err(e) => return Err(NexusError::io(&self.file)(e)),
        };
        let meta = f.metadata().map_err(NexusError::io(&self.file))?;
        if !meta.is_file() || meta.uid() != rustix::process::geteuid().as_raw() {
            return Err(NexusError::Keyring(format!(
                "{} is not a regular file owned by you; refusing to read it",
                self.file.display()
            )));
        }
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(NexusError::InsecureCredentials {
                path: self.file.clone(),
                mode,
            });
        }
        let mut text = String::new();
        f.read_to_string(&mut text)
            .map_err(NexusError::io(&self.file))?;
        // Parse errors could quote the file (the key), so they are not echoed.
        let parsed: CredentialsFile = toml::from_str(&text).map_err(|_| {
            NexusError::Keyring(format!("{} is not valid TOML", self.file.display()))
        })?;
        ApiKey::new(&parsed.nexus_api_key).map(Some)
    }
}
