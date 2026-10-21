// SPDX-License-Identifier: GPL-3.0-or-later
//! Mod thumbnail cache for the GUI. The webview never loads remote images
//! (its CSP allows no remote origins): the app asks for a mod's picture,
//! core fetches the Nexus `picture_url` once, checks it is a small image
//! from a Nexus host, caches it under `<data>/thumbs/<game>/<mod>` and
//! returns a `data:` URL.

use std::path::{Path, PathBuf};

use super::nexus::{self as nx, Nexus};
use super::{fail, Cx, R};
use crate::download::FetchRequest;
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

/// Fetches `url` into the cache after the host, size and type checks.
pub fn fetch_into(
    nexus: &Nexus,
    dir: &Path,
    game: &str,
    mod_id: u64,
    url: &str,
    allow: &dyn Fn(&str) -> bool,
) -> R<Vec<u8>> {
    if !allow(url) {
        fail!(
            "refusing to fetch a picture from {}",
            super::nxm::redact_url(url)
        );
    }
    let dest = cache_path(dir, game, mod_id)?;
    std::fs::create_dir_all(dest.parent().unwrap_or(dir))?;
    let tmp = dest.with_extension("download");
    let fetcher = crate::download::Fetcher::new()?.with_retry(1, std::time::Duration::ZERO);
    let fetched = nexus.run(fetcher.fetch(FetchRequest {
        url,
        dest: &tmp,
        md5: None,
        sha256: None,
        events: None,
    }))?;
    let res = (|| -> R<Vec<u8>> {
        let f = fetched?;
        if f.size > MAX_BYTES {
            fail!("picture too large ({} bytes)", f.size);
        }
        let bytes = std::fs::read(&f.path)?;
        if sniff(&bytes).is_none() {
            fail!("not an image");
        }
        std::fs::rename(&f.path, &dest)?;
        Ok(bytes)
    })();
    let _ = std::fs::remove_file(&tmp);
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
    let bytes = fetch_into(&nexus, &dir, game, mod_id, &url, &allowed_url)?;
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
}
