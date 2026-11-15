# Schedule I (`schedule1`): game support

## Summary
Re-run of a previously completed game. The definition keeps the Steam appid, the game-root deploy root, the `melonloader` profile and the MelonLoader `[[loaders]]` entry, all re-checked against sources. Under the current "Loader routes" rules the game can no longer be called fully supported: popular mods name MelonLoader as an off-site requirement, MelonLoader is also distributed through GitHub releases, an installer and Thunderstore, and its Nexus page on `schedule1` (mod 1196) could not be read. Vessel cannot handle those routes with config alone, so `support` drops from `tested` to `experimental`.

What changed versus the prior definition and note:
- `support` changed from `"tested"` to `"experimental"` (new Gaps).
- `schedule1` removed from the tested-domains list in `every_other_preset_is_experimental`; `schedule1_declares_the_melonloader_loader` now pins `Support::Experimental` and no `nexus_mod_id`.
- Comment added to `schedule1.toml` explaining the experimental level.
- Note rewritten to the current template: new Loader routes table and Gaps bullets.
- No change to appid, subdir, profile, target or loader fields.

## How mods are installed
- **Runtime:** Steam lists Windows only [1]; GamingOnLinux lists it as Proton/Wine [2]. MelonLoader's README says its Linux build has compatibility issues and Proton works much better [4][5].
- **Deploy root:** the game folder (holds `Schedule I.exe`). MelonLoader creates `Mods/`, `Plugins/`, `UserData/` and `MelonLoader/` there [3][6]. S1MAPI additionally ships `UserLibs/` [8]. The `melonloader` profile treats those folders as game-root folders and moves loose `.dll` files into `Mods/` (`staging.rs`).
- **Load order:** none. MelonLoader loads every mod in `Mods/`; no plugin list file, so no `[plugins]` and no shared file.
- **Mono vs IL2CPP:** the public Steam branch is IL2CPP; the `alternate` beta branch is Mono. Many mods ship one DLL per branch [7][8]. Vessel cannot choose between them (Open questions).
- **Proton:** the proxy is `version.dll`; the installer's Linux launch option is `WINEDLLOVERRIDES="version=n,b" %command%` [4][5]. The entry sets `dll_overrides = ["version"]`.

## Loaders and script extenders
| Name | Nexus id | Installs to | Detect | Build-coupled | Launch change | Source |
|---|---|---|---|---|---|---|
| MelonLoader | `schedule1` 1196 (page contents unverified, not set in definition) | game root: `version.dll`, `dobby.dll`, `MelonLoader/` | `MelonLoader`, `dobby.dll` | no | `dll_overrides = ["version"]` | [3][4][5][9][10] |

The manual-install archive layout (`MelonLoader/` folder plus `version.dll` and `dobby.dll`) is documented on Thunderstore and GitHub [3][4]. Latest release v0.7.3 (2026-05-14) ships `MelonLoader.Installer.exe`, `MelonLoader.Installer.Linux`, `MelonLoader.Installer.MacOS.dmg`, `MelonLoader.x64.zip`, `MelonLoader.x86.zip`, `MelonLoader.Linux.x64.zip` and `MelonLoader.macOS.x64.zip` [9]. The Windows x64 zip applies under Proton. Mods pin different versions (0.7.0, 0.7.2, 0.7.3 seen) [10].

## Loader routes
| Loader | Route | Identifier | Handled by |
|---|---|---|---|
| melonloader | Nexus page | schedule1 1196 (contents unverified, HTTP 403) | Gap |
| melonloader | Off-site requirement | GitHub LavaGang/MelonLoader, named by Schedule I Minimap (schedule1 2798), ModHub v2.0 (1567), MelonLoader FastGen (2746) | Gap |
| melonloader | GitHub release | LavaGang/MelonLoader `MelonLoader.x64.zip` | Gap |
| melonloader | GitHub release | LavaGang/MelonLoader `MelonLoader.Installer.exe` (installer) | Gap |
| melonloader | Thunderstore | schedule-i/LavaGang/MelonLoader | Gap |

## Configuration changes
- `support = "experimental"` (was `tested`): Gaps lists Rust changes.
- Kept `[[loaders]]` `melonloader`: `required_paths = ["version.dll", "MelonLoader"]` [3][4]; `forbidden_paths = ["Mods"]` so a mod pack bundling MelonLoader is not taken for the loader; `detect = ["MelonLoader", "dobby.dll"]` [3][4]; `launch.dll_overrides = ["version"]` [4][5].
- No `nexus_mod_id`: the page on `schedule1` (1196) exists per search results [10], but Nexus returned HTTP 403, so what it ships (zip or installer) is unverified. No `couples_to`: the loader is tied to the Mono/IL2CPP branch, not one executable.
- Unchanged and confirmed: `steam_appid = 3164500` (Steam store page names it Schedule I [1]), `subdir = "."`, `profile = "melonloader"`, `target = "game_dir"`.
- Test `schedule1_declares_the_melonloader_loader` updated; `every_other_preset_is_experimental` no longer lists `schedule1`.

## Gaps
- melonloader: `crates/vessel-core/src/loaders` and `requirements.rs`: unverified route: melonloader Nexus page schedule1 1196; its archive contents could not be read (HTTP 403), so `nexus_mod_id` is unset; once confirmed to ship a zip matching `required_paths`, set `loaders.nexus_mod_id = 1196`.
- melonloader: `requirements.rs` and the install flow: mods name MelonLoader as an off-site requirement (GitHub); Vessel recognizes the name but cannot download from a URL, so it never offers the install.
- melonloader: GitHub release `LavaGang/MelonLoader` (`MelonLoader.x64.zip`) and the `MelonLoader.Installer.exe` installer: a new download source that can fetch a GitHub release asset and claim it as the loader is needed; declarative loaders never run installers.
- melonloader: Thunderstore package `schedule-i/LavaGang/MelonLoader`: a new Thunderstore download source is needed so the archive is registered as the loader rather than a regular mod.

## Open questions
- Contents and requirements-entry style of the Nexus page `schedule1` 1196 and of popular mods' requirement lists: all Nexus pages returned HTTP 403; the off-site style comes from search summaries only.
- GOG/Epic ids: none found.
- Choosing the Mono or IL2CPP build of a mod is not expressible in the definition.
- Whether the Proton prefix needs extra runtimes (.NET 6 Desktop Runtime for IL2CPP is installed automatically only on Windows [3]).
- Hardware verification is not done.

## Check results
Run on 2026-11-15.
- `python3 scripts/check_game_support_note.py --definition crates/vessel-core/definitions/schedule1.toml --note docs/game-support/schedule1.md`: pass (partial outcome, 4 Gaps bullets, matches `support = "experimental"`)
- `cargo fmt --all -- --check`: pass (after `cargo fmt --all` reformatted the test list)
- `cargo clippy -p vessel-core --all-targets --locked -- -D warnings`: pass
- `cargo test -p vessel-core --locked`: pass (lib 511 passed, 2 ignored; integration tests all passed; 0 failed)

## Manual test
Install a mod, deploy, launch, uninstall (plan section 4.4). Promotion to tested here is based on research, not hardware testing.

## Sources
1. https://store.steampowered.com/app/3164500/Schedule_I/ (2026-11-15)
2. https://www.gamingonlinux.com/itemdb/app/41656/schedule-i (2026-11-15)
3. https://thunderstore.io/c/schedule-i/p/LavaGang/MelonLoader/ (2026-11-15)
4. https://github.com/LavaGang/MelonLoader (2026-11-15)
5. https://github.com/TLD-Mods/Linux_Proton_Guide (2026-11-15)
6. https://xgamingserver.com/docs/schedule-1/melonloader (2026-11-15)
7. https://www.nexusmods.com/schedule1/mods/556 (via search summary, 2026-11-15)
8. https://thunderstore.io/c/schedule-i/p/ifBars/S1MAPI/ (2026-11-15)
9. https://api.github.com/repos/LavaGang/MelonLoader/releases/latest (2026-11-15)
10. Search results listing https://www.nexusmods.com/schedule1/mods/1196, /2798, /1567, /2746 (2026-11-15)
