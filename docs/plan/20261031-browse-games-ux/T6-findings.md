# T6 findings: 2 frontend test failures after T5 (read-only)

Reproduced with `npx vitest run src/app/App.test.tsx src/style.test.ts`: 2 failed, 20 passed.

Evidence came from file mtimes, `git diff HEAD`, the validation logs, and the subagent transcripts for this session. Only T3 wrote to `store.ts`, at 18:32Z, and T3 did not change `VIEWS`. No agent wrote to `index.css` or `style.test.ts`.

## (a) App.test.tsx PageDown/PageUp: preexisting_user_edit (made mid-run, outside the plan)

- Introducing change: `apps/desktop/src/app/store.ts:21-22`. The working tree puts `browse` before `mods`. HEAD (09056e1) has `mods` and then `browse`.
- T3 read `store.ts` at 18:30:29Z. At that point the order was still `overview, mods, browse`. T3's only `store.ts` edit (18:32:33Z) added imports and the browse state slice, and it did not touch `VIEWS`.
- `store.ts` mtime is 13:36:06 local (18:36:06Z). T3 had no write call at that time; its last call was the validation run at 18:36:02Z. T4 had already finished (18:32Z) and T5 had not started yet (18:37:05Z). The writer is therefore outside the plan, i.e. the user or their editor.
- The test file `apps/desktop/src/app/App.test.tsx:32-35` is unchanged since 10-29. It still assumes 2× PageDown from Games lands on Mods.
- Intended truth: the source. The reorder was a deliberate edit to a single line, and nothing in the plan required it.
- Fix (test), confidence high: in `App.test.tsx:32`, use `{PageDown}{PageDown}{PageDown}` (games→overview→browse→mods). In `:35`, use `{PageUp}{PageUp}{PageUp}{PageUp}` (mods→browse→overview→games→wrap to settings). If the user did not mean to reorder the tabs, restore `store.ts:21-22` instead.

## (b) style.test.ts focus token: preexisting_user_edit (an uncommitted pair, with the css half reverted mid-run)

- Introducing changes:
  - `apps/desktop/src/style.test.ts:79` was changed from `zinc-100` (HEAD) to `zinc-200`. Its mtime is 2026-10-30 21:40, which is before the plan started (10-31 18:11Z). This is a user edit.
  - `apps/desktop/src/index.css:169` now reads `--color-focus: var(--color-zinc-100)`, which is identical to HEAD (empty diff). Its mtime is 13:37:23 local (18:37:23Z), and no agent wrote to it.
- T2, T3 and T4 all passed `style.test.ts` while it expected `zinc-200`. So until 18:37:23Z, `index.css` must have held an uncommitted `zinc-200` edit that matched the test. At 18:37:23Z something outside the plan restored the committed value, for example an editor revert or a `git checkout` of the file.
- Intended truth: unclear, so the user should confirm. The most recent action was the revert to `zinc-100`. The surrounding comments also refer to zinc-100: `style.test.ts:85` says "all >= 4.5:1 on zinc-100", and `index.css:97` says "solid white fill".
- Fix (test), confidence medium: revert `style.test.ts:79` to `var(--color-zinc-100)` (`git checkout HEAD -- apps/desktop/src/style.test.ts`, which is the only change in that file). The alternative is to set `index.css:169` back to `zinc-200` if the revert was accidental.

## Classification summary

| Failure | Introduced by | Class | Fix | Confidence |
|---|---|---|---|---|
| App.test.tsx:32-35 | user edit to store.ts:21-22 at 18:36:06Z | preexisting_user_edit | update test | high |
| style.test.ts:79 | user test edit 10-30 + user revert of index.css:169 at 18:37:23Z | preexisting_user_edit | revert test to zinc-100 | medium |

Neither failure was caused by a plan task (T2 to T5). No source files were modified.
