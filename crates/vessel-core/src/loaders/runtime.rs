// SPDX-License-Identifier: GPL-3.0-or-later
//! How a game runs here (native or Proton), and the checks that must pass
//! before a loader op touches it.
//!
//! Detection order: user override -> Steam `CompatToolMapping` for the app
//! -> file sniff (an ELF at the game root or a definition-provided native
//! marker wins; otherwise a Windows `.exe` means Proton) -> `Unknown`.
//! The Proton binary comes only from the mapped (or global default) tool
//! and must live in a Steam library or `compatibilitytools.d`, owned by the
//! user or root and not group/world writable.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::types::{ProtonEnv, Runtime};
use super::LoaderError;
use crate::games::steam::{self, CompatTool};

/// What Steam looks like on this machine. Built once per op.
#[derive(Debug, Clone, Default)]
pub struct SteamEnv {
    /// Steam roots (the client install), first = the one in use.
    pub roots: Vec<PathBuf>,
    pub libraries: Vec<PathBuf>,
    pub mapping: BTreeMap<String, CompatTool>,
}

impl SteamEnv {
    pub fn discover(home: &Path) -> Self {
        let roots = steam::steam_roots(home);
        Self {
            mapping: steam::read_compat_tool_mapping(&roots),
            libraries: steam::steam_libraries(home),
            roots,
        }
    }
}

/// The game facts detection needs.
#[derive(Debug, Clone, Copy)]
pub struct GameFacts<'a> {
    pub game_dir: &'a Path,
    pub appid: Option<u32>,
    /// `games.runtime_override`: `native` or `proton`.
    pub runtime_override: Option<&'a str>,
    /// Game-root relative files that prove a native build.
    pub native_markers: &'a [String],
}

/// Detects the runtime. `Ok(Runtime::Unknown)` when the files say nothing.
pub fn detect(game: GameFacts<'_>, steam: &SteamEnv) -> Result<Runtime, LoaderError> {
    match game.runtime_override {
        Some("native") => return Ok(Runtime::Native),
        Some("proton") => {
            let appid = need_appid(game)?;
            let tool = steam::mapped_tool(&steam.mapping, appid)
                .or_else(|| steam::default_tool(&steam.mapping))
                .ok_or(LoaderError::NoCompatTool { appid })?;
            return proton_env(&tool.name, appid, game.game_dir, steam).map(Runtime::Proton);
        }
        Some(other) => {
            return Err(LoaderError::UnknownRuntime(format!(
                "runtime override '{other}' is not 'native' or 'proton'"
            )))
        }
        None => {}
    }
    if let Some(appid) = game.appid {
        if let Some(tool) = steam::mapped_tool(&steam.mapping, appid) {
            return proton_env(&tool.name, appid, game.game_dir, steam).map(Runtime::Proton);
        }
    }
    if has_native_build(game) {
        return Ok(Runtime::Native);
    }
    if has_windows_exe(game.game_dir) {
        let Some(appid) = game.appid else {
            return Ok(Runtime::Unknown);
        };
        let tool =
            steam::default_tool(&steam.mapping).ok_or(LoaderError::NoCompatTool { appid })?;
        return proton_env(&tool.name, appid, game.game_dir, steam).map(Runtime::Proton);
    }
    Ok(Runtime::Unknown)
}

fn need_appid(game: GameFacts<'_>) -> Result<u32, LoaderError> {
    game.appid
        .ok_or_else(|| LoaderError::UnknownRuntime("Proton needs a Steam app id".into()))
}

/// A native marker exists, or any regular file at the game root is an ELF
/// (Stardew's `StardewValley` is a script; `Stardew Valley` is the ELF).
fn has_native_build(game: GameFacts<'_>) -> bool {
    if game
        .native_markers
        .iter()
        .any(|m| game.game_dir.join(m).symlink_metadata().is_ok())
    {
        return true;
    }
    let Ok(dir) = fs::read_dir(game.game_dir) else {
        return false;
    };
    dir.flatten().any(|e| {
        let is_file = e.file_type().is_ok_and(|t| t.is_file());
        let mut magic = [0u8; 4];
        is_file
            && fs::File::open(e.path())
                .and_then(|mut f| f.read_exact(&mut magic))
                .is_ok()
            && magic == *b"\x7fELF"
    })
}

/// A `.exe` within a few folders of the game root (UE games keep theirs in
/// `<Project>/Binaries/Win64`).
fn has_windows_exe(game_dir: &Path) -> bool {
    walkdir::WalkDir::new(game_dir)
        .min_depth(1)
        .max_depth(4)
        .follow_links(false)
        .into_iter()
        .flatten()
        .any(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("exe"))
        })
}

/// Builds the Proton env for tool `name`: trusted proton binary, the SLR
/// it requires, the app's compatdata and the Steam client root.
pub fn proton_env(
    name: &str,
    appid: u32,
    game_dir: &Path,
    steam: &SteamEnv,
) -> Result<ProtonEnv, LoaderError> {
    let tool = steam::find_compat_tool(&steam.roots, &steam.libraries, name)
        .ok_or_else(|| LoaderError::CompatToolMissing(name.to_string()))?;
    let proton = validate_trusted(&tool.join("proton"), steam)?;
    let manifest = tool.join("toolmanifest.vdf");
    let slr = match fs::read_to_string(&manifest)
        .ok()
        .and_then(|t| steam::parse_tool_manifest(&t, &manifest.display().to_string()).ok())
        .and_then(|m| m.require_tool_appid)
    {
        Some(id) => {
            let run = steam::find_runtime_entry(&steam.libraries, id)
                .ok_or(LoaderError::SlrMissing { appid: id })?;
            Some(validate_trusted(&run, steam)?)
        }
        None => None,
    };
    let own = steam
        .libraries
        .iter()
        .find(|l| game_dir.starts_with(l))
        .map(PathBuf::as_path);
    let compat_data = steam::find_prefix(&steam.libraries, appid, own)
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .or_else(|| own.map(|l| l.join(format!("steamapps/compatdata/{appid}"))))
        .or_else(|| {
            steam
                .libraries
                .first()
                .map(|l| l.join(format!("steamapps/compatdata/{appid}")))
        })
        .ok_or_else(|| LoaderError::UnknownRuntime("no Steam library found".into()))?;
    let client_root = steam
        .roots
        .first()
        .cloned()
        .ok_or_else(|| LoaderError::UnknownRuntime("no Steam root found".into()))?;
    Ok(ProtonEnv {
        proton,
        compat_data,
        client_root,
        app_id: appid,
        slr,
    })
}

/// Canonical `path` if it lies inside a tool folder of a Steam library
/// (`steamapps/common/<tool>/...`) or a root (`compatibilitytools.d/<tool>/...`),
/// and it and every folder down from the tool folder are owned by the user
/// or root and not writable by group or others.
pub fn validate_trusted(path: &Path, steam: &SteamEnv) -> Result<PathBuf, LoaderError> {
    let untrusted = |reason: String| LoaderError::ProtonUntrusted {
        path: path.to_path_buf(),
        reason,
    };
    let real = path
        .canonicalize()
        .map_err(|e| untrusted(format!("cannot resolve: {e}")))?;
    let mut bases = steam
        .libraries
        .iter()
        .map(|l| l.join("steamapps/common"))
        .chain(steam.roots.iter().map(|r| r.join("compatibilitytools.d")))
        .filter_map(|b| b.canonicalize().ok());
    // At least `<base>/<tool>/<file>`.
    let inside = bases.any(|b| {
        real.strip_prefix(&b)
            .is_ok_and(|r| r.components().count() >= 2)
    });
    if !inside {
        return Err(untrusted("not inside a Steam library tool folder".into()));
    }
    check_owner_chain(&real).map_err(untrusted)?;
    Ok(real)
}

/// Re-checks a path [`validate_trusted`] accepted, right before it is
/// executed: it must still be its own canonical path (no symlink swapped
/// in) and pass the ownership walk.
pub fn recheck_trusted(path: &Path) -> Result<(), LoaderError> {
    let untrusted = |reason: String| LoaderError::ProtonUntrusted {
        path: path.to_path_buf(),
        reason,
    };
    let real = path
        .canonicalize()
        .map_err(|e| untrusted(format!("cannot resolve: {e}")))?;
    if real != path {
        return Err(untrusted(format!("now resolves to {}", real.display())));
    }
    check_owner_chain(&real).map_err(untrusted)
}

/// `path` and every ancestor up to `/` are owned by the user or root and
/// not writable by group or others. A root-owned sticky folder (`/tmp`) is
/// allowed: others cannot rename or replace entries they do not own there.
fn check_owner_chain(path: &Path) -> Result<(), String> {
    const STICKY: u32 = 0o1000;
    let uid = rustix::process::geteuid().as_raw();
    for p in path.ancestors() {
        let m = fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
        if m.uid() != uid && m.uid() != 0 {
            return Err(format!("{} is owned by another user", p.display()));
        }
        let shared = m.mode() & 0o022 != 0;
        let sticky_root = m.is_dir() && m.uid() == 0 && m.mode() & STICKY != 0;
        if shared && !sticky_root {
            return Err(format!("{} is writable by group or others", p.display()));
        }
    }
    Ok(())
}

/// Inputs to [`preflight`].
#[derive(Debug, Clone, Copy)]
pub struct PreflightInput<'a> {
    pub game_dir: &'a Path,
    pub runtime: &'a Runtime,
    /// appmanifest `StateFlags`; `None` = not a Steam game / unknown.
    pub state_flags: Option<u32>,
    /// `/proc` (tests pass a fixture).
    pub proc_root: &'a Path,
    /// Filesystem root for the Flatpak check (`/`).
    pub fs_root: &'a Path,
}

/// Blocks an op that cannot run safely now. Returns warnings that should be
/// shown but do not block.
pub fn preflight(i: PreflightInput<'_>) -> Result<Vec<String>, LoaderError> {
    if let Some(pid) = running_in(i.proc_root, i.game_dir) {
        return Err(LoaderError::GameRunning { pid });
    }
    if let Some(flags) = i.state_flags {
        if !steam::state_flags::fully_installed(flags) {
            return Err(LoaderError::NotFullyInstalled { flags });
        }
    }
    let mut warnings = Vec::new();
    match i.runtime {
        Runtime::Native => {}
        Runtime::Unknown => {
            return Err(LoaderError::UnknownRuntime(
                "set the game's runtime to native or Proton".into(),
            ))
        }
        Runtime::Proton(env) => {
            if crate::autoupdate::in_flatpak(i.fs_root) {
                return Err(LoaderError::FlatpakUnsupported);
            }
            if steam::is_flatpak_steam(&env.client_root) || steam::is_flatpak_steam(&env.proton) {
                return Err(LoaderError::FlatpakSteamUnsupported);
            }
            let pfx = env.compat_data.join("pfx");
            if !pfx.is_dir() {
                return Err(LoaderError::PrefixMissing(pfx));
            }
            let recorded = fs::read_to_string(env.compat_data.join("config_info"))
                .ok()
                .and_then(|t| steam::config_info_proton_dir(&t));
            if let Some(dir) = recorded {
                let ours = env.proton.parent().and_then(|p| p.canonicalize().ok());
                if dir.canonicalize().ok() != ours {
                    warnings.push(format!(
                        "the prefix was last used by {}, not the mapped tool {}",
                        dir.display(),
                        env.proton.parent().unwrap_or(&env.proton).display()
                    ));
                }
            }
        }
    }
    Ok(warnings)
}

/// A process whose executable or working folder is inside `game_dir`.
fn running_in(proc_root: &Path, game_dir: &Path) -> Option<u32> {
    let game = game_dir
        .canonicalize()
        .unwrap_or_else(|_| game_dir.to_path_buf());
    let own = std::process::id();
    fs::read_dir(proc_root).ok()?.flatten().find_map(|e| {
        let pid: u32 = e.file_name().to_str()?.parse().ok()?;
        if pid == own {
            return None;
        }
        ["exe", "cwd"]
            .iter()
            .filter_map(|l| fs::read_link(e.path().join(l)).ok())
            .any(|t| t.starts_with(&game))
            .then_some(pid)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Fixture {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        game: PathBuf,
        steam: SteamEnv,
    }

    /// A Steam root that is also the library, a GE tool needing SLR sniper,
    /// and an empty game folder for app 100.
    fn fixture(mapping: &str) -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap().join("Steam");
        let game = root.join("steamapps/common/Game");
        fs::create_dir_all(&game).unwrap();
        let ge = root.join("compatibilitytools.d/GE-Proton10-34");
        fs::create_dir_all(&ge).unwrap();
        fs::write(ge.join("proton"), "#!/usr/bin/env python3\n").unwrap();
        fs::write(
            ge.join("toolmanifest.vdf"),
            r#""manifest" { "commandline" "/proton %verb%" "require_tool_appid" "1628350" }"#,
        )
        .unwrap();
        let slr = root.join("steamapps/common/SteamLinuxRuntime_sniper");
        fs::create_dir_all(&slr).unwrap();
        fs::write(slr.join("run"), "").unwrap();
        fs::write(
            root.join("steamapps/appmanifest_1628350.acf"),
            r#""AppState" { "appid" "1628350" "installdir" "SteamLinuxRuntime_sniper" }"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("steamapps/compatdata/100/pfx")).unwrap();
        // Independent of the test runner's umask.
        for e in walkdir::WalkDir::new(tmp.path()).into_iter().flatten() {
            fs::set_permissions(e.path(), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let text = format!(
            r#""InstallConfigStore" {{ "Software" {{ "Valve" {{ "Steam" {{ "CompatToolMapping" {{ {mapping} }} }} }} }} }}"#
        );
        let steam = SteamEnv {
            roots: vec![root.clone()],
            libraries: vec![root.clone()],
            mapping: steam::parse_compat_tool_mapping(&text, "config.vdf").unwrap(),
        };
        Fixture {
            _tmp: tmp,
            root,
            game,
            steam,
        }
    }

    const MAPPED: &str = r#""100" { "name" "GE-Proton10-34" }"#;
    const DEFAULT_ONLY: &str = r#""0" { "name" "GE-Proton10-34" }"#;

    fn facts<'a>(f: &'a Fixture, markers: &'a [String]) -> GameFacts<'a> {
        GameFacts {
            game_dir: &f.game,
            appid: Some(100),
            runtime_override: None,
            native_markers: markers,
        }
    }

    fn write_elf(path: &Path) {
        fs::write(path, b"\x7fELF\x02\x01\x01rest").unwrap();
    }

    /// Catches: the mapping ignored, the proton path not resolved from the
    /// mapped tool, the SLR wrapper or compatdata missing.
    #[test]
    fn mapping_gives_proton_with_resolved_paths() {
        let f = fixture(MAPPED);
        write_elf(&f.game.join("game.x86_64"));
        let Runtime::Proton(env) = detect(facts(&f, &[]), &f.steam).unwrap() else {
            panic!("mapped tool must win over the ELF");
        };
        assert_eq!(
            env.proton,
            f.root.join("compatibilitytools.d/GE-Proton10-34/proton")
        );
        assert_eq!(
            env.slr,
            Some(f.root.join("steamapps/common/SteamLinuxRuntime_sniper/run"))
        );
        assert_eq!(env.compat_data, f.root.join("steamapps/compatdata/100"));
        assert_eq!(
            (env.client_root.as_path(), env.app_id),
            (f.root.as_path(), 100)
        );
    }

    /// Catches: Stardew-style layouts (a launcher script plus an ELF under
    /// another name, Windows files alongside) detected as Proton.
    #[test]
    fn elf_without_mapping_is_native_even_with_exe() {
        let f = fixture(DEFAULT_ONLY);
        fs::write(
            f.game.join("StardewValley"),
            "#!/bin/bash\n./\"Stardew Valley\" $@\n",
        )
        .unwrap();
        write_elf(&f.game.join("Stardew Valley"));
        fs::write(f.game.join("Stardew Valley.exe"), "MZ").unwrap();
        assert_eq!(detect(facts(&f, &[]), &f.steam).unwrap(), Runtime::Native);
    }

    #[test]
    fn native_marker_wins_over_exe() {
        let f = fixture(DEFAULT_ONLY);
        fs::write(f.game.join("Game.exe"), "MZ").unwrap();
        fs::write(f.game.join("marker.sh"), "").unwrap();
        let markers = vec!["marker.sh".to_string()];
        assert_eq!(
            detect(facts(&f, &markers), &f.steam).unwrap(),
            Runtime::Native
        );
    }

    #[test]
    fn exe_only_is_proton_via_global_default() {
        let f = fixture(DEFAULT_ONLY);
        fs::create_dir_all(f.game.join("Game/Binaries/Win64")).unwrap();
        fs::write(f.game.join("Game/Binaries/Win64/Game.exe"), "MZ").unwrap();
        assert!(matches!(
            detect(facts(&f, &[]), &f.steam).unwrap(),
            Runtime::Proton(_)
        ));
    }

    /// Catches: the override not taking precedence over the mapping.
    #[test]
    fn override_wins() {
        let f = fixture(MAPPED);
        let mut g = facts(&f, &[]);
        g.runtime_override = Some("native");
        assert_eq!(detect(g, &f.steam).unwrap(), Runtime::Native);
        let f2 = fixture(DEFAULT_ONLY);
        write_elf(&f2.game.join("game"));
        let mut g = facts(&f2, &[]);
        g.runtime_override = Some("proton");
        assert!(matches!(detect(g, &f2.steam).unwrap(), Runtime::Proton(_)));
    }

    #[test]
    fn nothing_to_go_on_is_unknown() {
        let f = fixture(DEFAULT_ONLY);
        fs::write(f.game.join("readme.txt"), "").unwrap();
        assert_eq!(detect(facts(&f, &[]), &f.steam).unwrap(), Runtime::Unknown);
    }

    /// Catches: a proton binary that another user could replace, or one
    /// outside any Steam library, being executed.
    #[test]
    fn untrusted_proton_is_rejected() {
        let f = fixture(MAPPED);
        let proton = f.root.join("compatibilitytools.d/GE-Proton10-34/proton");
        fs::set_permissions(&proton, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(matches!(
            detect(facts(&f, &[]), &f.steam),
            Err(LoaderError::ProtonUntrusted { .. })
        ));
        fs::set_permissions(&proton, fs::Permissions::from_mode(0o755)).unwrap();
        let dir = proton.parent().unwrap();
        fs::set_permissions(dir, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(
            validate_trusted(&proton, &f.steam).is_err(),
            "writable tool folder"
        );
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(validate_trusted(&proton, &f.steam).is_ok());
        // F5: a writable folder above the library is as bad as one inside.
        fs::set_permissions(&f.root, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(
            validate_trusted(&proton, &f.steam).is_err(),
            "writable Steam root"
        );
        assert!(recheck_trusted(&proton).is_err());
        fs::set_permissions(&f.root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(recheck_trusted(&proton).is_ok());
        // A path that turned into a symlink after validation fails the recheck.
        let moved = dir.join("proton.real");
        fs::rename(&proton, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &proton).unwrap();
        assert!(recheck_trusted(&proton).is_err(), "symlink swapped in");
        fs::remove_file(&proton).unwrap();
        fs::rename(&moved, &proton).unwrap();

        let outside = f.root.parent().unwrap().join("proton");
        fs::write(&outside, "").unwrap();
        assert!(validate_trusted(&outside, &f.steam).is_err());
        // A symlink from inside the library to outside resolves outside.
        std::os::unix::fs::symlink(&outside, f.root.join("steamapps/common/Game/proton")).unwrap();
        assert!(validate_trusted(&f.game.join("proton"), &f.steam).is_err());
    }

    #[test]
    fn missing_slr_is_a_typed_error() {
        let f = fixture(MAPPED);
        fs::remove_file(f.root.join("steamapps/common/SteamLinuxRuntime_sniper/run")).unwrap();
        assert!(matches!(
            detect(facts(&f, &[]), &f.steam),
            Err(LoaderError::SlrMissing { appid: 1628350 })
        ));
    }

    fn proton_of(f: &Fixture) -> Runtime {
        detect(facts(f, &[]), &f.steam).unwrap()
    }

    fn input<'a>(
        f: &'a Fixture,
        rt: &'a Runtime,
        proc_root: &'a Path,
        fs_root: &'a Path,
    ) -> PreflightInput<'a> {
        PreflightInput {
            game_dir: &f.game,
            runtime: rt,
            state_flags: Some(4),
            proc_root,
            fs_root,
        }
    }

    /// Catches each Proton preflight gate failing open.
    #[test]
    fn proton_preflight_gates() {
        let f = fixture(MAPPED);
        let rt = proton_of(&f);
        let empty = tempfile::tempdir().unwrap();
        let (proc_root, fs_root) = (empty.path().join("proc"), empty.path().join("root"));
        fs::create_dir_all(&proc_root).unwrap();
        fs::create_dir_all(&fs_root).unwrap();
        assert_eq!(
            preflight(input(&f, &rt, &proc_root, &fs_root)).unwrap(),
            Vec::<String>::new()
        );

        let mut i = input(&f, &rt, &proc_root, &fs_root);
        i.state_flags = Some(6);
        assert!(matches!(
            preflight(i),
            Err(LoaderError::NotFullyInstalled { flags: 6 })
        ));

        // config_info from another tool only warns.
        fs::write(
            f.root.join("steamapps/compatdata/100/config_info"),
            "Proton 9\n/other/files/share/fonts/\n",
        )
        .unwrap();
        assert_eq!(
            preflight(input(&f, &rt, &proc_root, &fs_root))
                .unwrap()
                .len(),
            1
        );

        // Game running: a process whose cwd is in the game folder.
        let p = proc_root.join("4321");
        fs::create_dir_all(&p).unwrap();
        std::os::unix::fs::symlink(f.game.join("sub"), p.join("cwd")).unwrap();
        assert!(matches!(
            preflight(input(&f, &rt, &proc_root, &fs_root)),
            Err(LoaderError::GameRunning { pid: 4321 })
        ));
        fs::remove_dir_all(&p).unwrap();

        fs::write(fs_root.join(".flatpak-info"), "").unwrap();
        assert!(matches!(
            preflight(input(&f, &rt, &proc_root, &fs_root)),
            Err(LoaderError::FlatpakUnsupported)
        ));
        fs::remove_file(fs_root.join(".flatpak-info")).unwrap();

        fs::remove_dir_all(f.root.join("steamapps/compatdata/100/pfx")).unwrap();
        assert!(matches!(
            preflight(input(&f, &rt, &proc_root, &fs_root)),
            Err(LoaderError::PrefixMissing(_))
        ));

        let Runtime::Proton(mut env) = rt.clone() else {
            unreachable!()
        };
        env.client_root = "/home/u/.var/app/com.valvesoftware.Steam/.local/share/Steam".into();
        let flat = Runtime::Proton(env);
        assert!(matches!(
            preflight(input(&f, &flat, &proc_root, &fs_root)),
            Err(LoaderError::FlatpakSteamUnsupported)
        ));
    }
}
