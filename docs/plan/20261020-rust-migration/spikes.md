# T02 spikes: external dependencies and cross-cutting decisions

Plan `20261020-rust-migration`, task T02. Date 2026-10-20. Toolchain: rustc/cargo 1.99.1, Fedora 44 (Nobara).
All prototypes live in the session scratchpad (`.../scratchpad/t02/{loot-spike,rar-spike,kr,vdf-spike,nxm-spike,deps-spike,desk-spike}`);
nothing outside this file was changed in the workspace. Versions are the latest on crates.io on 2026-10-20
(queried with `cargo info`), pinned with `=` so that T03 can copy them verbatim.

Consumers: T03 (Cargo.toml + migrations), T04 (archive), T05 (deploy), T06 (nexus/download, keyring),
T07 (CLI nxm), T11/T12 (load order), T13 (Tauri backend), T15 (packaging, deny.toml).

---

## 1. libloot (load-order sorting)

**Decision: use the upstream pure-Rust crate from crates.io: `libloot = "=0.29.7"`. No C++/FFI build.**

Evidence:

- The hint is correct. Upstream libloot is now written in Rust and is published on crates.io:
  `libloot 0.29.7`, licence `GPL-3.0-or-later`, edition 2024, `rust-version = 1.89` (we use 1.99.1).
  Repository `github.com/loot/libloot`, tag `0.29.7` = commit `c242698ea86a382998cd8d1c5329f26aea666867`.
- Its dependencies are all Rust: `esplugin 6.1.4`, `libloadorder 18.8.2`, `loot-condition-interpreter 6.0.0`,
  `petgraph`, `rayon`, `regress`, `saphyr` (YAML masterlists), `crc32fast`, `unicase`, `rustc-hash`, `log`.
  `libloadorder` also pulls in `keyvalues-parser 0.2.4`, the same VDF crate chosen in section 4.
  `esplugin` and `libloadorder` declare `GPL-3.0` (the deprecated SPDX id). Their source headers say
  "or (at your option) any later version", so they are effectively GPL-3.0-or-later and compatible with us.
- `build.rs` only runs `git rev-parse` to embed a revision string. Outside a git checkout it prints a warning
  and leaves the string unset. To silence it, set `LIBLOOT_REVISION = "c242698"` under `[env]` in `.cargo/config.toml` (optional).
- **Throwaway compile test (passed):** `scratchpad/t02/loot-spike`. It writes three minimal TES4-header SSE plugins
  (`Skyrim.esm` with the master flag, `A.esp` with master Skyrim.esm, `B.esp` with masters Skyrim.esm and A.esp) into a temp
  `game/Data`. It then creates `Game::with_local_path(GameType::SkyrimSE, game, local)`, calls `load_plugins`, and
  calls `sort_plugins(["B.esp","A.esp","Skyrim.esm"])`.
  Output: `sorted: ["Skyrim.esm", "A.esp", "B.esp"]`, `OK libloot 0.29.7`. A cold release build took 8.1 s wall.
- `ldd` of the spike binary: only `libgcc_s.so.1`, `libc.so.6` and `ld-linux`. libloot adds no shared libraries.

API notes for T11/T12:

- On Linux, `Game::new` cannot find the local data path for Bethesda games. Always use
  `Game::with_local_path(game_type, game_dir, local_dir)`. Here `local_dir` is
  `<compatdata prefix>/drive_c/users/steamuser/AppData/Local/<extra>`, the same directory legacy
  `detect_plugins_file` uses (legacy/modman 434-439).
- Flow: `load_plugins` (or `load_plugin_headers` when only sorting) -> `database().write()` load masterlist/userlist
  -> `sort_plugins(&names)` -> `set_load_order`. The masterlist YAML is data that is downloaded at runtime and not bundled.
  The licence of the masterlist data was not checked in this spike.
- `GameType` covers every bethesda-profile preset: Oblivion, Skyrim, SkyrimSE, SkyrimVR, Fallout3, FalloutNV,
  Fallout4, Fallout4VR, Morrowind, Starfield, OpenMW, OblivionRemastered. Enderal SE uses `SkyrimSE`.

Bundling impact: nothing. The crate is statically linked Rust and needs no runtime libraries in AppImage or Flatpak.

Integration path: `libloot = "=0.29.7"` (crates.io). If a git pin is needed instead, use
`libloot = { git = "https://github.com/loot/libloot", rev = "c242698ea86a382998cd8d1c5329f26aea666867" }`.
Runtime libs to bundle: none.

libloot buildable: yes

---

## 2. RAR extraction

**Decision: read RAR through libarchive using `compress-tools = "=0.16.1"`, linked dynamically to the system
`libarchive.so.13`. Do not use the `unrar` crate. Do not shell out to `7z`.**
`zip` and `sevenz-rust2` still handle .zip and .7z in pure Rust. libarchive is used only for `.rar`
(T04 may also use it as a fallback for formats the pure-Rust readers reject).

| Option | Licence | Result here |
| :--- | :--- | :--- |
| `unrar 0.5.8` / `unrar_sys 0.5.8` | The crate is MIT OR Apache-2.0, but it vendors RARLAB UnRAR C++ source (`vendor/unrar/license.txt`: "cannot be used to ... re-create RAR compression algorithm"). | **Rejected.** The UnRAR licence adds a use restriction, so it is not GPL-compatible. Distributing it in a GPL-3.0-or-later binary is not allowed. |
| Shell out to `7z` | 7-Zip is LGPL, with the unRAR restriction on its RAR code. | **Rejected.** Fedora's `7zip-26.03` is built without the RAR codec: `7z l` on RAR4 and RAR5 samples gives "Cannot open the file as archive". Other distros ship different builds, so behaviour would vary by host. |
| Shell out to `unrar` | The installed `unrar` is `unrar-free 0.3.3` (GPL). Proprietary RARLAB unrar is not installed. | Rejected as the primary path because the binary differs per distro. Possible only as a last-resort fallback. |
| `compress-tools 0.16.1` -> libarchive 3.8.7 | MIT OR Apache-2.0 crate; libarchive is BSD-2-Clause. | **Chosen.** |

Evidence:

- Samples are the libarchive test vectors (`libarchive v3.8.1/libarchive/test/test_read_format_rar.rar.uu` for RAR v2.0/4,
  `test_read_format_rar5_solid.rar.uu` and `test_read_format_rar5_stored.rar.uu` for RAR5), decoded into `scratchpad/t02/rar-samples`.
- `bsdtar -tvf` lists all three. The spike `scratchpad/t02/rar-spike` (`list_archive_files` + `uncompress_archive(..., Ownership::Ignore)`)
  extracted the RAR5 solid archive (7 entries) and the RAR4 archive (5 entries, including a symlink and an empty directory). Both printed `extracted ok`.
- `ldd rarspike`: `libarchive.so.13`, `libcrypto.so.3`, `libacl.so.1`, `libzstd.so.1`, `liblz4.so.1`, `libbz2.so.1`, `libz.so.1`,
  `liblzma.so.5`, `libxml2.so.2`, `libm.so.6`. The build needs `libarchive-devel` and `pkg-config`, both present.
- Known libarchive limits for T04 to report as user-facing errors: no encrypted RAR5 headers and limited multi-volume support.

Bundling impact for T15:

- AppImage: bundle `libarchive.so.13` and its non-glibc dependencies listed above, or depend on the host copy.
  SteamOS has libarchive because pacman links it. Not verified on a Deck.
- Flatpak: make sure libarchive is in the runtime (expected in `org.freedesktop.Platform`). If it is not, build it as a manifest module. Unverified.
- `deny.toml` must ban `unrar_sys` so it cannot be added later.
- T04 should put compress-tools behind a default-on cargo feature `rar`. Packagers can then build without libarchive, and `.rar` returns a clear "RAR support not built" error.

---

## 3. Keyring (Nexus API key storage)

**Decision: `keyring-core = "=1.0.1"` plus `zbus-secret-service-keyring-store = "=1.0.1"` with feature `crypto-rust`.
This is pure-Rust D-Bus and crypto, with no libdbus or OpenSSL. Fall back to a permission-restricted file.**
We do not use the `keyring 4.2.0` umbrella crate. In v4 it is an "All-in-One" crate whose default `v1` feature pulls in
Apple, Windows and zbus stores. keyring-core is the API it re-exports, and choosing the store ourselves keeps the tree small.
keyring v4 dropped compile-time store selection: call `keyring_core::set_default_store(Store::new()?)` once at startup.

Probe `scratchpad/t02/kr`. It is read-only: it calls `get_password` on an entry that was never written, so the real keyring is not changed.

| Environment | Result | Meaning |
| :--- | :--- | :--- |
| Live desktop session (gnome-keyring owns `org.freedesktop.secrets`) | `Err(NoEntry)` | Secret Service usable |
| `DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent` | `Store::new()` -> `PlatformFailure(Zbus(Connection(NotFound)))` | fallback |
| Private bus with no activatable services | `Store::new()` -> `PlatformFailure(... ServiceUnknown "org.freedesktop.secrets was not provided by any .service files")` | fallback |
| `dbus-run-session` (gnome-keyring is activatable, but the new instance finds the running daemon) | **Hung until killed (`timeout 40` -> exit 124)** | fallback must use a timeout |

Fallback trigger, implemented by T06 in `nexus.rs` auth:

1. The `NEXUS_API_KEY` env var overrides everything for the current run and is never written (legacy/modman 230-233).
2. Probe once per process on a worker thread with a **3 s timeout**: `Store::new()`, then `Entry::new("modman", "nexus-api-key").get_password()`.
   - `Ok(_)` or `Err(NoEntry)` means use the keyring.
   - `Err(PlatformFailure | NoStorageAccess)`, an error from `Store::new`, or a timeout means use the file backend.
   - `Ambiguous`, `BadEncoding` and similar are real errors. Surface them and do not fall back.
3. File backend: `$XDG_CONFIG_HOME/modman/credentials.toml` (from `directories::ProjectDirs`). Create the directory with mode 0700
   and write the file with mode 0600 (`OpenOptions::mode(0o600)`, then write to a temp file and rename). On read, refuse or warn if the file
   is group- or world-readable.
4. Record the chosen backend in `settings` (`auth.backend = keyring|file`). Tell the user once when the file backend is used,
   which is expected in Deck Game Mode where no Secret Service may run. If a key already exists in the file, keep using the file
   until the next `login`, which tries the keyring first and deletes the file copy after a successful keyring write.
5. Calls are blocking. From async code wrap them in `tokio::task::spawn_blocking`.

---

## 4. VDF / KeyValues parser (Steam detection)

**Decision: `keyvalues-parser = "=0.2.4"` (MIT OR Apache-2.0).** It is already in the tree through libloot -> libloadorder, so it adds no extra crate.
We do not use `steamlocate 2.1.1`: its Steam-root heuristics would replace the legacy `steam_roots` order (legacy/modman 388-433),
which T03 must port exactly. We do not use `new-vdf-parser 0.2.0`, which is LGPL-2.1-only and duplicates the work.
`keyvalues-serde 0.2.4` is available if typed structs are wanted later.

Evidence: `scratchpad/t02/vdf-spike` parsed a synthetic `libraryfolders.vdf` with two libraries, one on the SD card
(`/run/media/mmcblk0p1`), app maps, and an escaped `\"` in the label. It also parsed an `appmanifest_489830.acf`.
Output: `lib 1: path=/run/media/mmcblk0p1 label=SD "Card" apps=["489830"]`, `acf: appid=489830 installdir=Skyrim Special Edition buildid=15916224`, `OK`.
API: `Vdf::parse(&str)` -> `.value.get_obj()` -> `obj.get("path")[0].get_str()`. Keys can repeat, so values are a `Vec`.
T03 tests read only synthetic fixtures. Detection may read `~/.steam` but never writes there.

---

## 5. nxm:// routing (T07 CLI, T13 GUI, T15 packaging)

**Single handler target: the CLI binary `modman`.** The GUI binary never registers the scheme.
`tauri-plugin-deep-link` is **not used on Linux**. This is a deliberate deviation from tech stack §4.2:
deep-link would register its own `Exec=<gui> %u` and compete with the CLI handler, and the CLI must work headless.
T13R/T17 should check that no second nxm handler is registered.

### Exec line (verbatim)

Packaged `.desktop` (T15; file `modman-nxm.desktop`, keeps the legacy name from legacy/modman 1533):

```
Exec=modman nxm %u
```

The full entry passes `desktop-file-validate` (rc 0):

```
[Desktop Entry]
Type=Application
Name=modman (Nexus Mods)
Comment=Handle Nexus Mods "Mod manager download" links
Exec=modman nxm %u
Icon=applications-games
Terminal=false
NoDisplay=true
MimeType=x-scheme-handler/nxm;x-scheme-handler/nxm-protocol;
```

- `modman nxm register` (T07, user-local `~/.local/share/applications/modman-nxm.desktop`, then `xdg-mime default` for both
  schemes, as in legacy 4230-4250) writes the same line with an absolute binary path: `Exec="/abs/path/modman" nxm %u`.
  - Under AppImage, the path is `$APPIMAGE`, not `current_exe()`, which points into a temporary `/tmp/.mount_*`.
  - Quote the path with double quotes, backslash-escaping `"`, `` ` ``, `$` and `\` (Desktop Entry spec, Exec key).
  - `%u` is always last, never quoted, and never followed by other arguments.
- Flatpak: export the packaged file unchanged. Flatpak rewrites `Exec` to `flatpak run --command=modman <app-id> nxm %u`.

### CLI -> GUI forwarding mechanism

D-Bus session-bus call to the endpoint that `tauri-plugin-single-instance 2.5.4` already serves in the GUI.
Source checked: `src/platform_impl/linux.rs`.

- Bus name: `org.modman.app.SingleInstance`. This is the tauri.conf.json `identifier` plus `.SingleInstance`. Do not enable the plugin's `semver` feature, which appends a version suffix.
- Object path: `/org/modman/app/SingleInstance`.
- Interface: `org.SingleInstance.DBus`.
- Method: `ExecuteCallback(argv: as, cwd: s)`.
- CLI argv sent: `["modman-desktop", "nxm", "<url>"]`.
- CLI side (T07): `zbus = "=5.19.0"` blocking API (zbus 5.x is already in the tree via the plugin and the keyring store), using `Connection::session()?.call_method(...)` with a 5 s timeout.
  The method returns only after the GUI callback has run, so a reply means the GUI accepted the link.
- GUI side (T13): the single-instance callback finds `nxm <url>` in argv. It treats argv as untrusted, validates the URL with the core nxm parser,
  enqueues it through the same core function as the paste-link box, starts the download, and focuses the window.
  The GUI is the only writer of that downloads row.
- Put the name, path, interface and method in core constants (e.g. `modman_core::ipc`). T13 adds a test that tauri.conf.json `identifier`
  is `org.modman.app`. Renaming the identifier silently breaks forwarding.
- Flatpak: an app with id `org.modman.app` may own `org.modman.app.*` names by default, so no extra `--own-name` is needed if T15 uses that app id.
- Log the URL with the query string stripped, because it holds `key` and `expires` (legacy logs `${url%%\?*}`, line 4278).

Prototype `scratchpad/t02/nxm-spike` (zbus 5.19.0; `fakegui` copies the plugin's interface exactly), run on an isolated `dbus-run-session`:

- GUI running: `GUI got argv=["modman-desktop", "nxm", "nxm://skyrimspecialedition/mods/266/files/1000?key=abc&expires=1&user_id=2"]`, then `CLI: forwarded to GUI`.
- No GUI: `org.freedesktop.DBus.Error.ServiceUnknown: The name org.modman.app.SingleInstance was not provided by any .service files`, then fallback.
- No session bus: `Failed to connect to address ... (os error 2)`, then fallback.

### Fallback when no GUI is running

This applies on `ServiceUnknown`, `NameHasNoOwner`, a connection error, or a timeout.

1. Parse and validate the URL in the CLI. An invalid URL gives a desktop notification plus a non-zero exit, as in legacy.
2. **Enqueue in the `downloads` table:** status `queued`, source `nxm`, game domain, mod id, file id, and key/expires with a TTL.
   The row is committed before any network I/O, so a killed process leaves a resumable row.
3. Process the row the way legacy `nxm_handle` does (4276-4305):
   - With a TTY: run the install in the foreground.
   - Without a TTY (browser launch): run with assume-yes, send notifications (`notify-send` as in legacy 157-160), and append the log.
   - A collection link without a TTY is only enqueued, with the notification "open modman to install collection".
4. On startup the GUI, and `modman download resume` in the CLI, drain `queued` rows.
5. Legacy `waiting.pid` handoff (free-account download waiting for a browser click): replace it with the waiting process polling
   `downloads` for a row matching (game, mod, file). No file queue.

Game Mode has no browser. The paste-link box in the GUI calls the same core enqueue function.

---

## 6. Other cross-cutting decisions

- **SQLite:** `rusqlite 0.40.2` with `bundled` (SQLite 3.53.2 compiled in, public domain). This avoids a runtime libsqlite dependency in AppImage/Flatpak.
  Enable WAL with `pragma_update(None, "journal_mode", "WAL")` (verified in deps-spike).
  **Migrations: a small hand-rolled runner, not `rusqlite_migration`.** The crate tracks versions in `PRAGMA user_version`, while tech stack §5 requires a
  `schema_migrations` table. Use embedded `include_str!("migrations/NNNN_name.sql")`. Apply each in its own transaction,
  insert `(version, name, applied_at)` into `schema_migrations`, and refuse to open a database whose version is newer than the binary knows.
- **TLS:** `reqwest 0.13.5` with `default-features = false`. Its default `rustls` feature selects aws-lc-rs, whose `aws-lc-sys`
  carries the OpenSSL licence and is treated as GPL-incompatible. It also needs cmake at build time. Use `rustls-no-provider` plus `rustls 0.23.46` with feature `ring`
  (`ring 0.17.14`, Apache-2.0 AND ISC). Call `rustls::crypto::ring::default_provider().install_default()` once before building a client.
  Verified: no `aws-lc-*`, `openssl-sys` or `native-tls` in the resolved tree. `openssl-probe` only locates system CA files.
- **thiserror:** move the workspace from `1` to `=2.0.21`. libloot already pulls in 2.x, so there is no duplicate major version.
- **flock / st_dev:** `rustix 1.1.5` features `fs`, `process` (`rustix::fs::flock`, `rustix::fs::stat().st_dev`). Do not add `nix`.
- **7z:** `sevenz-rust2 0.24.1`, the maintained fork. `sevenz-rust 0.6.1` is stale.
- **Mock HTTP:** `wiremock 0.6.5` (async, tokio). Do not add mockito.
- **Hashes:** `sha2 0.11.1` for the manifest and integrity checks. `md-5 0.11.0` for the Nexus md5 file lookup (legacy uses md5 39 times).

Build verification:

- `scratchpad/t02/deps-spike`: a two-crate workspace with every core and cli pin below. `cargo test --release` passed: 1 test, which runs a wiremock server, sends a reqwest+rustls(ring) GET returning 200, and queries SQLite with WAL. Build time 51.6 s cold.
- `scratchpad/t02/desk-spike`: `cargo check` with tauri 2.12.4 and tauri-plugin-single-instance 2.5.4 finished in 27.8 s. It includes the argv callback.
- Licence scan used `cargo metadata --filter-platform x86_64-unknown-linux-gnu` over the resolved tree.
  - Core+cli: 322 packages. All are MIT, Apache-2.0 (incl. WITH LLVM-exception), BSD-3-Clause, ISC, Zlib, Unicode-3.0, Unlicense, CC0-1.0, MIT-0,
    bzip2-1.0.6, MPL-2.0 (`option-ext`), GPL-3.0 (`esplugin`, `libloadorder`) or GPL-3.0-or-later (`libloot`).
  - Desktop: 325 packages. The only extra non-permissive licence is MPL-2.0 (`cssparser`, `cssparser-macros`, `dtoa-short`, `selectors`).
  - All of these are compatible with GPL-3.0-or-later. Apache-2.0 is GPL-3-compatible but not GPL-2-compatible, which is irrelevant here.
- deny.toml input for T15:
  - allow = the licences above.
  - ban `unrar_sys`, `aws-lc-sys`, `openssl-sys`, `native-tls`.

## 7. Pinned dependency table

Every version is exact and was published on crates.io as of 2026-10-20. Licences are as declared on crates.io.

### Core deps (`crates/modman-core`)

| Crate | Version | Features | Licence | Purpose |
| :--- | :--- | :--- | :--- | :--- |
| thiserror | =2.0.21 | | MIT OR Apache-2.0 | module error enums |
| tracing | =0.1.44 | | MIT | logging facade |
| serde | =1.0.229 | derive | MIT OR Apache-2.0 | |
| serde_json | =1.0.154 | | MIT OR Apache-2.0 | Nexus JSON, manifests |
| rusqlite | =0.40.2 | bundled | MIT | store.rs |
| reqwest | =0.13.5 | default-features=false; rustls-no-provider, http2, json, stream, charset, system-proxy | MIT OR Apache-2.0 | Nexus REST/GraphQL, downloads |
| rustls | =0.23.46 | default-features=false; ring, std, tls12, logging | Apache-2.0 OR ISC OR MIT | TLS provider (ring) |
| tokio | =1.54.0 | rt-multi-thread, macros, fs, io-util, sync, time | MIT | async runtime |
| zip | =9.0.5 | default-features=false; deflate, deflate64, bzip2, lzma, zstd, xz | MIT | .zip |
| sevenz-rust2 | =0.24.1 | | Apache-2.0 | .7z |
| compress-tools | =0.16.1 | (behind core feature `rar`, default on) | MIT OR Apache-2.0 | .rar via system libarchive (BSD-2-Clause) |
| reflink-copy | =0.1.30 | | MIT/Apache-2.0 | reflink deploy method |
| quick-xml | =0.42.0 | encoding | MIT | FOMOD ModuleConfig.xml (UTF-16 files are common) |
| toml | =1.1.8 | | MIT OR Apache-2.0 | game definitions, config |
| keyring-core | =1.0.1 | | MIT OR Apache-2.0 | credential API |
| zbus-secret-service-keyring-store | =1.0.1 | crypto-rust | MIT OR Apache-2.0 | Secret Service backend |
| keyvalues-parser | =0.2.4 | | MIT OR Apache-2.0 | libraryfolders.vdf / appmanifest acf |
| directories | =6.0.0 | | MIT OR Apache-2.0 | XDG paths |
| rustix | =1.1.5 | fs, process | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | flock, st_dev |
| sha2 | =0.11.1 | | MIT OR Apache-2.0 | hashes |
| md-5 | =0.11.0 | | MIT OR Apache-2.0 | Nexus md5 lookup |
| libloot | =0.29.7 | | GPL-3.0-or-later | LOOT sorting |
| url | =2.5.8 | | MIT OR Apache-2.0 | nxm/URL parsing |
| walkdir | =2.5.0 | | Unlicense/MIT | staging scans |

### Core dev-deps

| Crate | Version | Features | Licence |
| :--- | :--- | :--- | :--- |
| tempfile | =3.27.0 | | MIT OR Apache-2.0 |
| wiremock | =0.6.5 | | MIT/Apache-2.0 |
| tokio | =1.54.0 | macros, rt-multi-thread (for `#[tokio::test]`) | MIT |

### CLI deps (`crates/modman-cli`)

| Crate | Version | Features | Licence | Purpose |
| :--- | :--- | :--- | :--- | :--- |
| modman-core | path | | GPL-3.0-or-later | |
| anyhow | =1.0.104 | | MIT OR Apache-2.0 | binary errors |
| clap | =4.6.7 | derive | MIT OR Apache-2.0 | argument parsing |
| tracing-subscriber | =0.3.23 | env-filter | MIT | log output |
| tracing-appender | =0.2.5 | | MIT | log to file (`modman log`) |
| zbus | =5.19.0 | default-features=false; blocking-api, async-io | MIT | nxm forwarding to GUI |

CLI dev-deps: `assert_cmd =2.2.2` (MIT OR Apache-2.0), `predicates =3.1.4` (MIT OR Apache-2.0), `tempfile =3.27.0`.

### Desktop deps (`apps/desktop/src-tauri`)

| Crate | Version | Features | Licence | Purpose |
| :--- | :--- | :--- | :--- | :--- |
| tauri | =2.12.4 | | Apache-2.0 OR MIT | shell (already resolved in Cargo.lock) |
| tauri-build (build-dep) | =2.7.3 | | Apache-2.0 OR MIT | |
| tauri-plugin-single-instance | =2.5.4 | no `semver` feature | Apache-2.0 OR MIT | single instance + nxm forwarding endpoint |
| modman-core | path | | GPL-3.0-or-later | |
| serde | =1.0.229 | derive | MIT OR Apache-2.0 | |
| serde_json | =1.0.154 | | MIT OR Apache-2.0 | |

Not used:

- `tauri-plugin-deep-link` (2.6.2 is the latest 2.x): see section 5.
- The Tauri 3.0.0-alpha line: the plan targets v2.
- `keyring 4.2.0`, `unrar 0.5.8`, `rusqlite_migration 2.6.0`, `steamlocate 2.1.1`, `nix 0.31.3`, `mockito 1.7.2`, `sevenz-rust 0.6.1`: see the sections above.

MSRV of the pinned set: the highest is 1.93 (sevenz-rust2), followed by 1.90 (tauri 2.x). Installed rustc is 1.99.1.
