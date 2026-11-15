# T1 findings: Steam games missing from Games > Found in Steam

Diagnosis only. No code changed, nothing written to Steam folders.

## Root cause

**The preset-only filter in `detect_installed` is the cause.** `crates/vessel-core/src/games.rs:557-568` loops over `GameDefinition`s and calls `steam::find_app(&libs, d.steam_appid?)` (line 561). An installed Steam app with no matching preset is never looked at. On this host Steam root and library discovery work correctly, and every manifest parses. Every missing game is missing for this one reason.

Supporting evidence:
- `crates/vessel-core/src/app/games.rs:277-296` `detect()` only maps `detect_installed` results. `Detected` (line 268) has no `appid`, so the UI cannot request art for it (G1).
- `apps/desktop/src/views/Games.tsx:187` additionally hides `configured` games and managed ids/nexus domains. This is intended.
- Built-in presets: 24 files in `crates/vessel-core/definitions/*.toml`, all with `steam_appid`. There is no user override dir (`~/.config/vessel/games` is absent).
- `target/debug/vessel game detect` (read-only) prints exactly 7 games: skyrimspecialedition, stardewvalley, valheim, schedule1, palworld, eldenring, hades2.

Other candidates ruled out:
- **StateFlags filter:** `detect` applies none. `find_app` (`games/steam.rs:153-179`) only requires `steamapps/common/<installdir>` to exist. Valheim (flags=6, update required) is still detected. `state_flags::fully_installed` (`steam.rs:203`) is used only by `loaders/runtime.rs:276`.
- **Root/library discovery:** works (see enumeration below).
- **Manifest parse failure:** none. All 30 manifests have appid, name, StateFlags and an existing installdir.

## Host enumeration

Steam roots (`steam.rs:12-18` candidates, canonicalized + dedup at `steam.rs:99-117`):
| Candidate | Result |
|---|---|
| `~/.local/share/Steam` | native root (real dir) |
| `~/.steam/steam` | symlink to `~/.local/share/Steam` (deduped) |
| `~/.steam/root` | symlink to `~/.local/share/Steam` (deduped) |
| `~/.var/app/com.valvesoftware.Steam/.local/share/Steam` | absent (no Flatpak Steam) |
| `~/snap/steam/...` | absent |

Libraries: `steamapps/libraryfolders.vdf`, which is identical to `config/libraryfolders.vdf`, lists one library, `/home/jkempf/.local/share/Steam`. It contains 30 apps, which matches 30 `appmanifest_*.acf` files.

| appid | StateFlags | name | in detect()? | why not |
|---|---|---|---|---|
| 489830 | 4 | The Elder Scrolls V: Skyrim Special Edition | yes | |
| 413150 | 4 | Stardew Valley | yes | |
| 892970 | 6 | Valheim | yes | (update pending, still shown) |
| 3164500 | 4 | Schedule I | yes | |
| 1623730 | 4 | Palworld | yes | |
| 1245620 | 4 | ELDEN RING | yes | |
| 1145350 | 4 | Hades II | yes | |
| 1422450 | 6 | Deadlock | no | no preset |
| 1501750 | 4 | Lords of the Fallen | no | no preset |
| 1763250 | 4 | SWORN | no | no preset |
| 1808500 | 4 | ARC Raiders | no | no preset |
| 2102450 | 4 | Enotria: The Last Song | no | no preset |
| 2444750 | 4 | Shape of Dreams | no | no preset |
| 2622380 | 4 | ELDEN RING NIGHTREIGN | no | no preset |
| 2868840 | 4 | Slay the Spire 2 | no | no preset |
| 3108510 | 4 | FlyKnight Enhanced Edition | no | no preset |
| 3405340 | 4 | Megabonk | no | no preset |
| 632360 | 4 | Risk of Rain 2 | no | no preset |
| 646570 | 4 | Slay the Spire | no | no preset |
| 686060 | 4 | Mewgenics | no | no preset |
| 228980 | 4 | Steamworks Common Redistributables | no | tool (correctly excluded) |
| 1070560 | 4 | Steam Linux Runtime 1.0 (scout) | no | runtime |
| 1391110 | 4 | Steam Linux Runtime 2.0 (soldier) | no | runtime |
| 1628350 | 4 | Steam Linux Runtime 3.0 (sniper) | no | runtime |
| 4183110 | 4 | Steam Linux Runtime 4.0 | no | runtime |
| 1493710 | 4 | Proton Experimental | no | tool |
| 1887720 | 4 | Proton 7.0 | no | tool |
| 2180100 | 4 | Proton Hotfix | no | tool |
| 2805730 | 4 | Proton 9.0 | no | tool |
| 4628710 | 4 | Proton 11.0 | no | tool |

That is 13 real games missing, all because they have no preset. Deadlock has flags=6.

## Recommended fix scope (G2: all fully-installed Steam games minus tools/runtimes)

1. **steam.rs:** add `list_installed_apps(libraries) -> Vec<SteamApp + name>`. It should read every `steamapps/appmanifest_*.acf` in each library, dedupe by appid (first library wins, the same as `find_app`), and require that the install dir exists. `AppManifest` (`steam.rs:79-96`) must also expose `name`.
2. **Fully installed:** keep an app only when `state_flags::fully_installed(flags)`. Note that this hides apps with flags=6 (Deadlock, and Valheim, which shows today). Decide whether `UPDATE_REQUIRED` should still count as installed. Recommendation: accept `flags & FULLY_INSTALLED != 0` without BUSY_OR_BROKEN, so a pending update does not hide a game.
3. **Exclude tools/runtimes:** skip an app whose install dir contains `toolmanifest.vdf`. This covers all Proton and Steam Linux Runtime builds on this host. Add an appid denylist for the ones without a manifest: 228980 (Steamworks Common Redistributables). Belt-and-braces denylist from this host: 228980, 1070560, 1391110, 1628350, 4183110, 1493710, 1887720, 2180100, 2805730, 4628710.
4. **games.rs `detect_installed` / app `detect`:** join the apps to definitions by `steam_appid`. A preset match keeps its domain, name and support. A non-preset app gets a synthetic domain (for example a slug of the name, or `steam-<appid>`), the manifest name, and `support = Unsupported`/custom.
5. **Add `appid: Option<u32>` to `Detected`** (`app/games.rs:268`) so `DetectedTile` can use `art::lookup` by appid (`app/art.rs`). This fixes the G1 monogram. It needs an art command that takes an appid and does not require a managed game (`commands.rs:435` / `art.rs:194` currently need a managed game).

## Regression test fixture

Build a temp `home` containing `.local/share/Steam/steamapps/` with:
- `libraryfolders.vdf` listing the root plus a second library `<tmp>/lib2` (reuse the shape of `crates/vessel-core/tests/fixtures/steam/libraryfolders.vdf`).
- Root: `appmanifest_489830.acf` (preset, flags 4) and `appmanifest_632360.acf` "Risk of Rain 2" (no preset, flags 4), each with a `common/<installdir>/` folder.
- Root: `appmanifest_1493710.acf` "Proton Experimental" (flags 4) with `common/Proton - Experimental/toolmanifest.vdf`.
- Root: `appmanifest_228980.acf` "Steamworks Common Redistributables" (flags 4, no toolmanifest).
- lib2: `appmanifest_1808500.acf` "ARC Raiders" (flags 4) in the second library.
- Root: `appmanifest_999001.acf` with flags 1026 (update running) and an existing dir. The test expects it excluded.
- Root: `appmanifest_999002.acf` with flags 4 but no `common/<installdir>`. The test expects it excluded.
- Optional: the same appid in both libraries, to check dedupe.

Expected detect output: 489830 (preset domain `skyrimspecialedition`), 632360 and 1808500 (non-preset, carrying appid and manifest name). Proton, the redist, 999001 and 999002 must be absent. Each entry carries `appid`.
