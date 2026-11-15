# Stardew Valley (`stardewvalley`): game support

## Summary
`stardewvalley.toml` already had the Steam appid, the `Mods` deploy root and the `stardew` layout profile. This change marks the game `tested` (research-based), adds the completion header, documents why SMAPI is not a `[[loaders]]` entry (Vessel ships it as a built-in Rust loader plugin, `loaders/smapi.rs`, registered for this game, so a declarative entry would be a duplicate key) and adds a test pinning the config. No Rust change is needed.

## How mods are installed
- **Runtime:** Steam lists a native Linux depot (413153) next to Windows and macOS ones [1]. Some ProtonDB users report online multiplayer failing on the native build and working under Proton [1]. Vessel's SMAPI plugin handles both native and Proton installers [6].
- **Deploy root:** `<game>/Mods`, one folder per mod, each with `manifest.json` directly inside [2][3]. The `stardew` profile keeps archive structure and turns each folder holding `manifest.json` into a top-level mod folder (`staging.rs`, `marker_layout`).
- **Load order:** none. SMAPI discovers mods in `Mods` and orders them by manifest dependencies, so there is no `[plugins]` block and no shared file.
- **SMAPI install:** the installer is run from the SMAPI release (`install on Linux.sh` calls `internal/linux/SMAPI.Installer`) [2][4]. It replaces the `StardewValley` launcher script [6]; whether Steam needs a launch option on Linux is disputed (see Open questions).
- **Path override:** SMAPI accepts `--mods-path` / `SMAPI_MODS_PATH` for a non-standard mods folder, and its installer `--game-path` [5]. Vessel does not use the override.

## Loaders and script extenders
| Name | Nexus id | Installs to | Detect | Build-coupled | Launch change | Source |
|---|---|---|---|---|---|---|
| SMAPI | 2400 | game root (`StardewModdingAPI*`, `smapi-internal/`, replaced `StardewValley` launcher, `Mods/ConsoleCommands`, `Mods/SaveBackup`) | built into Vessel (`loaders/smapi.rs`) | built in | built in | [4][6] |

Nexus id 2400 and the touched paths come from Vessel's own `smapi.rs`; the Nexus page itself returned HTTP 403 and was not read. Content Patcher and other frameworks are ordinary SMAPI mods under `Mods/`.

## Configuration changes
- `support = "tested"` (was `experimental`): Gaps is `None`; promotion is research-based per the task.
- Added `# vessel-game-support: complete` and `# Sources:` header lines.
- Added a comment explaining why no `[[loaders]]` smapi entry exists (`loaders/registry.rs` registers `Smapi` for the `stardewvalley` definition and rejects duplicate keys) [6].
- Unchanged and confirmed: `steam_appid = 413150` [1], `subdir = "Mods"` [2][3], `profile = "stardew"`, `target = "game_dir"`.
- Test `stardew_deploys_to_mods_and_leaves_smapi_to_the_builtin_plugin`; `every_other_preset_is_experimental` now also lists `stardewvalley`.

## Gaps
None

## Open questions
- GOG and Epic ids: not found as numeric ids (a Lutris entry only gives the GOG slug `stardew_valley`), so left out.
- Whether Steam launches the SMAPI-patched game without a launch option on Linux: sources conflict [7].
- The Nexus page for SMAPI, the Stardew wiki and PCGamingWiki returned HTTP 403, so the mod id and layout were checked only against Vessel's code and secondary pages.
- Hardware verification is not done.

## Check results
Run on 2026-11-15 after rebasing on develop, with the app::games test fix applied.
- `check_game_support_note.py --definition crates/vessel-core/definitions/stardewvalley.toml --note docs/game-support/stardewvalley.md`: pass (outcome: full, gaps: none)
- `cargo fmt --all -- --check`: pass
- `cargo clippy -p vessel-core --all-targets --locked -- -D warnings`: pass
- `cargo test -p vessel-core --locked`: pass (lib 510 passed, 2 ignored; integration tests 129 passed; 0 failed)

## Manual test
Install a mod, deploy, launch, uninstall (plan section 4.4). Promotion to tested here is based on research, not hardware testing.

## Sources
1. https://steamdb.info/app/413150/depots and https://www.protondb.com/app/413150 (search results, read 2026-11-15)
2. https://stardewvalleywiki.com/Modding:Installing_SMAPI_on_Linux (search summary only, 2026-11-15)
3. https://raw.githubusercontent.com/Pathoschild/SMAPI/develop/docs/technical/mod-package.md (2026-11-15)
4. https://stardewmodding.wiki.gg/wiki/How_To_Install_SMAPI_on_the_Steam_Deck_(native) (2026-11-15)
5. https://raw.githubusercontent.com/Pathoschild/SMAPI/develop/docs/technical/smapi.md (2026-11-15)
6. `crates/vessel-core/src/loaders/smapi.rs` and `loaders/registry.rs` in this repository
7. https://forums.stardewvalley.net/threads/cant-launch-stardew-from-steam-with-mods-after-installing-smapi-on-linux.22158 (search result, 2026-11-15)
