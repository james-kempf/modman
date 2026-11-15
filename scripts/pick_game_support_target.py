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

Prints one JSON object and, under GitHub Actions, writes `domain`, `name`,
`definition` and `branch` to $GITHUB_OUTPUT (all empty when nothing is
pending). Exit codes: 0 = picked or nothing pending, 2 = bad --game.
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


def in_flight(root: Path, remote: str, use_gh: bool) -> set[str]:
    busy: set[str] = set()
    out = run(["git", "ls-remote", "--heads", remote, f"{BRANCH_PREFIX}*"], root)
    if out is None:
        sys.exit("error: cannot list remote branches; refusing to pick blindly")
    for line in out.splitlines():
        ref = line.split("\t", 1)[-1]
        busy.add(ref.removeprefix("refs/heads/").removeprefix(BRANCH_PREFIX))
    if use_gh:
        out = run(
            ["gh", "pr", "list", "--state", "open", "--limit", "200",
             "--json", "headRefName", "--jq", ".[].headRefName"],
            root,
        )
        if out is None:
            sys.exit("error: cannot list open PRs; refusing to pick blindly")
        busy |= {
            h.removeprefix(BRANCH_PREFIX)
            for h in out.split()
            if h.startswith(BRANCH_PREFIX)
        }
    return busy


def emit(result: dict) -> None:
    print(json.dumps(result, indent=2))
    out = os.environ.get("GITHUB_OUTPUT")
    if out:
        with open(out, "a", encoding="utf-8") as f:
            for k in ("domain", "name", "definition", "branch"):
                f.write(f"{k}={result.get(k) or ''}\n")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", type=Path, default=Path.cwd(), help="repo root")
    ap.add_argument("--game", default="", help="force this Nexus domain")
    ap.add_argument("--remote", default="origin")
    ap.add_argument("--no-remote", action="store_true",
                    help="skip the in-flight branch/PR check (offline use)")
    ap.add_argument("--no-gh", action="store_true",
                    help="skip the open-PR check (branches are still checked)")
    args = ap.parse_args()
    root = args.root.resolve()

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

    busy = set() if args.no_remote else in_flight(root, args.remote, not args.no_gh)

    if args.game:
        g = args.game.strip().lower()
        if g not in order:
            print(f"error: '{g}' has no built-in definition (known: {' '.join(order)})",
                  file=sys.stderr)
            return 2
        if g not in pending and g not in done:
            print(f"error: '{g}' is not experimental; nothing to add", file=sys.stderr)
            return 2
        if g in busy:
            print(f"error: '{g}' already has a branch or open PR", file=sys.stderr)
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
            "branch": f"{BRANCH_PREFIX}{pick}",
        }
    emit(result)
    return 0


if __name__ == "__main__":
    sys.exit(main())
