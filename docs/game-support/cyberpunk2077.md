# Cyberpunk 2077 (`cyberpunk2077`): game support

## Summary
`cyberpunk2077.toml` already had the right Steam appid, deploy root (`.`) and the `cyberpunk` layout profile. This change adds three `[[loaders]]` entries (RED4ext, Cyber Engine Tweaks, redscript) with their Nexus ids, detection files, Proton DLL overrides and game-build coupling, and promotes the game from `experimental` to `tested` because no Rust change is needed. Promotion is based on research, not hardware testing.

## How mods are installed
- **Runtime:** Proton only. The community guide assumes Proton/Wine and describes no native Linux build [3].
- **Deploy root:** the game folder (`subdir = "."`). Mod archives carry their own paths from the game root [2][4].
- **Folders by mod type** (already encoded by the `cyberpunk` staging profile):
  - `archive/pc/mod/*.archive` and `.xl`: archive mods [4]
  - `r6/scripts/`: redscript `.reds` [2]
  - `r6/tweaks/`: TweakXL `.yaml`/`.tweak` [4]
  - `bin/x64/plugins/cyber_engine_tweaks/mods/`: CET Lua mods [2]
  - `red4ext/plugins/<Name>/`: RED4ext plugins, including ArchiveXL, TweakXL and Codeware [2]
  - `mods/`: REDmod, used only by REDmod-format mods [4]
- **Load order:** there is no plugin list. `.archive` files load first from `archive/pc/mod` in ASCII order of file name, then REDmod folders from `mods/` in folder-name order. TweakXL files load in lexicographic order. An optional `archive/pc/mod/modlist.txt` overrides the archive order; the wiki advises renaming files instead [4]. So there is no `[plugins]` block (it is only valid for `bethesda`).
- **Proton:** both script-extender proxies need a native-first override. The Linux guide sets `WINEDLLOVERRIDES="winmm,version=n,b" %command%` [3]. It also needs `vcrun2022` and `d3dcompiler_47` installed in the prefix with protontricks; without the real VC++ 2022 runtime RED4ext fails with "error 998" and CET with "Unable to load cyber_engine_tweaks.asi" [3]. Vessel cannot install those runtimes (see Open questions).

## Loaders and script extenders
| Name | Nexus id | Installs to | Detect | Build-coupled | Launch change | Source |
|---|---|---|---|---|---|---|
| RED4ext | 2380 | `red4ext/` (plugins in `red4ext/plugins/`, log in `red4ext/logs/red4ext.log`) | `red4ext/RED4ext.dll` | yes, `bin/x64/Cyberpunk2077.exe`: its log reports a version incompatibility when it is out of date | `winmm` DLL override | [1][3][5][6][8] |
| Cyber Engine Tweaks | 107 | `bin/x64/` (`version.dll`, `global.ini`, `plugins/cyber_engine_tweaks.asi`) | `bin/x64/plugins/cyber_engine_tweaks.asi` | yes, `bin/x64/Cyberpunk2077.exe`: releases track the game's patches; requires RED4ext | `version` DLL override | [1][2][7][9] |
| redscript | 1511 | `r6/scripts/` for `.reds` mods; compiled output in `r6/cache/` | none declared (path unverified) | not declared | none | [2] |

ArchiveXL (4198), TweakXL (4197) and Codeware (7780) all need RED4ext and install under `red4ext/plugins/<Name>/` [2][10][11][12]. They need no launch change, and the `cyberpunk` profile already stages them as ordinary structured mods (`red4ext` is a recognised top folder), so they are not declared as loaders (see Open questions).

## Configuration changes
- `support = "tested"` (was `experimental`): Gaps is `None`; promotion is research-based per the task.
- Added the header lines `# vessel-game-support: complete` and `# Sources: docs/game-support/cyberpunk2077.md`.
- `[[loaders]] red4ext`: `nexus_mod_id = 2380` [2]; `required_paths = ["**/RED4ext.dll"]` and `detect = ["red4ext/RED4ext.dll"]`, from the `red4ext.dll` file name in Vessel's tool table (`requirements.rs`) and the `red4ext/` folder in [2][5]; `couples_to = "bin/x64/Cyberpunk2077.exe"` (game executable folder from [13]); `dll_overrides = ["winmm"]` from [3].
- `[[loaders]] cet`: `nexus_mod_id = 107` [2]; `required_paths = ["**/cyber_engine_tweaks.asi"]` and `detect = ["bin/x64/plugins/cyber_engine_tweaks.asi"]` from [7]; `couples_to` as above; `dll_overrides = ["version"]` from [3].
- `[[loaders]] redscript`: `nexus_mod_id = 1511` [2]; `required_paths = ["**/scc.exe"]` from Vessel's tool table.
- Test `cyberpunk_declares_red4ext_cet_and_redscript_loaders` in `games/definitions.rs`; `every_other_preset_is_experimental` now lists `skyrimspecialedition` and `cyberpunk2077` as tested.

## Gaps
None

## Open questions
- GOG and Epic ids: not found in a primary source, so left out.
- Release archive layouts of RED4ext, redscript, ArchiveXL, TweakXL and Codeware were not readable from the sources reached (README pages and docs give only "extract into the game folder"). The `required_paths` globs rely on file names only (`RED4ext.dll`, `scc.exe`), which come from Vessel's own tool table, not from a fresh primary source. Check them against the real zips.
- redscript's installed `scc.exe` path and whether it is tied to the game build: unverified, so no `detect` or `couples_to`.
- RED4ext's proxy DLL location (`bin/x64/winmm.dll`) is not confirmed by a fetched source; only the `winmm` override is (from [3]).
- Whether Vessel should declare ArchiveXL, TweakXL and Codeware as loaders: `[[loaders]]` keys must be known tool keys in `requirements.rs` (13 today). These three are not, so declaring them would need a Rust change. The game works without it, so this is a nice-to-have, not a gap.
- `vcrun2022` and `d3dcompiler_47` have to be installed in the Proton prefix by hand (protontricks) [3]; Vessel has no prefix-setup step.
- Exact version coupling ranges of RED4ext and CET per game patch: not documented in the pages read (RED4ext latest seen v1.31.1, CET v1.38.1 [8][9]).

## Check results
- `cargo fmt --all -- --check`: pass
- `check_game_support_note.py`: pass (full support, Gaps `None`)
- `cargo clippy -p vessel-core --all-targets --locked -- -D warnings`: **not run**, the shell denied it
- `cargo test -p vessel-core --locked games::`: **not run**, the shell denied it
- `cargo test -p vessel-core --locked loaders::`: **not run**, the shell denied it

The new definition and test code have not been compiled or executed. Run the last three commands before merging.

## Manual test
Install a mod, deploy, launch, uninstall (plan section 4.4). Promotion to tested here is based on research, not hardware testing. Suggested path: install RED4ext and CET through the loader flow, set the launch options Vessel shows (`WINEDLLOVERRIDES="winmm,version=n,b"`), start the game and check `red4ext/logs/red4ext.log` and `bin/x64/plugins/cyber_engine_tweaks/cyber_engine_tweaks.log`; then install an `.archive` mod, deploy, launch, uninstall.

## Sources
Read 2026-11-15.
1. https://github.com/WopsS/RED4ext (README: install docs live on docs.red4ext.com)
2. https://wiki.redmodding.org/cyberpunk-2077-modding/for-mod-creators-theory/core-mods-explained.md
3. https://wiki.redmodding.org/cyberpunk-2077-modding/for-mod-users/users-modding-cyberpunk-2077/the-cyberpunk-2077-game-directory/modding-on-linux.md
4. https://wiki.redmodding.org/cyberpunk-2077-modding/for-mod-users/users-modding-cyberpunk-2077/load-order.md
5. https://docs.red4ext.com/getting-started/installing-red4ext.md
6. https://github.com/WopsS/RED4ext/releases/latest (v1.31.1)
7. https://wiki.redmodding.org/cyber-engine-tweaks/getting-started/installing
8. https://wiki.redmodding.org/cyberpunk-2077-modding/for-mod-users/user-guide-troubleshooting/requirements-explained.md
9. https://github.com/maximegmd/CyberEngineTweaks/releases/latest (v1.38.1)
10. https://github.com/psiberx/cp2077-archive-xl
11. https://github.com/psiberx/cp2077-tweak-xl
12. https://github.com/psiberx/cp2077-codeware
13. https://wiki.redmodding.org/cyberpunk-2077-modding/for-mod-users/users-modding-cyberpunk-2077/getting-started/vortex-mod-manager.md
