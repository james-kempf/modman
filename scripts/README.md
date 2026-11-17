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

Tests: `python3 -B -m unittest scripts/test_*.py`.

`check_game_support_note.py` checks each `## Loader routes` row's
`Handled by` fields against the definition. Routes the schema handles:

| Route | Handled by |
|---|---|
| Nexus page, archive matches `required_paths` | `loaders.nexus_mod_id` + `loaders.required_paths` |
| Nexus page, installer-only (`.exe`/`.msi` at the root) | `loaders.nexus_mod_id` + `loaders.github` (installer redirect); Gap without `[loaders.github]` |
| GitHub release zip | `loaders.github` |
| Off-site requirement (URL or name) | `loaders.aliases` + `loaders.github` or `loaders.nexus_mod_id` |

A `loaders.<field>` must be set on the named `[[loaders]]` entry, and
`loaders.github` / `loaders.aliases` must be tables there. A non-Gap row
whose Route contains "installer" must cite `loaders.github`.

## Research helpers: Nexus data and archive listings

The research agent gets two read-only helpers (stdlib only, always run as
`python3 -I`). The runner passes their absolute paths to the agent as
`NEXUS_INFO` and `INSPECT_ARCHIVE` and allows exactly those commands.

`nexus_mod_info.py` prints compact JSON; rate-limit headers and warnings go
to stderr, and HTTP 429 is retried with backoff.

```sh
python3 -I scripts/nexus_mod_info.py mod schedule1 1196           # v1 mod data
python3 -I scripts/nexus_mod_info.py files schedule1 1196         # v1 file list
python3 -I scripts/nexus_mod_info.py requirements schedule1 397   # v2 GraphQL
python3 -I scripts/nexus_mod_info.py top schedule1 10             # most endorsed + requirements
python3 -I scripts/nexus_mod_info.py top schedule1 10 --trending  # v1 trending
python3 -I scripts/nexus_mod_info.py contents schedule1 1196      # archive listing from Nexus's index
python3 -I scripts/nexus_mod_info.py key-status                   # which key source is used
```

API v1 has no requirements or "most endorsed" endpoint, so `requirements`,
`top` and `contents` use the v2 GraphQL endpoint (they also work without a
key). `requirements` separates Nexus requirements (with `other_game_id`
for cross-game ones) from off-site requirements (name + URL).

`inspect_archive.py` downloads one archive and prints its file listing,
its top-level entries and any `.exe`/`.msi` at the archive root. It never
extracts or runs anything: zip is read with `zipfile`, 7z/rar/tar with
`bsdtar -tf` (without bsdtar it reports "unsupported format").

```sh
python3 -I scripts/inspect_archive.py github LavaGang/MelonLoader --asset 'MelonLoader.x64.zip'
python3 -I scripts/inspect_archive.py nexus schedule1 1196 3486
```

Downloads go only to `${XDG_CACHE_HOME:-~/.cache}/vessel-game-support/archives/`,
over HTTPS, up to 200 MiB, from these hosts (redirects included):
`api.github.com`, `github.com`, `objects.githubusercontent.com`,
`release-assets.githubusercontent.com`, `api.nexusmods.com` and the
`*.nexusmods.com` / `*.nexus-cdn.com` CDN hosts that the Nexus
`download_link` endpoint returns.

**Premium limitation:** Nexus only hands out download links to Premium
accounts. For other accounts `inspect_archive.py nexus` prints "Nexus
download requires Premium; listing unavailable" and exits 3; use
`nexus_mod_info.py contents` instead, which lists the files from Nexus's
own index without a download.

### Nexus API key

The helpers look for the key in this order and use the first one found:

1. Vessel's own stored key: the Secret Service item
   `service=vessel username=nexus-api-key` (read with `secret-tool`), then
   `$XDG_CONFIG_HOME/vessel/credentials.toml` (must be mode 0600, as Vessel
   requires).
2. `$XDG_CONFIG_HOME/vessel-game-support/nexus_api_key`, a file holding
   only the key. Create it with mode 0600; the helpers warn when it is
   readable by others.

The key is never printed (all output is scrubbed), never passed in argv or
in the environment of `claude`, and only sent to `api.nexusmods.com` in a
header that is not forwarded on redirects. The runner denies the agent
`Read`/`Grep`/`Glob` on both key files and `secret-tool`. Without a key the
runner warns and continues: `mod`, `files`, `top --trending` and
`inspect_archive.py nexus` fail cleanly and the agent falls back to the web.

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
