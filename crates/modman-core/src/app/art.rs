// SPDX-License-Identifier: GPL-3.0-or-later
//! Portrait library art (600x900, the art Steam's library and the Deck
//! show) for the GUI's game library. Looked up in this order:
//! 1. the local Steam cache, read-only:
//!    `<steam root>/appcache/librarycache/<appid>/library_600x900.jpg`,
//!    `<appid>/<hash>/library_600x900*.jpg` (newer clients) and the older
//!    flat `<appid>_library_600x900.jpg`;
//! 2. modman's own cache `<data>/art/<appid>`, refreshed after [`TTL_SECS`];
//! 3. a fetch from Steam's CDN (https, allow-listed hosts on every hop,
//!    size cap, image sniff) into that cache.
//!
//! The picture is returned as a `data:` URL, so the webview's CSP keeps
//! `img-src 'self' data:` with no remote origins. `None`: no art (the UI
//! shows a monogram).

use std::path::{Path, PathBuf};

use super::thumbs::{data_url, fetch_image, sniff};
use super::{Cx, R};
use crate::games::{self, steam};
use crate::paths::Paths;
use crate::store;

/// Cached downloads are refreshed after 30 days (store art rarely changes).
pub const TTL_SECS: i64 = 30 * 86_400;
/// Larger files (local or downloaded) are not used.
pub const MAX_BYTES: u64 = 4 << 20;
/// Steam's file name for the portrait art.
const NAME: &str = "library_600x900";
/// Hosts Steam serves store art from.
const HOSTS: [&str; 3] = [
    "shared.steamstatic.com",
    "cdn.cloudflare.steamstatic.com",
    "steamcdn-a.akamaihd.net",
];

pub fn art_dir(paths: &Paths) -> PathBuf {
    paths.data_dir.join("art")
}

/// The CDN URL of an app's portrait art.
pub fn art_url(appid: u32) -> String {
    format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{appid}/{NAME}.jpg")
}

/// Only https URLs on Steam's art hosts (exact host match) are fetched.
pub fn allowed_url(u: &str) -> bool {
    let Ok(url) = url::Url::parse(u) else {
        return false;
    };
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    url.scheme() == "https" && url.port().is_none() && HOSTS.contains(&host.as_str())
}

/// A small image file, else `None` (missing, too large, not an image).
fn read_image(p: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(p).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    std::fs::read(p).ok().filter(|b| sniff(b).is_some())
}

/// Sorted entries of `dir` (deterministic choice among hashed folders).
fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// Candidate paths of an app's portrait art in one Steam root, best first.
fn local_candidates(root: &Path, appid: u32) -> Vec<PathBuf> {
    let cache = root.join("appcache/librarycache");
    let app = cache.join(appid.to_string());
    let mut out = vec![app.join(format!("{NAME}.jpg"))];
    for sub in sorted_entries(&app).into_iter().filter(|p| p.is_dir()) {
        out.extend(sorted_entries(&sub).into_iter().filter(|f| {
            f.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(NAME) && n.ends_with(".jpg"))
        }));
    }
    out.push(cache.join(format!("{appid}_{NAME}.jpg")));
    out
}

/// The app's portrait art from the local Steam caches (read-only).
pub fn local_art(roots: &[PathBuf], appid: u32) -> Option<Vec<u8>> {
    roots
        .iter()
        .flat_map(|r| local_candidates(r, appid))
        .find_map(|p| read_image(&p))
}

/// A downloaded picture younger than [`TTL_SECS`].
pub fn cached(dir: &Path, appid: u32, now: i64) -> Option<Vec<u8>> {
    let p = dir.join(appid.to_string());
    let age = std::fs::metadata(&p)
        .ok()?
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(i64::MAX, |d| now - d.as_secs() as i64);
    if age > TTL_SECS {
        return None;
    }
    read_image(&p)
}

/// Local Steam cache, then modman's cache, then `fetch` (a failed fetch is
/// "no art": the UI falls back to a monogram).
pub fn lookup(
    roots: &[PathBuf],
    dir: &Path,
    appid: u32,
    now: i64,
    fetch: impl FnOnce(&Path, &str) -> R<Vec<u8>>,
) -> Option<String> {
    let bytes = local_art(roots, appid)
        .or_else(|| cached(dir, appid, now))
        .or_else(
            || match fetch(&dir.join(appid.to_string()), &art_url(appid)) {
                Ok(b) => Some(b),
                Err(e) => {
                    tracing::debug!("no library art for app {appid}: {e}");
                    None
                }
            },
        )?;
    data_url(&bytes)
}

/// The game's portrait art as a `data:` URL (`None`: no Steam app id, or
/// no art anywhere).
pub fn game_art(cx: &Cx<'_>, domain: &str) -> R<Option<String>> {
    let g = games::require_game(cx.store.conn(), domain)?;
    let Some(appid) = g.appid else {
        return Ok(None);
    };
    let roots = steam::steam_roots(&cx.paths.home);
    Ok(lookup(
        &roots,
        &art_dir(cx.paths),
        appid,
        store::now(),
        |dest, url| fetch_image(dest, url, MAX_BYTES, allowed_url),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const JPG: &[u8] = b"\xFF\xD8\xFF\xE0jpeg body";
    const JPG2: &[u8] = b"\xFF\xD8\xFF\xE1other jpeg";

    fn write(p: &Path, b: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b).unwrap();
    }

    fn no_fetch(_: &Path, _: &str) -> R<Vec<u8>> {
        panic!("must not fetch")
    }

    // Fails when the per-appid folder (current Steam layout) is not read.
    #[test]
    fn reads_the_per_app_folder_first() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Steam");
        let lc = root.join("appcache/librarycache");
        write(&lc.join("489830/library_600x900.jpg"), JPG);
        write(&lc.join("489830_library_600x900.jpg"), JPG2);
        assert_eq!(local_art(&[root], 489830).as_deref(), Some(JPG));
    }

    // Fails when hashed subfolders (<appid>/<hash>/library_600x900*.jpg) are not searched.
    #[test]
    fn finds_art_in_a_hashed_subfolder() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Steam");
        let app = root.join("appcache/librarycache/1091500");
        write(&app.join("header.jpg"), JPG2);
        write(&app.join("0dfe3eed/logo.png"), b"\x89PNG\r\n\x1a\n");
        write(&app.join("9a8b7c/library_600x900_2x.jpg"), JPG);
        assert_eq!(local_art(&[root], 1091500).as_deref(), Some(JPG));
    }

    // Fails when the older flat <appid>_library_600x900.jpg name is not read.
    #[test]
    fn falls_back_to_the_flat_name_and_later_roots() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("a");
        let b = t.path().join("b");
        fs::create_dir_all(a.join("appcache/librarycache")).unwrap();
        write(
            &b.join("appcache/librarycache/72850_library_600x900.jpg"),
            JPG,
        );
        assert_eq!(local_art(&[a, b], 72850).as_deref(), Some(JPG));
    }

    // Fails when a non-image or oversize file in the Steam cache is used.
    #[test]
    fn non_images_and_oversize_files_are_skipped() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Steam");
        let lc = root.join("appcache/librarycache");
        write(&lc.join("1/library_600x900.jpg"), b"<svg onload=alert(1)>");
        write(&lc.join("1_library_600x900.jpg"), JPG);
        assert_eq!(
            local_art(std::slice::from_ref(&root), 1).as_deref(),
            Some(JPG)
        );
        let mut big = JPG.to_vec();
        big.resize(MAX_BYTES as usize + 1, 0);
        write(&lc.join("2/library_600x900.jpg"), &big);
        assert_eq!(local_art(&[root], 2), None);
    }

    // Fails when the host allow-list admits plain http, look-alike hosts or ports.
    #[test]
    fn only_https_steam_art_hosts_are_allowed() {
        assert!(allowed_url(&art_url(489830)));
        assert!(allowed_url(
            "https://cdn.cloudflare.steamstatic.com/steam/apps/1/library_600x900.jpg"
        ));
        assert!(allowed_url(
            "https://steamcdn-a.akamaihd.net/steam/apps/1/library_600x900.jpg"
        ));
        assert!(!allowed_url("http://shared.steamstatic.com/x.jpg"));
        assert!(!allowed_url(
            "https://evil.shared.steamstatic.com.example/x.jpg"
        ));
        assert!(!allowed_url("https://steamstatic.com/x.jpg"));
        assert!(!allowed_url("https://shared.steamstatic.com:8443/x.jpg"));
        assert!(!allowed_url("https://127.0.0.1/x.jpg"));
        assert!(!allowed_url("file:///etc/passwd"));
    }

    // Fails when the fallback order (local, cache, fetch, none) breaks or a failed fetch is an error.
    #[test]
    fn lookup_order_and_fallback() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Steam");
        let dir = t.path().join("art");
        let now = store::now();
        // Nothing local, fetch fails: no art, not an error.
        let none = lookup(std::slice::from_ref(&root), &dir, 5, now, |_, _| {
            Err(super::super::AppError::Msg("404".into()))
        });
        assert_eq!(none, None);
        // Fetch succeeds: asked for the CDN URL, result returned as a data: URL.
        let got = lookup(std::slice::from_ref(&root), &dir, 5, now, |dest, url| {
            assert_eq!(url, art_url(5));
            write(dest, JPG);
            Ok(JPG.to_vec())
        });
        assert!(got.unwrap().starts_with("data:image/jpeg;base64,"));
        // Cached now: no second fetch, until the TTL runs out.
        assert!(lookup(std::slice::from_ref(&root), &dir, 5, now, no_fetch).is_some());
        assert!(cached(&dir, 5, now + TTL_SECS + 10).is_none());
        // Local Steam art wins over the cache.
        write(
            &root.join("appcache/librarycache/5/library_600x900.jpg"),
            JPG2,
        );
        let local = lookup(&[root], &dir, 5, now, no_fetch).unwrap();
        assert_eq!(local, data_url(JPG2).unwrap());
    }

    /// Probe of the real lookup against a Steam root (read-only, no fetch):
    /// `MODMAN_ART_PROBE_ROOT=<steam root> MODMAN_ART_PROBE_APPID=489830
    /// cargo test -p modman-core art::tests::probe -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads a real Steam root"]
    fn probe() {
        let root = PathBuf::from(std::env::var("MODMAN_ART_PROBE_ROOT").unwrap());
        let appid: u32 = std::env::var("MODMAN_ART_PROBE_APPID")
            .unwrap()
            .parse()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let url = lookup(
            std::slice::from_ref(&root),
            dir.path(),
            appid,
            store::now(),
            no_fetch,
        );
        let url = url.expect("art in the local cache");
        println!(
            "root={} appid={appid} -> {}... ({} chars)",
            root.display(),
            &url[..40],
            url.len()
        );
    }
}
