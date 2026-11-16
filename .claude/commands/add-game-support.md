---
description: Research one pending Nexus game and complete its Vessel definition (interactive version of scripts/run-game-support.sh)
argument-hint: "[nexus-domain] [--rerun]"
allowed-tools: Read, Write, Edit, Glob, Grep, WebSearch, WebFetch, Bash(python3 scripts/pick_game_support_target.py:*), Bash(python3 scripts/check_game_support_note.py:*), Bash(python3 -I scripts/nexus_mod_info.py:*), Bash(python3 -I scripts/inspect_archive.py:*), Bash(gh api repos/*:*), Bash(cargo fmt:*), Bash(cargo clippy:*), Bash(cargo test:*), Bash(git status:*), Bash(git diff:*), Bash(git switch:*), Bash(git ls-remote:*), Bash(gh pr list:*)
---

Interactive version of `scripts/run-game-support.sh`. Same picker, prompt
and checks, but it works in the current checkout and never commits, pushes
or opens a PR. For the unattended flow (isolated worktree, commit, push,
PR), run `scripts/run-game-support.sh` instead.

1. Pick the game. `$ARGUMENTS` is an optional domain, optionally followed
   by `--rerun`. Run:
   `python3 scripts/pick_game_support_target.py --game <domain>`,
   adding `--rerun` when it was given (add `--no-gh` if `gh` is not logged
   in). `--rerun` without a domain is an error: say so and stop. With
   `--rerun`, the picker accepts a game an earlier run completed (support
   `tested`, the completion marker, or an existing remote branch), refuses
   it while an open PR exists for `vessel/game-support-<domain>*` (it
   prints the PR URL), and returns
   `vessel/game-support-<domain>-rerun-<UTC yyyymmddHHMM>` as the branch
   when `vessel/game-support-<domain>` already exists on the remote. If
   `domain` is null, report that no game is pending and stop. If it exits
   non-zero, show the error and stop.
2. Check `git status --porcelain`. If any of the allowed files from the
   prompt (the definition, `crates/vessel-core/src/games/definitions.rs`,
   `docs/game-support/<domain>.md`) already has uncommitted changes, stop
   and ask the user. Other uncommitted files are fine; do not touch them.
3. Ask the user whether to create branch `<branch>` from the current HEAD
   (`git switch -c <branch>`), or to work on the current branch.
4. Read `.github/prompts/add-game-support.md` and follow it exactly, with
   DOMAIN, NAME, DEFINITION and BRANCH from step 1,
   CHECKER=`scripts/check_game_support_note.py`,
   NEXUS_INFO=`scripts/nexus_mod_info.py` and
   INSPECT_ARCHIVE=`scripts/inspect_archive.py`, plus `RERUN=1` when
   `--rerun` was given (follow the prompt's "Re-run" section). Do the "Loader routes"
   research in its section 2 for every loader: get Nexus data with
   `python3 -I scripts/nexus_mod_info.py` first (WebFetch on Nexus HTML
   only as a fallback), run the mandatory top-mods requirements check
   (`top <domain> 10`; say in the Summary if it cannot run), confirm what
   each route ships with `python3 -I scripts/inspect_archive.py` or
   `nexus_mod_info.py contents` against `required_paths`, and list every
   route (Nexus page, off-site sources) and how mods declare it. Never
   read, print or search for the Nexus API key; the helpers load it. Make every file change
   with Edit or Write, never with `python`, `sed` or heredoc scripts. A
   denied shell command does not mean Bash is unavailable; the allowed
   commands still work. Before finishing, run `cargo fmt --all -- --check`,
   `cargo clippy -p vessel-core --all-targets --locked -- -D warnings` and
   the full `cargo test -p vessel-core --locked` (not only filtered tests).
5. Run `python3 scripts/check_game_support_note.py --definition <definition>
   --note docs/game-support/<domain>.md`, the same check the runner uses:
   Gaps `None` must go with `support = "tested"` (full support), Gaps
   bullets with `support = "experimental"` (partial, Rust changes needed).
   It also requires the `## Loader routes` section: `None` only for a game
   with no loaders, otherwise one table row per distribution route of every
   loader (Nexus page, cross-listed Nexus page, off-site requirement,
   GitHub release, ...), each handled by a backticked definition field or
   `Gap` with a Gaps bullet naming the loader. Any `Gap` row rules out full
   support. Fix any error it reports. Promotion to tested here is based on
   research, not hardware testing; the note's Manual test section says so
   only when support is `"tested"`.
6. Show `git status` and `git diff --stat` for the allowed files only.
   Do not commit, push or open a PR. Tell the user the commands the
   runner would run:
   `git add <files> && git commit -m "games: <domain>: <full support | partial (Rust changes needed)>"`,
   `git push -u origin <branch>`,
   `gh pr create --base develop --head <branch> --title "games: <Name> (<domain>): <outcome>" --body-file <body>`.
   For a re-run, the runner appends " (re-run)" to the commit message and
   PR title and starts the PR body with "Re-run of a previously completed
   game."
