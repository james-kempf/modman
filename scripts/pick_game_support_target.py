#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Pick the next game for the game-support workflow.

Source of truth: the built-in game definitions.

- Registry and order: the `BUILTIN` table in
  crates/vessel-core/src/games/definitions.rs (legacy preset order).
- Pending full support: definitions in crates/vessel-core/definitions/
  with `support = "experimental"` (see docs/plan/20261101-nexus-game-support).
- Already handled: a definition that contains the completion marker
  (MARKER below), written by the workflow when a game's config is done.
- In flight: a remote branch `vessel/game-support-<domain>` or an open PR
  from that branch. Those games are skipped so runs never collide.
- Re-run (`--rerun`, only with `--game`): redo a game an earlier run
  completed (support "tested", the marker, and/or its old remote branch).
  Only an open PR from `vessel/game-support-<domain>` or
  `vessel/game-support-<domain>-*` blocks it. When the old branch still
  exists on the remote, the new branch is
  `vessel/game-support-<domain>-rerun-<UTC yyyymmddHHMM>`, so an old
  branch is never reused or force-pushed.

Prints one JSON object and, under GitHub Actions, writes `domain`, `name`,
`definition` and `branch` to $GITHUB_OUTPUT (all empty when nothing is
pending), plus `rerun=1` for a re-run. Exit codes: 0 = picked or nothing
pending, 2 = bad --game / --rerun.
Stdlib only (Python 3.11+ for tomllib).
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path

MARKER = "# vessel-game-support: complete"
BRANCH_PREFIX = "vessel/game-support-"
REGISTRY = Path("crates/vessel-core/src/games/definitions.rs")
DEFS_DIR = Path("crates/vessel-core/definitions")


def builtin_order(root: Path) -> list[str]:
    text = (root / REGISTRY).read_text(encoding="utf-8")
    block = re.search(r"const BUILTIN[^=]*=\s*&\[(.*?)\];", text, re.S)
    if not block:
        sys.exit(f"error: BUILTIN table not found in {REGISTRY}")
    return re.findall(r'def!\("([a-z0-9]+)"\)', block.group(1))


def load(root: Path, domain: str) -> tuple[dict, str]:
    path = root / DEFS_DIR / f"{domain}.toml"
    text = path.read_text(encoding="utf-8")
    return tomllib.loads(text), text


def run(cmd: list[str], root: Path) -> str | None:
    try:
        out = subprocess.run(cmd, cwd=root, capture_output=True, text=True, check=True)
        return out.stdout
    except (OSError, subprocess.CalledProcessError) as e:
        print(f"warning: {' '.join(cmd[:3])} failed: {e}", file=sys.stderr)
        return None


def remote_branches(root: Path, remote: str) -> set[str]:
    """Remote `vessel/game-support-*` branches, without the prefix."""
    out = run(["git", "ls-remote", "--heads", remote, f"{BRANCH_PREFIX}*"], root)
    if out is None:
        sys.exit("error: cannot list remote branches; refusing to pick blindly")
    return {
        line.split("\t", 1)[-1].removeprefix("refs/heads/").removeprefix(BRANCH_PREFIX)
        for line in out.splitlines()
    }


def open_prs(root: Path) -> dict[str, str]:
    """Open PRs from `vessel/game-support-*` branches: suffix -> PR URL.

    Module-level so tests can replace it instead of calling `gh`.
    """
    out = run(
        ["gh", "pr", "list", "--state", "open", "--limit", "200",
         "--json", "headRefName,url", "--jq", '.[] | "\\(.headRefName) \\(.url)"'],
        root,
    )
    if out is None:
        sys.exit("error: cannot list open PRs; refusing to pick blindly")
    prs: dict[str, str] = {}
    for line in out.splitlines():
        head, _, url = line.strip().partition(" ")
        if head.startswith(BRANCH_PREFIX):
            prs[head.removeprefix(BRANCH_PREFIX)] = url
    return prs


def rerun_branch(domain: str, branches: set[str] | None, now: datetime) -> str:
    """Branch for a re-run: the plain name unless it may already exist.

    `branches` is None when the remote was not checked; then the plain name
    cannot be proven free, so the timestamped name is used.
    """
    if branches is not None and domain not in branches:
        return f"{BRANCH_PREFIX}{domain}"
    return f"{BRANCH_PREFIX}{domain}-rerun-{now.astimezone(timezone.utc):%Y%m%d%H%M}"


def emit(result: dict) -> None:
    print(json.dumps(result, indent=2))
    out = os.environ.get("GITHUB_OUTPUT")
    if out:
        with open(out, "a", encoding="utf-8") as f:
            for k in ("domain", "name", "definition", "branch"):
                f.write(f"{k}={result.get(k) or ''}\n")
            if result.get("rerun"):
                f.write("rerun=1\n")


def main(argv: list[str] | None = None, now: datetime | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", type=Path, default=Path.cwd(), help="repo root")
    ap.add_argument("--game", default="", help="force this Nexus domain")
    ap.add_argument("--remote", default="origin")
    ap.add_argument("--no-remote", action="store_true",
                    help="skip the in-flight branch/PR check (offline use)")
    ap.add_argument("--no-gh", action="store_true",
                    help="skip the open-PR check (branches are still checked)")
    ap.add_argument("--rerun", action="store_true",
                    help="with --game: redo a game an earlier run completed")
    args = ap.parse_args(argv)
    root = args.root.resolve()
    if args.rerun and not args.game.strip():
        print("error: --rerun needs --game <domain>", file=sys.stderr)
        return 2

    order = builtin_order(root)
    pending: list[str] = []
    done: list[str] = []
    for d in order:
        data, text = load(root, d)
        if data.get("domain") != d:
            sys.exit(f"error: {d}.toml declares domain {data.get('domain')!r}")
        if data.get("support", "experimental") != "experimental":
            continue
        (done if MARKER in text else pending).append(d)

    branches = None if args.no_remote else remote_branches(root, args.remote)
    prs = {} if args.no_remote or args.no_gh else open_prs(root)
    busy = (branches or set()) | set(prs)

    branch = None
    if args.game:
        g = args.game.strip().lower()
        if g not in order:
            print(f"error: '{g}' has no built-in definition (known: {' '.join(order)})",
                  file=sys.stderr)
            return 2
        if args.rerun:
            support = load(root, g)[0].get("support", "experimental")
            if support not in ("tested", "experimental"):
                print(f"error: '{g}' has unknown support {support!r}", file=sys.stderr)
                return 2
            # Domains are [a-z0-9]+, so `<g>-` cannot match another domain.
            open_for_g = sorted(
                url for s, url in prs.items() if s == g or s.startswith(f"{g}-")
            )
            if open_for_g:
                print(f"error: '{g}' already has an open PR: {' '.join(open_for_g)}",
                      file=sys.stderr)
                return 2
            branch = rerun_branch(g, branches, now or datetime.now(timezone.utc))
            if branches is not None and branch.removeprefix(BRANCH_PREFIX) in branches:
                print(f"error: branch {branch} already exists on {args.remote}; retry later",
                      file=sys.stderr)
                return 2
        else:
            if g not in pending and g not in done:
                print(f"error: '{g}' is not experimental; nothing to add "
                      "(use --rerun to redo a completed game)", file=sys.stderr)
                return 2
            if g in busy:
                print(f"error: '{g}' already has a branch or open PR "
                      "(use --rerun to redo it on a new branch)", file=sys.stderr)
                return 2
        pick = g
    else:
        pick = next((d for d in pending if d not in busy), None)

    result: dict = {
        "pending": pending,
        "complete": done,
        "in_flight": sorted(busy),
        "domain": None,
    }
    if pick:
        data, _ = load(root, pick)
        result |= {
            "domain": pick,
            "name": data["name"],
            "definition": str(DEFS_DIR / f"{pick}.toml"),
            "branch": branch or f"{BRANCH_PREFIX}{pick}",
        }
        if args.rerun:
            result["rerun"] = True
    emit(result)
    return 0


if __name__ == "__main__":
    sys.exit(main())
