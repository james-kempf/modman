// SPDX-License-Identifier: GPL-3.0-or-later
//! Steam detection (read-only): roots, libraries (incl. SD cards),
//! appmanifest files and Proton prefixes. Ported from the legacy bash tool
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
    /// Display name from the appmanifest.
    pub name: Option<String>,
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

/// The app described by `<lib>/steamapps/appmanifest_<appid>.acf` when it
/// names an existing `steamapps/common/<installdir>` folder.
fn app_in_library(libraries: &[PathBuf], lib: &Path, appid: u32) -> Option<SteamApp> {
    let acf = lib.join(format!("steamapps/appmanifest_{appid}.acf"));
    let text = std::fs::read_to_string(&acf).ok()?;
    let manifest = parse_app_manifest(&text, &acf.display().to_string())
        .map_err(|e| tracing::warn!("skipping {}: {e}", acf.display()))
        .ok()?;
    let install_dir = lib.join("steamapps/common").join(&manifest.installdir);
    install_dir.is_dir().then(|| SteamApp {
        appid,
        library: lib.to_path_buf(),
        install_dir,
        prefix: find_prefix(libraries, appid, Some(lib)),
        buildid: manifest.buildid,
        state_flags: manifest.state_flags,
        name: manifest.name,
    })
}

/// First library whose `appmanifest_<appid>.acf` names an existing
/// `steamapps/common/<installdir>` folder.
pub fn find_app(libraries: &[PathBuf], appid: u32) -> Option<SteamApp> {
    libraries
        .iter()
        .find_map(|lib| app_in_library(libraries, lib, appid))
}

/// Steam tools without a `toolmanifest.vdf` (Steamworks Common
/// Redistributables), plus known Proton / Steam Linux Runtime ids in case a
/// tool folder lacks its manifest.
pub const TOOL_APPIDS: [u32; 10] = [
    228980, 1070560, 1391110, 1628350, 4183110, 1493710, 1887720, 2180100, 2805730, 4628710,
];

/// App ids of every `appmanifest_<appid>.acf` in `lib`, sorted.
fn manifest_appids(lib: &Path) -> Vec<u32> {
    let Ok(dir) = std::fs::read_dir(lib.join("steamapps")) else {
        return Vec::new();
    };
    let mut ids: Vec<u32> = dir
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_str()?
                .strip_prefix("appmanifest_")?
                .strip_suffix(".acf")?
                .parse()
                .ok()
        })
        .collect();
    ids.sort_unstable();
    ids
}

/// Every installed game across `libraries`: each appmanifest whose install
/// folder exists and whose `StateFlags` say installed (a pending update
/// still counts), minus Steam tools and runtimes (a `toolmanifest.vdf` in
/// the install folder, or [`TOOL_APPIDS`]). An app in several libraries is
/// taken from the first, as [`find_app`]. Read-only.
pub fn list_installed_apps(libraries: &[PathBuf]) -> Vec<SteamApp> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for lib in libraries {
        for appid in manifest_appids(lib) {
            if seen.contains(&appid) || TOOL_APPIDS.contains(&appid) {
                continue;
            }
            let Some(app) = app_in_library(libraries, lib, appid) else {
                continue;
            };
            seen.insert(appid);
            let installed = app.state_flags.is_some_and(state_flags::installed);
            if installed && !app.install_dir.join("toolmanifest.vdf").exists() {
                out.push(app);
            }
        }
    }
    out
}

/// Case-insensitive first object value for `key` in `obj`.
fn get_obj<'a>(obj: &'a Obj<'a>, key: &str) -> Option<&'a Obj<'a>> {
    obj.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .and_then(|(_, v)| v.first())
        .and_then(Value::get_obj)
}

fn parse_root<'a>(text: &'a str, origin: &str) -> Result<keyvalues_parser::Vdf<'a>, GamesError> {
    keyvalues_parser::parse(text)
        .map(|p| p.into_vdf())
        .map_err(|e| vdf_err(origin, e))
}

/// `appmanifest` `StateFlags` bits (EAppState).
pub mod state_flags {
    pub const UPDATE_REQUIRED: u32 = 2;
    pub const FULLY_INSTALLED: u32 = 4;
    /// Files missing, files corrupt, update running/paused/started.
    pub const BUSY_OR_BROKEN: u32 = 32 | 128 | 256 | 512 | 1024;

    /// Fully installed, no update pending or running, files intact.
    pub fn fully_installed(flags: u32) -> bool {
        flags & FULLY_INSTALLED != 0 && flags & (UPDATE_REQUIRED | BUSY_OR_BROKEN) == 0
    }

    /// Installed and intact; a pending update (not yet running) is allowed.
    pub fn installed(flags: u32) -> bool {
        flags & FULLY_INSTALLED != 0 && flags & BUSY_OR_BROKEN == 0
    }
}

/// One `CompatToolMapping` entry from `config/config.vdf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatTool {
    /// Internal tool name (`proton_9`, `GE-Proton10-34`, ...), not a path.
    pub name: String,
    pub config: Option<String>,
    pub priority: Option<String>,
}

/// `InstallConfigStore > Software > Valve > Steam > CompatToolMapping`,
/// keyed by appid string. `"0"` is the global default.
pub fn parse_compat_tool_mapping(
    text: &str,
    origin: &str,
) -> Result<std::collections::BTreeMap<String, CompatTool>, GamesError> {
    let vdf = parse_root(text, origin)?;
    let root = vdf
        .value
        .get_obj()
        .ok_or_else(|| vdf_err(origin, "top level is not an object"))?;
    let mut out = std::collections::BTreeMap::new();
    let Some(map) = get_obj(root, "Software")
        .and_then(|o| get_obj(o, "Valve"))
        .and_then(|o| get_obj(o, "Steam"))
        .and_then(|o| get_obj(o, "CompatToolMapping"))
    else {
        return Ok(out);
    };
    for (appid, values) in map.iter() {
        if let Some(o) = values.first().and_then(Value::get_obj) {
            let name = get_str(o, "name").unwrap_or_default().to_string();
            out.insert(
                appid.to_string(),
                CompatTool {
                    name,
                    config: get_str(o, "config").map(str::to_string),
                    priority: get_str(o, "priority").map(str::to_string),
                },
            );
        }
    }
    Ok(out)
}

/// The tool explicitly mapped to `appid` (non-empty name), if any.
pub fn mapped_tool(
    map: &std::collections::BTreeMap<String, CompatTool>,
    appid: u32,
) -> Option<&CompatTool> {
    map.get(&appid.to_string()).filter(|t| !t.name.is_empty())
}

/// The global default tool (`"0"`), used by Steam for Windows-only games.
pub fn default_tool(map: &std::collections::BTreeMap<String, CompatTool>) -> Option<&CompatTool> {
    map.get("0").filter(|t| !t.name.is_empty())
}

/// Reads `<root>/config/config.vdf` of the first root that has one.
pub fn read_compat_tool_mapping(
    roots: &[PathBuf],
) -> std::collections::BTreeMap<String, CompatTool> {
    for root in roots {
        let path = root.join("config/config.vdf");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match parse_compat_tool_mapping(&text, &path.display().to_string()) {
            Ok(m) => return m,
            Err(e) => tracing::warn!("skipping {}: {e}", path.display()),
        }
    }
    Default::default()
}

/// Parsed `toolmanifest.vdf` of a compat tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolManifest {
    pub commandline: Option<String>,
    /// Steam Linux Runtime the tool must run inside.
    pub require_tool_appid: Option<u32>,
}

pub fn parse_tool_manifest(text: &str, origin: &str) -> Result<ToolManifest, GamesError> {
    let vdf = parse_root(text, origin)?;
    let obj = vdf
        .value
        .get_obj()
        .ok_or_else(|| vdf_err(origin, "top level is not an object"))?;
    Ok(ToolManifest {
        commandline: get_str(obj, "commandline").map(str::to_string),
        require_tool_appid: get_str(obj, "require_tool_appid").and_then(|s| s.parse().ok()),
    })
}

/// Internal names declared by a `compatibilitytool.vdf`
/// (`compatibilitytools > compat_tools > <name>`).
pub fn parse_compatibility_tool_names(text: &str, origin: &str) -> Result<Vec<String>, GamesError> {
    let vdf = parse_root(text, origin)?;
    let obj = vdf
        .value
        .get_obj()
        .ok_or_else(|| vdf_err(origin, "top level is not an object"))?;
    Ok(get_obj(obj, "compat_tools")
        .map(|t| t.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default())
}

/// Lower-case alphanumerics only (`Proton - Experimental` -> `protonexperimental`).
fn tool_slug(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Folder of the compat tool named `name`: a `compatibilitytools.d` tool
/// whose `compatibilitytool.vdf` declares it, or an official Proton under
/// `steamapps/common` whose folder name matches (`proton_9` ->
/// `Proton 9.0`, `proton_experimental` -> `Proton - Experimental`).
pub fn find_compat_tool(roots: &[PathBuf], libraries: &[PathBuf], name: &str) -> Option<PathBuf> {
    for root in roots {
        let Ok(dirs) = std::fs::read_dir(root.join("compatibilitytools.d")) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = dirs.flatten().map(|e| e.path()).collect();
        dirs.sort();
        for dir in dirs {
            let vdf = dir.join("compatibilitytool.vdf");
            let declared = std::fs::read_to_string(&vdf)
                .ok()
                .and_then(|t| parse_compatibility_tool_names(&t, &vdf.display().to_string()).ok())
                .unwrap_or_default();
            let dir_name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
            if declared.iter().any(|d| d == name) || dir_name.as_deref() == Some(name) {
                return Some(dir);
            }
        }
    }
    let want = tool_slug(name);
    for lib in libraries {
        let Ok(dirs) = std::fs::read_dir(lib.join("steamapps/common")) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = dirs.flatten().map(|e| e.path()).collect();
        dirs.sort();
        for dir in dirs {
            if !dir.join("proton").is_file() {
                continue;
            }
            let slug = dir.file_name().map(|n| tool_slug(&n.to_string_lossy()));
            // Official folders carry a minor version the internal name drops.
            if slug.as_deref() == Some(want.as_str()) || slug == Some(format!("{want}0")) {
                return Some(dir);
            }
        }
    }
    None
}

/// The Steam Linux Runtime `run` entry point for `appid` (the tool's
/// `require_tool_appid`), from its appmanifest `installdir`.
pub fn find_runtime_entry(libraries: &[PathBuf], appid: u32) -> Option<PathBuf> {
    find_app(libraries, appid)
        .map(|a| a.install_dir.join("run"))
        .filter(|p| p.is_file())
}

/// Proton directory recorded in `<compatdata>/config_info` (line 2 or 3,
/// `<proton>/files/share/fonts/` or `<proton>/files/lib/`).
pub fn config_info_proton_dir(text: &str) -> Option<PathBuf> {
    text.lines().skip(1).take(2).find_map(|l| {
        let l = l.trim_end_matches('/');
        let i = l.find("/files/")?;
        Some(PathBuf::from(&l[..i]))
    })
}

/// True for a Steam root inside the Flatpak Steam sandbox.
pub fn is_flatpak_steam(path: &Path) -> bool {
    path.to_string_lossy()
        .contains(".var/app/com.valvesoftware.Steam/")
}

/// One account from `config/loginusers.vdf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamUser {
    pub steamid64: u64,
    pub account_name: Option<String>,
    pub persona_name: Option<String>,
    pub most_recent: bool,
    pub timestamp: u64,
}

impl SteamUser {
    /// The `userdata/<accountid>` folder name.
    pub fn account_id(&self) -> u64 {
        self.steamid64.saturating_sub(76_561_197_960_265_728)
    }
}

pub fn parse_login_users(text: &str, origin: &str) -> Result<Vec<SteamUser>, GamesError> {
    let vdf = parse_root(text, origin)?;
    let obj = vdf
        .value
        .get_obj()
        .ok_or_else(|| vdf_err(origin, "top level is not an object"))?;
    let mut out = Vec::new();
    for (id, values) in obj.iter() {
        let (Ok(steamid64), Some(u)) = (id.parse(), values.first().and_then(Value::get_obj)) else {
            continue;
        };
        out.push(SteamUser {
            steamid64,
            account_name: get_str(u, "AccountName").map(str::to_string),
            persona_name: get_str(u, "PersonaName").map(str::to_string),
            most_recent: get_str(u, "MostRecent") == Some("1"),
            timestamp: get_str(u, "timestamp")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
        });
    }
    Ok(out)
}

/// The account Steam last used: `MostRecent = 1` if any (current clients
/// no longer write it), else the newest `timestamp`, else the only user.
/// `None` when that is not decidable (no users, or a timestamp tie).
pub fn most_recent_user(users: &[SteamUser]) -> Option<&SteamUser> {
    if let Some(u) = users.iter().find(|u| u.most_recent) {
        return Some(u);
    }
    if let [only] = users {
        return Some(only);
    }
    let max = users.iter().map(|u| u.timestamp).max()?;
    let mut newest = users.iter().filter(|u| u.timestamp == max);
    match (newest.next(), newest.next()) {
        (Some(u), None) if max > 0 => Some(u),
        _ => None,
    }
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

    /// Catches: the mapping read from the wrong VDF path, the global
    /// default treated as a per-app mapping, or empty names counted.
    #[test]
    fn compat_tool_mapping_reads_per_app_and_global_default() {
        let text = r#""InstallConfigStore" { "Software" { "Valve" { "Steam" {
            "CompatToolMapping" {
                "0" { "name" "Proton-GE Latest" "config" "" "priority" "75" }
                "489830" { "name" "proton_9" "config" "" "priority" "250" }
                "413150" { "name" "" "config" "" "priority" "250" }
            } } } } }"#;
        let m = parse_compat_tool_mapping(text, "config.vdf").unwrap();
        assert_eq!(mapped_tool(&m, 489830).unwrap().name, "proton_9");
        assert_eq!(mapped_tool(&m, 413150), None);
        assert_eq!(mapped_tool(&m, 1), None);
        assert_eq!(default_tool(&m).unwrap().name, "Proton-GE Latest");
    }

    /// Catches: an update-pending or broken install counted as installed.
    #[test]
    fn state_flags_fully_installed_only_when_clean() {
        assert!(state_flags::fully_installed(4));
        assert!(!state_flags::fully_installed(6));
        assert!(!state_flags::fully_installed(2));
        assert!(!state_flags::fully_installed(4 | 32));
        assert!(!state_flags::fully_installed(1));
    }

    /// Catches: picking an arbitrary account when the client no longer
    /// writes MostRecent, or guessing on a tie.
    #[test]
    fn most_recent_user_prefers_flag_then_timestamp() {
        let text = r#""users" {
            "76561197960265729" { "AccountName" "a" "timestamp" "100" }
            "76561197960265730" { "AccountName" "b" "timestamp" "200" }
        }"#;
        let users = parse_login_users(text, "loginusers.vdf").unwrap();
        let u = most_recent_user(&users).unwrap();
        assert_eq!((u.account_name.as_deref(), u.account_id()), (Some("b"), 2));
        let flagged = text.replace(
            "\"timestamp\" \"100\"",
            "\"MostRecent\" \"1\" \"timestamp\" \"100\"",
        );
        let users = parse_login_users(&flagged, "x").unwrap();
        assert_eq!(
            most_recent_user(&users).unwrap().account_name.as_deref(),
            Some("a")
        );
        let tie = text.replace("\"200\"", "\"100\"");
        assert_eq!(
            most_recent_user(&parse_login_users(&tie, "x").unwrap()),
            None
        );
    }

    /// Catches: official Proton folders or custom tools not resolved from
    /// their internal names, and the SLR entry point not found.
    #[test]
    fn compat_tools_and_runtime_resolve_from_libraries() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, root, _) = fake_steam(tmp.path());
        let ge = root.join("compatibilitytools.d/GE-Proton10-34");
        fs::create_dir_all(&ge).unwrap();
        fs::write(
            ge.join("compatibilitytool.vdf"),
            r#""compatibilitytools" { "compat_tools" { "GE-Proton10-34" { "install_path" "." } } }"#,
        )
        .unwrap();
        let official = root.join("steamapps/common/Proton 9.0");
        fs::create_dir_all(&official).unwrap();
        fs::write(official.join("proton"), "").unwrap();
        let libs = vec![root.clone()];
        let roots = vec![root.clone()];
        assert_eq!(find_compat_tool(&roots, &libs, "GE-Proton10-34"), Some(ge));
        assert_eq!(find_compat_tool(&roots, &libs, "proton_9"), Some(official));
        assert_eq!(find_compat_tool(&roots, &libs, "proton_8"), None);

        fs::write(
            root.join("steamapps/appmanifest_1628350.acf"),
            r#""AppState" { "appid" "1628350" "installdir" "SteamLinuxRuntime_sniper" "StateFlags" "4" }"#,
        )
        .unwrap();
        assert_eq!(
            find_runtime_entry(&libs, 1628350),
            None,
            "no run script yet"
        );
        let slr = root.join("steamapps/common/SteamLinuxRuntime_sniper");
        fs::create_dir_all(&slr).unwrap();
        fs::write(slr.join("run"), "").unwrap();
        assert_eq!(find_runtime_entry(&libs, 1628350), Some(slr.join("run")));
        let tm = parse_tool_manifest(
            r#""manifest" { "commandline" "/proton %verb%" "require_tool_appid" "1628350" }"#,
            "toolmanifest.vdf",
        )
        .unwrap();
        assert_eq!(tm.require_tool_appid, Some(1628350));
    }

    /// Writes `appmanifest_<appid>.acf` in `lib`, and its install folder
    /// unless `dir` is false.
    fn manifest(lib: &Path, appid: u32, name: &str, flags: u32, dir: bool) -> PathBuf {
        let installdir = format!("{name} dir");
        fs::create_dir_all(lib.join("steamapps")).unwrap();
        fs::write(
            lib.join(format!("steamapps/appmanifest_{appid}.acf")),
            format!(
                "\"AppState\" {{ \"appid\" \"{appid}\" \"name\" \"{name}\" \"installdir\" \"{installdir}\" \"StateFlags\" \"{flags}\" }}"
            ),
        )
        .unwrap();
        let p = lib.join("steamapps/common").join(installdir);
        if dir {
            fs::create_dir_all(&p).unwrap();
        }
        p
    }

    /// Catches: only preset/known apps scanned, second libraryfolders entry
    /// ignored, update-pending games hidden, tools/runtimes, busy or
    /// folderless installs listed, or a duplicate appid listed twice.
    #[test]
    fn list_installed_apps_scans_every_manifest_in_every_library() {
        let tmp = tempfile::tempdir().unwrap();
        let (home, root, sd) = fake_steam(tmp.path());
        manifest(&root, 632360, "Risk of Rain 2", 4, true);
        manifest(&root, 1422450, "Deadlock", 6, true);
        let proton = manifest(&root, 1493710, "Proton Experimental", 4, true);
        fs::write(proton.join("toolmanifest.vdf"), "").unwrap();
        let ge = manifest(&root, 3000001, "Some Runtime", 4, true);
        fs::write(ge.join("toolmanifest.vdf"), "").unwrap();
        manifest(&root, 228980, "Steamworks Common Redistributables", 4, true);
        manifest(&root, 999001, "Updating", 1026, true);
        manifest(&root, 999002, "No Folder", 4, false);
        manifest(&root, 999003, "Not Installed", 0, true);
        manifest(&sd, 1808500, "ARC Raiders", 4, true);
        manifest(&sd, 632360, "Risk of Rain 2 copy", 4, true);

        let libs = steam_libraries(&home);
        let apps = list_installed_apps(&libs);
        let mut got: Vec<(u32, &str)> = apps
            .iter()
            .map(|a| (a.appid, a.name.as_deref().unwrap()))
            .collect();
        got.sort();
        assert_eq!(
            got,
            vec![
                (632360, "Risk of Rain 2"),
                (1422450, "Deadlock"),
                (1808500, "ARC Raiders"),
            ]
        );
        let arc = apps.iter().find(|a| a.appid == 1808500).unwrap();
        assert_eq!(arc.library, sd.canonicalize().unwrap());
    }

    /// Catches: the Flatpak Steam root not scanned.
    #[test]
    fn list_installed_apps_finds_games_under_flatpak_steam() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp
            .path()
            .join(".var/app/com.valvesoftware.Steam/.local/share/Steam");
        manifest(&root, 646570, "Slay the Spire", 4, true);
        let apps = list_installed_apps(&steam_libraries(tmp.path()));
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].appid, 646570);
        assert!(is_flatpak_steam(&apps[0].library));
    }

    /// Catches: a pending update hiding a game, or busy/broken counted.
    #[test]
    fn state_flags_installed_allows_pending_update_only() {
        assert!(state_flags::installed(4));
        assert!(state_flags::installed(6));
        assert!(!state_flags::installed(2));
        assert!(!state_flags::installed(4 | 1024));
        assert!(!state_flags::installed(4 | 32));
    }

    #[test]
    fn config_info_names_the_proton_dir() {
        let text = "GE-Proton10-34\n/s/compatibilitytools.d/GE/files/share/fonts/\n/s/compatibilitytools.d/GE/files/lib/\n";
        assert_eq!(
            config_info_proton_dir(text),
            Some(PathBuf::from("/s/compatibilitytools.d/GE"))
        );
        assert_eq!(config_info_proton_dir("only one line"), None);
        assert!(is_flatpak_steam(Path::new(
            "/home/u/.var/app/com.valvesoftware.Steam/.local/share/Steam"
        )));
    }
}
