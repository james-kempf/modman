# Feature-Parity Checklist: legacy `modman` v3.2.0 -> Rust rewrite

Source: the legacy `modman` help text and command dispatch, plus the view list in `web/src/views`. Behaviour details inside each command were not audited line by line; the "Verify" column marks places where the legacy code must be read before porting.

**Status values** (proposals, edit freely): `Port` = same behaviour, `Change` = ported with a deliberate difference, `Drop` = not carried over, `Later` = after v1, `New` = does not exist in legacy.
**Milestones** refer to §8 of `linux_mod_manager_tech_stack.md` (M1..M7).
**v1 reference game:** Skyrim Special Edition.

---

## 1. Global CLI behaviour

| Feature | Legacy | Status | MS | Notes / Verify |
| :--- | :--- | :--- | :--- | :--- |
| `-g/--game <domain>` per-command game override | yes | Port | M1 | |
| `-y/--yes` assume yes | yes | Port | M1 | |
| `--no-color`, `--debug` | yes | Port | M1 | Also honour `NO_COLOR`. |
| `help`, `version` | yes | Port | M1 | |
| Mod reference forms: `#12` position, full id, exact name, Nexus mod id, unique name part | yes | Port | M3 | Keep all five; add ambiguity errors that list matches. |
| Command aliases (`i`, `ls`, `rm`, `dl`, `st`, ...) | yes | Port | M1 | Verify full alias list in dispatch (~line 5009). |
| Lock file preventing concurrent runs | `modman.lock` (flock) | Change | M2 | Replaced by SQLite WAL + deploy lock (see tech stack §2). |
| Required external tools (`curl jq python3 file flock 7z bsdtar unrar`) | yes | Change | M3 | Rust replaces curl/jq/python/flock. Archive tools: decide per RAR spike (§7). |

## 2. Setup and accounts

| Command | Legacy behaviour | Status | MS | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `login` | Nexus API key + first game setup | Change | M3 | API key first; add keyring storage; OAuth2/SSO later (M7). Verify: key validation call, premium detection (`IS_PREMIUM`). |
| `logout` | Forget API key | Port | M3 | |
| `game detect` | List supported Steam games installed | Port | M1 | `libraryfolders.vdf` + `appmanifest_*.acf`, including SD cards. |
| `game add <domain> [path]` | Set up a game, folder auto-detected | Port | M1 | Manual path is retained as an override. |
| `game list` / `use <domain>` | List games, switch active game | Port | M1 | |
| `game info` / `set <key> <value>` | Show/change folder, subdir, method | Port | M1 | Keys: folder, subdir, method, prefix, plugins format. Verify the full key list in `cmd_game` (~line 4134). |
| `game remove <domain>` | Forget a game (purge first) | Port | M2 | Refuse if a deployment is active. |
| `config [<key> [<value>]]` | Global settings | Port | M1 | Keys: data-dir, deploy-method, jobs, cdn, adult, auto-deploy, fomod, autoupdate-collections, autoupdate-interval. |
| `nxm register / unregister` | Register `nxm://` handler | Port | M3 | `.desktop` + mimeapps. See tech stack §4.2. |
| `nxm <url>` | Queue incoming link | Change | M3 | Forwards to a running GUI via single-instance. Legacy used the `nxm-queue` file. |
| Per-game config files (`games/<domain>.cfg`) | yes | Change | M1 | Moves to SQLite `games` table. |
| v2 -> v3 config migration | yes | Drop | - | Fresh start decision. Warn if legacy state is found. |

## 3. Mods

| Command | Legacy behaviour | Status | MS | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `install <id\|url\|nxm\|file>` | Nexus id, URL, nxm link or local archive | Port | M3 | |
| `install --file <id>` | Pick a specific file | Port | M3 | |
| `install --fomod ask\|default` | FOMOD behaviour | Port | M5 | |
| `install --deploy` | Deploy afterwards | Port | M3 | |
| `download <id\|url\|collection>` | Download without installing | Port | M3 | |
| `list [text] [-e\|-d] [-c <slug>]` | Priority order; filter enabled/disabled/collection | Port | M3 | |
| `info <mod>` | Details, FOMOD choices, conflicts | Port | M3 | FOMOD choices need M5. |
| `enable / disable <mod...>` (`--all`, `--collection`) | Toggle | Port | M3 | |
| `order <mod> <where>` (`#`, first, last, up, down, before X, after X) | Priority | Port | M3 | |
| `uninstall <mod...>` (`--delete-archive`) | Remove | Port | M3 | |
| `reinstall <mod> [--ask]` | Re-extract; `--ask` re-runs FOMOD | Port | M5 | |
| `conflicts [mod] [--files]` | Which mods override each other | Port | M3 | GUI conflict view in M6. |
| `deps [mod] [--refresh]` | Required mods + external tools (UE4SS, SKSE...) | Port | M3 | Verify the source of requirement data. |
| `deps ack / unack <name>` | Mark external tool installed by hand | Port | M3 | |
| `updates [--install] [--all]` | Check Nexus mods for newer files | Port | M3 | |
| `search <text>` | Search Nexus (GraphQL) | Port | M3 | |
| `profile list/save/load/delete` | Save/restore load order + enabled state | Port | M3 | SQLite `profiles` table. |
| Per-mod staging folder `<data-dir>/<game>/mods/<id>` | yes | Change | M2 | Staging moves per Steam library (hardlink constraint, tech stack §4.1). |
| Archive formats zip / 7z / rar | yes | Port | M3 | See RAR spike. |
| Case-insensitive folder matching | yes | Port | M2 | Required for Windows-authored mods on Linux. |
| Layout profiles: bethesda, cyberpunk, stardew, bannerlord, bepinex, melonloader, witcher3, ue, generic | yes | Port | M2 | As TOML game definitions (tech stack §9.1). Only `bethesda` for Skyrim SE is tested in v1. |

## 4. Collections

| Command | Legacy behaviour | Status | MS | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `collection install <url\|slug>` | Download + install with FOMOD choices and load-order rules | Port | M5 | |
| `--optional all\|none\|ask` | Optional mods | Port | M5 | |
| `--revision N`, `--deploy` | Pin revision; deploy after | Port | M5 | |
| `collection list / info` | Installed; details and update availability | Port | M5 | |
| `collection update <slug>` | Newest revision, removes dropped mods | Port | M5 | |
| `collection remove <slug>` (`--keep-mods`) | Uninstall | Port | M5 | |
| `collection download` | Download only | Port | M5 | |
| `nxm://.../collections/<slug>/revisions/<n>` input | yes | Port | M5 | |
| FOMOD choices from collection (`collection` / `reuse` / `ask` / `default` modes) | yes | Port | M5 | Verify mode semantics (~line 1385). |

## 5. Deployment

| Command | Legacy behaviour | Status | MS | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `deploy [--dry-run] [--method m]` | Link enabled mods into the game | Port | M2 | |
| Methods hardlink / symlink / copy (reflink) | yes | Change | M2 | Adds automatic fallback chain and per-file method record. |
| Back up replaced game files | yes | Port | M2 | Content-addressed backup store. |
| `purge` / `undeploy` | Remove deployed files, restore originals | Port | M2 | Must restore the game exactly. |
| `verify` | Check deployed files intact | Port | M2 | |
| `status` / `st` | Account, games, deployment state | Port | M2 | |
| `launch` / `play` / `run` | Start via Steam, offers to deploy first | Port | M4 | |
| `clean` | Delete unused downloads and temp files | Port | M3 | |
| `log [-f] [lines]` | Show log | Port | M1 | `tracing` to file. |
| Interrupted-deploy recovery | partial | New | M2 | SQLite journal. |
| Refuse deploy/purge while the game runs | unknown | New | M2 | Verify legacy behaviour. |

## 6. Bethesda specifics (Skyrim SE target)

| Feature | Legacy | Status | MS | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `plugins.txt` writing (`star:` format for SSE/FO4/Starfield, `plain:` for older) | yes | Port | M6 | Location in the Proton prefix `AppData/Local/<folder>`. |
| Load-order from collection rules | yes | Port | M5 | |
| LOOT plugin sorting | no | New | M6 | libloot; see spike. |
| Script-extender and tool requirements (SKSE etc.) | `deps` external list | Port | M3 | |

## 7. Auto-update

| Feature | Legacy | Status | MS | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `autoupdate enable / disable / status / run [--force]` | yes | Port | M7 | Single implementation in core (tech stack §9.2). |
| Schedulers: systemd, cron, autostart | yes | Change | M7 | Decided: systemd user timer only. cron and autostart are dropped. `--scheduler` flag removed. |
| Detect running package managers via `/proc` | yes | Port | M7 | |
| Detect Steam game updates | yes | Port | M7 | `appmanifest` build id. |
| Collections auto-update setting, interval setting | yes | Port | M7 | |

## 8. Web UI (`modman ui`) vs Tauri GUI

| Legacy web view | Status | MS | Notes |
| :--- | :--- | :--- | :--- |
| Overview | Port | M4 | |
| Mods | Port | M4 | Add conflict view in M6. |
| Install | Port | M4 | FOMOD wizard in M5. |
| Collections | Port | M5 | |
| Requirements | Port | M4 | |
| Activity (jobs, log) | Port | M4 | Job island/progress via core event channel. |
| Settings | Port | M4 | Includes auto-update toggle (M7). |
| `ui [--port N] [--no-open]` browser server (`server.py`) | Drop | - | Decided: GUI-only. Replaced by the Tauri window; no headless web mode. |
| `ui desktop` (menu entry / Deck non-Steam shortcut) | Change | M7 | Done by AppImage/Flatpak/package `.desktop` files; document the Deck shortcut. |
| `ui-state`, `ui-detect`, `ui-thumbs` (JSON helpers for the UI) | Drop | - | Replaced by typed `tauri::command`s. Thumbnails need a cache in core. |
| Token + localhost-only server security | Drop | - | No server. |
| Controller / spatial navigation | New | M4 | See tech stack §4.4. |

## 9. Needs a decision or a code read before porting

1. Exact `game set` key list and per-game config fields (`cmd_game`, ~line 4134).
2. FOMOD mode semantics and how recorded choices are stored (~line 1385).
3. What data `deps` uses for requirements and external tools.
4. Whether legacy blocks deploy while the game is running.
5. Alias lists and all flags per command (read dispatch + each `cmd_*` argument parser).
6. Which unlisted legacy behaviours exist (anything not shown in the help text, such as rate-limit handling and adult-content setting `adult`).

When a row is ported, record the test that proves it. Suggested end-to-end acceptance test for v1: fresh install -> `login` -> `game detect` -> install a Skyrim SE mod from Nexus -> `deploy` -> `verify` -> `purge` -> game folder byte-identical to before.
