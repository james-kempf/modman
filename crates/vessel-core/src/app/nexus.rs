// SPDX-License-Identifier: GPL-3.0-or-later
//! Nexus session glue: API key lookup, client construction, login/logout,
//! account tier, game ids and file downloads into
//! `<data-dir>/<game>/downloads` (legacy `download_nexus_file` L1482-1512).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use super::{fail, AppError, Cx, Msg, Ui, R};
use crate::blocking::block_on;
use crate::download::{self, DownloadSource, DownloadStatus, FetchRequest, Fetcher, NewDownload};
use crate::games::{self, GameConfig};
use crate::migrate::LegacyKeyRetry;
use crate::mods::downloads_dir;
use crate::nexus::{
    keyring_opener, ApiKey, ApiKeyAuth, AuthProvider, Backend, FileInfo, KeyStorage, NexusClient,
    NexusConfig, NxmKey, ProbeOutcome, RateLimit, SharedStore, API_KEY_ENV, CREDENTIALS_FILE,
    PROBE_TIMEOUT,
};
use crate::paths::Paths;
use crate::staging::sanitize_name;
use crate::store::Store;

/// Where the API key lives (`keyring` or `file`); absent = never logged in.
pub const AUTH_BACKEND_KEY: &str = "auth.backend";
pub const USER_KEY: &str = "nexus.user";
pub const PREMIUM_KEY: &str = "nexus.premium";
/// Test hook: Nexus API base URL (REST `<base>/v1`, GraphQL `<base>/v2/graphql`).
pub const BASE_URL_ENV: &str = "VESSEL_NEXUS_URL";

/// Client settings, honouring [`BASE_URL_ENV`].
pub fn config() -> R<NexusConfig> {
    Ok(match std::env::var(BASE_URL_ENV) {
        Ok(base) if !base.is_empty() => NexusConfig::with_base(&base)?,
        _ => NexusConfig::default(),
    })
}

fn env_key() -> Option<String> {
    std::env::var(API_KEY_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
}

/// Key storage without probing the keyring unless the key was stored there
/// (the probe can take up to 3 s).
pub fn storage_for_read(paths: &Paths, store: &Store) -> R<Option<KeyStorage>> {
    Ok(match store.setting(AUTH_BACKEND_KEY)?.as_deref() {
        Some("keyring") => Some(KeyStorage::open(paths, keyring_opener(), PROBE_TIMEOUT)?),
        Some(_) => Some(KeyStorage::new(
            paths,
            ProbeOutcome::Unavailable("file backend selected at login".into()),
        )),
        None => None,
    })
}

/// The API key: `NEXUS_API_KEY`, else the stored one.
pub fn load_key(paths: &Paths, store: &Store) -> R<Option<ApiKey>> {
    if let Some(k) = env_key() {
        return Ok(Some(ApiKey::new(&k)?));
    }
    let Some(s) = storage_for_read(paths, store)? else {
        return Ok(None);
    };
    match s.load(None)? {
        // The migration could not reach the keyring: move the legacy entry
        // now, unless the keyring already failed in this process (each try
        // can wait PROBE_TIMEOUT).
        None if !KEYRING_GAVE_UP.load(Ordering::Relaxed) => {
            match crate::migrate::retry_legacy_key(paths, Arc::new(|| keyring_opener()())) {
                LegacyKeyRetry::Done { moved: true } => {
                    Ok(KeyStorage::open(paths, keyring_opener(), PROBE_TIMEOUT)?.load(None)?)
                }
                LegacyKeyRetry::Unreachable => {
                    KEYRING_GAVE_UP.store(true, Ordering::Relaxed);
                    Ok(None)
                }
                _ => Ok(None),
            }
        }
        k => Ok(k),
    }
}

/// Set once the deferred legacy key move found the keyring unreachable;
/// later lookups in this process skip it.
static KEYRING_GAVE_UP: AtomicBool = AtomicBool::new(false);

/// A short description of where the key comes from, without loading it.
pub fn key_source(paths: &Paths, store: &Store) -> R<Option<String>> {
    if env_key().is_some() {
        return Ok(Some(format!("from {API_KEY_ENV}")));
    }
    Ok(match store.setting(AUTH_BACKEND_KEY)?.as_deref() {
        Some("keyring") => Some("stored in the keyring".into()),
        Some(_) => {
            let f = paths.config_dir.join(CREDENTIALS_FILE);
            f.exists().then(|| format!("stored in {}", f.display()))
        }
        None => None,
    })
}

/// A second connection to the database for the client (rate-limit state,
/// caches); SQLite WAL lets both coexist.
pub fn shared_store(paths: &Paths) -> R<SharedStore> {
    Ok(Arc::new(Mutex::new(Store::open(&paths.db_path())?)))
}

pub struct Nexus {
    pub client: NexusClient,
    pub shared: SharedStore,
}

impl Nexus {
    /// Runs an async client call on a private current-thread runtime. Never
    /// call this from inside an async task.
    pub fn run<F: std::future::Future>(&self, f: F) -> R<F::Output> {
        block_on(f).map_err(|e| AppError::Msg(format!("cannot start the async runtime: {e}")))
    }
}

const NOT_LOGGED_IN: &str = "Not logged in to Nexus Mods. Run: vessel login";

/// Client with the stored key. `require` fails when not logged in;
/// otherwise an anonymous client is returned.
pub fn connect(paths: &Paths, store: &Store, require: bool) -> R<Option<Nexus>> {
    let key = load_key(paths, store)?;
    if key.is_none() && require {
        fail!("{NOT_LOGGED_IN}");
    }
    let shared = shared_store(paths)?;
    let auth = key.map(|k| Arc::new(ApiKeyAuth::new(k)) as Arc<dyn AuthProvider>);
    let client = NexusClient::new(config()?, auth, Some(shared.clone()))?;
    Ok(Some(Nexus { client, shared }))
}

/// Logged-in client or an error.
pub fn require(paths: &Paths, store: &Store) -> R<Nexus> {
    connect(paths, store, true)?.ok_or_else(|| AppError::Msg(NOT_LOGGED_IN.into()))
}

/// Premium flag; asks Nexus once when unknown (legacy `refresh_account`).
pub fn is_premium(store: &Store, nx: &Nexus) -> R<bool> {
    if let Some(v) = store.setting(PREMIUM_KEY)? {
        return Ok(v == "true");
    }
    let acct = nx.run(nx.client.validate())??;
    store.set_setting(USER_KEY, &acct.name)?;
    store.set_setting(PREMIUM_KEY, if acct.is_premium { "true" } else { "false" })?;
    Ok(acct.is_premium)
}

/// Numeric Nexus game id, fetched once and saved with the game.
pub fn game_id(store: &Store, nx: &Nexus, game: &mut GameConfig) -> R<u64> {
    if let Some(id) = game.nexus_game_id.and_then(|i| u64::try_from(i).ok()) {
        return Ok(id);
    }
    let info = nx.run(nx.client.game(&game.domain))??;
    game.nexus_game_id = Some(info.id as i64);
    games::save_game(store.conn(), game)?;
    Ok(info.id)
}

/// A finished download of this file whose archive still exists.
fn existing_download(
    store: &Store,
    game: &str,
    mod_id: u64,
    file_id: u64,
) -> R<Option<(PathBuf, Option<String>)>> {
    let done = download::list(store, Some(DownloadStatus::Done))?;
    Ok(done
        .into_iter()
        .rev()
        .filter(|d| {
            d.game.as_deref() == Some(game)
                && d.nexus_mod_id == Some(mod_id)
                && d.nexus_file_id == Some(file_id)
        })
        .find_map(|d| {
            let p = PathBuf::from(d.dest_path?);
            p.is_file().then_some((p, d.md5))
        }))
}

/// Fetches `url` to `dest`, forwarding progress to the UI.
pub fn fetch_with_progress(
    nx: &Nexus,
    ui: &dyn Ui,
    name: &str,
    req: FetchRequest<'_>,
) -> R<Result<download::Fetched, download::DownloadError>> {
    let fetcher = Fetcher::new()?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let fetch = async {
        let r = fetcher
            .fetch(FetchRequest {
                events: Some(&tx),
                ..req
            })
            .await;
        drop(tx);
        r
    };
    let drain = async {
        while let Some(e) = rx.recv().await {
            ui.download_event(name, &e);
        }
    };
    nx.run(async { tokio::join!(fetch, drain).0 })
}

/// A Nexus file to download.
pub struct FileRequest<'a> {
    pub game: &'a str,
    pub mod_id: u64,
    pub file_id: u64,
    pub info: Option<&'a FileInfo>,
    pub nxm: Option<NxmKey>,
    /// Reuse this `downloads` row (nxm intake).
    pub row: Option<i64>,
}

/// Downloads a Nexus file and returns the archive path and its md5. A
/// `downloads` row is committed before any network I/O.
pub fn download_file(
    cx: &Cx<'_>,
    nx: &Nexus,
    req: FileRequest<'_>,
) -> R<(PathBuf, Option<String>)> {
    let (game, mod_id, file_id) = (req.game, req.mod_id, req.file_id);
    if let Some(found) = existing_download(cx.store, game, mod_id, file_id)? {
        if let Some(r) = req.row {
            download::set_status(cx.store, r, DownloadStatus::Done)?;
        }
        cx.ui
            .info(&format!("Using the earlier download {}", found.0.display()));
        return Ok(found);
    }
    let id = match req.row {
        Some(r) => r,
        None => {
            let mut d = NewDownload::new(DownloadSource::Nexus);
            d.game = Some(game.to_string());
            d.url = Some(format!("nxm://{game}/mods/{mod_id}/files/{file_id}"));
            d.nexus_mod_id = Some(mod_id);
            d.nexus_file_id = Some(file_id);
            d.nxm = req.nxm.clone();
            d.size = req.info.and_then(|f| f.size_in_bytes);
            download::enqueue(cx.store, &d)?
        }
    };
    let res = (|| -> R<(PathBuf, Option<String>)> {
        download::set_status(cx.store, id, DownloadStatus::Downloading)?;
        let premium = is_premium(cx.store, nx)?;
        let cfg = cx.config()?;
        let uri = nx.run(nx.client.download_uri(
            game,
            mod_id,
            file_id,
            premium,
            req.nxm.as_ref(),
            cfg.cdn.as_deref(),
        ))??;
        let fetcher = Fetcher::new()?;
        let name = match download::file_name_from_uri(&uri) {
            Some(n) if n.contains('.') => n,
            _ => nx
                .run(fetcher.head_file_name(&uri))?
                .or_else(|| {
                    req.info
                        .and_then(|f| f.file_name.as_deref())
                        .map(sanitize_name)
                })
                .unwrap_or_else(|| format!("{mod_id}-{file_id}")),
        };
        let dest = downloads_dir(&cfg.data_dir, game).join(&name);
        cx.ui.info(&format!("Downloading {name}..."));
        let fetched = fetch_with_progress(
            nx,
            cx.ui,
            &name,
            FetchRequest {
                url: &uri,
                dest: &dest,
                md5: None,
                sha256: None,
                events: None,
            },
        )??;
        download::mark_done(cx.store, id, &fetched)?;
        Ok((fetched.path, Some(fetched.md5)))
    })();
    if let Err(e) = &res {
        let _ = download::mark_failed(cx.store, id, &super::chain(e));
    }
    res
}

// ── login / account ─────────────────────────────────────────

/// Result of [`login`]. Never carries the key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoginResult {
    pub user: String,
    pub premium: bool,
    /// `keyring` or `file`.
    pub backend: String,
    /// Credentials file when the key went to a file.
    pub file: Option<PathBuf>,
    /// Why the keyring was not used, when the file backend was newly chosen.
    pub keyring_unavailable: Option<String>,
}

/// Validates `raw` with Nexus and stores it (keyring, else a 0600 file).
/// Whitespace is stripped like legacy (L4187).
pub fn login(paths: &Paths, store: &Store, raw: &str) -> R<LoginResult> {
    let raw: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    if raw.is_empty() {
        fail!("API key cannot be empty.");
    }
    let key = ApiKey::new(&raw)?;
    drop(raw);
    let shared = shared_store(paths)?;
    let client = NexusClient::new(
        config()?,
        Some(Arc::new(ApiKeyAuth::new(key.clone()))),
        Some(shared.clone()),
    )?;
    let nexus = Nexus { client, shared };
    let acct = nexus
        .run(nexus.client.validate())?
        .map_err(|e| AppError::Msg(format!("Authentication failed: {e}")))?;
    let storage = KeyStorage::open(paths, keyring_opener(), PROBE_TIMEOUT)?;
    let previous = store.setting(AUTH_BACKEND_KEY)?;
    let backend = storage.save(&key)?;
    store.set_setting(AUTH_BACKEND_KEY, backend.as_str())?;
    store.set_setting(USER_KEY, &acct.name)?;
    store.set_setting(PREMIUM_KEY, if acct.is_premium { "true" } else { "false" })?;
    let keyring_unavailable = (backend == Backend::File && previous.as_deref() != Some("file"))
        .then(|| {
            storage
                .unavailable_reason()
                .unwrap_or("the keyring refused the write")
                .to_string()
        });
    Ok(LoginResult {
        user: acct.name,
        premium: acct.is_premium,
        backend: backend.as_str().to_string(),
        file: (backend == Backend::File).then(|| storage.file_path().to_path_buf()),
        keyring_unavailable,
    })
}

/// Removes the stored key and account facts. `true` when `NEXUS_API_KEY`
/// is still set (and will be used).
pub fn logout(paths: &Paths, store: &Store) -> R<bool> {
    let storage = match store.setting(AUTH_BACKEND_KEY)?.as_deref() {
        Some("keyring") => KeyStorage::open(paths, keyring_opener(), PROBE_TIMEOUT)?,
        _ => KeyStorage::new(paths, ProbeOutcome::Unavailable("logout".into())),
    };
    storage.clear()?;
    for k in [AUTH_BACKEND_KEY, USER_KEY, PREMIUM_KEY] {
        store.delete_setting(k)?;
    }
    Ok(std::env::var_os(API_KEY_ENV).is_some())
}

/// What is known about the account without network access or a keyring
/// probe (legacy `cmd_status` L4323-4345).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountInfo {
    /// Where the key comes from; `None` = not logged in.
    pub key_source: Option<String>,
    pub user: Option<String>,
    /// `None` = unknown tier.
    pub premium: Option<bool>,
    /// Remaining API quota, e.g. `"2400/2500 daily, 98/100 hourly"`.
    pub quota: Option<String>,
}

pub fn account_info(paths: &Paths, store: &Store) -> R<AccountInfo> {
    Ok(AccountInfo {
        key_source: key_source(paths, store)?,
        user: store.setting(USER_KEY)?,
        premium: store.setting(PREMIUM_KEY)?.map(|v| v == "true"),
        quota: RateLimit::load(store)?.map(|rl| rl.summary()),
    })
}

/// The messages `login` shows after success.
pub fn login_msgs(r: &LoginResult) -> Vec<Msg> {
    let mut v = Vec::new();
    if let Some(why) = &r.keyring_unavailable {
        v.push(Msg::warn(format!(
            "Secret Service keyring not available ({why}). The API key is stored in {} (readable only by you).",
            r.file.as_deref().unwrap_or(std::path::Path::new("")).display()
        )));
    }
    v.push(Msg::success("Authenticated as ").b(&r.user));
    if r.premium {
        v.push(Msg::success(
            "Account tier: Premium — direct downloads available ✓",
        ));
    } else {
        v.push(Msg::warn(
            "Account tier: Free — each file needs a click on \"Slow download\" on the website.",
        ));
        v.push(Msg::warn(
            "Run 'vessel nxm register' so the browser can hand links to vessel.",
        ));
    }
    v.push(Msg::success(format!(
        "API key saved ({}).",
        match &r.file {
            Some(f) => f.display().to_string(),
            None => "keyring".to_string(),
        }
    )));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_result_never_serializes_a_key_field() {
        let r = LoginResult {
            user: "u".into(),
            premium: false,
            backend: "file".into(),
            file: Some("/c/credentials.toml".into()),
            keyring_unavailable: None,
        };
        let v = serde_json::to_value(&r).unwrap();
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert!(keys
            .iter()
            .all(|k| !k.contains("key") || *k == "keyring_unavailable"));
    }

    #[test]
    fn empty_keys_are_rejected_before_any_network_call() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::for_home(dir.path());
        let store = Store::open_in_memory().unwrap();
        let e = login(&paths, &store, " \n\t").unwrap_err();
        assert_eq!(e.to_string(), "API key cannot be empty.");
    }
}
