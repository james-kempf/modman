// SPDX-License-Identifier: GPL-3.0-or-later
//! HTTP core (headers, retries, rate limits) and the REST v1 endpoints.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use super::ratelimit::RateLimit;
use super::{AuthProvider, NexusError, NxmKey};
use crate::store::{self, Store};

pub const DEFAULT_REST_BASE: &str = "https://api.nexusmods.com/v1";
pub const DEFAULT_GRAPHQL_URL: &str = "https://api.nexusmods.com/v2/graphql";

/// A store shared between the client and its caller. Never locked across an
/// `.await`, so client futures stay `Send`.
pub type SharedStore = Arc<Mutex<Store>>;

/// Installs the ring rustls provider (reqwest is built with
/// `rustls-no-provider`). Idempotent.
pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Retry back-off (legacy L443-481): 429 waits `rate_limit_step * attempt`,
/// 5xx and network errors wait `server_step * attempt`.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub attempts: u32,
    pub rate_limit_step: Duration,
    pub server_step: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 4,
            rate_limit_step: Duration::from_secs(15),
            server_step: Duration::from_secs(2),
        }
    }
}

/// Client settings. Base URLs are injectable for tests.
#[derive(Debug, Clone)]
pub struct NexusConfig {
    pub rest_base: Url,
    pub graphql_url: Url,
    pub timeout: Duration,
    pub retry: RetryPolicy,
}

impl Default for NexusConfig {
    fn default() -> Self {
        Self {
            rest_base: Url::parse(DEFAULT_REST_BASE).expect("valid default URL"),
            graphql_url: Url::parse(DEFAULT_GRAPHQL_URL).expect("valid default URL"),
            timeout: Duration::from_secs(90),
            retry: RetryPolicy::default(),
        }
    }
}

impl NexusConfig {
    /// REST base `<base>/v1` and GraphQL `<base>/v2/graphql` on one host.
    pub fn with_base(base: &str) -> Result<Self, NexusError> {
        let base = base.trim_end_matches('/');
        let parse = |s: String| Url::parse(&s).map_err(|e| NexusError::BadUrl(e.to_string()));
        Ok(Self {
            rest_base: parse(format!("{base}/v1"))?,
            graphql_url: parse(format!("{base}/v2/graphql"))?,
            ..Self::default()
        })
    }
}

/// `GET /users/validate.json`. The response's `key` field is dropped.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Account {
    pub user_id: u64,
    pub name: String,
    #[serde(default)]
    pub is_premium: bool,
    #[serde(default)]
    pub is_supporter: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct GameInfo {
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub domain_name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ModInfo {
    pub mod_id: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub category_id: Option<u64>,
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub contains_adult_content: bool,
    #[serde(default)]
    pub picture_url: Option<String>,
    #[serde(default)]
    pub updated_timestamp: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FileInfo {
    pub file_id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub category_name: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub size_in_bytes: Option<u64>,
    #[serde(default)]
    pub uploaded_timestamp: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct FileUpdate {
    pub old_file_id: u64,
    pub new_file_id: u64,
}

/// `GET /games/<g>/mods/<m>/files.json`, trimmed to the fields modman uses.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct ModFiles {
    #[serde(default)]
    pub files: Vec<FileInfo>,
    #[serde(default)]
    pub file_updates: Vec<FileUpdate>,
}

/// One CDN entry of `download_link.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct CdnLink {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub short_name: String,
    #[serde(rename = "URI")]
    pub uri: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct UpdatedMod {
    pub mod_id: u64,
    #[serde(default)]
    pub latest_file_update: i64,
    #[serde(default)]
    pub latest_mod_activity: i64,
}

/// Validates a Nexus game domain (`skyrimspecialedition`) before it goes
/// into a URL path.
pub(crate) fn check_domain(game: &str) -> Result<&str, NexusError> {
    let ok = !game.is_empty()
        && game.len() <= 64
        && game
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if ok {
        Ok(game)
    } else {
        Err(NexusError::BadUrl(format!("invalid game domain {game:?}")))
    }
}

/// Percent-encodes the characters legacy encodes in CDN URIs (L117-121).
fn encode_uri(uri: &str) -> String {
    uri.replace(' ', "%20")
        .replace('[', "%5B")
        .replace(']', "%5D")
        .replace('"', "%22")
}

/// Network error text without the URL (which may hold `key=`/`expires=`).
pub(crate) fn network_message(e: reqwest::Error) -> String {
    let e = e.without_url();
    let mut msg = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        src = s.source();
    }
    msg
}

/// Readable message from an API error body (legacy `api_error_msg`).
fn error_message(body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let msg = parsed.as_ref().and_then(|v| {
        v.get("message")
            .or_else(|| v.get("error"))
            .or_else(|| v.pointer("/errors/0/message"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let msg = msg.unwrap_or_else(|| body.chars().take(200).collect());
    if msg.trim().is_empty() {
        "no response body".into()
    } else {
        msg
    }
}

/// Follows redirects only within one origin (scheme, host, port): the
/// `apikey` header is a custom header, which reqwest would otherwise forward
/// to any host a redirect names. A cross-origin redirect is returned as is
/// (and reported as an HTTP error).
fn same_origin_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let same = attempt
            .previous()
            .last()
            .is_some_and(|prev| prev.origin() == attempt.url().origin());
        if !same {
            attempt.stop()
        } else if attempt.previous().len() > 10 {
            attempt.error("too many redirects")
        } else {
            attempt.follow()
        }
    })
}

/// Nexus Mods API client.
pub struct NexusClient {
    http: reqwest::Client,
    cfg: NexusConfig,
    auth: Option<Arc<dyn AuthProvider>>,
    rate: Mutex<Option<RateLimit>>,
    store: Option<SharedStore>,
}

impl std::fmt::Debug for NexusClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NexusClient")
            .field("cfg", &self.cfg)
            .field("auth", &self.auth)
            .field("persisted", &self.store.is_some())
            .finish_non_exhaustive()
    }
}

impl NexusClient {
    /// `store`, when given, persists the rate-limit state and seeds it.
    pub fn new(
        cfg: NexusConfig,
        auth: Option<Arc<dyn AuthProvider>>,
        store: Option<SharedStore>,
    ) -> Result<Self, NexusError> {
        install_crypto_provider();
        let http = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .redirect(same_origin_redirects())
            .build()
            .map_err(network_message)
            .map_err(NexusError::Network)?;
        let rate = match &store {
            Some(s) => RateLimit::load(&s.lock().unwrap_or_else(|p| p.into_inner()))?,
            None => None,
        };
        Ok(Self {
            http,
            cfg,
            auth,
            rate: Mutex::new(rate),
            store,
        })
    }

    pub fn config(&self) -> &NexusConfig {
        &self.cfg
    }

    pub fn is_authenticated(&self) -> bool {
        self.auth.is_some()
    }

    /// Last rate-limit state seen (or loaded from the store).
    pub fn rate_limit(&self) -> Option<RateLimit> {
        self.rate.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Errors with [`NexusError::RateLimitLow`] when fewer than `reserve`
    /// hourly requests are known to be left.
    pub fn ensure_budget(&self, reserve: u32) -> Result<(), NexusError> {
        match self.rate_limit() {
            Some(rl) => rl.ensure_budget(reserve),
            None => Ok(()),
        }
    }

    fn record_rate_limit(&self, headers: &HeaderMap) {
        let Some(rl) = RateLimit::from_headers(headers, store::now()) else {
            return;
        };
        if let Some(s) = &self.store {
            let guard = s.lock().unwrap_or_else(|p| p.into_inner());
            if let Err(e) = rl.save(&guard) {
                tracing::warn!(error = %e, "could not persist Nexus rate-limit state");
            }
        }
        *self.rate.lock().unwrap_or_else(|p| p.into_inner()) = Some(rl);
    }

    fn redact(&self, text: &str, extra: Option<&str>) -> String {
        let mut t = match &self.auth {
            Some(a) => a.redact(text),
            None => text.to_owned(),
        };
        if let Some(s) = extra.filter(|s| !s.is_empty()) {
            t = t.replace(s, "***");
        }
        t
    }

    fn headers(&self) -> Result<HeaderMap, NexusError> {
        let mut h = HeaderMap::new();
        h.insert(ACCEPT, HeaderValue::from_static("application/json"));
        h.insert("Application-Name", HeaderValue::from_static("modman"));
        h.insert(
            "Application-Version",
            HeaderValue::from_static(crate::VERSION),
        );
        let ua = format!("modman/{} (Linux; rust)", crate::VERSION);
        h.insert(
            USER_AGENT,
            HeaderValue::from_str(&ua).map_err(|e| NexusError::Decode(e.to_string()))?,
        );
        if let Some(auth) = &self.auth {
            auth.apply(&mut h)?;
        }
        Ok(h)
    }

    /// Sends with retries and returns the body of a 2xx response.
    /// `what` names the resource for `NotFound`; `secret` is an extra value
    /// (nxm key) scrubbed from error messages.
    pub(crate) async fn send(
        &self,
        method: Method,
        url: Url,
        body: Option<&Value>,
        what: &str,
        secret: Option<&str>,
    ) -> Result<String, NexusError> {
        let policy = &self.cfg.retry;
        let attempts = policy.attempts.max(1);
        let mut attempt = 0;
        loop {
            attempt += 1;
            if let Some(rl) = self.rate_limit() {
                rl.check(store::now())?;
            }
            tracing::debug!(%method, host = url.host_str().unwrap_or(""), path = url.path(), attempt, "Nexus request");
            let mut req = self
                .http
                .request(method.clone(), url.clone())
                .headers(self.headers()?);
            if let Some(b) = body {
                req = req.json(b);
            }
            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    let msg = self.redact(&network_message(e), secret);
                    if attempt < attempts {
                        tracing::debug!(error = %msg, "network error; retrying");
                        tokio::time::sleep(policy.server_step * attempt).await;
                        continue;
                    }
                    return Err(NexusError::Network(msg));
                }
            };
            self.record_rate_limit(resp.headers());
            let status = resp.status();
            let text = resp
                .text()
                .await
                .map_err(|e| NexusError::Network(self.redact(&network_message(e), secret)));
            if status.is_success() {
                return text;
            }
            let text = text.unwrap_or_default();
            tracing::debug!(
                status = status.as_u16(),
                path = url.path(),
                "Nexus HTTP error"
            );
            match status {
                StatusCode::TOO_MANY_REQUESTS if attempt < attempts => {
                    let wait = policy.rate_limit_step * attempt;
                    tracing::warn!(
                        wait_s = wait.as_secs(),
                        "Nexus rate limit hit; waiting before retrying"
                    );
                    tokio::time::sleep(wait).await;
                    continue;
                }
                StatusCode::TOO_MANY_REQUESTS => {
                    return Err(self.rate_limit().map(|r| r.limited()).unwrap_or(
                        NexusError::RateLimited {
                            hourly: "?".into(),
                            daily: "?".into(),
                            reset: "unknown".into(),
                        },
                    ))
                }
                s if s.is_server_error() && attempt < attempts => {
                    tokio::time::sleep(policy.server_step * attempt).await;
                    continue;
                }
                StatusCode::UNAUTHORIZED => return Err(NexusError::Unauthorized),
                StatusCode::NOT_FOUND => return Err(NexusError::NotFound(what.to_owned())),
                s => {
                    return Err(NexusError::Http {
                        status: s.as_u16(),
                        message: self.redact(&error_message(&text), secret),
                    })
                }
            }
        }
    }

    fn rest_url(&self, path: &str, query: &[(&str, &str)]) -> Result<Url, NexusError> {
        let base = self.cfg.rest_base.as_str().trim_end_matches('/');
        let mut url =
            Url::parse(&format!("{base}{path}")).map_err(|e| NexusError::BadUrl(e.to_string()))?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        Ok(url)
    }

    async fn rest_get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
        what: &str,
        secret: Option<&str>,
    ) -> Result<T, NexusError> {
        let url = self.rest_url(path, query)?;
        let body = self.send(Method::GET, url, None, what, secret).await?;
        serde_json::from_str(&body).map_err(|e| NexusError::Decode(format!("{what}: {e}")))
    }

    fn require_auth(&self) -> Result<(), NexusError> {
        if self.auth.is_some() {
            Ok(())
        } else {
            Err(NexusError::NotLoggedIn)
        }
    }

    /// Validates the key and reports the account tier.
    pub async fn validate(&self) -> Result<Account, NexusError> {
        self.require_auth()?;
        self.rest_get("/users/validate.json", &[], "account", None)
            .await
    }

    pub async fn game(&self, game: &str) -> Result<GameInfo, NexusError> {
        let g = check_domain(game)?;
        self.rest_get(&format!("/games/{g}.json"), &[], &format!("game {g}"), None)
            .await
    }

    pub async fn mod_info(&self, game: &str, mod_id: u64) -> Result<ModInfo, NexusError> {
        let g = check_domain(game)?;
        self.rest_get(
            &format!("/games/{g}/mods/{mod_id}.json"),
            &[],
            &format!("mod {mod_id}"),
            None,
        )
        .await
    }

    pub async fn mod_files(&self, game: &str, mod_id: u64) -> Result<ModFiles, NexusError> {
        let g = check_domain(game)?;
        self.rest_get(
            &format!("/games/{g}/mods/{mod_id}/files.json"),
            &[],
            &format!("files of mod {mod_id}"),
            None,
        )
        .await
    }

    /// `period` is `1d`, `1w` or `1m`.
    pub async fn updated_mods(
        &self,
        game: &str,
        period: &str,
    ) -> Result<Vec<UpdatedMod>, NexusError> {
        let g = check_domain(game)?;
        if !matches!(period, "1d" | "1w" | "1m") {
            return Err(NexusError::BadUrl(format!("invalid period {period:?}")));
        }
        self.rest_get(
            &format!("/games/{g}/mods/updated.json"),
            &[("period", period)],
            "updated mods",
            None,
        )
        .await
    }

    /// Raw CDN list. Premium accounts pass `nxm = None`; free accounts must
    /// pass the key from an nxm:// link.
    pub async fn download_links(
        &self,
        game: &str,
        mod_id: u64,
        file_id: u64,
        nxm: Option<&NxmKey>,
    ) -> Result<Vec<CdnLink>, NexusError> {
        self.require_auth()?;
        let g = check_domain(game)?;
        let expires;
        let query: Vec<(&str, &str)> = match nxm {
            Some(k) => {
                if k.is_expired(store::now()) {
                    return Err(NexusError::NxmKeyExpired);
                }
                expires = k.expires.to_string();
                vec![("key", k.key.as_str()), ("expires", expires.as_str())]
            }
            None => Vec::new(),
        };
        let path = format!("/games/{g}/mods/{mod_id}/files/{file_id}/download_link.json");
        let what = format!("file {file_id} of mod {mod_id}");
        let secret = nxm.map(|k| k.key.as_str());
        match self.rest_get(&path, &query, &what, secret).await {
            // Free account without a key (or a refused key).
            Err(NexusError::Http { status: 403, .. }) if nxm.is_none() => {
                Err(NexusError::NxmKeyRequired)
            }
            other => other,
        }
    }

    /// The download URI for a file: premium accounts get a direct link,
    /// free accounts need `nxm`. `cdn` picks a CDN by short name or name
    /// (config `cdn`), else the first entry.
    pub async fn download_uri(
        &self,
        game: &str,
        mod_id: u64,
        file_id: u64,
        is_premium: bool,
        nxm: Option<&NxmKey>,
        cdn: Option<&str>,
    ) -> Result<String, NexusError> {
        if nxm.is_none() && !is_premium {
            return Err(NexusError::NxmKeyRequired);
        }
        let links = self.download_links(game, mod_id, file_id, nxm).await?;
        let pick = cdn
            .filter(|c| !c.is_empty())
            .and_then(|c| links.iter().find(|l| l.short_name == c || l.name == c))
            .or_else(|| links.first())
            .ok_or(NexusError::NoDownloadLink)?;
        if pick.uri.is_empty() {
            return Err(NexusError::NoDownloadLink);
        }
        Ok(encode_uri(&pick.uri))
    }

    pub(crate) async fn post_graphql(&self, payload: &Value) -> Result<String, NexusError> {
        self.send(
            Method::POST,
            self.cfg.graphql_url.clone(),
            Some(payload),
            "GraphQL endpoint",
            None,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_validation_blocks_path_injection() {
        assert!(check_domain("skyrimspecialedition").is_ok());
        for bad in ["", "../x", "a/b", "Skyrim", "a.b", "a?b"] {
            assert!(check_domain(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn uri_encoding_matches_legacy() {
        assert_eq!(
            encode_uri("https://c/a b[1]\"x\".7z?x=1"),
            "https://c/a%20b%5B1%5D%22x%22.7z?x=1"
        );
    }

    #[test]
    fn error_message_prefers_json_message() {
        assert_eq!(error_message(r#"{"message":"nope"}"#), "nope");
        assert_eq!(error_message(r#"{"errors":[{"message":"g"}]}"#), "g");
        assert_eq!(error_message("plain"), "plain");
        assert_eq!(error_message(""), "no response body");
    }

    #[test]
    fn with_base_builds_both_endpoints() {
        let c = NexusConfig::with_base("http://127.0.0.1:9/").unwrap();
        assert_eq!(c.rest_base.as_str(), "http://127.0.0.1:9/v1");
        assert_eq!(c.graphql_url.as_str(), "http://127.0.0.1:9/v2/graphql");
    }
}
