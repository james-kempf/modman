// SPDX-License-Identifier: GPL-3.0-or-later
//! Nexus glue for commands: API key lookup, client construction, account
//! tier, game ids and file downloads into `<data-dir>/<game>/downloads`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use modman_core::blocking::block_on;
use modman_core::config::Config;
use modman_core::download::{
    self, DownloadSource, DownloadStatus, FetchRequest, Fetcher, NewDownload,
};
use modman_core::games::{self, GameConfig};
use modman_core::mods::downloads_dir;
use modman_core::nexus::{
    ApiKey, ApiKeyAuth, FileInfo, KeyStorage, NexusClient, NexusConfig, NxmKey, ProbeOutcome,
    SharedStore, API_KEY_ENV,
};
use modman_core::staging::sanitize_name;
use modman_core::store::Store;

use crate::context::Ctx;

/// Where the API key lives (`keyring` or `file`); absent = never logged in.
pub const AUTH_BACKEND_KEY: &str = "auth.backend";
pub const USER_KEY: &str = "nexus.user";
pub const PREMIUM_KEY: &str = "nexus.premium";
/// Test hook: Nexus API base URL (REST `<base>/v1`, GraphQL `<base>/v2/graphql`).
pub const BASE_URL_ENV: &str = "MODMAN_NEXUS_URL";

/// Client settings, honouring [`BASE_URL_ENV`].
pub fn config() -> Result<NexusConfig> {
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
pub fn storage_for_read(ctx: &Ctx) -> Result<Option<KeyStorage>> {
    Ok(match ctx.store.setting(AUTH_BACKEND_KEY)?.as_deref() {
        Some("keyring") => Some(KeyStorage::open(
            &ctx.paths,
            modman_core::nexus::keyring_opener(),
            modman_core::nexus::PROBE_TIMEOUT,
        )?),
        Some(_) => Some(KeyStorage::new(
            &ctx.paths,
            ProbeOutcome::Unavailable("file backend selected at login".into()),
        )),
        None => None,
    })
}

/// The API key: `NEXUS_API_KEY`, else the stored one.
pub fn load_key(ctx: &Ctx) -> Result<Option<ApiKey>> {
    if let Some(k) = env_key() {
        return Ok(Some(ApiKey::new(&k)?));
    }
    match storage_for_read(ctx)? {
        Some(s) => Ok(s.load(None)?),
        None => Ok(None),
    }
}

/// A short description of where the key comes from, without loading it.
pub fn key_source(ctx: &Ctx) -> Result<Option<String>> {
    if env_key().is_some() {
        return Ok(Some(format!("from {API_KEY_ENV}")));
    }
    Ok(match ctx.store.setting(AUTH_BACKEND_KEY)?.as_deref() {
        Some("keyring") => Some("stored in the keyring".into()),
        Some(_) => {
            let f = ctx
                .paths
                .config_dir
                .join(modman_core::nexus::CREDENTIALS_FILE);
            f.exists().then(|| format!("stored in {}", f.display()))
        }
        None => None,
    })
}

/// A second connection to the database for the client (rate-limit state,
/// caches); SQLite WAL lets both coexist.
pub fn shared_store(ctx: &Ctx) -> Result<SharedStore> {
    Ok(Arc::new(Mutex::new(Store::open(&ctx.paths.db_path())?)))
}

pub struct Nexus {
    pub client: NexusClient,
    pub shared: SharedStore,
}

impl Nexus {
    pub fn run<F: std::future::Future>(&self, f: F) -> Result<F::Output> {
        block_on(f).context("cannot start the async runtime")
    }
}

/// Client with the stored key. `require` fails when not logged in;
/// otherwise an anonymous client is returned.
pub fn connect(ctx: &Ctx, require: bool) -> Result<Option<Nexus>> {
    let key = load_key(ctx)?;
    if key.is_none() && require {
        bail!("Not logged in to Nexus Mods. Run: modman login");
    }
    let shared = shared_store(ctx)?;
    let auth =
        key.map(|k| Arc::new(ApiKeyAuth::new(k)) as Arc<dyn modman_core::nexus::AuthProvider>);
    if auth.is_none() && require {
        return Ok(None);
    }
    let client = NexusClient::new(config()?, auth, Some(shared.clone()))?;
    Ok(Some(Nexus { client, shared }))
}

/// Logged-in client or an error.
pub fn require(ctx: &Ctx) -> Result<Nexus> {
    connect(ctx, true)?.ok_or_else(|| anyhow!("Not logged in to Nexus Mods. Run: modman login"))
}

/// Premium flag; asks Nexus once when unknown (legacy `refresh_account`).
pub fn is_premium(ctx: &Ctx, nx: &Nexus) -> Result<bool> {
    if let Some(v) = ctx.store.setting(PREMIUM_KEY)? {
        return Ok(v == "true");
    }
    let acct = nx.run(nx.client.validate())??;
    ctx.store.set_setting(USER_KEY, &acct.name)?;
    ctx.store
        .set_setting(PREMIUM_KEY, if acct.is_premium { "true" } else { "false" })?;
    Ok(acct.is_premium)
}

/// Numeric Nexus game id, fetched once and saved with the game.
pub fn game_id(ctx: &Ctx, nx: &Nexus, game: &mut GameConfig) -> Result<u64> {
    if let Some(id) = game.nexus_game_id.and_then(|i| u64::try_from(i).ok()) {
        return Ok(id);
    }
    let info = nx.run(nx.client.game(&game.domain))??;
    game.nexus_game_id = Some(info.id as i64);
    games::save_game(ctx.store.conn(), game)?;
    Ok(info.id)
}

/// A finished download of this file whose archive still exists.
fn existing_download(
    ctx: &Ctx,
    game: &str,
    mod_id: u64,
    file_id: u64,
) -> Result<Option<(PathBuf, Option<String>)>> {
    let done = download::list(&ctx.store, Some(DownloadStatus::Done))?;
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

/// Downloads a Nexus file (legacy `download_nexus_file`, L1482-1512) and
/// returns the archive path and its md5. A `downloads` row is committed
/// before any network I/O; `row` reuses an existing one (nxm intake).
#[allow(clippy::too_many_arguments)]
pub fn download_file(
    ctx: &Ctx,
    nx: &Nexus,
    game: &str,
    mod_id: u64,
    file_id: u64,
    info: Option<&FileInfo>,
    nxm: Option<NxmKey>,
    row: Option<i64>,
) -> Result<(PathBuf, Option<String>)> {
    if let Some(found) = existing_download(ctx, game, mod_id, file_id)? {
        if let Some(r) = row {
            download::set_status(&ctx.store, r, DownloadStatus::Done)?;
        }
        ctx.out
            .info(&format!("Using the earlier download {}", found.0.display()));
        return Ok(found);
    }
    let id = match row {
        Some(r) => r,
        None => {
            let mut d = NewDownload::new(DownloadSource::Nexus);
            d.game = Some(game.to_string());
            d.url = Some(format!("nxm://{game}/mods/{mod_id}/files/{file_id}"));
            d.nexus_mod_id = Some(mod_id);
            d.nexus_file_id = Some(file_id);
            d.nxm = nxm.clone();
            d.size = info.and_then(|f| f.size_in_bytes);
            download::enqueue(&ctx.store, &d)?
        }
    };
    let res = (|| -> Result<(PathBuf, Option<String>)> {
        download::set_status(&ctx.store, id, DownloadStatus::Downloading)?;
        let premium = is_premium(ctx, nx)?;
        let cfg = Config::load(&ctx.store, &ctx.paths)?;
        let uri = nx.run(nx.client.download_uri(
            game,
            mod_id,
            file_id,
            premium,
            nxm.as_ref(),
            cfg.cdn.as_deref(),
        ))??;
        let fetcher = Fetcher::new()?;
        let name = match download::file_name_from_uri(&uri) {
            Some(n) if n.contains('.') => n,
            _ => nx
                .run(fetcher.head_file_name(&uri))?
                .or_else(|| info.and_then(|f| f.file_name.as_deref()).map(sanitize_name))
                .unwrap_or_else(|| format!("{mod_id}-{file_id}")),
        };
        let dest = downloads_dir(&cfg.data_dir, game).join(&name);
        ctx.out.info(&format!("Downloading {name}..."));
        let fetched = nx.run(fetcher.fetch(FetchRequest {
            url: &uri,
            dest: &dest,
            md5: None,
            sha256: None,
            events: None,
        }))??;
        download::mark_done(&ctx.store, id, &fetched)?;
        Ok((fetched.path, Some(fetched.md5)))
    })();
    if let Err(e) = &res {
        let _ = download::mark_failed(&ctx.store, id, &format!("{e:#}"));
    }
    res
}
