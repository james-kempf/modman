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

/// UE4SS settings for ue-profile games (`[ue4ss]` in the game TOML). All
/// paths are relative to the deploy root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ue4ssDef {
    /// Mods folder of current UE4SS releases (`ue4ss/Mods`).
    pub mods_dir: String,
    /// Mods folder of older UE4SS releases (next to the game binary).
    pub legacy_mods_dir: String,
    /// File whose presence marks a current UE4SS install.
    pub probe: String,
}

impl Ue4ssDef {
    /// The mods folder in use. `exists` answers "does this deploy-root
    /// relative path exist?" and is injected so tests never touch a real
    /// game folder. Current probe present -> `mods_dir`; only the legacy
    /// folder present -> `legacy_mods_dir`; nothing found -> `mods_dir`.
    pub fn resolve_mods_dir(&self, exists: impl Fn(&str) -> bool) -> &str {
        if !exists(&self.probe) && exists(&self.legacy_mods_dir) {
            &self.legacy_mods_dir
        } else {
            &self.mods_dir
        }
    }
}

/// Folder a shared file lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedFileBase {
    /// The resolved UE4SS mods folder (see [`Ue4ssDef::resolve_mods_dir`]).
    Ue4ssModsDir,
}

/// How a shared file is merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedFileFormat {
    /// UE4SS `mods.txt`: `Name : 0|1` lines, `Keybinds` last.
    Ue4ssModsTxt,
}

/// A file many mods may ship that modman owns and merges instead of
/// placing (`[[shared_files]]`). Schema only; merging lives in deploy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedFileDef {
    /// File name inside `relative_to` (one path component).
    pub name: String,
    pub relative_to: SharedFileBase,
    pub format: SharedFileFormat,
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
    /// UE4SS mod folders (ue profile only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ue4ss: Option<Ue4ssDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared_files: Vec<SharedFileDef>,
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
    let u = def.ue4ss.as_ref();
    for (key, value) in [
        ("subdir", Some(&def.subdir)),
        ("paks_dir", def.paks_dir.as_ref()),
        ("ue4ss.mods_dir", u.map(|u| &u.mods_dir)),
        ("ue4ss.legacy_mods_dir", u.map(|u| &u.legacy_mods_dir)),
        ("ue4ss.probe", u.map(|u| &u.probe)),
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
    if let Some(u) = u {
        if def.profile != "ue" {
            return Err(bad("[ue4ss] is only valid for the ue profile".into()));
        }
        // An empty or "." folder would make every mod root a UE4SS folder.
        for (key, v) in [
            ("mods_dir", &u.mods_dir),
            ("legacy_mods_dir", &u.legacy_mods_dir),
            ("probe", &u.probe),
        ] {
            if v.trim_start_matches("./").trim_matches('/').is_empty() || v == "." {
                return Err(bad(format!("ue4ss.{key} must not be empty")));
            }
        }
    }
    for f in &def.shared_files {
        match f.relative_to {
            SharedFileBase::Ue4ssModsDir if def.ue4ss.is_none() => {
                return Err(bad(format!(
                    "shared file '{}' is relative to ue4ss_mods_dir but there is no [ue4ss]",
                    f.name
                )));
            }
            SharedFileBase::Ue4ssModsDir => {}
        }
        let single = !f.name.is_empty()
            && !f.name.contains(['/', '\\', '\0'])
            && !matches!(f.name.as_str(), "." | "..");
        if !single {
            return Err(bad(format!(
                "shared file name '{}' must be one path component",
                f.name
            )));
        }
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

    /// Catches: palworld.toml losing or misspelling the [ue4ss] block or the
    /// mods.txt rule, and the new fields leaking into other definitions.
    #[test]
    fn palworld_declares_ue4ss_and_mods_txt_only_palworld_does() {
        let defs = builtin_definitions();
        let pal = find_definition(&defs, "palworld").unwrap();
        assert_eq!(
            pal.ue4ss,
            Some(Ue4ssDef {
                mods_dir: "Pal/Binaries/Win64/ue4ss/Mods".into(),
                legacy_mods_dir: "Pal/Binaries/Win64/Mods".into(),
                probe: "Pal/Binaries/Win64/ue4ss/UE4SS.dll".into(),
            })
        );
        assert_eq!(
            pal.shared_files,
            [SharedFileDef {
                name: "mods.txt".into(),
                relative_to: SharedFileBase::Ue4ssModsDir,
                format: SharedFileFormat::Ue4ssModsTxt,
            }]
        );
        for d in defs.iter().filter(|d| d.domain != "palworld") {
            assert!(
                d.ue4ss.is_none() && d.shared_files.is_empty(),
                "{}",
                d.domain
            );
        }
        // Other definitions serialise exactly as before (no new keys).
        let sse = find_definition(&defs, "skyrimspecialedition").unwrap();
        let json = serde_json::to_string(sse).unwrap();
        assert!(
            !json.contains("ue4ss") && !json.contains("shared_files"),
            "{json}"
        );
    }

    /// Catches: invalid [ue4ss]/[[shared_files]] combinations being accepted.
    #[test]
    fn ue4ss_and_shared_files_are_validated() {
        let ue = "domain = \"x\"\nname = \"X\"\nprofile = \"ue\"\n";
        let block = "[ue4ss]\nmods_dir = \"A/Mods\"\nlegacy_mods_dir = \"B/Mods\"\nprobe = \"A/UE4SS.dll\"\n";
        let shared = |name: &str| {
            format!("[[shared_files]]\nname = \"{name}\"\nrelative_to = \"ue4ss_mods_dir\"\nformat = \"ue4ss_mods_txt\"\n")
        };
        assert!(parse_definition(&format!("{ue}{block}{}", shared("mods.txt")), "x.toml").is_ok());
        let bad = [
            // [ue4ss] outside the ue profile.
            format!("domain = \"x\"\nname = \"X\"\nprofile = \"generic\"\n{block}"),
            // Escaping and empty folders.
            format!("{ue}[ue4ss]\nmods_dir = \"../M\"\nlegacy_mods_dir = \"B\"\nprobe = \"p\"\n"),
            format!("{ue}[ue4ss]\nmods_dir = \".\"\nlegacy_mods_dir = \"B\"\nprobe = \"p\"\n"),
            // Unknown key, missing key.
            format!("{ue}{block}extra = 1\n"),
            format!("{ue}[ue4ss]\nmods_dir = \"A\"\n"),
            // shared file without [ue4ss], with a path, unknown format.
            format!("{ue}{}", shared("mods.txt")),
            format!("{ue}{block}{}", shared("../mods.txt")),
            format!("{ue}{block}{}", shared("a/mods.txt")),
            format!("{ue}{block}[[shared_files]]\nname = \"m\"\nrelative_to = \"ue4ss_mods_dir\"\nformat = \"json\"\n"),
        ];
        for text in bad {
            assert!(parse_definition(&text, "x.toml").is_err(), "{text}");
        }
    }

    /// Catches: picking the wrong UE4SS mods folder (D01 risk 5).
    #[test]
    fn ue4ss_mods_dir_resolution_uses_the_injected_probe() {
        let defs = builtin_definitions();
        let u = find_definition(&defs, "palworld")
            .unwrap()
            .ue4ss
            .clone()
            .unwrap();
        let has = |set: &'static [&'static str]| move |p: &str| set.contains(&p);
        let new = "Pal/Binaries/Win64/ue4ss/Mods";
        let legacy = "Pal/Binaries/Win64/Mods";
        let dll = "Pal/Binaries/Win64/ue4ss/UE4SS.dll";
        assert_eq!(
            u.resolve_mods_dir(has(&["Pal/Binaries/Win64/ue4ss/UE4SS.dll"])),
            new
        );
        assert_eq!(
            u.resolve_mods_dir(has(&["Pal/Binaries/Win64/Mods"])),
            legacy
        );
        // Both present: the current UE4SS wins.
        assert_eq!(
            u.resolve_mods_dir(has(&[
                "Pal/Binaries/Win64/ue4ss/UE4SS.dll",
                "Pal/Binaries/Win64/Mods"
            ])),
            new
        );
        assert_eq!(u.resolve_mods_dir(has(&[])), new, "unknown -> default");
        assert_eq!(u.probe, dll);
    }

    #[test]
    fn plugins_rejected_outside_bethesda() {
        let text = "domain = \"x\"\nname = \"X\"\nprofile = \"generic\"\n[plugins]\nformat = \"star\"\nappdata_folder = \"X\"\n";
        assert!(parse_definition(text, "x.toml").is_err());
    }
}
