# T17 gate report: parity and quality

- Plan: `20261020-rust-migration`, task T17 (critic review, integration target)
- Baseline: branch `rust-rewrite` at `1ccd93d`
- Date: 2026-10-21
- Toolchain: Rust at `~/.cargo/bin`, node 24, cargo-deny at `~/.cargo/bin`

## Final verdict: PASS (re-gated at `d7d14fd`, see §9)

The first gate at `1ccd93d` failed on two gaps, G1 and G2. T17F (`d7d14fd`) fixed them and the LOW findings F3-F7. A narrow re-gate verified each fix (§9). The rest of the original gate passed: the quality gates, the drop-row greps, the bundles, the AppImage smoke, the findings spot-checks and the T16 flow rerun.

Gaps from the first gate (both now **fixed** in `d7d14fd`):

| # | Gap | Severity | Evidence | Suggested fix |
| :--- | :--- | :--- | :--- | :--- |
| G1 | `modman purge` does not leave the Proton prefix byte-identical. `plugins.txt` is rewritten with modman's two fixed header lines (original 90 bytes -> 175 bytes). If the file did not exist before deploy, purge leaves a header-only `plugins.txt` behind. The game folder itself is byte-identical. This copies legacy `plugins_remove_managed` (legacy_spec §10), but it fails the T17 criterion "game folder and prefix byte-identical". | MEDIUM | §4 below, runs `run` and `run2` | On purge, when the remaining (non-managed) entries equal the pre-deploy file, restore the backed-up original byte for byte, or delete the file when there was none. Otherwise keep the current merge. Add a `core/loadorder` test that asserts byte identity. If this legacy behaviour is accepted instead, that is a change to the acceptance criterion, which the orchestrator has to approve. |
| G2 | `deploy -n` is rejected (`error: unexpected argument '-n'`). Legacy accepts `-n\|--dry-run` (legacy_spec §1.2, L1738-1747). This is the only unexplained gap in the CLI diff. | LOW | §3 below | Add `short = 'n'` to the deploy `--dry-run` arg, and add the case to cli/cli `every_legacy_alias_resolves_to_its_command`. |

## 1. Quality gates (all exit 0)

| Command | Result |
| :--- | :--- |
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0, no warnings |
| `cargo test --workspace` | exit 0. modman-cli: 12 unit + 88 integration (autoupdate 7, cli 28, collections 8, deploy 6, mods 21, nexus 15, plugins 3). modman-core: 259 unit + 95 integration (collections 6, deploy 36, download 10, loadorder 14, nexus 28, nexus_secrets 1). modman-desktop: 21. **Total 475 passed, 0 failed, 0 ignored.** |
| `cargo deny --all-features check licenses bans sources` | `bans ok, licenses ok, sources ok`. 2 `parse-error` warnings for a crate's non-SPDX licence string; they do not fail the check. |
| `npx vitest run` (apps/desktop) | 11 files, 110 tests passed |
| `npx tsc --noEmit` | exit 0 |
| `npm run build` | exit 0 (vite: index 332.87 kB, gzip 102.61 kB) |

## 2. Drop rows: grep evidence

These paths were searched for each pattern: `crates/`, `apps/desktop/src`, `apps/desktop/src-tauri/src` and `packaging/` (excluding node_modules).

| Drop row | Pattern | Hits | Hit locations |
| :--- | :--- | :--- | :--- |
| ui server | `server\.py`, `--port`, `no-open` | 0 | - |
| ui server | `TcpListener\|HttpServer\|axum\|warp\|actix\|tiny_http` | 11 | All in tests: HTTP mocks in `cli/tests/support`, `core/tests/download.rs`, `core/tests/nexus_secrets.rs`, `download/fetch.rs` tests, `desktop tests.rs:146`, plus the deny-list in `desktop tests.rs:49-59`. `hyper` appears in Cargo.lock only as the reqwest client. |
| ui-state/ui-detect/ui-thumbs | the three names | 1 each | Only `apps/desktop/src-tauri/src/tests.rs:59`, which asserts that they are absent |
| token server | `X-Modman-Token\|ui_token\|token server` | 0 | - |
| cron/autostart | `crontab\|cron\|autostart` | 3 | Only `core::autoupdate::tests::no_other_scheduler_code`, which asserts that they are absent |
| `--scheduler` | `scheduler` | 9 | Status labels "Scheduler :" in `cli/commands/autoupdate.rs`, plus tests asserting the flag is rejected (`no_scheduler_flag`) |
| v2->v3 migration | `migrate_v2\|v2_to_v3\|mods\.json` | 3 | Doc comments citing legacy line numbers in `core/src/mods.rs` only; legacy state only triggers a warning (`warns_when_legacy_state_exists`) |

Runtime checks: `modman ui` and `modman ui-state` print "Unknown command"; `modman autoupdate enable --scheduler cron` exits 2 ("unexpected argument").

Change row "no parallel Premium downloads": `jobs` is read only by `core::config` (definition and tests).

## 3. CLI diff against legacy_spec.md §1

Method: ran `target/debug/modman --help` and `<cmd> --help` for every command and subcommand under a temp HOME (`help.txt` in the scratchpad), then probed edge cases.

| Legacy item | Rust | Status |
| :--- | :--- | :--- |
| Global `-y/--yes`, `-g/--game`, `--game=<d>`, `--no-color`, `--debug`, `--`; globals accepted after the command | present (`modman list -y` works) | OK |
| help/`--help`/`-h`, version/`--version`/`-V`; unknown command exits 1 | present | OK |
| login, logout | present; login rejects arguments (key never in argv) | OK (Change, security) |
| game/games: list/ls, add `--subdir --profile`, use/switch/select, remove/rm `--delete-data`, info/show, set, detect | present | OK |
| config `[key [value]]` | present | OK |
| install/add/i: `--file\|-f`, `--fomod ask\|default\|none\|reuse`, `--name`, `--deploy`, `--optional`, `--revision\|-r`, `--download-only` | present | OK |
| download/dl `--file\|-f` + collection pass-through | present | OK |
| collection/collections/col: install/add/i, download/dl, list/ls, info/show, update/upgrade, remove/rm/uninstall `--keep-mods`; `--optional`, `--fomod collection\|ask\|default`, `--revision\|-r`, `--deploy`, `--download-only` | present. A bare collection ref as the subcommand is treated as install (verified: it reaches the login check). | OK |
| list/ls `-e -d -c` | present | OK |
| info/show | present | OK |
| enable/disable `--all\|-a`, `--collection\|-c` | present | OK |
| uninstall/remove/rm `--delete-archive\|--purge-archive` | present | OK |
| reinstall `--fomod`, `--ask` | present (`--fomod` now validated) | OK |
| order/move/mv with every spec | present | OK |
| conflicts `-f\|--files` | present | OK |
| deps/requirements/req `--refresh\|-r`, `--json`, ack/unack | present | OK |
| updates/update/outdated `-i\|--install`, `--all`, `--refresh` | present | OK |
| search/find `--json` | present (`--json` is accepted in any position, a superset) | OK |
| profile/profiles list/ls, save, load/use, delete/rm | present | OK |
| deploy `-n\|--dry-run`, `--method` | `--dry-run` and `--method` present; **`-n` missing** | **GAP G2** |
| purge/undeploy `--legacy` (no-op, A3) | `--legacy` rejected | Explained: v2-manifest flag, dropped with the v2->v3 Drop row |
| verify, status/st, launch/play/run, clean | present | OK |
| log/logs `-f\|--follow`, `[N]` (default 40) | present | OK |
| nxm register/unregister/status/`<url>`; bare `nxm://` link | present | OK |
| autoupdate/auto-update/au enable/on, disable/off, status, run `--force\|-f --dry-run\|-n`; `--scheduler` | present; `--scheduler` removed | OK (Change row) |
| ui/gui/web, ui-state, ui-detect, ui-thumbs | absent | OK (Drop rows; `ui desktop` is now packaging) |
| (new) `plugins` list/ls, sort, masterlist | present | New row |

Note (LOW, F5): `modman nxm register --help` performs the registration instead of printing help, because `nxm` takes free-form words. The same applies to `nxm unregister --help` and `nxm status --help`. The probe ran under a temp HOME, so nothing real was changed.

## 4. End-to-end bethesda scenario

Script: `e2e.sh` in the T17 scratchpad, with `mock.py` (a Python HTTP mock of `/v1/users/validate.json` and `/v1/games/skyrimspecialedition.json`; every other path returns 404) and `mkfix.py` (fixture zip with `E2ETestMod.esp` (TES4 header, master Skyrim.esm), `textures/sky.dds` overriding an original game file, `meshes/e2e/rock.nif`, and `readme.txt`).

Environment: `env -i` with a temp `HOME`, `XDG_RUNTIME_DIR` in the temp dir, a `DBUS_SESSION_BUS_ADDRESS` pointing at a socket that does not exist (so no real keyring and no GUI forwarding), `MODMAN_NEXUS_URL` and `MODMAN_LOOT_URL` set to the mock, and binary `target/debug/modman` built at `1ccd93d`. The fixture Steam root is `$HOME/.local/share/Steam` with `libraryfolders.vdf`, `appmanifest_489830.acf`, `steamapps/common/Skyrim Special Edition/{SkyrimSE.exe, Data/Skyrim.esm, Data/textures/sky.dds (mode 0640)}` and the prefix `compatdata/489830/pfx`, which has an existing `AppData/Local/Skyrim Special Edition/plugins.txt` (CRLF). `~/.steam` and the real game were never touched; the real `~/.local/share/modman` and `~/.config/modman` do not exist after the run.

| Step | Result |
| :--- | :--- |
| `login` (key on stdin) | PASS. Mock got `validate.json` with the apikey header; the key appears 0 times in output and logs. |
| `game detect` | PASS (lists skyrimspecialedition) |
| `-y game add skyrimspecialedition` | PASS. Folder and prefix found in the fixture root; the domain was checked against the mock. |
| `install <fixture zip>` | PASS. md5 lookup sent to the mock (404 = local mod). |
| `deploy` | PASS. `sky.dds` is overridden and `E2ETestMod.esp` is deployed. |
| `verify`, `status`, `plugins list` | PASS |
| plugins.txt in prefix | PASS. CRLF, contains `*Unofficial.esp` (kept) and `*E2ETestMod.esp` (added). |
| `purge` | PASS ("Removed 3, restored 1 original(s)"; "plugins.txt: removed 1 plugin(s) added by modman") |
| Game folder after purge | **Byte-identical**: same tree, file types, modes, sizes and sha256, and file mtimes are identical too. |
| Prefix after purge | **Not byte-identical**: `plugins.txt` is 90 -> 175 bytes, because the user's header comment was replaced by modman's two header lines; the entries are the same. Variant run2 (no `plugins.txt` before deploy): purge leaves a new 158-byte header-only `plugins.txt`. This is **G1**. |

Other profiles are not E2E-tested in v1, by plan decision.

## 5. Bundles and AppImage smoke

- `packaging/appimage/build-appimage.sh`: exit 0, produced `target/release/bundle/appimage/modman_0.1.0_amd64.AppImage` (133.5 MB).
- `npx tauri build --bundles deb,rpm`: exit 0, produced `target/release/bundle/deb/modman_0.1.0_amd64.deb` (21.9 MB) and `target/release/bundle/rpm/modman-0.1.0-1.x86_64.rpm` (21.9 MB). The only bundler warning is that the identifier `org.modman.app` ends in `.app`, which matters on macOS only.
- Contents of the deb and the rpm: `/usr/bin/modman`, `/usr/bin/modman-desktop`, `/usr/share/applications/modman.desktop`, `modman-nxm.desktop`. Deb Depends: `libarchive13t64 | libarchive13, libwebkit2gtk-4.1-0, libgtk-3-0`. RPM Requires: `libarchive`, gtk3, webkit2gtk-4.1.
- `<AppImage> --version` printed `modman 0.1.0`, exit 0. `<AppImage> status` under a temp HOME took the CLI path.
- GUI: a bare launch with `WEBKIT_DISABLE_DMABUF_RENDERER=1` and a temp HOME/XDG on DISPLAY :0 / wayland-0 was still running after 12 s (`modman-desktop` and `WebKitWebProcess` alive; the log in temp state says `GUI start (modman 0.1.0)`). It was then closed. Nothing was written to the real HOME.

## 6. Findings disposition

| Source | Finding(s) | Disposition | Regression evidence (passed in T17 run) |
| :--- | :--- | :--- | :--- |
| T08 (2 HIGH + medium/low, T10-1..12) | Deploy symlink escape; purge restore when content is identical; subdir/paks traversal; API key on cross-origin redirect; orphan blobs; extraction caps; non-UTF-8 names; 416 resume; credentials file symlink/owner; ProcProbe fail-open | **fixed** (T10 `ef60e01`; T10-5 in T07) | core/deploy `deploy_refuses_symlinked_folder_leading_outside_game_dir`, `..._outside_proton_prefix`, `folder_swapped_for_outside_symlink_after_preflight_is_refused_at_placement`, `purge_restores_original_when_hardlinked_mod_file_is_byte_identical`, `purge_restores_original_mode_when_copied_mod_file_is_byte_identical`, `crash_rollback_restores_original_when_mod_file_is_byte_identical`, `recovery_sweeps_orphaned_backup_blobs_but_keeps_held_ones`, `subdir_with_parent_component_is_refused`; core::archive `extraction_stops_at_file_count_and_size_caps`; core::staging `non_utf8_names_are_refused_not_mangled`, `paks_dir_with_parent_component_is_refused`; core/nexus `api_key_is_not_forwarded_on_cross_origin_redirect`, `symlinked_credentials_file_is_refused`; core/download `unverifiable_part_refused_with_416_is_discarded_not_accepted` |
| T13R (3 MEDIUM + 5 LOW) | Central subdir/paks_dir check; fetch size cap and redirect hook; thumbnail per-hop allow-list and cap; `$APPIMAGE` only under `$APPDIR`; nxm rejection echo truncated and escaped; prompts re-emitted on `app_ready`; mimeapps symlink | **fixed** (T13F `fbfbc5c`) | core::games `save_game_refuses_folders_outside_the_game`; core::app::games `add_rejects_a_subdir_outside_the_game_folder`, `add_rejects_a_traversal_typed_at_the_subdir_question`; core::download::fetch `an_oversize_stream_is_aborted_and_its_part_deleted`, `an_announced_oversize_body_is_refused_before_download`, `a_redirect_to_a_refused_host_is_not_followed`; core::app::thumbs `a_redirect_off_the_allowed_hosts_is_refused`, `an_oversize_picture_is_rejected`, `concurrent_fetches_of_one_picture_do_not_splice`; cli/nexus `nxm_register_ignores_an_inherited_appimage_variable`; cli/autoupdate `an_inherited_appimage_variable_does_not_replace_the_cli`; core::app::nxm `rejections_echo_a_short_escaped_link`; desktop::prompt `open_prompts_are_emitted_again_after_a_reload`; modman-cli nxm `writing_through_a_symlinked_mimeapps_keeps_the_link` |
| T15 (3 Rust findings) | GUI in an AppImage scheduled `/tmp/.mount_*`; DMABUF renderer default; Flatpak auto-update | **fixed** (T15F `26aac3e`) | core::app::actions `the_gui_in_an_appimage_schedules_the_image`, `the_gui_in_an_appimage_keeps_a_cli_outside_the_mount`, `an_inherited_appimage_variable_is_ignored_for_the_gui`, `enable_inside_the_flatpak_refuses_before_systemd`, `status_inside_the_flatpak_refuses`; desktop `dmabuf_renderer_is_disabled_only_when_unset`; cli/autoupdate `flatpak_sandbox_refuses_enable_status_and_run`; core::autoupdate `flatpak_info_at_the_root_refuses_with_a_clear_message` |
| T15 residual | `autoupdate disable` and the GUI startup check are not Flatpak-guarded; Settings shows the Flatpak error text | **accepted** (LOW; disable is harmless, startup check fails closed with a message) | none needed |
| T16 F1-F5 | Focus restore/recovery; busy buttons focusable; spatial leading-edge + rail penalty; OSK submit; About dt/dd; notice dedup/TTL/scroll-padding | **fixed** (T16F `4dda37c`) | vitest `Dialog.test.tsx` (focus restore, busy Button, focus recovery, OSK), `spatial.test.ts` (Settings layout scoring), `store.test.ts` (notices T16 F5), `Settings.test.tsx` (About dt/dd), `views.test.tsx` (Install OSK). **T17 reran** the T16 Playwright flows against the vite demo backend on a scratch copy: 30/30 ok; audit over 7 views had 0 axe issues, 0 unreachable, 0 traps, 0 overflow, no console or network errors; axe-dialogs had 0 issues. The committed `t16/flows.json` still holds the pre-fix 23/30 run (F6). |

New findings from T17:

| ID | Severity | Location | Finding | Disposition |
| :--- | :--- | :--- | :--- | :--- |
| F1 = G2 | LOW | `crates/modman-cli/src/commands/deploy.rs` (deploy args) | `deploy -n` missing | **fixed** `d7d14fd` (`deploy_short_n_is_dry_run`) |
| F2 = G1 | MEDIUM | `crates/modman-core/src/loadorder/plugins_txt.rs` (`remove_managed`, ~L478) | Purge does not restore `plugins.txt` byte-identical and leaves a header-only file when none existed | **fixed** `d7d14fd` (`purge_restores_original_plugins_txt_byte_identical`, `purge_deletes_plugins_txt_modman_created`, `purge_keeps_user_entries_in_modman_created_plugins_txt`; E2E re-run byte-identical) |
| F3 | LOW | `crates/modman-core/src/config.rs:84`, `crates/modman-core/src/app/nexus.rs:386` | The `jobs` help text and the Premium login message promise parallel downloads, which are not implemented (Change row) | **fixed** `d7d14fd` (texts now say sequential / no parallel) |
| F4 | LOW | `crates/modman-cli/tests/cli.rs:189` | Test name `later_milestone_commands_report_not_yet_implemented` is stale: it now asserts the opposite | **fixed** `d7d14fd` (renamed `former_stub_commands_are_implemented_and_bare_nxm_link_is_routed`) |
| F5 | LOW | `crates/modman-cli/src/commands/nxm.rs` | `nxm register --help` (and unregister/status) acts instead of printing help | **fixed** `d7d14fd` (`nxm_subcommand_help_prints_help_and_changes_nothing`; manual check: help printed, no `.desktop`/mimeapps written) |
| F6 | LOW | `docs/plan/20261020-rust-migration/t16/flows.json` | Committed evidence is the failing pre-fix run; the 30/30 rerun is not committed | **fixed** `d7d14fd` (`t16/flows.json` 30/30 ok) |
| F7 | LOW | Change row `config fomod` | Reading of the `fomod` config key has no behavioural test (code `app/install.rs:153-158` only) | **fixed** `d7d14fd` (`app::install::tests::fomod_config_key_sets_install_mode_unless_flag_given`) |
| F8 | INFO | data dir after purge | Empty `backups/<xx>/` shard dir and `staging/.tmp` remain after purge/install | accepted |

## 7. Residual risks (not verifiable in this environment)

- No real Nexus API: all network behaviour is tested against HTTP mocks (REST, GraphQL, CDN, 429/5xx, redirects). Schema drift on the live API is untested.
- No real Steam library and no SD-card deploy: detection is tested with fixture VDF/ACF files only, including the SD-card path. Hardlinks across real library mounts and ext4/exFAT SD cards are unverified.
- No Steam Deck: the Game Mode Gamepad API (webview gamepad events under Steam Input) and the Secret Service keyring in Game Mode are unverified. The controller UI was verified only with a 1280x800 viewport plus keyboard and emulated `navigator.getGamepads`.
- The LOOT masterlist licence for redistribution or caching is unverified. It is downloaded at runtime, not bundled.
- libarchive on SteamOS is unverified. The AppImage bundles its own copy; the deb and rpm depend on the distro package.
- Flatpak, RPM-spec and AUR recipes are not fully built: placeholder upstream URL, `sha256sums=SKIP`. Only the Tauri AppImage, deb and rpm were built.
- OAuth2/SSO login is not implemented (Later). Only the personal API key is supported.
- Only the bethesda profile (Skyrim SE) is end-to-end tested. Other presets are unit-tested and ship as experimental.
- The AppImage GUI smoke shows only that the process stays up and logs `GUI start`. Rendering was checked separately: T16 took a Tauri-window screenshot, and T17 reran the flows in the vite demo.

## 8. Gap list for orchestrator replan (first gate; all items closed by `d7d14fd`)

1. G1 / F2 (MEDIUM): make purge restore `plugins.txt` byte-identical when only modman entries changed, and delete the file when none existed before. Add a regression test. Alternatively, the orchestrator explicitly accepts legacy parity and amends the criterion.
2. G2 / F1 (LOW): add `-n` to `deploy --dry-run` and add it to the alias test.
3. LOW clean-ups, optional for v1: F3 text, F4 test rename, F5 `nxm <sub> --help`, F6 re-commit the T16 30/30 evidence, F7 a test for the `fomod` config key.

## 9. Re-gate at `d7d14fd` (T17F)

Scope: only G1, G2 and F3-F7, plus the Rust gates.

| Check | Result |
| :--- | :--- |
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo test --workspace` | exit 0: **481 passed, 0 failed** (+6 new tests) |
| G1 `e2e.sh` (prefix holds an original `plugins.txt`) | All steps PASS. During deploy, `plugins.txt` holds `*E2ETestMod.esp`. After purge, both the **game folder and the prefix are byte-identical** (tree, type, mode, size, sha256). Game file mtimes are identical; the restored `plugins.txt` has a new mtime (content identical; INFO only). |
| G1 `e2e_noorig.sh` (no `plugins.txt` before deploy) | All steps PASS. Game folder and prefix are byte-identical; the `plugins.txt` modman created is deleted, and all prefix mtimes are identical. |
| G1 tests | core/loadorder `purge_restores_original_plugins_txt_byte_identical`, `purge_deletes_plugins_txt_modman_created`, `purge_keeps_user_entries_in_modman_created_plugins_txt`: ok |
| G2 | cli/cli `deploy_short_n_is_dry_run`: ok. `modman deploy -n` now parses (it reaches "No game selected" instead of "unexpected argument"). |
| F3 | `config.rs` jobs help is now "reserved; downloads run one at a time"; the Premium login message is now "direct downloads available" |
| F4 | cli/cli `former_stub_commands_are_implemented_and_bare_nxm_link_is_routed`: ok (the old name is gone) |
| F5 | cli/nexus `nxm_subcommand_help_prints_help_and_changes_nothing`: ok. Manual check: `nxm register --help` printed help and wrote no `.desktop` or `mimeapps.list` under the temp HOME. |
| F6 | `t16/flows.json`: 30/30 `ok: true` |
| F7 | core::app::install `fomod_config_key_sets_install_mode_unless_flag_given`: ok |

Open after the re-gate: none blocking. F8 (INFO, empty backup shard and `.tmp` dirs) is accepted. The residual risks in §7 are unchanged.
