# scripts

## run-game-support.sh: complete one game's definition

`scripts/run-game-support.sh` picks the next pending game
(`pick_game_support_target.py`), has the `claude` CLI research it in an
isolated worktree, checks the result (`check_game_support_note.py`, cargo
fmt/clippy/test), then commits, pushes `vessel/game-support-<domain>` and
opens a PR against `develop`. `--help` lists all options; `--pick-only`
and `--dry-run` change nothing on the remote.

`--rerun` (only with `--game <domain>`) redoes a game an earlier run
completed: one with `support = "tested"`, the
`# vessel-game-support: complete` marker, and/or an existing remote
branch `vessel/game-support-<domain>`. It is refused while an open PR
exists from `vessel/game-support-<domain>` or
`vessel/game-support-<domain>-*`. If the old branch still exists, the run
uses `vessel/game-support-<domain>-rerun-<UTC yyyymmddHHMM>`, so it never
reuses or force-pushes an old branch. The agent gets `RERUN=1` and
re-researches from scratch (see `.github/prompts/add-game-support.md`); the
PR title ends in " (re-run)". Example:

```sh
scripts/run-game-support.sh --pick-only --game schedule1 --rerun
scripts/run-game-support.sh --game schedule1 --rerun
```

The interactive equivalent is `/add-game-support <domain> --rerun`.

Tests: `python3 -m unittest scripts/test_pick_game_support_target.py
scripts/test_check_game_support_note.py`.

## rebrand.py: rename the app

`scripts/rebrand.py` repeats the modman -> Vessel rename for any later
OLD -> NEW pair. It needs only `git` and `python3` (stdlib). `cargo` is
optional and is used for the post-apply steps.

```sh
# 1. Dry run (the default): prints the planned `git mv` operations, the
#    replacement count per file, the protected lines and the manual checklist.
scripts/rebrand.py \
  --from-display Vessel --to-display Foo \
  --from-id vessel --to-id foo \
  --from-appid org.vessel.app --to-appid org.foo.app

# 2. Apply on a clean tree, also moving the legacy names forward and
#    rewriting the migration module.
scripts/rebrand.py ...same arguments... --apply --update-legacy --rewrite-migrate
```

The ids must match `^[a-z][a-z0-9-]*$`. The script refuses to run when the
new name already appears in the tree (override with `--force`). `--apply`
also refuses on a dirty tree (override with `--allow-dirty`). The script
never commits.

What it changes:

- Paths: renames tracked paths that contain the old id or app id with
  `git mv`, deepest first (`crates/<id>-*`, `packaging/rpm/<id>.spec`,
  `packaging/common/<appid>.desktop`, metainfo, Flatpak manifest, nxm
  desktop file).
- Contents, in one pass that does not cascade: the D-Bus path
  (`/org/<id>/app`), the app id, the display name (whole word),
  PascalCase identifiers, `UPPER_SNAKE` (the `<ID>_` env prefix),
  `snake_case` (`<id>_core`) and the lowercase id.
- Skips `.git`, `target`, `node_modules`, `dist`, `legacy/`,
  `docs/plan/`, `Cargo.lock` (regenerated) and binary files.

Protected areas are not rewritten. Lines in them that mention the name are
listed instead:

| Area | Rewritten by |
| --- | --- |
| `crates/*/src/migrate.rs`, `crates/*/src/migrate/**` | `--rewrite-migrate` |
| `pub const LEGACY_*` values in `crates/*/src/legacy.rs` | `--update-legacy`: each value becomes the outgoing name's equivalent (`modman.db` -> `vessel.db`; the Steam AppName becomes the outgoing display name) |
| Package rename metadata: AUR `provides/conflicts/replaces=`, RPM `Obsoletes:/Provides:/Conflicts:`, `X-Flatpak-RenamedFrom`, metainfo `<replaces>`/`<provides>` | by hand |
| README sections headed "Migrating from ..." / "Upgrading from ..." | by hand |

After `--apply`, the script runs `cargo fmt --all` (renames change the
import order), `cargo update -w` (regenerates `Cargo.lock`, tries
`--offline` first) and a case-insensitive residual grep for the old id.
It exits with code 3 when hits remain outside the protected areas. Use
`--no-post` to skip the cargo steps.

Then work through the printed checklist: rename metadata for the outgoing
name, the Flatpak end-of-life rebase, the README migration sections, and a
review of the migration module. The migration supports only one previous
name, so users who skip a release are not migrated from older names.

The script does not run `npm install`, and it does not regenerate
`package-lock.json`. It only rewrites the name fields, so run `npm test`
and `npm run build` in `apps/desktop` afterwards.
