// SPDX-License-Identifier: GPL-3.0-or-later
//! Fetches a loader archive from a GitHub release (`[loaders.github]`).
//!
//! The release metadata comes from the unauthenticated REST API; the asset
//! is downloaded with [`Fetcher`]. Everything GitHub returns is untrusted:
//! the download URL is pinned to the repo and tag, redirects may only go to
//! GitHub's own hosts over https, both bodies are capped, and the file must
//! have the size (and sha256, when published) the API announced. Nothing
//! downloaded here is ever executed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{ACCEPT, RETRY_AFTER, USER_AGENT};
use reqwest::StatusCode;
use serde::Deserialize;
use url::{Origin, Url};

use super::glob::Glob;
use super::types::DownloadOrigin;
use crate::download::{DownloadError, FetchRequest, Fetcher};

const API_BASE: &str = "https://api.github.com";
/// Cap on the releases API JSON body.
const API_MAX_BYTES: u64 = 1024 * 1024;
/// Cap on a release asset.
const ASSET_MAX_BYTES: u64 = 200 * 1024 * 1024;
/// Hosts a release download may be served from (GitHub redirects
/// `github.com/.../releases/download/...` to its asset CDN).
const DOWNLOAD_HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GithubError {
    #[error("'{0}' is not a GitHub repository of the form owner/repo")]
    InvalidRepo(String),
    #[error("invalid asset pattern: {0}")]
    InvalidGlob(String),
    #[error("the GitHub API rate limit was hit; {}", reset_text(*.resets_in_secs))]
    RateLimited { resets_in_secs: Option<u64> },
    #[error("GitHub API request failed: {0}")]
    Api(String),
    #[error("GitHub release of {repo} rejected: {reason}")]
    Release { repo: String, reason: String },
    #[error("downloaded {got} bytes, but GitHub announced {expected}")]
    SizeMismatch { got: u64, expected: u64 },
    #[error(transparent)]
    Download(#[from] DownloadError),
}

fn reset_text(secs: Option<u64>) -> String {
    match secs {
        Some(s) => {
            let mins = s.div_ceil(60).max(1);
            format!("it resets in about {mins} minute(s)")
        }
        None => "try again later".into(),
    }
}

/// A release asset saved to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedAsset {
    pub path: PathBuf,
    /// Always [`DownloadOrigin::GitHub`].
    pub origin: DownloadOrigin,
    /// The release tag.
    pub version: String,
}

/// Downloads the single asset matching `asset_glob` from the latest
/// non-prerelease release of `repo` (`owner/repo`) into `dest_dir`.
pub async fn fetch_latest_asset(
    repo: &str,
    asset_glob: &Glob,
    dest_dir: &Path,
) -> Result<FetchedAsset, GithubError> {
    Source::production()
        .fetch_latest(repo, asset_glob, dest_dir)
        .await
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    size: u64,
    browser_download_url: String,
    /// `sha256:<hex>` on assets uploaded since mid-2025.
    #[serde(default)]
    digest: Option<String>,
}

/// Endpoints and limits. Only [`Source::production`] exists outside tests,
/// so no caller can point the fetcher at another host.
struct Source {
    api_base: String,
    download_origin: Origin,
    redirect_origins: Vec<Origin>,
    api_max: u64,
    asset_max: u64,
    attempts: u32,
    retry_step: Duration,
}

fn https_origin(host: &str) -> Origin {
    Url::parse(&format!("https://{host}/"))
        .expect("constant host")
        .origin()
}

impl Source {
    fn production() -> Self {
        Self {
            api_base: API_BASE.into(),
            download_origin: https_origin("github.com"),
            redirect_origins: DOWNLOAD_HOSTS.iter().map(|h| https_origin(h)).collect(),
            api_max: API_MAX_BYTES,
            asset_max: ASSET_MAX_BYTES,
            attempts: 3,
            retry_step: Duration::from_secs(2),
        }
    }

    /// Points at a mock server: downloads must come from `api_base`'s
    /// origin, redirects may also go to `extra_redirects` origins.
    #[cfg(test)]
    fn for_test(api_base: &str, extra_redirects: &[&str]) -> Self {
        let origin = |u: &str| Url::parse(u).unwrap().origin();
        let base = origin(api_base);
        let mut redirect_origins = vec![base.clone()];
        redirect_origins.extend(extra_redirects.iter().map(|u| origin(u)));
        Self {
            api_base: api_base.trim_end_matches('/').into(),
            download_origin: base,
            redirect_origins,
            api_max: API_MAX_BYTES,
            asset_max: ASSET_MAX_BYTES,
            attempts: 1,
            retry_step: Duration::ZERO,
        }
    }

    async fn fetch_latest(
        &self,
        repo: &str,
        asset_glob: &Glob,
        dest_dir: &Path,
    ) -> Result<FetchedAsset, GithubError> {
        if !valid_repo(repo) {
            return Err(GithubError::InvalidRepo(repo.into()));
        }
        asset_glob.check().map_err(GithubError::InvalidGlob)?;
        let release = self.latest_release(repo).await?;
        let reject = |reason: String| GithubError::Release {
            repo: repo.into(),
            reason,
        };
        if release.draft || release.prerelease {
            return Err(reject(format!(
                "release '{}' is a draft or prerelease",
                release.tag_name
            )));
        }
        let tag = release.tag_name;
        if !valid_tag(&tag) {
            return Err(reject("the release tag is empty or malformed".into()));
        }
        let matching: Vec<&Asset> = release
            .assets
            .iter()
            .filter(|a| asset_glob.matches(&a.name))
            .collect();
        let asset = match matching.as_slice() {
            [a] => *a,
            [] => {
                return Err(reject(format!(
                    "no asset of release '{tag}' matches '{}'",
                    asset_glob.as_str()
                )))
            }
            many => {
                return Err(reject(format!(
                    "{} assets of release '{tag}' match '{}'; expected exactly one",
                    many.len(),
                    asset_glob.as_str()
                )))
            }
        };
        if !valid_asset_name(&asset.name) {
            return Err(reject(format!("unsafe asset name '{}'", asset.name)));
        }
        if asset.size > self.asset_max {
            return Err(reject(format!(
                "asset '{}' is {} bytes, over the {}-byte limit",
                asset.name, asset.size, self.asset_max
            )));
        }
        let sha256 = match asset.digest.as_deref() {
            Some(d) => sha256_digest(d).map_err(reject)?,
            None => None,
        };
        let url = self
            .pin_download_url(&asset.browser_download_url, repo, &tag, &asset.name)
            .map_err(reject)?;

        let dest = dest_dir.join(&asset.name);
        let part = Fetcher::part_path(&dest);
        // A stale partial file would be resumed; start clean instead.
        let _ = tokio::fs::remove_file(&part).await;
        let result = self.download(&url, &dest, asset, sha256.as_deref()).await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&part).await;
            let _ = tokio::fs::remove_file(&dest).await;
        }
        result?;
        Ok(FetchedAsset {
            path: dest,
            origin: DownloadOrigin::GitHub {
                repo: repo.into(),
                tag: tag.clone(),
                asset: asset.name.clone(),
            },
            version: tag,
        })
    }

    async fn download(
        &self,
        url: &str,
        dest: &Path,
        asset: &Asset,
        sha256: Option<&str>,
    ) -> Result<(), GithubError> {
        let allowed = self.redirect_origins.clone();
        let fetcher = Fetcher::new()?
            .with_retry(self.attempts, self.retry_step)
            // The announced size is the most the body may be.
            .with_max_bytes(asset.size.min(self.asset_max))
            .with_redirect_check(Arc::new(move |u: &str| redirect_allowed(&allowed, u)))?;
        let fetched = fetcher
            .fetch(FetchRequest {
                url,
                dest,
                md5: None,
                sha256,
                events: None,
            })
            .await?;
        if fetched.size != asset.size {
            return Err(GithubError::SizeMismatch {
                got: fetched.size,
                expected: asset.size,
            });
        }
        Ok(())
    }

    async fn latest_release(&self, repo: &str) -> Result<Release, GithubError> {
        crate::nexus::install_crypto_provider();
        let api = |e: reqwest::Error| GithubError::Api(crate::nexus::client_network_message(e));
        let base = self.api_base.clone();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(60))
            // Renamed repos redirect within the API host only.
            .redirect(reqwest::redirect::Policy::custom(move |a| {
                if a.previous().len() < 5 && a.url().as_str().starts_with(&format!("{base}/")) {
                    a.follow()
                } else {
                    a.error("redirect target not allowed")
                }
            }))
            .build()
            .map_err(api)?;
        let mut resp = http
            .get(format!("{}/repos/{repo}/releases/latest", self.api_base))
            .header(USER_AGENT, format!("vessel/{}", crate::VERSION))
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(api)?;
        let status = resp.status();
        if let Some(secs) = rate_limited(status, resp.headers()) {
            return Err(GithubError::RateLimited {
                resets_in_secs: secs,
            });
        }
        if status == StatusCode::NOT_FOUND {
            return Err(GithubError::Api(format!(
                "{repo} has no published release (HTTP 404)"
            )));
        }
        if !status.is_success() {
            return Err(GithubError::Api(format!("HTTP {}", status.as_u16())));
        }
        let too_large = || GithubError::Api(format!("response over {} bytes", self.api_max));
        if resp.content_length().is_some_and(|n| n > self.api_max) {
            return Err(too_large());
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(api)? {
            if (body.len() + chunk.len()) as u64 > self.api_max {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body)
            .map_err(|e| GithubError::Api(format!("unexpected release JSON: {e}")))
    }

    /// Accepts only `https://github.com/{repo}/releases/download/{tag}/{name}`
    /// (repo case-insensitive, tag and name percent-decoded).
    fn pin_download_url(
        &self,
        raw: &str,
        repo: &str,
        tag: &str,
        name: &str,
    ) -> Result<String, String> {
        let bad = || "the asset download URL is not this release's".to_string();
        let url = Url::parse(raw).map_err(|_| bad())?;
        if url.origin() != self.download_origin
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(bad());
        }
        let segs: Vec<&str> = url.path_segments().ok_or_else(bad)?.collect();
        let (owner, repo_name) = repo.split_once('/').ok_or_else(bad)?;
        let ok = segs.len() >= 6
            && segs[0].eq_ignore_ascii_case(owner)
            && segs[1].eq_ignore_ascii_case(repo_name)
            && segs[2] == "releases"
            && segs[3] == "download"
            && percent_decode(&segs[4..segs.len() - 1].join("/")).as_deref() == Some(tag)
            && percent_decode(segs[segs.len() - 1]).as_deref() == Some(name);
        if ok {
            Ok(url.into())
        } else {
            Err(bad())
        }
    }
}

fn redirect_allowed(allowed: &[Origin], target: &str) -> bool {
    Url::parse(target).is_ok_and(|u| {
        u.username().is_empty() && u.password().is_none() && allowed.contains(&u.origin())
    })
}

/// `Some(resets_in)` when the response is GitHub's rate limit.
fn rate_limited(status: StatusCode, headers: &reqwest::header::HeaderMap) -> Option<Option<u64>> {
    let header = |n: &str| {
        headers
            .get(n)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().to_owned())
    };
    let exhausted = header("x-ratelimit-remaining").as_deref() == Some("0");
    if !(status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::FORBIDDEN && exhausted) {
        return None;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let from_reset = header("x-ratelimit-reset")
        .and_then(|v| v.parse::<u64>().ok())
        .map(|at| at.saturating_sub(now));
    let from_retry = header(RETRY_AFTER.as_str()).and_then(|v| v.parse::<u64>().ok());
    Some(from_reset.or(from_retry))
}

/// `owner/repo`, each part `[A-Za-z0-9._-]+` and not `.`/`..`.
fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let part_ok = |p: Option<&str>| {
        p.is_some_and(|p| {
            !p.is_empty()
                && !p.contains("..")
                && p != "."
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
    };
    part_ok(parts.next()) && part_ok(parts.next()) && parts.next().is_none()
}

fn valid_tag(tag: &str) -> bool {
    !tag.is_empty() && tag.len() <= 255 && !tag.chars().any(char::is_control)
}

/// A plain file name: no separators, `..`, leading dot or control chars.
fn valid_asset_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('.')
        && !name.contains("..")
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
}

/// The hex of a `sha256:<hex>` digest; other algorithms are ignored.
fn sha256_digest(d: &str) -> Result<Option<String>, String> {
    let Some((algo, hex)) = d.split_once(':') else {
        return Err(format!("malformed asset digest '{d}'"));
    };
    if !algo.eq_ignore_ascii_case("sha256") {
        return Ok(None);
    }
    if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(Some(hex.to_ascii_lowercase()))
    } else {
        Err(format!("malformed sha256 digest '{d}'"))
    }
}

/// Strict percent-decoding: `None` on a bad escape or invalid UTF-8.
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const REPO: &str = "LavaGang/MelonLoader";
    const TAG: &str = "v0.7.3";
    const NAME: &str = "MelonLoader.x64.zip";
    const API_PATH: &str = "/repos/LavaGang/MelonLoader/releases/latest";

    fn dl_path(name: &str) -> String {
        format!("/{REPO}/releases/download/{TAG}/{name}")
    }

    fn asset(server: &MockServer, name: &str, size: usize) -> serde_json::Value {
        json!({
            "name": name,
            "size": size,
            "browser_download_url": format!("{}{}", server.uri(), dl_path(name)),
        })
    }

    fn release(assets: Vec<serde_json::Value>) -> serde_json::Value {
        json!({ "tag_name": TAG, "draft": false, "prerelease": false, "assets": assets })
    }

    async fn serve_api(server: &MockServer, body: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path(API_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    async fn serve_file(server: &MockServer, p: &str, body: &[u8]) {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(server)
            .await;
    }

    fn glob() -> Glob {
        Glob::new("MelonLoader.x64.zip")
    }

    async fn run(
        server: &MockServer,
        g: &Glob,
        extra: &[&str],
    ) -> (tempfile::TempDir, Result<FetchedAsset, GithubError>) {
        let dir = tempfile::tempdir().unwrap();
        let r = Source::for_test(&server.uri(), extra)
            .fetch_latest(REPO, g, dir.path())
            .await;
        (dir, r)
    }

    fn assert_empty(dir: &tempfile::TempDir) {
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .map(|r| r.map(|e| e.unwrap().file_name()).collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "files left behind: {left:?}");
    }

    async fn downloads(server: &MockServer) -> usize {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path().contains("/releases/download/"))
            .count()
    }

    fn sha(data: &[u8]) -> String {
        Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[tokio::test]
    async fn happy_path_writes_the_asset_and_reports_its_origin() {
        let server = MockServer::start().await;
        let data = b"zip bytes".to_vec();
        let mut a = asset(&server, NAME, data.len());
        a["digest"] = json!(format!("sha256:{}", sha(&data)));
        let other = asset(&server, "MelonLoader.x86.zip", 3);
        serve_api(&server, release(vec![a, other])).await;
        serve_file(&server, &dl_path(NAME), &data).await;
        let (dir, r) = run(&server, &glob(), &[]).await;
        let f = r.unwrap();
        assert_eq!(f.path, dir.path().join(NAME));
        assert_eq!(std::fs::read(&f.path).unwrap(), data);
        assert_eq!(f.version, TAG);
        assert_eq!(
            f.origin,
            DownloadOrigin::GitHub {
                repo: REPO.into(),
                tag: TAG.into(),
                asset: NAME.into()
            }
        );
        let ua = &server.received_requests().await.unwrap()[0].headers;
        assert_eq!(
            ua.get("user-agent").unwrap().to_str().unwrap(),
            format!("vessel/{}", crate::VERSION)
        );
    }

    /// Runs a fetch whose API answer is `body` and expects a rejection
    /// before any download, with nothing left in the destination.
    async fn rejected_before_download(body: serde_json::Value, g: &Glob, server: &MockServer) {
        serve_api(server, body).await;
        let (dir, r) = run(server, g, &[]).await;
        let e = r.unwrap_err();
        assert!(matches!(e, GithubError::Release { .. }), "{e}");
        assert_eq!(downloads(server).await, 0);
        assert_empty(&dir);
    }

    #[tokio::test]
    async fn zero_matching_assets_is_rejected() {
        let s = MockServer::start().await;
        let body = release(vec![asset(&s, "Other.zip", 1)]);
        rejected_before_download(body, &glob(), &s).await;
    }

    #[tokio::test]
    async fn multiple_matching_assets_are_rejected() {
        let s = MockServer::start().await;
        let body = release(vec![asset(&s, "a.zip", 1), asset(&s, "b.zip", 1)]);
        rejected_before_download(body, &Glob::new("*.zip"), &s).await;
    }

    #[tokio::test]
    async fn unsafe_asset_names_are_rejected() {
        for (name, g) in [
            ("../evil.zip", "**"),
            ("sub/evil.zip", "**"),
            ("evil..zip", "*.zip"),
            ("a\\evil.zip", "*.zip"),
        ] {
            let s = MockServer::start().await;
            let body = release(vec![asset(&s, name, 1)]);
            rejected_before_download(body, &Glob::new(g), &s).await;
        }
    }

    #[tokio::test]
    async fn prerelease_and_draft_are_rejected() {
        for flag in ["prerelease", "draft"] {
            let s = MockServer::start().await;
            let mut body = release(vec![asset(&s, NAME, 1)]);
            body[flag] = json!(true);
            rejected_before_download(body, &glob(), &s).await;
        }
    }

    #[tokio::test]
    async fn repo_must_be_owner_slash_repo() {
        let s = MockServer::start().await;
        for repo in [
            "",
            "noslash",
            "a/b/c",
            "https://github.com/a/b",
            "../b",
            "a/..",
            "a/",
            "a b/c",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let e = Source::for_test(&s.uri(), &[])
                .fetch_latest(repo, &glob(), dir.path())
                .await
                .unwrap_err();
            assert!(matches!(e, GithubError::InvalidRepo(_)), "{repo}: {e}");
        }
        assert!(s.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn download_url_must_be_pinned_to_the_release() {
        let s = MockServer::start().await;
        let base = s.uri();
        let bad_urls = [
            // http instead of the expected scheme of this origin.
            format!("{}{}", base.replace("http://", "https://"), dl_path(NAME)),
            // another host.
            format!("http://localhost:1{}", dl_path(NAME)),
            // another repo / tag / name / layout.
            format!("{base}/Evil/MelonLoader/releases/download/{TAG}/{NAME}"),
            format!("{base}/{REPO}/releases/download/v9/{NAME}"),
            format!("{base}/{REPO}/releases/download/{TAG}/Other.zip"),
            format!("{base}/{REPO}/archive/{TAG}/{NAME}"),
            format!("{base}{}?x=1", dl_path(NAME)),
        ];
        for u in bad_urls {
            let s = MockServer::start().await;
            let body = release(vec![json!({
                "name": NAME, "size": 1, "browser_download_url": u.replace(&base, &s.uri()),
            })]);
            rejected_before_download(body, &glob(), &s).await;
        }
    }

    #[test]
    fn production_pin_requires_https_github_com() {
        let p = Source::production();
        let pin = |u: &str| p.pin_download_url(u, REPO, TAG, NAME);
        let good = format!("https://github.com{}", dl_path(NAME));
        assert_eq!(pin(&good).unwrap(), good);
        // Repo is matched case-insensitively.
        assert!(pin(&good.replace("LavaGang", "lavagang")).is_ok());
        for bad in [
            format!("http://github.com{}", dl_path(NAME)),
            format!("https://api.github.com{}", dl_path(NAME)),
            format!("https://github.com.evil.com{}", dl_path(NAME)),
            format!("https://github.com:8443{}", dl_path(NAME)),
            format!("https://user@github.com{}", dl_path(NAME)),
            format!("https://objects.githubusercontent.com{}", dl_path(NAME)),
            format!("https://github.com/{REPO}/releases/download/{TAG}/sub/{NAME}"),
        ] {
            assert!(pin(&bad).is_err(), "{bad}");
        }
        // Percent-encoded tags are decoded before comparison.
        let tagged = p.pin_download_url(
            &format!("https://github.com/{REPO}/releases/download/v1%2B2/{NAME}"),
            REPO,
            "v1+2",
            NAME,
        );
        assert!(tagged.is_ok());
    }

    #[test]
    fn production_redirects_only_to_github_hosts_over_https() {
        let allowed = Source::production().redirect_origins;
        for ok in [
            "https://github.com/x",
            "https://objects.githubusercontent.com/x?sig=1",
            "https://release-assets.githubusercontent.com/x",
        ] {
            assert!(redirect_allowed(&allowed, ok), "{ok}");
        }
        for bad in [
            "http://release-assets.githubusercontent.com/x",
            "http://github.com/x",
            "https://evil.com/x",
            "https://githubusercontent.com/x",
            "https://release-assets.githubusercontent.com:444/x",
            "https://a@objects.githubusercontent.com/x",
            "not a url",
        ] {
            assert!(!redirect_allowed(&allowed, bad), "{bad}");
        }
    }

    async fn redirect_to(target: &str) -> (MockServer, tempfile::TempDir, GithubError) {
        let s = MockServer::start().await;
        serve_api(&s, release(vec![asset(&s, NAME, 4)])).await;
        Mock::given(path(dl_path(NAME)))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", target))
            .mount(&s)
            .await;
        // The target's https origin is allowlisted, so only the scheme or
        // host decides.
        let https_target = target.replace("http://", "https://");
        let (dir, r) = run(&s, &glob(), &[&https_target]).await;
        (s, dir, r.unwrap_err())
    }

    #[tokio::test]
    async fn redirect_to_a_non_allowlisted_host_is_refused() {
        let evil = MockServer::start().await;
        serve_file(&evil, "/x", b"evil").await;
        let target = format!("{}/x", evil.uri().replace("127.0.0.1", "localhost"));
        let s = MockServer::start().await;
        serve_api(&s, release(vec![asset(&s, NAME, 4)])).await;
        Mock::given(path(dl_path(NAME)))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", target.as_str()))
            .mount(&s)
            .await;
        // Only evil's 127.0.0.1 origin is allowlisted; localhost is another host.
        let (dir, r) = run(&s, &glob(), &[&evil.uri()]).await;
        let e = r.unwrap_err();
        assert!(
            matches!(e, GithubError::Download(DownloadError::BadUrl(_))),
            "{e}"
        );
        assert!(evil.received_requests().await.unwrap().is_empty());
        assert_empty(&dir);
    }

    #[tokio::test]
    async fn redirect_to_http_is_refused() {
        let cdn = MockServer::start().await;
        serve_file(&cdn, "/x", b"data").await;
        let (_s, dir, e) = redirect_to(&format!("{}/x", cdn.uri())).await;
        assert!(
            matches!(e, GithubError::Download(DownloadError::BadUrl(_))),
            "{e}"
        );
        assert!(cdn.received_requests().await.unwrap().is_empty());
        assert_empty(&dir);
    }

    #[tokio::test]
    async fn an_allowlisted_redirect_is_followed() {
        let cdn = MockServer::start().await;
        serve_file(&cdn, "/x", b"data").await;
        let s = MockServer::start().await;
        serve_api(&s, release(vec![asset(&s, NAME, 4)])).await;
        Mock::given(path(dl_path(NAME)))
            .respond_with(
                ResponseTemplate::new(302).insert_header("Location", format!("{}/x", cdn.uri())),
            )
            .mount(&s)
            .await;
        let (_dir, r) = run(&s, &glob(), &[&cdn.uri()]).await;
        assert_eq!(std::fs::read(r.unwrap().path).unwrap(), b"data");
    }

    #[tokio::test]
    async fn api_body_over_one_mib_is_rejected() {
        let s = MockServer::start().await;
        let mut body = release(vec![asset(&s, NAME, 1)]);
        body["body"] = json!("x".repeat(API_MAX_BYTES as usize));
        serve_api(&s, body).await;
        let (dir, r) = run(&s, &glob(), &[]).await;
        let e = r.unwrap_err();
        assert!(matches!(e, GithubError::Api(_)), "{e}");
        assert_eq!(downloads(&s).await, 0);
        assert_empty(&dir);
    }

    #[tokio::test]
    async fn asset_size_over_the_cap_is_rejected_before_download() {
        let s = MockServer::start().await;
        let body = release(vec![asset(&s, NAME, ASSET_MAX_BYTES as usize + 1)]);
        rejected_before_download(body, &glob(), &s).await;
    }

    #[tokio::test]
    async fn streamed_body_over_the_cap_is_aborted() {
        let s = MockServer::start().await;
        serve_api(&s, release(vec![asset(&s, NAME, 100)])).await;
        serve_file(&s, &dl_path(NAME), &[7u8; 5000]).await;
        let dir = tempfile::tempdir().unwrap();
        let mut src = Source::for_test(&s.uri(), &[]);
        src.asset_max = 100;
        let e = src
            .fetch_latest(REPO, &glob(), dir.path())
            .await
            .unwrap_err();
        assert!(e.to_string().contains("limit"), "{e}");
        assert_empty(&dir);
    }

    #[tokio::test]
    async fn length_must_equal_asset_size() {
        let s = MockServer::start().await;
        serve_api(&s, release(vec![asset(&s, NAME, 12)])).await;
        serve_file(&s, &dl_path(NAME), b"short").await;
        let (dir, r) = run(&s, &glob(), &[]).await;
        let e = r.unwrap_err();
        assert!(
            matches!(
                e,
                GithubError::SizeMismatch {
                    got: 5,
                    expected: 12
                }
            ),
            "{e}"
        );
        assert_empty(&dir);
    }

    #[tokio::test]
    async fn sha256_digest_mismatch_is_rejected() {
        let s = MockServer::start().await;
        let mut a = asset(&s, NAME, 4);
        a["digest"] = json!(format!("sha256:{}", sha(b"else")));
        serve_api(&s, release(vec![a])).await;
        serve_file(&s, &dl_path(NAME), b"data").await;
        let (dir, r) = run(&s, &glob(), &[]).await;
        let e = r.unwrap_err();
        assert!(
            matches!(
                e,
                GithubError::Download(DownloadError::ChecksumMismatch { .. })
            ),
            "{e}"
        );
        assert_empty(&dir);
    }

    #[tokio::test]
    async fn malformed_sha256_digest_is_rejected() {
        let s = MockServer::start().await;
        let mut a = asset(&s, NAME, 4);
        a["digest"] = json!("sha256:nothex");
        rejected_before_download(release(vec![a]), &glob(), &s).await;
    }

    #[tokio::test]
    async fn rate_limits_map_to_rate_limited() {
        let reset = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 600;
        let cases = [
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("x-ratelimit-reset", reset.to_string().as_str()),
            ResponseTemplate::new(429),
        ];
        for (i, resp) in cases.into_iter().enumerate() {
            let s = MockServer::start().await;
            Mock::given(path(API_PATH))
                .respond_with(resp)
                .mount(&s)
                .await;
            let (dir, r) = run(&s, &glob(), &[]).await;
            let e = r.unwrap_err();
            assert!(matches!(e, GithubError::RateLimited { .. }), "{e}");
            let text = e.to_string();
            assert!(text.contains("rate limit"), "{text}");
            if i == 0 {
                assert!(text.contains("resets in about 10 minute"), "{text}");
            }
            assert_empty(&dir);
        }
    }

    #[tokio::test]
    async fn a_plain_403_is_not_a_rate_limit() {
        let s = MockServer::start().await;
        Mock::given(path(API_PATH))
            .respond_with(ResponseTemplate::new(403).insert_header("x-ratelimit-remaining", "12"))
            .mount(&s)
            .await;
        let (_dir, r) = run(&s, &glob(), &[]).await;
        assert!(matches!(r.unwrap_err(), GithubError::Api(_)));
    }

    #[test]
    fn helpers() {
        assert_eq!(percent_decode("v1%2B2").as_deref(), Some("v1+2"));
        assert_eq!(percent_decode("bad%zz"), None);
        assert_eq!(percent_decode("bad%"), None);
        assert_eq!(sha256_digest("md5:abc"), Ok(None));
        assert!(sha256_digest("nocolon").is_err());
    }
}
