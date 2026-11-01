# T1 findings: Browse-5 / Browse-6 (read-only diagnosis)

## Browse-5: clicking a non-actionable area scrolls the view

### Root cause
`apps/desktop/src/nav/spatial.ts:208-219` (`installFocusRecovery`), together with `recoverFocus` at `spatial.ts:184-200`.

1. The user clicks a control (button, row with `tabIndex=0`). WebKit/Chromium focus it.
2. The user scrolls `<main>` with the wheel, then clicks blank space (card padding, text, gaps).
3. A mousedown on non-focusable content runs the unfocus steps. The focused control blurs, focus goes to `<body>`, and `focusout` fires with `relatedTarget === null`.
4. `onOut` returns early only for a target in a nav scope (dialog) or for a pointer-mode **text input** (`spatial.ts:211`). For a button or tabindex row it records `lastLost = box(target)`. That box uses viewport coordinates, and the control is now scrolled off-screen.
5. On the next frame `activeElement === body`, so `recoverFocus()` runs. It picks the focusable nearest to `lastLost`, which is the same control (distance 0), and calls `.focus()` (`spatial.ts:199`) without `preventScroll`.
6. `focus()` scrolls `<main>` (`App.tsx:111`, `overflow-y-auto`) to bring that control back into view. Controls clicked near the top of a view make this look like "scroll to top".

The bug is app-wide: any view, and Collections in particular, where the last-clicked control sits above the fold.

### Suspects cleared
- `nav/spatial.ts:167-168` (`moveFocus` scrollIntoView) runs only for arrow keys and the gamepad, not for clicks.
- `App.tsx:93` runs once on mount, and it focuses the tab rail, which is outside `<main>`.
- `nav/actions.ts` runs only for keyboard and gamepad actions.
- `ErrorBoundary.tsx:73-78` (`refocusView`) runs only after Try again.
- `Collections.tsx:86-90` runs only after a collection is removed.
- `tabIndex=0` containers (Mods/Activity/Browse `li`) do not cause it. Clicking them focuses them, so nothing is recovered.

### Repro
1. Open Collections (or Mods, Activity, Settings) with enough content to scroll.
2. Click any button or focusable row near the top (for example a Segmented option or the Refresh button).
3. Scroll down with the wheel.
4. Click empty space or plain text in a card.
- Expected: no scroll, and focus may drop.
- Actual: the view jumps back to the control clicked in step 2.

jsdom has no layout, so a unit test can assert the mechanism but not the scroll. Fire a pointerdown, then `focusout` on a focused button with `relatedTarget: null`, blur to body, and flush rAF. Today `document.activeElement` is the same button again.

### Minimal app-wide fix (spatial.ts only)
- In `installFocusRecovery.onOut`: when `dataset.input === "pointer"` and the target is still connected and not `:disabled`, return. The user deliberately clicked away, and the safety net exists only for disabled or unmounted controls. Generalize the text-input check at `spatial.ts:211` to this condition.
- In `recoverFocus`: call `.focus({ preventScroll: true })` and then `scrollIntoView({ block: "nearest" })` only if needed. This keeps recovery from jumping when it does run. Optional but recommended.
- Update tests: `nav/spatial.test.ts` (or wherever recovery is tested) needs a new case for "pointer click-away on a connected button does not refocus".
- **No change to Browse.tsx or store.ts is needed** for Browse-5.

---

## Browse-6: a loader-installed mod (SMAPI) is not flagged installed

### Data model
Loader installs never create a `mods` row (`app/install.rs:198-236`: "A loader archive never becomes a mod"). They live in the `loader_installs` table, which is written by `app/loaders.rs:1253-1281` (`run`, `Op::Install|Adopt` → `Store::insert_loader_install`, with `nexus_mod_id` from the `DownloadOrigin::Nexus`). An install is active when `status IN ('installed','running','interrupted')` (`loaders/records.rs:226`). Read it with `Store::active_loader_install(game_id, key)` (`records.rs:271`) or `Store::loader_installs(game_id)` (`records.rs:290`). For Adopt or local origins the row's `nexus_mod_id` is NULL. In that case the loader's Nexus id comes from the definition (`LoaderDef.nexus_mod_id`), or from `loaders::smapi::NEXUS_MOD_ID` as fallback (`app/loaders.rs:1715-1722`).

### Root cause: Browse flag
`crates/vessel-core/src/app/views.rs:198-201` builds `installed` only from `mods::list_mods(...).nexus_mod_id`. It never consults `loader_installs`, so SMAPI (Nexus 2400) gets `installed: false`.

### Root cause: Mods view
`crates/vessel-core/src/app/mods.rs:194-207` (`mods_view`) lists only `mods::list_mods`. `ModsView` (`mods.rs:187-192`) has no loader field, so an installed loader never appears. Its only UI is `LoaderCards` in Requirements (`views/Requirements.tsx:98`).

### Proposed fix
1. Add a helper to `crates/vessel-core/src/app/loaders.rs`:
   `pub fn installed_loaders(cx: &Cx<'_>, game: &GameConfig) -> R<Vec<InstalledLoader>>`
   `InstalledLoader { key, name, version: Option<String>, nexus_mod_id: Option<u64>, install_id: i64, status: String }`.
   - Iterate `registry_for(&LoaderEnv::new(cx.paths, &cfg.data_dir), game)?.keys()` and call `cx.store.active_loader_install(&game.id, key)`.
   - Set `nexus_mod_id` to `row.nexus_mod_id`, else the definition or SMAPI fallback, using the same name/id resolution as `views()` lines 1715-1722. Factor that into a small `fn loader_identity(def, key) -> (String, Option<u64>)` shared with `views()`.
   - Do not use `views()` here: it runs runtime detection, `steam_running()` and `detect()`, which is too heavy for Browse.
2. Browse flag in `views.rs::browse` (`BrowseKind::Mods`): extend `installed` with `installed_loaders(cx, game)?.iter().filter_map(|l| l.nexus_mod_id)`. `SearchHit` needs no type change. Optionally add `SearchHit.loader: Option<String>` (the key) so Browse can label the hit "Loader" and route its actions. That would need a Browse.tsx change at the `m.installed` branch (`Browse.tsx:523`) and an `ipc/types.ts` change.
3. Mods view: add `pub loaders: Vec<InstalledLoader>` to `ModsView` (`app/mods.rs:187`) and fill it in `mods_view`. Mirror it in `apps/desktop/src/ipc/types.ts` (`ModsView`) and render it in `views/Mods.tsx` as a separate "Loaders" group. Do not merge loaders into the `mods` array: they have no uid, priority, enable state or deploy files. Update the mocks in `mock/demo.ts` and the tests.
4. Uninstall path for loader rows: do **not** use `mods_uninstall`, which resolves `mods` rows and would fail or touch nothing. Use the loader pipeline that `LoaderCards.tsx:94-95,192` already uses:
   `ipc.loaderPreview({ op: "uninstall", loaderKey: key }, game)` → `openConsent({ game, preview })` → `LoaderConsent` → `loader_run(preview_id, plan_hash)`. That reaches `app/loaders.rs::preview` (`Op::Uninstall`, line 627) and then `run`, which restores backups and sets the row to `Uninstalled`.
   `LoaderConsent.tsx:104` already calls `refreshAll(qc)`, so the mods and browse queries refresh with no extra invalidation. Upgrade and Repair use the same path with `op` changed.
5. Tests: in `app/loaders/pipeline_tests.rs`, after an SMAPI install run, assert that `installed_loaders` returns 2400 and that `mods_view(...).loaders` is non-empty. The `browse` flag test needs a mocked Nexus. Follow the existing views tests.

### Files requiring change
- Core: `crates/vessel-core/src/app/loaders.rs`, `app/views.rs`, `app/mods.rs`.
- Frontend: `apps/desktop/src/ipc/types.ts`, `views/Mods.tsx`, `mock/demo.ts` (plus tests). Browse.tsx only if the optional `loader` label is added. store.ts needs no change.
