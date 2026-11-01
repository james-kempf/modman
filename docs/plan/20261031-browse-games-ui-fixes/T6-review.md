# T6 review: B4 renderer, open_url, steam_art, capabilities

Target: uncommitted working tree vs HEAD 4b2bfdc. Mode: high, scope: changed.
Verdict: **warning** (no blocking findings). Confidence 0.88.

## Verification run

- `npx vitest run src/components/rich src/views/browse.test.tsx src/views/Games.test.tsx src/views/GamesUnmanaged.test.tsx`: 4 files, 75/75 pass.
- `npm run typecheck`: clean.
- `cargo test -p vessel-core`: 491 passed, 0 failed. `src-tauri` tests: 12 passed (includes CSP test).

## Security criteria

| # | Criterion | Result | Evidence |
|---|-----------|--------|----------|
| S1 | No HTML injection in description rendering | PASS | `rich/bbcode.tsx`: tokenizer only builds elements for the `KNOWN` set (24-27); everything else is pushed as text (99, 104, 124); entities decode to text (37-44, 187); no `dangerouslySetInnerHTML`, no `style` (size/color map to classes, 148-166, 200-202); img/youtube become links (233-237). `rich/markdown.tsx`: `remarkSafe` turns `html` nodes into text before rehype (89-92), `allowedElements` limited to our component map with `unwrapDisallowed` (146, 153); components pass no attributes through (105-107). URLs: `ExternalLink.webUrl` allows only http/https with a host (ExternalLink.tsx 14-21); non-web hrefs render as `<span>` (27). |
| S2 | steam_art cannot fetch arbitrary URLs / read arbitrary paths | PASS | `commands.rs` `steam_art(appid: u32, kind: Option<ArtKind>)`; `art.rs::steam_art` calls `lookup(...)` with `fetch_image(..., allowed_url)`; cache names derived from `appid` only (art.rs 76, 133, 141). |
| S3 | open_url parses in Rust, opens http/https only; anchors do not navigate the webview | PASS (with warning W1) | `app/games.rs::open_web_url` uses `url::Url::parse`, requires scheme http/https and non-empty host, passes the re-serialized URL to the opener; tests reject file:, javascript:, data:, steam:, nxm:, vbscript:, ftp:, scheme-relative, hostless. Serialized URL always starts with `http`, so no argv option injection into xdg-open. Renderer: `onClick` and `onAuxClick` call `preventDefault` (ExternalLink.tsx 28-41). |
| S4 | capabilities diff adds only allow-steam-art and allow-open-url | PASS | `capabilities/default.json` diff is exactly two added lines; both commands also added to `command_names.rs` (generated `allow-*` via build.rs), `lib.rs` invoke_handler, and `ipc/commands.ts` COMMANDS. |
| S5 | `browse` about extension does not widen attack surface | PASS | `views.rs` `AboutTarget` is a tagged enum (`mod_id: u64` or `slug: String`), deserialization tested incl. non-numeric rejection. Both go to Nexus GraphQL as **variables** (graphql.rs `MOD_DESCRIPTION_QUERY` / `COLLECTION_DESCRIPTION_QUERY`, `json!` vars), domain via existing `check_domain`. No new host, path, or write. |

## Acceptance criteria

| AC | Result | Evidence |
|----|--------|----------|
| B1 | PASS | browse.test.tsx 245 "the Sort label sits left of the dropdown with the Segmented label style". |
| B2 | PASS | browse.test.tsx 260 (Details default, Mods tab), 302 (mod opens details; Back returns to Mods tab, focus on row). |
| B3 | PASS | browse.test.tsx 260 asserts `flex flex-col sm:flex-row` row and `min-w-0 flex-1` facts. |
| B4 | PASS | browse.test.tsx 409-473 (fetch on open + BBCode, links through open_url, empty/error/loading fallbacks, collection Markdown); rich.test.tsx; graphql.rs parse tests; views.rs `browse_request_reads_about_targets`. |
| G1 | PASS | Games.tsx 252 `ipc.steamArt(found.appid)`; GamesUnmanaged.test.tsx 127 "shows Steam art by app id, else a monogram"; `Detected.appid: u32`. |
| G2 | PASS | steam.rs tests `list_installed_apps_scans_every_manifest_in_every_library`, `list_installed_apps_finds_games_under_flatpak_steam`, `state_flags_installed_allows_pending_update_only`; games.rs `detect_installed_lists_non_preset_games_from_every_library`; GamesUnmanaged.test.tsx 145 unsupported badge + AddGameDialog prefill. Tool/runtime exclusion in steam.rs 181-209. |

## Findings

- **W1 (LOW, security, inference)** `apps/desktop/src/components/rich/ExternalLink.tsx:33-41`: anchors carry a real `href`; click and middle-click are cancelled, but no Rust-side navigation guard exists (`grep on_navigation` in src-tauri: none) and the app does not suppress the context menu (`grep contextmenu` in src: none). On WebKitGTK the default context menu ("Open Link", "Open Link in New Window") or dragging a link can still navigate the webview to an attacker-chosen http(s) page. The CSP does not govern top-level navigation. Impact is limited: remote origins are not granted IPC by the capability file. Not reproduced at runtime. Fix: add a `on_navigation` guard in `lib.rs` that allows only the app origin, or render links without `href` (button semantics), or cancel `contextmenu`/`dragstart` on links.
- **W2 (LOW, robustness)** `apps/desktop/src/components/rich/bbcode.tsx:200,202`: `SIZE[n.arg]` / `COLOR[arg]` index plain objects with untrusted keys, so `[color=constructor]` passes a function as `className` (React drops it with a dev warning). Not an injection path. Fix: `Object.hasOwn(COLOR, k) ? COLOR[k] : undefined`, or use a `Map`.
- **I1 (INFO)** `crates/vessel-core/src/app/views.rs` ~319: the new test was added between the comment "Fails when config_list stops carrying..." and its test `config_entries_carry_kind_and_options`, so that comment now sits above the wrong test.
- **I2 (INFO)** `apps/desktop/src/components/rich/About.tsx:180-181`: `game` is in the query key but not passed to `ipc.browseAbout`, which uses the backend's active game. This only matters if the active game changes while a dialog is open.

Over-engineering pass: no unrequested abstractions; two new deps (`react-markdown@10.1.0`, `remark-gfm@4.0.1`) are justified by the Markdown collection descriptions. No `regression_risk` estimate was supplied in the handoff. Reviewer estimate: medium, because `Detected.support` is now `Option<Support>` (IPC and CLI contract change). The CLI (`vessel-cli/src/commands/game.rs`) and TS types are updated, and the tests pass.
