// SPDX-License-Identifier: GPL-3.0-or-later
//! Mod thumbnail cache for the GUI. The webview never loads remote images
//! (its CSP allows no remote origins): the app asks for a mod's picture,
//! core fetches the Nexus `picture_url` once, checks it is a small image
//! from a Nexus host, caches it under `<data>/thumbs/<game>/<mod>` and
//! returns a `data:` URL.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::nexus as nx;
use super::{fail, AppError, Cx, R};
use crate::blocking::block_on;
use crate::download::{FetchRequest, Fetcher};
use crate::games;
use crate::paths::Paths;
use crate::store;

/// Cached pictures are refreshed after a week.
pub const TTL_SECS: i64 = 7 * 86_400;
/// Larger downloads are rejected.
pub const MAX_BYTES: u64 = 4 << 20;

pub fn thumbs_dir(paths: &Paths) -> PathBuf {
    paths.data_dir.join("thumbs")
}

/// Image type from the first bytes (JPEG, PNG, GIF, WebP only).
pub fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Only https pictures from Nexus hosts are fetched.
pub fn allowed_url(u: &str) -> bool {
    let Ok(url) = url::Url::parse(u) else {
        return false;
    };
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    url.scheme() == "https"
        && ["nexusmods.com", "nexus-cdn.com"]
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= c.len() {
                s.push(T[(n >> shift & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// `data:<mime>;base64,...` for an image; `None` when not an image.
pub fn data_url(bytes: &[u8]) -> Option<String> {
    sniff(bytes).map(|m| format!("data:{m};base64,{}", base64(bytes)))
}

fn cache_path(dir: &Path, game: &str, mod_id: u64) -> R<PathBuf> {
    if !games::is_valid_domain(game) {
        fail!("invalid game domain '{game}'");
    }
    Ok(dir.join(game).join(mod_id.to_string()))
}

/// A cached picture younger than [`TTL_SECS`].
pub fn cached(dir: &Path, game: &str, mod_id: u64, now: i64) -> R<Option<Vec<u8>>> {
    let p = cache_path(dir, game, mod_id)?;
    let Ok(meta) = std::fs::metadata(&p) else {
        return Ok(None);
    };
    let age = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(i64::MAX, |d| now - d.as_secs() as i64);
    if age > TTL_SECS {
        return Ok(None);
    }
    Ok(std::fs::read(&p).ok().filter(|b| sniff(b).is_some()))
}

/// A download name of its own for each fetch, so concurrent fetches of
/// the same picture never write into one `.part`.
fn temp_path(dest: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    dest.with_extension(format!("{}-{n}.download", std::process::id()))
}

/// Fetches `url` into the cache after the host, size and type checks;
/// `allow` is applied to the URL and to every redirect hop, and the body
/// is capped at [`MAX_BYTES`] while streaming.
pub fn fetch_into(
    dir: &Path,
    game: &str,
    mod_id: u64,
    url: &str,
    allow: impl Fn(&str) -> bool + Send + Sync + 'static,
) -> R<Vec<u8>> {
    let dest = cache_path(dir, game, mod_id)?;
    fetch_image(&dest, url, MAX_BYTES, allow)
}

/// Downloads an image from `url` to `dest` (parent dirs created): `allow`
/// must accept the URL and every redirect hop, the body is capped at
/// `max_bytes` while streaming, and only a sniffed image is kept. Each
/// fetch writes a temp file of its own, renamed into place when complete.
pub(crate) fn fetch_image(
    dest: &Path,
    url: &str,
    max_bytes: u64,
    allow: impl Fn(&str) -> bool + Send + Sync + 'static,
) -> R<Vec<u8>> {
    if !allow(url) {
        fail!(
            "refusing to fetch a picture from {}",
            super::nxm::redact_url(url)
        );
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = temp_path(dest);
    let fetcher = Fetcher::new()?
        .with_retry(1, std::time::Duration::ZERO)
        .with_max_bytes(max_bytes)
        .with_redirect_check(Arc::new(allow))?;
    let fetched = block_on(fetcher.fetch(FetchRequest {
        url,
        dest: &tmp,
        md5: None,
        sha256: None,
        events: None,
    }))
    .map_err(|e| AppError::Msg(format!("cannot start the async runtime: {e}")))?;
    let res = (|| -> R<Vec<u8>> {
        let f = fetched?;
        if f.size > max_bytes {
            fail!("picture too large ({} bytes)", f.size);
        }
        let bytes = std::fs::read(&f.path)?;
        if sniff(&bytes).is_none() {
            fail!("not an image");
        }
        std::fs::rename(&f.path, dest)?;
        Ok(bytes)
    })();
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(Fetcher::part_path(&tmp));
    res
}

/// The mod's picture as a `data:` URL (`None`: the mod has none).
pub fn thumbnail(cx: &Cx<'_>, game: &str, mod_id: u64) -> R<Option<String>> {
    let dir = thumbs_dir(cx.paths);
    if let Some(b) = cached(&dir, game, mod_id, store::now())? {
        return Ok(data_url(&b));
    }
    let Some(nexus) = nx::connect(cx.paths, cx.store, false)? else {
        return Ok(None);
    };
    let info = nexus.run(nexus.client.mod_info(game, mod_id))??;
    let Some(url) = info.picture_url.filter(|u| !u.is_empty()) else {
        return Ok(None);
    };
    let bytes = fetch_into(&dir, game, mod_id, &url, allowed_url)?;
    Ok(data_url(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_nexus_hosts_are_allowed() {
        assert!(allowed_url(
            "https://staticdelivery.nexusmods.com/mods/1704/images/1.jpg"
        ));
        assert!(allowed_url("https://nexus-cdn.com/x.png"));
        assert!(!allowed_url("http://staticdelivery.nexusmods.com/x.jpg"));
        assert!(!allowed_url("https://evilnexusmods.com/x.jpg"));
        assert!(!allowed_url("https://127.0.0.1/x.jpg"));
        assert!(!allowed_url("file:///etc/passwd"));
    }

    #[test]
    fn data_urls_are_built_only_for_images() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        let png = b"\x89PNG\r\n\x1a\nrest";
        assert!(data_url(png).unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(data_url(b"<svg onload=alert(1)>"), None);
    }

    #[test]
    fn cache_paths_reject_traversal_and_honour_the_ttl() {
        let dir = tempfile::tempdir().unwrap();
        assert!(cached(dir.path(), "../etc", 1, 0).is_err());
        let p = dir.path().join("g").join("7");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"GIF89a....").unwrap();
        let now = store::now();
        assert!(cached(dir.path(), "g", 7, now).unwrap().is_some());
        assert!(cached(dir.path(), "g", 7, now + TTL_SECS + 10)
            .unwrap()
            .is_none());
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nbody";

    /// Two servers on 127.0.0.1; `allow` accepts only the first's URLs.
    fn servers() -> (tokio::runtime::Runtime, MockServer, MockServer) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (a, b) = rt.block_on(async { (MockServer::start().await, MockServer::start().await) });
        (rt, a, b)
    }

    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn a_redirect_off_the_allowed_hosts_is_refused() {
        let (rt, good, evil) = servers();
        rt.block_on(async {
            Mock::given(path("/p.png"))
                .respond_with(
                    ResponseTemplate::new(302)
                        .insert_header("Location", format!("{}/p.png", evil.uri())),
                )
                .mount(&good)
                .await;
            Mock::given(path("/p.png"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(PNG.to_vec()))
                .mount(&evil)
                .await;
        });
        let dir = tempfile::tempdir().unwrap();
        let prefix = format!("{}/", good.uri());
        let url = format!("{}/p.png", good.uri());
        let r = fetch_into(dir.path(), "g", 1, &url, move |u| u.starts_with(&prefix));
        assert!(r.is_err());
        assert!(rt.block_on(evil.received_requests()).unwrap().is_empty());
        assert!(!dir.path().join("g").join("1").exists());
    }

    #[test]
    fn an_oversize_picture_is_rejected() {
        let (rt, good, _) = servers();
        rt.block_on(
            Mock::given(path("/big.png"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_bytes(vec![0u8; MAX_BYTES as usize + 1]),
                )
                .mount(&good),
        );
        let dir = tempfile::tempdir().unwrap();
        let url = format!("{}/big.png", good.uri());
        let e = fetch_into(dir.path(), "g", 1, &url, |_| true).unwrap_err();
        assert!(e.to_string().contains("limit"), "{e}");
        let left: Vec<_> = std::fs::read_dir(dir.path().join("g")).unwrap().collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn concurrent_fetches_of_one_picture_do_not_splice() {
        let (rt, good, _) = servers();
        let body: Vec<u8> = PNG
            .iter()
            .copied()
            .chain((0..200_000).map(|i| i as u8))
            .collect();
        rt.block_on(
            Mock::given(path("/p.png"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_bytes(body.clone())
                        .set_delay(std::time::Duration::from_millis(50)),
                )
                .mount(&good),
        );
        let dir = tempfile::tempdir().unwrap();
        let url = format!("{}/p.png", good.uri());
        let a = temp_path(&dir.path().join("1"));
        assert_ne!(a, temp_path(&dir.path().join("1")));
        let results: Vec<_> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..4)
                .map(|_| s.spawn(|| fetch_into(dir.path(), "g", 1, &url, |_| true)))
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for r in results {
            assert_eq!(r.unwrap(), body);
        }
        assert_eq!(std::fs::read(dir.path().join("g").join("1")).unwrap(), body);
        let names: Vec<_> = std::fs::read_dir(dir.path().join("g"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("1")]);
    }
}
