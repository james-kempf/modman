// SPDX-License-Identifier: GPL-3.0-or-later
//! Steam library art for the GUI: the portrait art (600x900, the art
//! Steam's library and the Deck show) for the game library, and the wide
//! hero art behind a game's Overview. Looked up in this order:
//! 1. the local Steam cache, read-only:
//!    `<steam root>/appcache/librarycache/<appid>/<name>.jpg`,
//!    `<appid>/<hash>/<name>*.jpg` (newer clients) and the older flat
//!    `<appid>_<name>.jpg`, where `<name>` is `library_600x900` or
//!    `library_hero` (never the `library_hero_blur` variant);
//! 2. modman's own cache `<data>/art/<appid>[_hero]`, refreshed after
//!    [`TTL_SECS`];
//! 3. a fetch from Steam's CDN (https, allow-listed hosts on every hop,
//!    size cap, image sniff) into that cache.
//!
//! The picture is returned as a `data:` URL, so the webview's CSP keeps
//! `img-src 'self' data:` with no remote origins. `None`: no art (the UI
//! shows a monogram, or no background).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::thumbs::{data_url, fetch_image, sniff};
use super::{Cx, R};
use crate::games::{self, steam};
use crate::paths::Paths;
use crate::store;

/// Cached downloads are refreshed after 30 days (store art rarely changes).
pub const TTL_SECS: i64 = 30 * 86_400;
/// Larger portrait files (local or downloaded) are not used.
pub const MAX_BYTES: u64 = 4 << 20;
/// Larger hero files are not used: hero art is 3840x1240 and some games
/// ship more than 4 MiB of it.
pub const HERO_MAX_BYTES: u64 = 8 << 20;
/// Hosts Steam serves store art from.
const HOSTS: [&str; 3] = [
    "shared.steamstatic.com",
    "cdn.cloudflare.steamstatic.com",
    "steamcdn-a.akamaihd.net",
];

/// Which library picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum ArtKind {
    /// Portrait 600x900 (the game library tiles).
    #[default]
    #[serde(rename = "library_600x900")]
    Portrait,
    /// Wide hero art (the Overview background).
    #[serde(rename = "library_hero")]
    Hero,
}

impl ArtKind {
    /// Steam's file name (without `.jpg`).
    pub fn name(self) -> &'static str {
        match self {
            ArtKind::Portrait => "library_600x900",
            ArtKind::Hero => "library_hero",
        }
    }

    /// Size cap of a local or downloaded file.
    pub fn max_bytes(self) -> u64 {
        match self {
            ArtKind::Portrait => MAX_BYTES,
            ArtKind::Hero => HERO_MAX_BYTES,
        }
    }

    /// modman's cache file name (the portrait keeps its original name).
    fn cache_name(self, appid: u32) -> String {
        match self {
            ArtKind::Portrait => appid.to_string(),
            ArtKind::Hero => format!("{appid}_hero"),
        }
    }

    /// A file name in a hashed subfolder that is this art (`<name>*.jpg`,
    /// not the blurred hero).
    fn matches(self, file: &str) -> bool {
        file.strip_prefix(self.name())
            .and_then(|rest| rest.strip_suffix(".jpg"))
            .is_some_and(|mid| !mid.starts_with("_blur"))
    }
}

pub fn art_dir(paths: &Paths) -> PathBuf {
    paths.data_dir.join("art")
}

/// The CDN URL of an app's library art.
pub fn art_url(appid: u32, kind: ArtKind) -> String {
    format!(
        "https://shared.steamstatic.com/store_item_assets/steam/apps/{appid}/{}.jpg",
        kind.name()
    )
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
fn read_image(p: &Path, max_bytes: u64) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(p).ok()?;
    if !meta.is_file() || meta.len() > max_bytes {
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

/// Candidate paths of an app's library art in one Steam root, best first.
fn local_candidates(root: &Path, appid: u32, kind: ArtKind) -> Vec<PathBuf> {
    let name = kind.name();
    let cache = root.join("appcache/librarycache");
    let app = cache.join(appid.to_string());
    let mut out = vec![app.join(format!("{name}.jpg"))];
    for sub in sorted_entries(&app).into_iter().filter(|p| p.is_dir()) {
        out.extend(sorted_entries(&sub).into_iter().filter(|f| {
            f.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| kind.matches(n))
        }));
    }
    out.push(cache.join(format!("{appid}_{name}.jpg")));
    out
}

/// The app's library art from the local Steam caches (read-only).
pub fn local_art(roots: &[PathBuf], appid: u32, kind: ArtKind) -> Option<Vec<u8>> {
    roots
        .iter()
        .flat_map(|r| local_candidates(r, appid, kind))
        .find_map(|p| read_image(&p, kind.max_bytes()))
}

/// A downloaded picture younger than [`TTL_SECS`].
pub fn cached(dir: &Path, appid: u32, kind: ArtKind, now: i64) -> Option<Vec<u8>> {
    let p = dir.join(kind.cache_name(appid));
    let age = std::fs::metadata(&p)
        .ok()?
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(i64::MAX, |d| now - d.as_secs() as i64);
    if age > TTL_SECS {
        return None;
    }
    read_image(&p, kind.max_bytes())
}

/// Local Steam cache, then modman's cache, then `fetch` (a failed fetch is
/// "no art": the UI falls back to a monogram or no background).
pub fn lookup(
    roots: &[PathBuf],
    dir: &Path,
    appid: u32,
    kind: ArtKind,
    now: i64,
    fetch: impl FnOnce(&Path, &str) -> R<Vec<u8>>,
) -> Option<String> {
    let bytes = local_art(roots, appid, kind)
        .or_else(|| cached(dir, appid, kind, now))
        .or_else(
            || match fetch(&dir.join(kind.cache_name(appid)), &art_url(appid, kind)) {
                Ok(b) => Some(b),
                Err(e) => {
                    tracing::debug!("no {} art for app {appid}: {e}", kind.name());
                    None
                }
            },
        )?;
    data_url(&bytes)
}

/// The game's library art as a `data:` URL (`None`: no Steam app id, or
/// no art anywhere).
pub fn game_art(cx: &Cx<'_>, domain: &str, kind: ArtKind) -> R<Option<String>> {
    let g = games::require_game(cx.store.conn(), domain)?;
    let Some(appid) = g.appid else {
        return Ok(None);
    };
    let roots = steam::steam_roots(&cx.paths.home);
    Ok(lookup(
        &roots,
        &art_dir(cx.paths),
        appid,
        kind,
        store::now(),
        |dest, url| fetch_image(dest, url, kind.max_bytes(), allowed_url),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const P: ArtKind = ArtKind::Portrait;
    const H: ArtKind = ArtKind::Hero;
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
        assert_eq!(
            local_art(&[root], 489830, ArtKind::Portrait).as_deref(),
            Some(JPG)
        );
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
        assert_eq!(
            local_art(&[root], 1091500, ArtKind::Portrait).as_deref(),
            Some(JPG)
        );
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
        assert_eq!(
            local_art(&[a, b], 72850, ArtKind::Portrait).as_deref(),
            Some(JPG)
        );
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
            local_art(std::slice::from_ref(&root), 1, ArtKind::Portrait).as_deref(),
            Some(JPG)
        );
        let mut big = JPG.to_vec();
        big.resize(MAX_BYTES as usize + 1, 0);
        write(&lc.join("2/library_600x900.jpg"), &big);
        assert_eq!(local_art(&[root], 2, ArtKind::Portrait), None);
    }

    // Fails when the host allow-list admits plain http, look-alike hosts or ports.
    #[test]
    fn only_https_steam_art_hosts_are_allowed() {
        assert!(allowed_url(&art_url(489830, ArtKind::Portrait)));
        assert!(allowed_url(&art_url(489830, ArtKind::Hero)));
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
        let none = lookup(std::slice::from_ref(&root), &dir, 5, P, now, |_, _| {
            Err(super::super::AppError::Msg("404".into()))
        });
        assert_eq!(none, None);
        // Fetch succeeds: asked for the CDN URL, result returned as a data: URL.
        let got = lookup(std::slice::from_ref(&root), &dir, 5, P, now, |dest, url| {
            assert_eq!(url, art_url(5, P));
            write(dest, JPG);
            Ok(JPG.to_vec())
        });
        assert!(got.unwrap().starts_with("data:image/jpeg;base64,"));
        // Cached now: no second fetch, until the TTL runs out.
        assert!(lookup(std::slice::from_ref(&root), &dir, 5, P, now, no_fetch).is_some());
        assert!(cached(&dir, 5, P, now + TTL_SECS + 10).is_none());
        // Local Steam art wins over the cache.
        write(
            &root.join("appcache/librarycache/5/library_600x900.jpg"),
            JPG2,
        );
        let local = lookup(&[root], &dir, 5, P, now, no_fetch).unwrap();
        assert_eq!(local, data_url(JPG2).unwrap());
    }

    // Fails when hero art is not found in the three Steam layouts, or the blurred variant is used.
    #[test]
    fn hero_art_layouts_and_no_blur() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Steam");
        let lc = root.join("appcache/librarycache");
        // Per-app folder: the hero, never library_hero_blur.jpg next to it.
        write(&lc.join("10/library_hero_blur.jpg"), JPG2);
        write(&lc.join("10/library_hero.jpg"), JPG);
        write(&lc.join("10/library_600x900.jpg"), JPG2);
        assert_eq!(
            local_art(std::slice::from_ref(&root), 10, H).as_deref(),
            Some(JPG)
        );
        // Hashed subfolder holding only a blurred hero and the real one.
        write(&lc.join("20/abc/library_hero_blur.jpg"), JPG2);
        write(&lc.join("20/def/library_hero.jpg"), JPG);
        assert_eq!(
            local_art(std::slice::from_ref(&root), 20, H).as_deref(),
            Some(JPG)
        );
        // Flat name.
        write(&lc.join("30_library_hero.jpg"), JPG);
        assert_eq!(
            local_art(std::slice::from_ref(&root), 30, H).as_deref(),
            Some(JPG)
        );
        // Only blurred or portrait art: no hero.
        write(&lc.join("40/library_hero_blur.jpg"), JPG2);
        write(&lc.join("40/library_600x900.jpg"), JPG2);
        assert_eq!(local_art(&[root], 40, H), None);
    }

    // Fails when hero art over the portrait cap (some exceed 4 MiB) is dropped, or the hero cap is not enforced.
    #[test]
    fn hero_art_has_its_own_size_cap() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Steam");
        let lc = root.join("appcache/librarycache");
        let mut five = JPG.to_vec();
        five.resize(5 << 20, 0);
        write(&lc.join("1/library_hero.jpg"), &five);
        write(&lc.join("1/library_600x900.jpg"), &five);
        assert_eq!(
            local_art(std::slice::from_ref(&root), 1, H).map(|b| b.len()),
            Some(5 << 20)
        );
        assert_eq!(local_art(std::slice::from_ref(&root), 1, P), None);
        let mut big = JPG.to_vec();
        big.resize(HERO_MAX_BYTES as usize + 1, 0);
        write(&lc.join("2/library_hero.jpg"), &big);
        assert_eq!(local_art(&[root], 2, H), None);
    }

    // Fails when the hero shares the portrait's cache file or CDN URL (one would show as the other).
    #[test]
    fn hero_lookup_uses_its_own_cache_and_url() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("art");
        let now = store::now();
        let p = lookup(&[], &dir, 7, P, now, |dest, url| {
            assert_eq!(url, art_url(7, P));
            write(dest, JPG);
            Ok(JPG.to_vec())
        });
        assert_eq!(p, data_url(JPG));
        let h = lookup(&[], &dir, 7, H, now, |dest, url| {
            assert!(url.ends_with("/apps/7/library_hero.jpg"), "{url}");
            assert!(dest.ends_with("7_hero"), "{}", dest.display());
            write(dest, JPG2);
            Ok(JPG2.to_vec())
        });
        assert_eq!(h, data_url(JPG2));
        assert_eq!(lookup(&[], &dir, 7, H, now, no_fetch), data_url(JPG2));
        assert_eq!(lookup(&[], &dir, 7, P, now, no_fetch), data_url(JPG));
    }

    // Fails when the IPC names of the kinds change (the GUI sends them).
    #[test]
    fn kinds_deserialize_from_steam_names() {
        let k: ArtKind = serde_json::from_str("\"library_hero\"").unwrap();
        assert_eq!(k, H);
        let k: ArtKind = serde_json::from_str("\"library_600x900\"").unwrap();
        assert_eq!(k, P);
        assert!(serde_json::from_str::<ArtKind>("\"header\"").is_err());
        assert_eq!(ArtKind::default(), P);
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
        let kind = match std::env::var("MODMAN_ART_PROBE_KIND").as_deref() {
            Ok("library_hero") => ArtKind::Hero,
            _ => P,
        };
        let url = lookup(
            std::slice::from_ref(&root),
            dir.path(),
            appid,
            kind,
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
