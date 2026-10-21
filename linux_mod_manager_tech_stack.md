# Tech Stack: Linux Mod Manager (modman rewrite)

## 1. Decisions

| Area | Decision |
| :--- | :--- |
| Relationship to existing code | Full rewrite. The bash `modman` (v3.2.0) and the Python/Vite web UI are replaced. They stay in the repo as the behavioural reference until the feature-parity checklist (§9) is complete. |
| Language / core | Rust, in a Cargo workspace |
| GUI | Tauri v2 + React + Vite + TypeScript |
| CLI | Rust + clap, built on the same core crate |
| Targets | Steam Deck Game Mode, desktop Linux |
| Mod source | Nexus Mods only (REST v1 + GraphQL v2) |
| Game detection | Steam only (native and Proton) |
| Deployment | Hardlinks by default; symlink or reflink fallback |
| State | SQLite |
| Nexus auth | Personal API key first; OAuth2/SSO added later |
| Downloads | Premium: direct API links. Free: `nxm://` handler. Both supported. |
| Controller input | Frontend only: HTML5 Gamepad API + spatial navigation |
| Packaging | AppImage, Flatpak, distro packages (AUR, RPM, deb) |
| Features | Single mods, Nexus collections, FOMOD installers, LOOT plugin sorting (required: Bethesda games are a primary target) |
| Licence | GPL-3.0-or-later |

---

## 2. Architecture

```
+------------------------------------------------------------+
|                        USER INTERFACE                      |
|   [ CLI: modman ]            [ GUI: Tauri v2 + React ]     |
|    clap, same binary name     Vite, TS, Tailwind           |
|                               Gamepad API + spatial nav    |
+-----------------------+------------------------------------+
                        |  Rust function calls / tauri::command (IPC)
                        v
+------------------------------------------------------------+
|                  modman-core  (library crate)              |
|  games     Steam library + appmanifest (.acf) detection    |
|  nexus     REST v1 + GraphQL v2 client, auth, rate limits  |
|  download  resumable downloads, nxm:// intake queue        |
|  archive   zip / 7z / rar extraction                       |
|  fomod     ModuleConfig.xml parser + install plan          |
|  staging   one folder per mod version                      |
|  deploy    hardlink / symlink / reflink engine, purge      |
|  loadorder LOOT sorting + collection rules                 |
|  store     SQLite (schema + migrations)                    |
+------------------------------------------------------------+
```

The core has no UI dependencies and does no printing. It exposes typed functions and emits progress through a channel. The CLI and the Tauri app are thin adapters over it.

### Workspace layout

```
modman/
  Cargo.toml                 # workspace
  crates/
    modman-core/             # all logic, no UI
    modman-cli/              # clap binary -> `modman`
  apps/
    desktop/                 # Tauri v2 app (src-tauri + React/Vite frontend)
  packaging/                 # AppImage, Flatpak manifest, PKGBUILD, rpm spec, deb
  legacy/                    # old bash modman + web UI, kept as reference
```

### Process model
- The GUI runs the core in-process. The CLI runs it in its own process.
- Both open the same SQLite database. Use WAL mode and a lock file for deploy and purge, so a CLI deploy cannot race a GUI deploy (replaces the current `modman.lock`).
- The GUI must also be able to run headless commands. For example, `modman nxm <url>` hands the link to a running GUI instance when one exists, otherwise it enqueues the download.

---

## 3. Stack by layer

| Layer | Choice | Notes |
| :--- | :--- | :--- |
| Async / HTTP | `tokio`, `reqwest` | Streaming downloads, retries, and rate-limit handling driven by Nexus response headers. |
| CLI | `clap` (derive) | Subcommands mirror the current bash CLI so users' habits carry over. |
| GUI shell | Tauri v2 | Uses the system WebKitGTK webview. Small bundle, no bundled Chromium. |
| Frontend | React + Vite + TypeScript + Tailwind | State via TanStack Query for IPC calls plus a small store (Zustand) for UI state. |
| State | `rusqlite` (or `sqlx`), `refinery`/embedded migrations | Transactions make deploy and purge atomic and crash-recoverable. |
| Steam parsing | A VDF/KeyValues parser crate | `libraryfolders.vdf` for libraries (including SD card), `appmanifest_*.acf` for installs, `compatdata/<appid>` for Proton prefixes. |
| Archives | `zip`, `sevenz-rust`, RAR via `unrar` or shelling out to `7z` | RAR support carries a licensing and native-library cost; decide in a spike. |
| FOMOD | `quick-xml` + `serde` | Parse `fomod/ModuleConfig.xml`; the GUI renders steps, groups and conditions. |
| Load order | libloot | Bethesda plugin sorting. Verify Rust binding maturity in a spike; fallback is FFI to the C++ API. |
| Reflink | `reflink-copy` | Used on btrfs/XFS. Falls back automatically. |
| Logging | `tracing` | Log to file and to the GUI log view. |
| Errors | `thiserror` in core, `anyhow` in binaries | |

---

## 4. Key requirements and how the stack meets them

### 4.1 Deployment engine
- **Default: hardlinks.** Fast, no extra disk, and purge restores the game exactly.
- **Constraint:** a hardlink needs staging and the game on the same filesystem. On Steam Deck the game may live on the SD card while `$HOME` is on internal storage.
  - Staging therefore lives **per Steam library**, for example `<library>/.modman/staging/`, not in a single global directory.
  - Detect the case where staging and game differ at deploy time (compare `st_dev`) and choose the fallback automatically.
- **Fallback order:** hardlink, then reflink (if the fs supports it), then symlink, then copy. Record the chosen method per file in SQLite.
- **Safety:** before overwriting any game file, back it up to a content-addressed backup store and record it in the manifest. Purge restores originals. Deploy runs inside a transaction journaled in SQLite; an interrupted deploy resumes or rolls back at next launch.
- **Conflicts:** one winner per path, decided by profile mod order. Surface conflicts in the UI and CLI.
- **Proton specifics:** some games read from the prefix (`compatdata/<appid>/pfx/...`, for example `My Games`, `AppData`). Mod targets can be `game_dir` or `prefix_dir`; each game definition declares which.

### 4.2 Nexus Mods integration
- **Endpoints:** REST v1 `https://api.nexusmods.com/v1` for mods, files and download links; GraphQL v2 `https://api.nexusmods.com/v2/graphql` for collections and search (the same ones the bash tool uses).
- **Auth:** API key first, stored in the system keyring via the Secret Service API (`keyring` crate), with a config-file fallback on systems without one (common on Deck Game Mode). OAuth2/SSO later behind the same `AuthProvider` trait.
- **Downloads:**
  - Premium: request a download link directly from the API.
  - Free: register as the `nxm://` handler. The user clicks "Download with manager" in the browser; the link carries `key` and `expires`, which modman exchanges for the CDN URL.
- **`nxm://` plumbing:** install a `.desktop` file with `MimeType=x-scheme-handler/nxm;`. Use Tauri's single-instance and deep-link plugins so the second invocation forwards the URL to the running app. In Game Mode there is no browser integration, so the Deck flow is: switch to Desktop Mode to click the link, or paste the link into the app.
- **Rate limits:** read the `x-rl-*` response headers and throttle. Persist the state (replaces the current `ratelimit` file).
- **Updates:** poll `updated.json`-style endpoints rather than checking every mod; cache results in SQLite.

### 4.3 Collections, FOMOD, load order
- **Collections:** fetch the collection manifest via GraphQL, queue every mod, apply per-mod choices and rules. Same behaviour as the current `modman` collection install.
- **FOMOD:** parse `ModuleConfig.xml` into a step/group/option tree with visibility conditions. The GUI renders it as a wizard, the CLI as prompts. Collections supply pre-recorded choices, so no prompt is needed.
- **Load order:** run LOOT sorting for Bethesda games and merge it with collection-provided rules. Write the result to `plugins.txt` in the correct Proton prefix location. Non-plugin games skip this stage.

### 4.4 Controller support (Steam Deck Game Mode)
- **Frontend only.** Use the HTML5 Gamepad API plus a spatial-navigation library for D-pad and stick focus movement; A selects, B goes back, shoulder buttons switch tabs.
- Steam Input may present the controller as keyboard and mouse instead of a gamepad. Make the UI **fully keyboard-navigable too** (visible focus ring, arrow-key spatial nav). That covers both modes.
- **Risk to spike first:** whether WebKitGTK inside a Game Mode shortcut exposes the Gamepad API reliably. If not, ship a Steam Input layout that maps buttons to keys and rely on keyboard nav.
- Build for 1280x800: large hit targets, no hover-only affordances, on-screen text input fallback for pasting an API key (Steam's on-screen keyboard).

### 4.5 Distribution

| Format | Notes |
| :--- | :--- |
| **AppImage** | Primary for Steam Deck. Full filesystem access, so hardlinks and Steam libraries work. Add as a non-Steam shortcut. |
| **Flatpak** | Needs `--filesystem` access to Steam libraries and the staging dirs. Hardlinks across sandbox mount boundaries can fail; test on a real SD card library. Handle `nxm://` via a `.desktop` entry in the manifest. |
| **Distro packages** | AUR (`PKGBUILD`), RPM, deb. These depend on system `webkit2gtk-4.1`. |
| **CLI** | Shipped in the same packages as `modman`. |

---

## 5. SQLite data model (outline)

| Table | Purpose |
| :--- | :--- |
| `games` | Detected games: appid, install path, prefix path, library, supported-game definition |
| `profiles` | Per-game profiles: name, mod order, plugin order |
| `mods` | Nexus mod id, file id, version, source, status |
| `mod_files` | Per-mod staged file list with hashes |
| `deployments` | Active deploy state per game: file -> link method, backup reference |
| `downloads` | Queue and resume state, `nxm` intake |
| `collections` | Installed collection revisions and their rules |
| `schema_migrations` | Migration history |

No migration from the legacy bash tool: the rewrite starts fresh. It does not read `~/.config/nexus-mod-manager` or `deployed.manifest`. Users must purge with the legacy `modman` before switching, so the game folder is clean. The new tool should warn if it finds legacy state.

---

## 6. Alternative considered

**Slint or egui (pure-Rust GUI).** One language and no webview dependency, but fluid controller-friendly layouts, FOMOD wizards and long virtualised mod lists take noticeably longer to build than in React. Rejected for v1. The core crate is UI-agnostic, so a native front end remains possible later.

---

## 7. Risks and spikes (do these first)

1. **Gamepad API in WebKitGTK under Game Mode.** Decides whether keyboard nav plus a Steam Input layout is the primary path.
2. **Hardlinks with split filesystems.** Verify the per-library staging design on a Deck with an SD card library, and under Flatpak.
3. **libloot from Rust.** Binding availability, build complexity, and AppImage/Flatpak bundling.
4. **RAR extraction.** `unrar` licensing and native dependency versus shelling out to `7z`.
5. **`nxm://` in Game Mode.** No browser integration; confirm the paste-link flow is acceptable.
6. **Keyring on Deck Game Mode.** Confirm Secret Service availability or fall back to a permission-restricted file.

---

## 8. Milestones

1. **Core skeleton:** workspace, SQLite schema, Steam game detection, `modman games list`.
2. **Deploy/purge:** staging, hardlink engine with fallbacks, backups, manifest, crash recovery. This is the highest-risk core and comes first.
3. **Nexus:** API key auth, single mod download and install (Premium and `nxm://`), update checks.
4. **GUI v1:** Tauri shell, game and mod lists, deploy/purge, keyboard and gamepad navigation, 1280x800 layout.
5. **FOMOD + collections.**
6. **LOOT sorting + conflict view.**
7. **OAuth2/SSO, packaging (AppImage, Flatpak, AUR/RPM/deb), Deck polish.**

---

## 9. Open items

- **Feature-parity checklist:** written in `feature_parity_checklist.md`. Its §9 lists the legacy code that must be read before porting.
- **Supported games for v1:** see §9.1. **v1 tested set: Skyrim Special Edition** (Steam appid 489830, deploy to `Data`, `plugins.txt` in the Proton prefix, LOOT sorting). It is the reference game for the Bethesda profile and the first end-to-end test target on desktop and Deck. Other Bethesda titles and all other presets ship as experimental. More titles can be added to the tested set later.

### 9.1 Game definitions

A game definition tells the core where mods go and how to treat them. It is data (one TOML file per game, embedded in the binary and overridable by the user), not code. Fields: Nexus domain, Steam appid, deploy subdirectory, target (game dir or Proton prefix), layout profile, and extras (plugins.txt format, Paks folder).

The legacy `modman` already has these presets, so v1 ports them as data. Layout profiles to port: `bethesda`, `cyberpunk`, `stardew`, `bannerlord`, `witcher3`, `bepinex`, `melonloader`, `ue` (Unreal pak mods), `generic`.

| Profile | Games in the legacy presets |
| :--- | :--- |
| bethesda (plugins, LOOT) | Skyrim SE, Skyrim, Enderal SE, Fallout 4, Fallout 3, Fallout New Vegas, Oblivion, Morrowind, Starfield |
| cyberpunk | Cyberpunk 2077 |
| stardew | Stardew Valley |
| bannerlord | Mount & Blade II: Bannerlord |
| witcher3 | The Witcher 3 |
| bepinex / melonloader | Valheim, Outward, Schedule I |
| ue (Unreal paks) | Palworld, Clair Obscur: Expedition 33, Oblivion Remastered |
| generic | Elden Ring, Control, Hades II, Space Marine 2, Monster Hunter Wilds |

Proposal: all presets ship as definitions, but only a small set is "supported" (tested end to end on Deck). Games outside that set are labelled "experimental" in the UI. Pick the tested set from the games you actually play. LOOT only applies to the `bethesda` profile.

### 9.2 Auto-update (kept)

The legacy tool updates mods and redeploys after system or game updates. The rewrite keeps it:
- `modman autoupdate` is a CLI command and the single implementation. The GUI calls the same core function.
- Scheduler: systemd user timer only (cron and autostart are dropped). GUI-only: no headless web mode.
- Trigger: a systemd **user** timer (periodic, interval configurable, 0 = only after updates) plus a check at app start. Game updates are detected by watching `appmanifest_*.acf` build ids. System updates are detected by watching for running package managers, as the legacy tool does via `/proc`.
- Steam Deck: user timers work in Desktop Mode and Game Mode, but SteamOS resets system files on update, so keep everything under `$HOME` (`~/.config/systemd/user`).
- Policy: never redeploy while the game is running. Collections update to the newest revision only if the user enabled that setting. Always journal the deploy so a failed update rolls back.
- The GUI has a toggle and last-run log; the timer unit is installed or removed from there.

### 9.3 Licence

Decision: **GPL-3.0-or-later**. LOOT is kept, and libloot is GPL-3.0, so linking it makes the combined app GPL-3.0 anyway. It also fits Linux desktop norms and the AUR/RPM/deb packaging. Add a `LICENSE` file and SPDX headers. Check the licences of every dependency (especially `unrar`) with `cargo deny` in CI.

---

## 10. References to verify before relying on them

- Nexus Mods public API documentation (REST v1, GraphQL v2, `nxm://` link format, rate-limit headers).
- Tauri v2 docs: single-instance, deep-link, CLI and bundling plugins.
- LOOT/libloot documentation.
- Prior art for deployment and FOMOD behaviour: Mod Organizer 2, Vortex, and existing Linux managers. Confirm each project's current state and licence before borrowing ideas.
