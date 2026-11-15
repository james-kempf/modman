# Schedule I (`schedule1`): game support

## Summary
`schedule1.toml` already had the Steam appid, the game-root deploy root and the `melonloader` layout profile. This change adds a declarative `[[loaders]]` entry for MelonLoader (Windows x64 zip into the game root, `version` DLL override under Proton), marks the game `tested` (research-based), adds the completion header and a test pinning the config. No Rust change is needed.

## How mods are installed
- **Runtime:** GamingOnLinux lists the game as Proton/Wine only [5]; no native Linux build was found. MelonLoader's own README says its Linux build has compatibility issues and that Proton works much better [3][4].
- **Deploy root:** the game folder (the one holding `Schedule I.exe`) [6]. MelonLoader creates `Mods/`, `Plugins/`, `UserData/` and `MelonLoader/` there [2]. S1MAPI additionally ships `UserLibs/` [7]. The `melonloader` profile already treats `Mods`, `Plugins`, `UserData`, `UserLibs` and `MelonLoader` as game-root folders and moves loose `.dll` files into `Mods/` (`staging.rs`, `melonloader`).
- **Load order:** none. MelonLoader loads every mod in `Mods/`; there is no plugin list file, so no `[plugins]` and no shared file.
- **Mono vs IL2CPP:** the current public Steam branch is IL2CPP; the Steam beta branch `alternate` is Mono. Many mods publish one DLL per branch and S1MAPI ships both in `UserLibs/` [7][6]. Vessel cannot choose between them (see Open questions).
- **Proton:** the proxy DLL is `version.dll` by default; the MelonLoader installer's Linux launch option is `WINEDLLOVERRIDES="version=n,b" %command%` [3][4]. The loader entry sets `dll_overrides = ["version"]`, which Vessel composes into the Steam launch options.

## Loaders and script extenders
| Name | Nexus id | Installs to | Detect | Build-coupled | Launch change | Source |
|---|---|---|---|---|---|---|
| MelonLoader | not verified (Nexus returned HTTP 403) | game root: `version.dll`, `dobby.dll`, `MelonLoader/` | `MelonLoader`, `dobby.dll` | no | `dll_overrides = ["version"]` | [2][3][4][8] |

The archive layout (`MelonLoader/` folder plus `version.dll` and `dobby.dll` at the root) is the manual install described in the README [2]. The latest release at the time of reading was v0.7.3 (2026-05-14), which ships `MelonLoader.x64.zip` for Windows and a separate `MelonLoader.Linux.x64.zip` for native Linux [8]; the Windows zip is the one that applies under Proton. S1MAPI requires MelonLoader 0.7.0 or newer [7].

## Configuration changes
- `support = "tested"` (was `experimental`): Gaps is `None`; promotion is research-based per the task.
- Added `# vessel-game-support: complete` and `# Sources:` header lines.
- Added `[[loaders]]` `melonloader` (key is a known tool in `requirements.rs`): `required_paths = ["version.dll", "MelonLoader"]` [2]; `forbidden_paths = ["Mods"]` so a mod pack bundling MelonLoader is not taken for the loader; `detect = ["MelonLoader", "dobby.dll"]` [2]; `launch.dll_overrides = ["version"]` [3][4].
- No `nexus_mod_id` and no `couples_to`: the Nexus id could not be verified, and nothing found ties the loader zip to a game executable (it is tied to the Mono/IL2CPP branch, which is not a single file).
- Unchanged and confirmed: `steam_appid = 3164500` (from the existing preset; not independently re-verified, see Open questions), `subdir = "."`, `profile = "melonloader"`, `target = "game_dir"`.
- Test `schedule1_declares_the_melonloader_loader`; `every_other_preset_is_experimental` now also lists `schedule1`.

## Gaps
None

## Open questions
- Nexus mod id of MelonLoader on `nexusmods.com/schedule1`: Nexus pages returned HTTP 403, so it is left out. Without it the loader is installed as "unverified" provenance when downloaded from Nexus.
- Steam appid 3164500 and GOG/Epic ids: Steam appid is from the existing definition; searches did not confirm it directly. No GOG/Epic ids were found.
- Choosing the Mono or IL2CPP build of a mod (per Steam branch) is not expressible in the definition; users pick the right file on the mod page.
- Whether the Proton prefix needs extra runtimes beyond what Proton ships, and whether the installer's launch option differs for IL2CPP games: not verified for this game.
- Hardware verification is not done.

## Check results
Run on 2026-11-15.
- `python3 scripts/check_game_support_note.py --definition crates/vessel-core/definitions/schedule1.toml --note docs/game-support/schedule1.md`: pass (full support, gaps: none)
- `cargo fmt --all -- --check`: pass
- `cargo clippy -p vessel-core --all-targets --locked -- -D warnings`: pass
- `cargo test -p vessel-core --locked`: pass (lib 511 passed, 2 ignored; integration tests all passed; 0 failed)

## Manual test
Install a mod, deploy, launch, uninstall (plan section 4.4). Promotion to tested here is based on research, not hardware testing.

## Sources
1. https://www.gamingonlinux.com/itemdb/app/41656/schedule-i (search result, read 2026-11-15)
2. https://github.com/LavaGang/MelonLoader (README, 2026-11-15)
3. https://github.com/TLD-Mods/Linux_Proton_Guide (MelonLoader on Linux/Proton guide, 2026-11-15)
4. https://thunderstore.io/c/cairn/p/LavaGang/MelonLoader/ and the guide above, via search summary (2026-11-15)
5. https://www.gamingonlinux.com/itemdb/app/41656 (search result, 2026-11-15)
6. https://xgamingserver.com/docs/schedule-1/melonloader (2026-11-15)
7. https://thunderstore.io/c/schedule-i/p/ifBars/S1MAPI/ (2026-11-15)
8. https://api.github.com/repos/LavaGang/MelonLoader/releases/latest (2026-11-15)
