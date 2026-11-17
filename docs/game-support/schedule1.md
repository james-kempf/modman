# Schedule I (`schedule1`): game support

## Summary
Re-run after Vessel gained GitHub release sources, loader aliases and the installer-only redirect (plan 20261115-loader-sources). The definition keeps the Steam appid, the game-root deploy root, the `melonloader` profile and the MelonLoader `[[loaders]]` entry, and now declares every route mods use to reach MelonLoader: `nexus_mod_id = 1196`, `[loaders.github]` (`LavaGang/MelonLoader` `MelonLoader.x64.zip`) and `[loaders.aliases]`. Every route in the Loader routes table maps to a definition field, so `support` goes back to `tested`.

Facts re-checked with the helpers on 2026-11-15:
- `inspect_archive.py github LavaGang/MelonLoader --asset MelonLoader.x64.zip`: v0.7.3, 175 entries, top level `MelonLoader/` and `version.dll`, no installer [9].
- `nexus_mod_info.py contents schedule1 1196`: one file (3486) holding only `MelonLoader.Installer.exe` [11].
- `nexus_mod_info.py requirements schedule1 1196`: required by 341 mods on Nexus [11].
- `nexus_mod_info.py top schedule1 10`: 8 of the 10 most-endorsed mods declare MelonLoader off-site, none on Nexus. Seven link `melonwiki.xyz` (Mod Manager - Phone App 397, TrashGrabber PLUS 210, Better Employees 140, Better Stacks 208, OG Backpack 818, ExpandRecipe 453, Warehouse Always Open 189); Minimap - Full Version (113) links `https://github.com/LavaGang/MelonLoader/releases/download/v0.6.6/MelonLoader.Installer.exe`. Mod Manager (58) and Quality Bonus (314) declare no requirements [12].
- No top-10 mod points to Thunderstore, so Thunderstore is not listed as a route.

What changed versus the prior definition and note:
- `support` changed from `"experimental"` to `"tested"` (Gaps `None`).
- Added `nexus_mod_id = 1196`, `[loaders.github]` and `[loaders.aliases]`; comments in `schedule1.toml` rewritten to the verified facts.
- `schedule1` is back in the tested-domains list in `every_other_preset_is_experimental`; `schedule1_declares_the_melonloader_loader` pins `Support::Tested`, the Nexus id, the GitHub source, the aliases and the installer redirect; a requirements test resolves the observed off-site URLs to `install_loader` `melonloader`.

## How mods are installed
- **Runtime:** Steam lists Windows only [1]; GamingOnLinux lists it as Proton/Wine [2]. MelonLoader's README says its Linux build has compatibility issues and Proton works much better [4][5].
- **Deploy root:** the game folder (holds `Schedule I.exe`). MelonLoader creates `Mods/`, `Plugins/`, `UserData/` and `MelonLoader/` there [3][6]. S1MAPI additionally ships `UserLibs/` [8]. The `melonloader` profile treats those folders as game-root folders and moves loose `.dll` files into `Mods/` (`staging.rs`).
- **Load order:** none. MelonLoader loads every mod in `Mods/`; no plugin list file, so no `[plugins]` and no shared file.
- **Mono vs IL2CPP:** the public Steam branch is IL2CPP; the `alternate` beta branch is Mono. Many mods ship one DLL per branch [7][8]. Vessel cannot choose between them (Open questions).
- **Proton:** the proxy is `version.dll`; the installer's Linux launch option is `WINEDLLOVERRIDES="version=n,b" %command%` [4][5]. The entry sets `dll_overrides = ["version"]`.

## Loaders and script extenders
| Name | Nexus id | Installs to | Detect | Build-coupled | Launch change | Source |
|---|---|---|---|---|---|---|
| MelonLoader | `schedule1` 1196 (installer only; install redirected to the GitHub zip) | game root: `version.dll`, `MelonLoader/` | `MelonLoader`, `dobby.dll` | no | `dll_overrides = ["version"]` | [3][4][5][9][10] |

The GitHub asset `MelonLoader.x64.zip` (v0.7.3) has `MelonLoader/` (with `net6`, `net35`, `net472` and `Dependencies/`) and `version.dll` at its top level [9]; the Thunderstore page and the GitHub README also document `dobby.dll` [3][4], which `detect` keeps as extra evidence of a manual install. The release also ships `MelonLoader.Installer.exe`, Linux/macOS installers and zips, and `MelonLoader.x86.zip` [9]. The Windows x64 zip applies under Proton. Mods pin different versions (0.6.6, 0.7.0 seen) [12].

## Loader routes
| Loader | Route | Identifier | Handled by |
|---|---|---|---|
| melonloader | Nexus page (installer exe) | schedule1 1196 (file 3486, only `MelonLoader.Installer.exe`) | `loaders.nexus_mod_id` + `loaders.github` |
| melonloader | Nexus requirement | schedule1 1196 (required by 341 mods) | `loaders.nexus_mod_id` + `loaders.github` |
| melonloader | GitHub release | LavaGang/MelonLoader `MelonLoader.x64.zip` | `loaders.github` |
| melonloader | Off-site requirement | melonwiki.xyz URLs (`https://melonwiki.xyz/`, `/#`, `/#/?id=requirements`, ...) and github.com/LavaGang/MelonLoader URLs (mod 113 links the v0.6.6 `MelonLoader.Installer.exe` asset) | `loaders.aliases` + `loaders.github` |

## Configuration changes
- `support = "tested"` (was `experimental`): every loader route maps to a definition field; Gaps is `None`.
- `nexus_mod_id = 1196`: the MelonLoader page on `schedule1` [11]. A Nexus requirement on 1196 resolves to the loader; a download of its installer-only file is redirected to the GitHub source and the installer is never run or deployed.
- `[loaders.github]` `repo = "LavaGang/MelonLoader"`, `asset = "MelonLoader.x64.zip"`: the asset listing matches `required_paths` and the glob names exactly one asset [9].
- `[loaders.aliases]` `urls = ["https://melonwiki.xyz/", "https://github.com/LavaGang/MelonLoader"]`, `names = ["MelonLoader", "Melon Loader"]`: the off-site URLs and names seen in the top-10 mods' requirements [12].
- Kept `required_paths = ["version.dll", "MelonLoader"]` [9]; `forbidden_paths = ["Mods"]` so a mod pack bundling MelonLoader is not taken for the loader; `detect = ["MelonLoader", "dobby.dll"]` [3][4][9]; `launch.dll_overrides = ["version"]` [4][5]. No `couples_to`: the loader is tied to the Mono/IL2CPP branch, not one executable.
- Unchanged and confirmed: `steam_appid = 3164500` (Steam store page names it Schedule I [1]), `subdir = "."`, `profile = "melonloader"`, `target = "game_dir"`.

## Gaps
None

## Open questions
- Version pins: mods ask for MelonLoader 0.6.6 or 0.7.0, while the GitHub source installs the latest release (v0.7.3). The definition cannot pin a loader version.
- Thunderstore (`schedule-i/LavaGang/MelonLoader` [3]) also distributes MelonLoader, but no top-10 Nexus mod points to it; an archive downloaded there and added by hand installs only if it matches `required_paths`.
- GOG/Epic ids: none found.
- Choosing the Mono or IL2CPP build of a mod is not expressible in the definition.
- Whether the Proton prefix needs extra runtimes (.NET 6 Desktop Runtime for IL2CPP is installed automatically only on Windows [3]).
- Hardware verification is not done.

## Check results
Run on 2026-11-16.
- `python3 scripts/check_game_support_note.py --definition crates/vessel-core/definitions/schedule1.toml --note docs/game-support/schedule1.md`: pass (full outcome, Gaps `None`, matches `support = "tested"`)
- `cargo test -p vessel-core --lib -- games::definitions:: requirements:: loaders::`: pass (174 passed, 1 ignored)
- `cargo test --workspace --no-fail-fast`: pass (815 passed, 0 failed)
- `cargo clippy --workspace --all-targets -- -D warnings`: pass
- `cargo fmt --all -- --check`: pass
- `python3 -B -m unittest discover -s scripts -p 'test_*.py'`: pass (63 tests)

## Manual test
Install a mod, deploy, launch, uninstall (plan section 4.4). Not done on hardware.
Promotion to tested here is based on research, not hardware testing.

## Sources
1. https://store.steampowered.com/app/3164500/Schedule_I/ (2026-11-15)
2. https://www.gamingonlinux.com/itemdb/app/41656/schedule-i (2026-11-15)
3. https://thunderstore.io/c/schedule-i/p/LavaGang/MelonLoader/ (2026-11-15)
4. https://github.com/LavaGang/MelonLoader (2026-11-15)
5. https://github.com/TLD-Mods/Linux_Proton_Guide (2026-11-15)
6. https://xgamingserver.com/docs/schedule-1/melonloader (2026-11-15)
7. https://www.nexusmods.com/schedule1/mods/556 (via search summary, 2026-11-15)
8. https://thunderstore.io/c/schedule-i/p/ifBars/S1MAPI/ (2026-11-15)
9. https://api.github.com/repos/LavaGang/MelonLoader/releases/latest and `scripts/inspect_archive.py github LavaGang/MelonLoader --asset MelonLoader.x64.zip` (v0.7.3) (2026-11-15)
10. Search results listing https://www.nexusmods.com/schedule1/mods/1196, /2798, /1567, /2746 (2026-11-15)
11. https://www.nexusmods.com/schedule1/mods/1196 via `scripts/nexus_mod_info.py contents schedule1 1196` and `requirements schedule1 1196` (2026-11-15)
12. `scripts/nexus_mod_info.py top schedule1 10` (Nexus API, mods 397, 113, 210, 58, 140, 208, 818, 314, 453, 189) (2026-11-15)
