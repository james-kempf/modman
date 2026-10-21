// SPDX-License-Identifier: GPL-3.0-or-later
//! Steam detection (read-only): roots, libraries (incl. SD cards),
//! appmanifest files and Proton prefixes. Ported from legacy/modman
//! `steam_roots`/`steam_libraries`/`steam_app_dir`/`steam_prefix` (L388-433).

use std::path::{Path, PathBuf};

use keyvalues_parser::{Obj, Value};

use super::GamesError;

/// Steam root candidates relative to `$HOME`, in legacy order (L390-391).
pub const STEAM_ROOT_CANDIDATES: [&str; 5] = [
    ".local/share/Steam",
    ".steam/steam",
    ".steam/root",
    ".var/app/com.valvesoftware.Steam/.local/share/Steam",
    "snap/steam/common/.local/share/Steam",
];

/// Parsed `appmanifest_<appid>.acf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppManifest {
    pub appid: Option<u32>,
    pub name: Option<String>,
    pub installdir: String,
    pub buildid: Option<String>,
    pub state_flags: Option<u32>,
}

/// An installed Steam app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamApp {
    pub appid: u32,
    /// Library root that holds the app.
    pub library: PathBuf,
    pub install_dir: PathBuf,
    /// `<lib>/steamapps/compatdata/<appid>/pfx`, if present.
    pub prefix: Option<PathBuf>,
    pub buildid: Option<String>,
    pub state_flags: Option<u32>,
}

fn vdf_err(origin: &str, message: impl ToString) -> GamesError {
    GamesError::Vdf {
        origin: origin.to_string(),
        message: message.to_string(),
    }
}

/// Case-insensitive first string value for `key` in `obj`.
fn get_str<'a>(obj: &'a Obj<'_>, key: &str) -> Option<&'a str> {
    obj.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .and_then(|(_, v)| v.first())
        .and_then(Value::get_str)
}

/// Library paths listed in `libraryfolders.vdf` (`"path"` entries).
/// Backslashes become `/`, as in legacy.
pub fn parse_library_folders(text: &str, origin: &str) -> Result<Vec<PathBuf>, GamesError> {
    let vdf = keyvalues_parser::parse(text).map_err(|e| vdf_err(origin, e))?;
    let root = vdf
        .value
        .get_obj()
        .ok_or_else(|| vdf_err(origin, "top level is not an object"))?;
    let mut out = Vec::new();
    for values in root.values() {
        for v in values {
            if let Some(path) = v.get_obj().and_then(|o| get_str(o, "path")) {
                out.push(PathBuf::from(path.replace('\\', "/")));
            }
        }
    }
    Ok(out)
}

/// Parses an `appmanifest_*.acf` file.
pub fn parse_app_manifest(text: &str, origin: &str) -> Result<AppManifest, GamesError> {
    let vdf = keyvalues_parser::parse(text).map_err(|e| vdf_err(origin, e))?;
    let obj = vdf
        .value
        .get_obj()
        .ok_or_else(|| vdf_err(origin, "top level is not an object"))?;
    let installdir = get_str(obj, "installdir")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| vdf_err(origin, "missing installdir"))?
        .to_string();
    Ok(AppManifest {
        appid: get_str(obj, "appid").and_then(|s| s.parse().ok()),
        name: get_str(obj, "name").map(str::to_string),
        installdir,
        buildid: get_str(obj, "buildid").map(str::to_string),
        state_flags: get_str(obj, "StateFlags").and_then(|s| s.parse().ok()),
    })
}

/// Sorted, deduplicated canonical paths (legacy `readlink -f | sort -u`).
fn canonical_sorted(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = paths
        .into_iter()
        .filter_map(|p| p.canonicalize().ok())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Steam roots under `home` that contain `steamapps/`.
pub fn steam_roots(home: &Path) -> Vec<PathBuf> {
    canonical_sorted(
        STEAM_ROOT_CANDIDATES
            .iter()
            .map(|c| home.join(c))
            .filter(|r| r.join("steamapps").is_dir()),
    )
}

/// All Steam libraries: the roots plus every `libraryfolders.vdf` path whose
/// `steamapps/` exists. Unreadable or malformed vdf files are skipped.
pub fn steam_libraries(home: &Path) -> Vec<PathBuf> {
    let mut all = Vec::new();
    for root in steam_roots(home) {
        let vdf = root.join("steamapps/libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&vdf) {
            match parse_library_folders(&text, &vdf.display().to_string()) {
                Ok(paths) => all.extend(paths.into_iter().filter(|p| p.join("steamapps").is_dir())),
                Err(e) => tracing::warn!("skipping {}: {e}", vdf.display()),
            }
        }
        all.push(root);
    }
    canonical_sorted(all)
}

/// Prefix for `appid`: the app's own library first, then the first library
/// (sorted) that has one, as legacy `steam_prefix`.
pub fn find_prefix(
    libraries: &[PathBuf],
    appid: u32,
    own_library: Option<&Path>,
) -> Option<PathBuf> {
    let pfx = |lib: &Path| lib.join(format!("steamapps/compatdata/{appid}/pfx"));
    own_library
        .into_iter()
        .chain(libraries.iter().map(PathBuf::as_path))
        .map(pfx)
        .find(|p| p.is_dir())
}

/// First library whose `appmanifest_<appid>.acf` names an existing
/// `steamapps/common/<installdir>` folder.
pub fn find_app(libraries: &[PathBuf], appid: u32) -> Option<SteamApp> {
    for lib in libraries {
        let acf = lib.join(format!("steamapps/appmanifest_{appid}.acf"));
        let Ok(text) = std::fs::read_to_string(&acf) else {
            continue;
        };
        let manifest = match parse_app_manifest(&text, &acf.display().to_string()) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("skipping {}: {e}", acf.display());
                continue;
            }
        };
        let install_dir = lib.join("steamapps/common").join(&manifest.installdir);
        if install_dir.is_dir() {
            return Some(SteamApp {
                appid,
                library: lib.clone(),
                install_dir,
                prefix: find_prefix(libraries, appid, Some(lib)),
                buildid: manifest.buildid,
                state_flags: manifest.state_flags,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const FIXTURE_VDF: &str = include_str!("../../tests/fixtures/steam/libraryfolders.vdf");
    const FIXTURE_ACF: &str = include_str!("../../tests/fixtures/steam/appmanifest_489830.acf");

    #[test]
    fn parses_fixture_libraryfolders_with_sd_card() {
        let libs = parse_library_folders(FIXTURE_VDF, "fixture").unwrap();
        assert_eq!(
            libs,
            vec![
                PathBuf::from("/home/deck/.local/share/Steam"),
                PathBuf::from("/run/media/mmcblk0p1"),
            ]
        );
    }

    #[test]
    fn parses_fixture_appmanifest() {
        let m = parse_app_manifest(FIXTURE_ACF, "fixture").unwrap();
        assert_eq!(m.appid, Some(489830));
        assert_eq!(m.installdir, "Skyrim Special Edition");
        assert_eq!(m.buildid.as_deref(), Some("15916224"));
        assert_eq!(m.state_flags, Some(4));
    }

    #[test]
    fn malformed_vdf_is_an_error() {
        assert!(parse_library_folders("\"libraryfolders\" {", "x").is_err());
        assert!(parse_app_manifest("\"AppState\" { \"appid\" \"1\" }", "x").is_err());
    }

    /// Builds a fake HOME with an internal library (the Steam root) and an
    /// SD-card library, both listed in libraryfolders.vdf.
    fn fake_steam(tmp: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let home = tmp.join("home");
        let root = home.join(".local/share/Steam");
        let sd = tmp.join("run/media/mmcblk0p1");
        fs::create_dir_all(root.join("steamapps/common")).unwrap();
        fs::create_dir_all(sd.join("steamapps/common")).unwrap();
        let vdf = FIXTURE_VDF
            .replace("/home/deck/.local/share/Steam", root.to_str().unwrap())
            .replace("/run/media/mmcblk0p1", sd.to_str().unwrap());
        fs::write(root.join("steamapps/libraryfolders.vdf"), vdf).unwrap();
        (home, root, sd)
    }

    #[test]
    fn detects_app_on_sd_card_with_prefix_and_buildid() {
        let tmp = tempfile::tempdir().unwrap();
        let (home, root, sd) = fake_steam(tmp.path());
        fs::write(sd.join("steamapps/appmanifest_489830.acf"), FIXTURE_ACF).unwrap();
        fs::create_dir_all(sd.join("steamapps/common/Skyrim Special Edition/Data")).unwrap();
        fs::create_dir_all(sd.join("steamapps/compatdata/489830/pfx")).unwrap();

        let libs = steam_libraries(&home);
        let root = root.canonicalize().unwrap();
        let sd = sd.canonicalize().unwrap();
        assert!(libs.contains(&root) && libs.contains(&sd), "{libs:?}");
        assert_eq!(libs.len(), 2, "root listed twice must dedupe: {libs:?}");

        let app = find_app(&libs, 489830).unwrap();
        assert_eq!(app.library, sd);
        assert_eq!(
            app.install_dir,
            sd.join("steamapps/common/Skyrim Special Edition")
        );
        assert_eq!(app.prefix, Some(sd.join("steamapps/compatdata/489830/pfx")));
        assert_eq!(app.buildid.as_deref(), Some("15916224"));
        assert_eq!(app.state_flags, Some(4));
    }

    #[test]
    fn manifest_without_install_folder_is_not_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let (home, root, _) = fake_steam(tmp.path());
        fs::write(root.join("steamapps/appmanifest_489830.acf"), FIXTURE_ACF).unwrap();
        assert_eq!(find_app(&steam_libraries(&home), 489830), None);
    }

    #[test]
    fn prefix_falls_back_to_other_library() {
        let tmp = tempfile::tempdir().unwrap();
        let (home, root, sd) = fake_steam(tmp.path());
        fs::write(sd.join("steamapps/appmanifest_489830.acf"), FIXTURE_ACF).unwrap();
        fs::create_dir_all(sd.join("steamapps/common/Skyrim Special Edition")).unwrap();
        fs::create_dir_all(root.join("steamapps/compatdata/489830/pfx")).unwrap();
        let app = find_app(&steam_libraries(&home), 489830).unwrap();
        assert_eq!(
            app.prefix,
            Some(
                root.canonicalize()
                    .unwrap()
                    .join("steamapps/compatdata/489830/pfx")
            )
        );
    }

    #[test]
    fn library_without_steamapps_is_ignored_and_missing_steam_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(steam_libraries(tmp.path()).is_empty());
        let (home, _, sd) = fake_steam(tmp.path());
        fs::remove_dir_all(sd.join("steamapps")).unwrap();
        assert_eq!(steam_libraries(&home).len(), 1);
    }
}
