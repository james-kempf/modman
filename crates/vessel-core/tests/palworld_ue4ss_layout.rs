// SPDX-License-Identifier: GPL-3.0-or-later
//! D01: Palworld UE4SS Lua mods must keep their per-mod folder, so every
//! mod's Scripts/main.lua lands on its own deploy path. Fixture trees mirror
//! the real archives (listed with `unzip -Z1`). Temp dirs only.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use vessel_core::deploy::build_plan;
use vessel_core::games::{self, builtin_definitions, find_definition, GameConfig};
use vessel_core::loadorder::{conflict_data, PluginsFiles};
use vessel_core::mods::{install_archive, InstallHooks, InstallMeta};
use vessel_core::staging::{normalize_layout, LayoutOptions, LayoutProfile};
use vessel_core::store::Store;

const PAKS: &str = "Pal/Content/Paks/~mods";
const UE4SS_MODS: &str = "Pal/Binaries/Win64/ue4ss/Mods";
const LEGACY_MODS: &str = "Pal/Binaries/Win64/Mods";
const PROBE: &str = "Pal/Binaries/Win64/ue4ss/UE4SS.dll";

/// (vessel mod name, archive entries, UE4SS mod folder the game needs)
type Fixture = (&'static str, &'static [&'static str], &'static str);

/// The 9 reported mods, then Ultra Graphics, Ultra Weather,
/// RecoverPalSpheres, PalInfoCustomHUD, then the AntiWaste control.
const FIXTURES: &[Fixture] = &[
    (
        "AutomaticallySkipModCaution",
        &[
            "AutomaticallySkipModCaution/enabled.txt",
            "AutomaticallySkipModCaution/Scripts/main.lua",
        ],
        "AutomaticallySkipModCaution",
    ),
    (
        "CustomBaseRadius125",
        &[
            "CustomBaseRadius/enabled.txt",
            "CustomBaseRadius/Scripts/main.lua",
        ],
        "CustomBaseRadius",
    ),
    (
        "FSS - Full Sphere Summon",
        &[
            "FullSphereSummon/enabled.txt",
            "FullSphereSummon/Scripts/config.txt",
            "FullSphereSummon/Scripts/main.lua",
            "FullSphereSummon/Scripts/multiplayer.lua",
            "FullSphereSummon/Scripts/singleplayer.lua",
            "README.md",
        ],
        "FullSphereSummon",
    ),
    (
        "Grounded Grass",
        &[
            "GroundedGrass/enabled.txt",
            "GroundedGrass/Scripts/main.lua",
        ],
        "GroundedGrass",
    ),
    (
        "InstantPalboxSearch",
        &[
            "InstantPalboxSearch/Scripts/main.lua",
            "InstantPalboxSearch/Scripts/search.lua",
        ],
        "InstantPalboxSearch",
    ),
    (
        "Move Common Reward Notifications Down",
        &[
            "ue4ss/Mods/MoveCommonRewardDown/enabled.txt",
            "ue4ss/Mods/MoveCommonRewardDown/scripts/main.lua",
        ],
        "MoveCommonRewardDown",
    ),
    (
        "no food decay",
        &["NoFoodDecay/enabled.txt", "NoFoodDecay/Scripts/main.lua"],
        "NoFoodDecay",
    ),
    (
        "Remove flying stamina cost",
        &[
            "vuxRemoveFlyingStamina/enabled.txt",
            "vuxRemoveFlyingStamina/Scripts/config.lua",
            "vuxRemoveFlyingStamina/Scripts/main.lua",
        ],
        "vuxRemoveFlyingStamina",
    ),
    (
        "Remove pal revive timer",
        &[
            "vuxRemovePalReviveTimer/enabled.txt",
            "vuxRemovePalReviveTimer/Scripts/config.lua",
            "vuxRemovePalReviveTimer/Scripts/main.lua",
        ],
        "vuxRemovePalReviveTimer",
    ),
    (
        "Ultra Graphics 1.2.2",
        &[
            "UltraGraphics/config.lua",
            "UltraGraphics/enabled.txt",
            "UltraGraphics/Scripts/main.lua",
        ],
        "UltraGraphics",
    ),
    (
        "Ultra Weather 1.0.1",
        &[
            "UltraWeather/config.lua",
            "UltraWeather/enabled.txt",
            "UltraWeather/Scripts/main.lua",
        ],
        "UltraWeather",
    ),
    (
        "RecoverPalSpheres 2.0",
        &[
            "RecoverPalSpheres/config.json",
            "RecoverPalSpheres/enabled.txt",
            "RecoverPalSpheres/README.txt",
            "RecoverPalSpheres/Scripts/debug.lua",
            "RecoverPalSpheres/Scripts/main.lua",
        ],
        "RecoverPalSpheres",
    ),
    (
        "PalInfoCustomHUD",
        &[
            "PalInfoCustomHUD/enabled.txt",
            "PalInfoCustomHUD/Info.json",
            "PalInfoCustomHUD/PalInfoCustomHUD.modconfig.json",
            "PalInfoCustomHUD/PalInfoCustomHUD.userconfig.lua",
            "PalInfoCustomHUD/Scripts/main.lua",
        ],
        "PalInfoCustomHUD",
    ),
    // Control: already game-root shaped; must stay as-is.
    (
        "AntiWaste",
        &[
            "Pal/Binaries/Win64/ue4ss/Mods/AntiWaste/enabled.txt",
            "Pal/Binaries/Win64/ue4ss/Mods/AntiWaste/Scripts/main.lua",
        ],
        "AntiWaste",
    ),
];

fn mk(root: &Path, paths: &[&str]) {
    for p in paths {
        let f = root.join(p);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        // Same bytes for same relative path: the old layout made these clash.
        let rel = p.split_once('/').map_or(*p, |(_, r)| r);
        fs::write(&f, rel.as_bytes()).unwrap();
    }
}

fn staged(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = walkdir::WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            e.path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    v.sort();
    v
}

/// normalize_layout only, Palworld options with the default UE4SS dir.
fn layout(name: &str, entries: &[&str]) -> Vec<String> {
    let t = tempfile::tempdir().unwrap();
    let src = t.path().join("src");
    let out = t.path().join("out");
    mk(&src, entries);
    let mut o = LayoutOptions::new(LayoutProfile::Ue, name);
    o.paks_dir = Some(PAKS);
    o.ue4ss_mods_dir = Some(UE4SS_MODS);
    normalize_layout(&src, &out, &o).unwrap();
    staged(&out)
}

/// Each Lua mod's main.lua (and enabled.txt) lands in its own UE4SS folder.
#[test]
fn ue4ss_lua_mods_keep_their_mod_folder() {
    let mut failures = Vec::new();
    for (name, entries, folder) in FIXTURES {
        let files = layout(name, entries);
        let want = format!("{UE4SS_MODS}/{folder}/scripts/main.lua");
        if !files.iter().any(|f| f.eq_ignore_ascii_case(&want)) {
            failures.push(format!("{name}: want {want}, got {files:?}"));
        }
        // enabled.txt is how UE4SS turns a mod on; it must not be dropped as a doc.
        if entries.iter().any(|e| e.ends_with("/enabled.txt")) {
            let en = format!("{UE4SS_MODS}/{folder}/enabled.txt");
            if !files.iter().any(|f| f.eq_ignore_ascii_case(&en)) {
                failures.push(format!("{name}: enabled.txt missing (want {en})"));
            }
        }
        // Everything lives in the mod's folder (README.md at the archive
        // root is a doc and is dropped).
        let prefix = format!("{UE4SS_MODS}/{folder}/").to_lowercase();
        for f in &files {
            if !f.to_lowercase().starts_with(&prefix) {
                failures.push(format!("{name}: stray file {f}"));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Catches: config files inside a UE4SS mod folder dropped by the doc filter.
#[test]
fn ue4ss_mod_folders_keep_config_files() {
    let fss = layout(FIXTURES[2].0, FIXTURES[2].1);
    assert!(fss.contains(&format!("{UE4SS_MODS}/FullSphereSummon/Scripts/config.txt")));
    let ug = layout(FIXTURES[9].0, FIXTURES[9].1);
    assert!(ug.contains(&format!("{UE4SS_MODS}/UltraGraphics/config.lua")));
    let rps = layout(FIXTURES[11].0, FIXTURES[11].1);
    assert!(rps.contains(&format!("{UE4SS_MODS}/RecoverPalSpheres/README.txt")));
}

/// No two distinct Lua mods may share a staged path (the user's report).
#[test]
fn distinct_ue4ss_mods_do_not_share_paths() {
    let mut owners: HashMap<String, Vec<&str>> = HashMap::new();
    for (name, entries, _) in FIXTURES {
        for f in layout(name, entries) {
            owners.entry(f.to_lowercase()).or_default().push(name);
        }
    }
    let mut clashes: Vec<String> = owners
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(k, v)| format!("{k}: {v:?}"))
        .collect();
    clashes.sort();
    assert!(clashes.is_empty(), "\n{}", clashes.join("\n"));
}

// ---- full install pipeline + conflict computation ----

struct Fx {
    tmp: tempfile::TempDir,
    store: Store,
    game: GameConfig,
}

impl Fx {
    /// Palworld from its built-in definition, with a temp game folder
    /// holding `present` (relative paths; a trailing '/' makes a dir).
    fn palworld(present: &[&str]) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let game_dir = tmp.path().join("game");
        fs::create_dir_all(&game_dir).unwrap();
        for p in present {
            let f = game_dir.join(p);
            if p.ends_with('/') {
                fs::create_dir_all(&f).unwrap();
            } else {
                fs::create_dir_all(f.parent().unwrap()).unwrap();
                fs::write(&f, b"probe").unwrap();
            }
        }
        let defs = builtin_definitions();
        let mut game = GameConfig::from_definition(find_definition(&defs, "palworld").unwrap());
        game.folder = Some(game_dir);
        let store = Store::open_in_memory().unwrap();
        games::save_game(store.conn(), &game).unwrap();
        Fx { tmp, store, game }
    }

    fn install(&mut self, name: &str, entries: &[&str]) {
        let src: PathBuf = self.tmp.path().join("src").join(name);
        mk(&src, entries);
        let meta = InstallMeta {
            name: Some(name.to_string()),
            ..InstallMeta::default()
        };
        let data = self.tmp.path().join("data");
        install_archive(
            &mut self.store,
            &self.game,
            &data,
            &src,
            &meta,
            InstallHooks::default(),
        )
        .unwrap();
    }

    /// Deploy-plan target paths.
    fn targets(&self) -> Vec<String> {
        let mut v: Vec<String> = build_plan(self.store.conn(), &self.game)
            .unwrap()
            .items
            .into_iter()
            .map(|i| i.rel_path)
            .collect();
        v.sort();
        v
    }

    fn conflicts(&self) -> Vec<String> {
        let files = PluginsFiles::new(self.tmp.path().join("game"));
        conflict_data(self.store.conn(), &self.game, &files)
            .unwrap()
            .files
            .into_iter()
            .map(|c| c.rel_path)
            .collect()
    }
}

/// Catches: the user's report (every Lua mod's main.lua/config.lua on one
/// deploy path) through the real install pipeline and conflict view.
#[test]
fn installed_ue4ss_mods_have_no_conflicts() {
    let mut fx = Fx::palworld(&[PROBE]);
    for (name, entries, _) in FIXTURES {
        fx.install(name, entries);
    }
    let targets = fx.targets();
    for (name, _, folder) in FIXTURES {
        let want = format!("{UE4SS_MODS}/{folder}/scripts/main.lua");
        assert!(
            targets.iter().any(|t| t.eq_ignore_ascii_case(&want)),
            "{name}: {want} not in {targets:?}"
        );
    }
    let conflicts = fx.conflicts();
    assert!(conflicts.is_empty(), "{conflicts:?}");
    for t in &targets {
        let l = t.to_lowercase();
        assert!(
            !matches!(
                l.as_str(),
                "main.lua" | "config.lua" | "scripts/main.lua" | "scripts/config.lua"
            ),
            "root-level clash path staged: {t}"
        );
    }
}

/// Catches: two mods that differ only by their folder name (same file
/// contents) still conflicting.
#[test]
fn mods_differing_only_by_folder_name_do_not_conflict() {
    let mut fx = Fx::palworld(&[PROBE]);
    fx.install("Mod A", &["ModA/enabled.txt", "ModA/Scripts/main.lua"]);
    fx.install("Mod B", &["ModB/enabled.txt", "ModB/Scripts/main.lua"]);
    assert_eq!(fx.conflicts(), Vec::<String>::new());
    assert_eq!(
        fx.targets(),
        [
            format!("{UE4SS_MODS}/ModA/Scripts/main.lua"),
            format!("{UE4SS_MODS}/ModA/enabled.txt"),
            format!("{UE4SS_MODS}/ModB/Scripts/main.lua"),
            format!("{UE4SS_MODS}/ModB/enabled.txt"),
        ]
    );
}

/// Catches (risk 5): the install picking the wrong mods folder for the
/// UE4SS version found in the (temp) game folder.
#[test]
fn install_uses_probed_ue4ss_layout() {
    let cases: [(&[&str], &str); 3] = [
        (&[PROBE], UE4SS_MODS),
        (&["Pal/Binaries/Win64/Mods/"], LEGACY_MODS),
        (&[], UE4SS_MODS),
    ];
    for (present, dir) in cases {
        let mut fx = Fx::palworld(present);
        fx.install("Lua", &["Lua/enabled.txt", "Lua/Scripts/main.lua"]);
        assert_eq!(
            fx.targets(),
            [
                format!("{dir}/Lua/Scripts/main.lua"),
                format!("{dir}/Lua/enabled.txt")
            ],
            "{present:?}"
        );
    }
}
