---
description: Research one pending Nexus game and complete its Vessel definition (interactive version of scripts/run-game-support.sh)
argument-hint: "[nexus-domain]"
allowed-tools: Read, Write, Edit, Glob, Grep, WebSearch, WebFetch, Bash(python3 scripts/pick_game_support_target.py:*), Bash(python3 scripts/check_game_support_note.py:*), Bash(cargo fmt:*), Bash(cargo clippy:*), Bash(cargo test:*), Bash(git status:*), Bash(git diff:*), Bash(git switch:*), Bash(git ls-remote:*), Bash(gh pr list:*)
---

Interactive version of `scripts/run-game-support.sh`. Same picker, prompt
and checks, but it works in the current checkout and never commits, pushes
or opens a PR. For the unattended flow (isolated worktree, commit, push,
PR), run `scripts/run-game-support.sh` instead.

1. Pick the game. Run:
   `python3 scripts/pick_game_support_target.py --game "$ARGUMENTS"`
   (add `--no-gh` if `gh` is not logged in). If `domain` is null, report
   that no game is pending and stop. If it exits non-zero, show the error
   and stop.
2. Check `git status --porcelain`. If any of the allowed files from the
   prompt (the definition, `crates/vessel-core/src/games/definitions.rs`,
   `docs/game-support/<domain>.md`) already has uncommitted changes, stop
   and ask the user. Other uncommitted files are fine; do not touch them.
3. Ask the user whether to create branch `<branch>` from the current HEAD
   (`git switch -c <branch>`), or to work on the current branch.
4. Read `.github/prompts/add-game-support.md` and follow it exactly, with
   DOMAIN, NAME, DEFINITION and BRANCH from step 1 and
   CHECKER=`scripts/check_game_support_note.py`. Make every file change
   with Edit or Write, never with `python`, `sed` or heredoc scripts. A
   denied shell command does not mean Bash is unavailable; the allowed
   commands still work. Before finishing, run `cargo fmt --all -- --check`,
   `cargo clippy -p vessel-core --all-targets --locked -- -D warnings` and
   the full `cargo test -p vessel-core --locked` (not only filtered tests).
5. Run `python3 scripts/check_game_support_note.py --definition <definition>
   --note docs/game-support/<domain>.md`, the same check the runner uses:
   Gaps `None` must go with `support = "tested"` (full support), Gaps
   bullets with `support = "experimental"` (partial, Rust changes needed).
   Fix any error it reports. Promotion to tested here is based on
   research, not hardware testing.
6. Show `git status` and `git diff --stat` for the allowed files only.
   Do not commit, push or open a PR. Tell the user the commands the
   runner would run:
   `git add <files> && git commit -m "games: <domain>: <full support | partial (Rust changes needed)>"`,
   `git push -u origin <branch>`,
   `gh pr create --base develop --head <branch> --title "games: <Name> (<domain>): <outcome>" --body-file <body>`.
