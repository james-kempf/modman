// SPDX-License-Identifier: GPL-3.0-or-later
//! Game definitions: one TOML file per game, embedded in the binary and
//! overridable by files in the user's definitions directory.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{GamesError, KNOWN_PROFILES};

/// How well a game is supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Support {
    /// Tested end to end.
    Tested,
    /// Shipped as data, not tested end to end.
    #[default]
    Experimental,
}

impl Support {
    pub fn as_str(self) -> &'static str {
        match self {
            Support::Tested => "tested",
            Support::Experimental => "experimental",
        }
    }
}

/// Where mod files are deployed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    #[default]
    GameDir,
    PrefixDir,
}

impl Target {
    pub fn as_str(self) -> &'static str {
        match self {
            Target::GameDir => "game_dir",
            Target::PrefixDir => "prefix_dir",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "game_dir" => Some(Target::GameDir),
            "prefix_dir" => Some(Target::PrefixDir),
            _ => None,
        }
    }
}

/// plugins.txt line format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginsFormat {
    /// `*Name.esp` marks active plugins (SSE, Enderal SE, FO4, Starfield).
    Star,
    /// Bare names (Skyrim LE, FNV, FO3, Oblivion).
    Plain,
}

impl PluginsFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            PluginsFormat::Star => "star",
            PluginsFormat::Plain => "plain",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "star" => Some(PluginsFormat::Star),
            "plain" => Some(PluginsFormat::Plain),
            _ => None,
        }
    }
}

/// plugins.txt settings for bethesda-profile games.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginsDef {
    pub format: PluginsFormat,
    /// Folder under `AppData/Local` in the Proton prefix.
    pub appdata_folder: String,
}

fn default_subdir() -> String {
    ".".to_string()
}

/// One game definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameDefinition {
    /// Nexus game domain.
    pub domain: String,
    pub name: String,
    pub steam_appid: Option<u32>,
    /// Deploy root relative to the game folder (`.` = game root).
    #[serde(default = "default_subdir")]
    pub subdir: String,
    #[serde(default)]
    pub target: Target,
    /// Layout profile name; rules live in `staging.rs`.
    pub profile: String,
    #[serde(default)]
    pub support: Support,
    #[serde(default)]
    pub plugins: Option<PluginsDef>,
    /// Pak mods folder relative to the game root (ue profile).
    #[serde(default)]
    pub paks_dir: Option<String>,
}

macro_rules! def {
    ($d:literal) => {
        ($d, include_str!(concat!("../../definitions/", $d, ".toml")))
    };
}

/// Embedded definitions in legacy preset order (`game detect` order).
const BUILTIN: &[(&str, &str)] = &[
    def!("cyberpunk2077"),
    def!("skyrimspecialedition"),
    def!("enderalspecialedition"),
    def!("skyrim"),
    def!("fallout4"),
    def!("starfield"),
    def!("newvegas"),
    def!("fallout3"),
    def!("oblivion"),
    def!("morrowind"),
    def!("stardewvalley"),
    def!("mountandblade2bannerlord"),
    def!("witcher3"),
    def!("valheim"),
    def!("outward"),
    def!("schedule1"),
    def!("palworld"),
    def!("clairobscurexpedition33"),
    def!("oblivionremastered"),
    def!("eldenring"),
    def!("control"),
    def!("hades2"),
    def!("warhammer40000spacemarine2"),
    def!("monsterhunterwilds"),
];

/// Parses and validates one definition. `origin` names the file in errors.
pub fn parse_definition(text: &str, origin: &str) -> Result<GameDefinition, GamesError> {
    let bad = |message: String| GamesError::Definition {
        origin: origin.to_string(),
        message,
    };
    let def: GameDefinition = toml::from_str(text).map_err(|e| bad(e.to_string()))?;
    if !super::is_valid_domain(&def.domain) {
        return Err(bad(format!("invalid domain '{}'", def.domain)));
    }
    if !KNOWN_PROFILES.contains(&def.profile.as_str()) {
        return Err(bad(format!(
            "unknown profile '{}' (known: {})",
            def.profile,
            KNOWN_PROFILES.join(" ")
        )));
    }
    for (key, value) in [
        ("subdir", Some(&def.subdir)),
        ("paks_dir", def.paks_dir.as_ref()),
    ] {
        if let Some(Err(m)) = value.map(|v| crate::deploy::check_rel_dir(v)) {
            return Err(bad(format!("{key} '{}': {m}", value.expect("checked"))));
        }
    }
    if def.plugins.is_some() && def.profile != "bethesda" {
        return Err(bad(
            "[plugins] is only valid for the bethesda profile".into()
        ));
    }
    Ok(def)
}

/// The embedded definitions, in legacy preset order.
pub fn builtin_definitions() -> Vec<GameDefinition> {
    BUILTIN
        .iter()
        .map(|(name, text)| {
            // Embedded data is validated by tests; a failure here is a build bug.
            parse_definition(text, &format!("builtin:{name}.toml"))
                .unwrap_or_else(|e| panic!("embedded game definition is invalid: {e}"))
        })
        .collect()
}

/// Builtins merged with `*.toml` files from `override_dir` (if it exists).
/// A user file with an existing domain replaces that definition in place;
/// new domains are appended in file-name order.
pub fn load_definitions(override_dir: Option<&Path>) -> Result<Vec<GameDefinition>, GamesError> {
    let mut defs = builtin_definitions();
    let Some(dir) = override_dir.filter(|d| d.is_dir()) else {
        return Ok(defs);
    };
    let io = |source| GamesError::Io {
        path: dir.to_path_buf(),
        source,
    };
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(io)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml") && p.is_file())
        .collect();
    files.sort();
    for file in files {
        let text = std::fs::read_to_string(&file).map_err(|source| GamesError::Io {
            path: file.clone(),
            source,
        })?;
        let def = parse_definition(&text, &file.display().to_string())?;
        match defs.iter_mut().find(|d| d.domain == def.domain) {
            Some(slot) => *slot = def,
            None => defs.push(def),
        }
    }
    Ok(defs)
}

/// Finds a definition by domain.
pub fn find_definition<'a>(defs: &'a [GameDefinition], domain: &str) -> Option<&'a GameDefinition> {
    defs.iter().find(|d| d.domain == domain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_builtin_per_legacy_preset() {
        let defs = builtin_definitions();
        assert_eq!(defs.len(), 24);
        let mut domains: Vec<_> = defs.iter().map(|d| d.domain.as_str()).collect();
        domains.sort();
        domains.dedup();
        assert_eq!(domains.len(), 24, "duplicate domains");
        for (name, _) in BUILTIN {
            assert!(
                find_definition(&defs, name).is_some(),
                "{name} file/domain mismatch"
            );
        }
    }

    #[test]
    fn skyrim_se_is_the_tested_reference_game() {
        let defs = builtin_definitions();
        let d = find_definition(&defs, "skyrimspecialedition").unwrap();
        assert_eq!(d.steam_appid, Some(489830));
        assert_eq!(d.subdir, "Data");
        assert_eq!(d.profile, "bethesda");
        assert_eq!(d.target, Target::GameDir);
        let p = d.plugins.as_ref().unwrap();
        assert_eq!(p.format, PluginsFormat::Star);
        assert_eq!(p.appdata_folder, "Skyrim Special Edition");
        assert_eq!(d.support, Support::Tested);
    }

    #[test]
    fn every_other_preset_is_experimental() {
        for d in builtin_definitions() {
            if d.domain != "skyrimspecialedition" {
                assert_eq!(d.support, Support::Experimental, "{}", d.domain);
            }
        }
    }

    #[test]
    fn presets_match_legacy_table() {
        // (domain, appid, subdir, profile, plugins, paks_dir) from legacy_spec §4.
        let defs = builtin_definitions();
        let check = |dom: &str, appid: u32, subdir: &str, profile: &str| {
            let d = find_definition(&defs, dom).unwrap();
            assert_eq!(
                (d.steam_appid, d.subdir.as_str(), d.profile.as_str()),
                (Some(appid), subdir, profile),
                "{dom}"
            );
            d.clone()
        };
        assert!(check("morrowind", 22320, "Data Files", "bethesda")
            .plugins
            .is_none());
        let fnv = check("newvegas", 22380, "Data", "bethesda")
            .plugins
            .unwrap();
        assert_eq!(
            (fnv.format, fnv.appdata_folder.as_str()),
            (PluginsFormat::Plain, "FalloutNV")
        );
        check("stardewvalley", 413150, "Mods", "stardew");
        check("mountandblade2bannerlord", 261550, "Modules", "bannerlord");
        check("witcher3", 292030, ".", "witcher3");
        check("schedule1", 3164500, ".", "melonloader");
        check("valheim", 892970, ".", "bepinex");
        check("cyberpunk2077", 1091500, ".", "cyberpunk");
        check("eldenring", 1245620, ".", "generic");
        let pal = check("palworld", 1623730, ".", "ue");
        assert_eq!(pal.paks_dir.as_deref(), Some("Pal/Content/Paks/~mods"));
        assert_eq!(
            find_definition(&defs, "mountandblade2bannerlord")
                .unwrap()
                .name,
            "Mount & Blade II: Bannerlord"
        );
    }

    #[test]
    fn user_override_replaces_and_extends() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("skyrimspecialedition.toml"),
            "domain = \"skyrimspecialedition\"\nname = \"My Skyrim\"\nsteam_appid = 489830\nsubdir = \"Data\"\nprofile = \"bethesda\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("newgame.toml"),
            "domain = \"newgame\"\nname = \"New Game\"\nprofile = \"generic\"\n",
        )
        .unwrap();
        let defs = load_definitions(Some(dir.path())).unwrap();
        assert_eq!(defs.len(), 25);
        let sse = find_definition(&defs, "skyrimspecialedition").unwrap();
        assert_eq!(sse.name, "My Skyrim");
        assert_eq!(
            sse.support,
            Support::Experimental,
            "override drops tested unless stated"
        );
        let new = find_definition(&defs, "newgame").unwrap();
        assert_eq!((new.subdir.as_str(), new.steam_appid), (".", None));
    }

    #[test]
    fn invalid_override_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("bad.toml"),
            "domain = \"bad\"\nname = \"Bad\"\nprofile = \"nosuch\"\n",
        )
        .unwrap();
        let err = load_definitions(Some(dir.path())).unwrap_err().to_string();
        assert!(err.contains("bad.toml") && err.contains("nosuch"), "{err}");
    }

    #[test]
    fn definition_with_escaping_subdir_or_paks_dir_is_rejected() {
        let base = "domain = \"x\"\nname = \"X\"\nprofile = \"generic\"\n";
        for extra in ["subdir = \"../escape\"\n", "paks_dir = \"/abs\"\n"] {
            let err = parse_definition(&format!("{base}{extra}"), "x.toml").unwrap_err();
            assert!(err.to_string().contains("x.toml"), "{err}");
        }
        assert!(parse_definition(&format!("{base}subdir = \"Data\"\n"), "x.toml").is_ok());
    }

    #[test]
    fn plugins_rejected_outside_bethesda() {
        let text = "domain = \"x\"\nname = \"X\"\nprofile = \"generic\"\n[plugins]\nformat = \"star\"\nappdata_folder = \"X\"\n";
        assert!(parse_definition(text, "x.toml").is_err());
    }
}
