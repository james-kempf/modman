# Toast message audit (T03)

Standard: ux-microcopy (what happened + why + what next; past-tense verb + object; no generic words) and
ux-feedback-affordance (toast for transient results; errors that need action belong on a lasting surface).
All notices now render in the app bar's toast slot (one truncated line, full text in the tooltip and DOM).

**T05 update: toasts are informational only.** They have no controls and close on their own (6 s info and
success, 10 s warn and error). Removed actions: **Undo** on uninstall, bulk uninstall and profile load (with
its "before-<name>" snapshot), **Deploy now** (game-scoped deploy-due notice; the Deploy control on Overview
stays), and the close button. The 15 s "has action" timing and the focus-hold pause went with them. Messages
that existed only for Undo were removed (see below). Every other text is unchanged.

Totals (T03): 48 entries (the duplicated clipboard text counted once). Rewritten 23 (one of them, Play, changes tone only), kept 25, removed 0, moved to the app bar 48 (all). T05: 2 rows removed with Undo (restore results, undo of profile load); 1 row lost its action (profile load).
"Should not be a toast" recommendations: 5. Each is listed below with what was done.

Paths are relative to `apps/desktop/src/`.

## Shared formats

| Source | Old text | New text | Reason |
|---|---|---|---|
| `app/run.ts` useRun catch | `<Label>: <backend message>` | `<Label> failed: <backend message>` | `Install: mod not found` did not say it failed; every command error goes through this path (labels: Install, Uninstall, Move, Verify, Purge, Sign in, Save profile, Switch game, Enable/disable, …). |
| `app/run.ts` useRun catch | `<Label> cancelled.` | kept | Clear, names the action. |
| `components/loaders/LoaderCards.tsx:57` | `<key> cancelled.` | kept | Same pattern as above. |
| `app/run.ts` deployedText | `Deployed N files from M mods, X conflicts, Y skipped, plugins.txt failed: D` | `…, couldn't write plugins.txt: D` | "plugins.txt failed" did not say what failed. |
| `app/run.ts:178` deploy | `<Game>: <deployedText>` | kept | Names the game; the deploy can target a non-active game. |
| `app/run.ts:67` applyAfterChange | `<deployedText>` | kept (uses the rewrite above) | |

## App shell and bridge

| Source | Old text | New text | Reason / recommendation |
|---|---|---|---|
| `app/bridge.ts:35` nxm rejected | `nxm link rejected: <err or "unknown">` | `Couldn't start the Nexus download: <err>` / `Couldn't start the Nexus download.` | Jargon ("nxm link"), and "unknown" said nothing. |
| `app/bridge.ts:52` | `Backend not ready: <err>` | `Vessel's backend didn't start: <err>. Restart Vessel.` | Says what to do next. **Should not be a toast**: the app does not work at all after this, and the toast vanishes after 10 s. Recommend a persistent ErrorNote/banner. Not applied (it needs a new shell state; out of scope). |
| `app/bridge.ts:64` | `A loader install was interrupted. Open Requirements to roll it back or keep it.` | kept | Already says what happened and what to do. **Should not be a toast**: it needs action and times out. The Requirements loader card already shows the interrupted install with Roll back / Keep files, so the lasting surface exists and the toast only points to it. Kept as a pointer. |
| `app/store.ts:311` orphaned prompts | `A question from job #N closed: the job ended.` / `Questions from job #N closed: …` | `Job #N ended, so its question was closed.` / `… its questions were closed.` | Cause first, then the result. Reads as one sentence. |
| `components/dialogs/PromptDialog.tsx:43` | `Answer not delivered: <err>` | `Couldn't send your answer: <err>` | Plain voice, says what failed. |
| `components/shell/Osk.tsx:58` | `Clipboard not readable here; paste into the field with Ctrl+V.` | `Can't read the clipboard here. Paste with Ctrl+V instead.` (`CLIPBOARD_BLOCKED`) | Shorter for the bar. Was duplicated in Settings; now one constant. |

## Overview

| Source | Old text | New text | Reason |
|---|---|---|---|
| `views/Overview.tsx:125` Play | `Starting <Game> through Steam.` (success) | same text, tone `info` | Nothing has succeeded yet (Steam starts the game later), so success tone overstated it. Kept as a toast: the launch has no visible result in Vessel for several seconds. |
| `views/Overview.tsx` purgedText | `Purged: removed N files, restored M originals, …` | `Purged N files, restored M originals, …` | Dropped the colon/verb doubling. |
| purgedText (nothing deployed) | `Nothing was deployed; nothing to purge.` | `Nothing to purge: no mods are deployed.` | Result first, then the reason. |
| purgedText plugins | `plugins.txt failed: D` | `couldn't write plugins.txt: D` | Same as deployedText. |

## Game repair (Settings)

| Source | Old text | New text | Reason / recommendation |
|---|---|---|---|
| `views/GameRepair.tsx:59` | `Quarantined N backup(s) and M game file(s) as ID.` | `Quarantined 1 backup and 3 game files as ID.` (via `count`) | "(s)" plurals. |
| `views/GameRepair.tsx:60` | `Nothing to quarantine.` | `No poisoned backups to quarantine.` | Names what was looked for. |
| `views/GameRepair.tsx:66` | `Restored quarantine ID: N backup(s), M game file(s).` | `Restored quarantine ID: 1 backup, 2 game files.` | Plurals. |
| `views/GameRepair.tsx:87` | `Restaged N mod(s) of <Game>. Deploy to apply.` | `Restaged 2 mods of <Game>. Deploy to apply.` | Plurals. |
| `views/GameRepair.tsx:88` | `Nothing to restage: no mod is staged in an outdated layout.` | kept | Clear. |
| `views/GameRepair.tsx:89` | `No archive kept for A, B; reinstall these mods.` | kept | Says what to do. **Should not be a toast**: it lists mods the user must reinstall and times out after 10 s. Recommend a result list in the Restage card. Not applied (new UI). |
| `views/GameRepair.tsx:90` | `Restage failed for uid (err), …` | `Couldn't restage uid (err), ….` | Plain voice. Same recommendation as above. |

## Mods and profiles

| Source | Old text | New text | Reason |
|---|---|---|---|
| `views/Mods.tsx` uninstalledText | `Uninstalled X` / `… and deleted its archive` / `Uninstalled N mods …` / `X was not uninstalled.` | kept | Verb + object + count. |
| `views/Mods.tsx:936` | `Reinstalled X (N files)` | kept | |
| `views/Mods.tsx` updatesInstalledText | `Installed N update(s)` / `No updates were installed.` | kept | |
| `views/Mods.tsx` profileLoadedText | `Loaded profile X; …` | kept, Undo action removed (T05) | Plain informational text. |
| `views/Mods.tsx` undo of a profile load (deleted, T05) | `Restored the mod set from before loading X` | removed | Undo is gone. |
| `views/Mods.tsx:1326` | `Profile X was not overwritten.` | kept | |
| `views/Mods.tsx:1329` | `Saved profile X (N mods)` | kept | |
| `views/Mods.tsx:1402` | `Deleted profile X` | kept | |
| `views/mods/restore.ts` (deleted, T05) | `Restored X` / `Restored N mods`; `Couldn't restore …`; `No Undo for X: <reason>.`; backend restore `warnings[]` | removed | Undo is gone, so these can no longer appear. Uninstall toasts are now `uninstalledText` only. |

## Browse, collections, loaders

| Source | Old text | New text | Reason |
|---|---|---|---|
| `components/browse/ModActions.tsx` installedText | `Installed X (N files, replaced Y)`; `Collection S: …` | kept | |
| installedText fallback | `Nothing installed` | `Nothing was installed.` | Full sentence, consistent with "No updates were installed." |
| `views/Collections.tsx` collectionText | `S is up to date`, `S: N installed, M already present, …`, `S: cancelled` | kept | Compact enough for one line. |
| `views/Collections.tsx` removedText | `Removed collection S (and N mods)` / `Collection S was kept.` | kept | |
| `components/loaders/LoaderCards.tsx:92` | `<Label>: nothing to review.` | kept | |
| `components/loaders/LoaderCards.tsx:106` | `Launch options set in Steam: V` / `… (cleared)` | `Set the Steam launch options to V` / `Cleared the Steam launch options.` | Verb first; "(cleared)" was a placeholder value. |
| `components/loaders/LoaderCards.tsx:207` | `Rolled back; could not restore: A, B` / `Rolled back.` | `Rolled back the install, but couldn't restore A, B` / `Rolled back the interrupted install.` | Names the object. |
| `components/loaders/LoaderCards.tsx:281` | `Launch options copied.` | `Copied the launch options.` | Verb-first like every other result. Kept as a toast: copying has no visible result. |
| `components/loaders/LoaderCards.tsx:371` | `Removed N leftover loader archive(s).` | kept | Already pluralised. |

## Settings

| Source | Old text | New text | Reason / recommendation |
|---|---|---|---|
| `views/Settings.tsx:88` | `NEXUS_API_KEY is still set in the environment.` | `NEXUS_API_KEY is still set, so Vessel still uses it. Unset it to sign out.` | Said what but not why it matters or what to do. **Should not be a toast**: the account chip and the Settings account card keep showing the signed-in state, which is the lasting surface. Kept as a toast to explain why sign-out "didn't work". |
| `views/Settings.tsx:125` | `Signed in as U (Premium); key stored in the keyring/<file>` | kept | |
| `views/Settings.tsx:126` | `Keyring unavailable: E` | `Couldn't use the system keyring: E` | Plain voice. |
| `views/Settings.tsx:160` | clipboard (duplicate) | `CLIPBOARD_BLOCKED` | See Osk. |
| `views/Settings.tsx:243` | `Added/Removed Vessel to/from Steam. Restart Steam to apply it.` | kept | |
| `views/Settings.tsx:400` | `<key> = <value>` | `Set <key> to <value>` | Read like code. **Possibly noise**: the field already shows the new value. Kept, because the change only takes effect after the backend accepts it. |
| `views/Settings.tsx:401` | `Old data is still in <path>` | kept | **Should not be a toast**: a path the user may need later. Recommend showing it in the config row. Not applied. |

## Backend and demo strings (not edited)

These reach the toast verbatim after `<Label> failed: `. They live in Rust (`crates/`) or mirror it in
`mock/demo.ts`: `no game '<id>'`, `no mod '<ref>'`, `bad move '<spec>'`, `profile name is empty`,
`no profile '<name>'`, `collection S is not installed`, `unknown config key 'K'`, `invalid value 'V' for K`,
`D is already added`, `<id>: no game folder is set`, `FOMOD installer cancelled`. They are terse but
accurate. Not changed (constraint: no backend string edits unless clearly wrong).

## Timing

Unchanged: info/success 6 s, warn/error 10 s (the 15 s action timing went with the actions in T05).
ux-feedback-affordance says errors should stay until acknowledged; the user chose auto-closing,
control-free toasts instead (T05).

T06: there is only one toast. Each message replaces the current one at once, in the same pill, with a fresh
timer for its own tone; the queue (`MAX_NOTICES`, `+n`) is gone. Consequence: when two messages come
back to back (e.g. a collection install that deploys: "Deployed N files…" then the collection result), only
the last stays on screen. The earlier one shows only briefly (not at all when both arrive in the same
tick); the job output stays in Activity.

## T07: one toast state, one API, last write wins

- Cause of the T06 rejection: there were still two toast sources with a priority rule. The notice
  (`notify`) and the job island (derived from job events, with a 5 s linger) shared the slot, and the
  job pill was shown only when no notice was up. So job progress waited behind a message's expiry, and
  job updates neither replaced the message nor reset its timer.
- Now: one `toast` state, written only through `showToast(tone, text, opts?)` (`app/toast.ts`). Every
  former `notify` call site (14 files) uses it, and so do the job events (`onJob`, `onActivity`,
  `onDeployProgress`). The separate JobIsland pill, its 5 s linger and the priority logic are deleted.
- New job texts (all through the same API): "<Label>" + "Starting…" / latest output line /
  "deploying n/total" (running, busy disc); outcome "<Label>" + "Finished" (success),
  "<Label> cancelled" (info), "<Label> failed" + "<error> · see Activity" (error). Labels come from
  `jobLabel` (e.g. "Install", "Uninstall mods", "Deploy").
- Throttle: job progress writes at most once per 250 ms (first at once, then one trailing write of the
  latest). The outcome is never throttled.
- Outcome rule (T08, user decision): messages show in arrival order. A job's generic outcome
  replaces whatever is up, including a command result that arrived first (e.g. "Uninstalled SkyUI"
  then "Uninstall mods · Finished"). The only ordering guard left: an outcome cancels a pending
  throttled progress write, so no progress line lands after its job's outcome.

## T09: no Activity tab counter

The running-jobs + prompts counter next to the Activity tab label is removed (and the Tabs `badge` slot it
alone used). The toast is now the only live indicator of running work; the Activity view still lists every job,
and the deploy progress beside the game name stays.

## T10: Verify result moved from Overview to the toast

Verify (`views/Overview.tsx`, `reportVerify`) used to render its result inline under the Verify and Purge
buttons. It had no running or error text inline: the button showed busy, and errors already went to the toast
as "Verify failed: …" via `useRun`.

| Source | Old (inline) | New (toast) | Reason |
|---|---|---|---|
| Verify, clean | `N files intact, 0 issues` (green line with a shield icon) | success: `Verified N files: all intact` | Says the outcome first; "0 issues" was noise. |
| Verify, issues | `N files intact, M issues` (amber line) | warn: `Verify found M issues (N files intact)`, detail `<kind>: <path> +M-1 more · see Activity` | Issues lead; the first issue fits the detail line. |
| Verify, issue list | up to 50 `<kind>: <path>` lines in a scroll box | each issue logged to Activity → Messages as `Verify: <kind>: <path>` (warn) | Verify is a query, not a job, so no job log held the list. It has no fix buttons (informational), so nothing actionable is lost; the full list stays readable in Activity, and is no longer capped at 50. |

## T12: minimum dwell (200 ms)

User request; it changes the toast store's display logic, not just its look. `toast-min-dwell` = 200 ms
(DESIGN.md §8, `TOAST_MIN_DWELL_MS` in `app/store.ts`). Each message stays on screen at least 200 ms. A write
while the shown one is younger than that waits in a FIFO and shows when the 200 ms are up, and each waiting
message then gets its own 200 ms. A write when the shown one is older shows at once. The auto-close TTL starts at
show time. `closeToast` ignores anything but the shown id, so no waiting message is dropped. Backlog: at most 5
waiting (`TOAST_QUEUE_MAX`). A job's progress update merges into the latest (it replaces that job's waiting
update and re-queues at the end), a job's outcome drops its own waiting progress, and over the cap the oldest
waiting progress updates go. Starts, outcomes, errors and other messages are never dropped. Worst-case lag: none
for a single job (its progress writes every ≥250 ms, more than the 200 ms dwell). In bursts, waiting progress is
bounded by the cap, so its lag is at most about 5 × 200 ms = 1 s. Non-progress messages are never dropped, so a
burst of N of them within 200 ms delays the last one by about N × 200 ms; they come from user actions, so N
stays small.

## T13: dwell raised to 300 ms

`toast-min-dwell` = 300 ms (`TOAST_MIN_DWELL_MS`). It is now longer than the 250 ms job-progress throttle. A
single job's next progress update can therefore arrive while the previous one is still in its dwell. It waits,
and any later update of the same job merges into it (it replaces the waiting one, latest content). So at most one
progress update per job is ever waiting, and a single job's progress shows about every 300 ms with its latest
line. Updated worst-case lag (supersedes the T12 numbers):
- Single job: at most one dwell (≤ 300 ms) between its latest line and that line showing; no growth over time
  (tested: 24 lines at 250 ms intervals show fewer than 24 times, at least 300 ms apart, and the last line shows
  within 300 ms of being written).
- Burst of several jobs: waiting progress is capped at 5 (`TOAST_QUEUE_MAX`), so at most about 5 × 300 ms = 1.5 s.
- Non-progress messages (starts, outcomes, errors, results) are never dropped: N of them within 300 ms delay
  the last by about N × 300 ms.
- First message: its dwell counts from mount, and the open reveal is also 300 ms, so the pill is fully open
  when the dwell ends.

## T14: one completion toast per deploy

Found: a successful deploy wrote two completion toasts. One is the command result from `useDeploy`
(`app/run.ts`), "<Game>: Deployed N files from M mods[, …]" (warn when plugins.txt failed). The other is the
generic job outcome from the job writer (`store.ts` jobToast), "Deploy · Finished". They arrive in whichever
order the backend's job event and the IPC reply land, and with the dwell both show for 300 ms each.
Kept: the command result, which names the game and the counts. The job writer now writes no "Finished" for
`deploy` jobs. A deploy's failure ("Deploy failed · <error> · see Activity") and cancellation still come from
the job writer. Every other job keeps the generic outcome, in arrival order (no general skip rule).

Other jobs that still produce two completion toasts (command result + "<Label> · Finished"), not changed:
- `mods_uninstall` ("Uninstalled X"), `mod_reinstall` ("Reinstalled X"), `install` from Browse ("Installed X")
- `purge` ("Purged N files…"), `launch` ("Starting <Game> through Steam.")
- `profile_load` ("Loaded profile X"), `profile_save` ("Saved profile X (N mods)"), `updates_install` ("Installed N updates")
- `collection_install` / `collection_update` (collection result), `collection_remove` ("Removed collection …")
- `backups_clean` ("Quarantined …"), `quarantine_restore` ("Restored quarantine …"), `restage` ("Restaged …")
- `loader_junk_remove` ("Removed N leftover loader archives."), `loader_rollback` ("Rolled back the interrupted install.")
- `mods_set_enabled` / `mod_move` only when they auto-deploy ("Deployed N files…" from applyAfterChange).
Single completion (generic outcome only): `game_add`, `game_remove`, `updates_check`, `plugins_masterlist`,
`plugins_sort`, `loader_run`, `mod_restore` (the UI no longer calls it).

## T15: short title + detail

Every toast now has a 1-2 word generic title (`text`) and the specifics in `detail`. Vocabulary and rules:
DESIGN.md §Notice "Copy" and `app/toastText.ts`. The active game's name is dropped (the app bar shows it).
Another game's name (a deploy started for a non-active game) leads the detail.

| Source | Old text | Title | Detail |
|---|---|---|---|
| deploy result (`deployedMsg`, useDeploy / applyAfterChange) | `<Game>: Deployed N files from M mods, X conflicts, Y skipped, couldn't write plugins.txt: …` | Deployed | `N files · M mods · X conflicts · Y skipped · couldn't write plugins.txt: …` |
| deploy progress (job writer) | Deploy · `deploying n/total` | Deploying | `n / total files` |
| job start / progress | `<Label>` · Starting… / line | `<-ing>` (e.g. Installing, Updating; "Working" if unknown) | `<Label>` / the latest output line |
| job finished (not deploy) | `<Label>` · Finished | `<past>` (e.g. Purged, Uninstalled; "Finished" if unknown) | `<Label>` |
| job failed | `<Label> failed` · `<error> · see Activity` | `<Label> failed` (one-word label) / Failed | `[<Label> · ]<error> · see Activity` |
| job / command cancelled | `<Label> cancelled[.]` | `<Label> cancelled` / Cancelled | `[<Label>]` |
| command error (`useRun`) | `<Label> failed: <msg>` | `<Label> failed` / Failed | `[<Label> · ]<msg>` |
| question closed (store) | `Job #N ended, so its question was closed.` | Question closed | `Job #N ended, so its question was closed` |
| nxm rejected (bridge) | `Couldn't start the Nexus download: <err>` | Download failed | `<err>` (or "Nexus Mods refused the download link") |
| backend not ready (bridge) | `Vessel's backend didn't start: <err>. Restart Vessel.` | Startup failed | `Vessel's backend didn't start: <err> · Restart Vessel` |
| loader interrupted (bridge) | `A loader install was interrupted. Open Requirements …` | Install interrupted | `A loader install was interrupted · Open Requirements to roll it back or keep it` |
| answer not sent (PromptDialog) | `Couldn't send your answer: <err>` | Answer failed | `Couldn't send your answer: <err>` |
| clipboard (Osk, Settings) | `Can't read the clipboard here. Paste with Ctrl+V instead.` | Paste blocked | same sentence |
| play (Overview) | `Starting <Game> through Steam.` | Launching | `Starting the game through Steam` |
| purge (`purgedMsg`) | `Purged N files, restored M originals, …` / `Nothing to purge: no mods are deployed.` | Purged / Nothing deployed | `N files · M originals restored · …` / `Nothing to purge: no mods are deployed` |
| verify (`reportVerify`) | `Verified N files: all intact` / `Verify found M issues (N files intact)` | Verified / Issues found | `N files · all intact` / `M issues · N intact · <first> +k more · see Activity` |
| quarantine / restore / restage (GameRepair) | `Quarantined …`, `No poisoned backups …`, `Restored quarantine …`, `Restaged N mods of <Game>. Deploy to apply.`, `Nothing to restage: …`, `No archive kept for …`, `Couldn't restage …` | Quarantined / Nothing found / Restored / Restaged / No changes / Reinstall needed / Restage failed | counts, id, `Deploy to apply`, the reason / mod list (game name dropped) |
| collections (`collectionMsg`, `removedMsg`) | `abc: cancelled`, `abc is up to date`, `abc: N installed, …`, `Removed collection abc and N mods`, `Collection abc was kept.` | Cancelled / Already current / Installed / Removed / Kept | `Collection abc[ · N installed · M already present · …]` |
| install from Browse (`installedMsg`) | `Installed X (N files, replaced Y)`, `Collection abc: …`, `Nothing was installed.` | Installed / Cancelled / Nothing installed | `X · N files · replaced Y`, `Collection abc · …` |
| uninstall (`uninstalledMsg`) | `Uninstalled X[ and deleted its archive]`, `Uninstalled N mods[ and deleted K archives]`, `X was not uninstalled.` | Uninstalled / Kept | `X[ · archive deleted]`, `N mods[ · K archives deleted]`, `X was not uninstalled` |
| reinstall | `Reinstalled X (N files)` | Reinstalled | `X · N files` |
| updates (`updatesInstalledMsg`) | `Installed N updates` / `No updates were installed.` | Updated / No updates | `N updates installed` / `Nothing was installed` |
| profiles (Mods) | `Loaded profile X; …`, `Profile X was not overwritten.`, `Saved profile X (N mods)`, `Deleted profile X` | Loaded / Kept / Saved / Deleted | `Profile X[ · …]`, `Profile X was not overwritten`, `Profile X · N mods`, `Profile X` |
| loaders (LoaderCards) | `<label>: nothing to review.`, `Set the Steam launch options to V` / `Cleared …`, `Rolled back the install, but couldn't restore …` / `Rolled back the interrupted install.`, `Copied the launch options.`, `Removed N leftover loader archives.` | No changes / Options set / Options cleared / Rolled back / Copied / Removed | `<label>: nothing to review`, `Steam launch options: V`, `Couldn't restore …` / `The interrupted install`, `Launch options`, `N leftover loader archives` |
| settings | `NEXUS_API_KEY is still set …`, `Signed in as U (Premium); key stored in …`, `Couldn't use the system keyring: E`, `Added/Removed Vessel to/from Steam. Restart Steam …`, `Set K to V`, `Old data is still in P` | Key active / Signed in / Keyring unavailable / Added / Removed / Setting saved / Old data | the sentence; `U (Premium) · key stored in …`; `E`; `Vessel added to Steam · Restart Steam to apply it`; `K = V`; `Still in P` |

Every message was converted; none were left out of the rule. Three titles that would have been 3 words were
shortened ("Up to date" became "Already current", "Nothing to restage" and "Nothing to review" became "No
changes"). Backend error strings go into the detail unchanged.

## T16: live (persistent) toasts for ongoing processes

- API (`app/toast.ts`): `showProgress(key, tone, title, opts?)` starts or updates a live toast, and
  `endProgress(key, outcome?)` ends it. The outcome is `{ tone, text, opts }`, a normal transient toast that
  takes the live toast's place. `showToast` is unchanged and stays transient.
- Persistent processes: every backend job, keyed `job:<id>`. That covers install (including nxm and loader
  installs), uninstall, reinstall, deploy, purge, collection install/update/remove, profile load/save, updates
  check/install, plugins sort/masterlist, quarantine, restage, game add/remove and launch. Their start, output
  lines, deploy progress (`n / total files`) and download progress (`<file> · <done> / <total>`, from the
  existing `vessel:download` started/progress events with byte counts) update the live toast in place. Downloads
  belong to their job (install, nxm or collection), so they update that job's toast. Verify is a query, not a
  job, and stays a transient result.
- Throttle: removed. In-place updates have no motion, so they stay readable, and each event is a single small
  store write.
- Transients interrupt: a transient takes the slot for its dwell and TTL, and FIFO transients follow. The live
  toast returns when the last transient closes, with its latest state and one cycle. Updates while hidden are
  applied, not queued.
- Concurrency: the most recently started live toast shows, with "· +n running" in its detail; when it ends, the
  next most recent returns.
- Outcomes: unchanged texts (T15). A deploy's success still writes no generic outcome (T14). A job whose start was
  missed still gets its outcome toast.
- The FIFO backlog cap (T12/T13) is removed: it only ever dropped progress updates, and progress no longer enters
  the FIFO. The worst-case lag of a single job's progress is now 0: it is in place and immediate while shown.

## T17: no progress bar beside the toast

Removed: the deploy progress in the app bar's game column (`GameContext` in `App.tsx`), right next to the toast
slot. It was a `<progress aria-label="Deploy progress" class="w-24">` and the mono text "deploying n/total",
shown while a deploy ran. It was the only progress element outside the pill in the app bar; the pill's own busy
disc (three animated rails) stays. Its now-dead state was removed with it: the store's `deployProgress` and
`deployGame` / `setDeployGame` (`useDeploy` no longer sets them). `onDeployProgress` stays and only updates the
deploy job's live toast ("Deploying" · "n / total files"), which is now the only progress indicator. This replaces
the T09 and T16 notes above that said the deploy progress beside the game name stays.
