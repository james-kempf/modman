#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Check that a game-support definition and its research note agree.

Gaps format (in docs/game-support/<domain>.md): the body of the `## Gaps`
section, ignoring blank lines, is either exactly `None` (no Rust changes
needed) or one or more lines that each start with `- ` (one needed Rust
change per bullet). Anything else is an error.

Rules:
- Gaps `None`        -> the definition must have support = "tested".
- Gaps bullet list   -> the definition must have support = "experimental".
- The definition must contain the completion marker either way.
- The note must have a non-empty `## Sources` section.

Prints `outcome` (full | partial) and the gaps. Under GitHub Actions also
writes `outcome`, `outcome_label` and a multi-line `gaps` to $GITHUB_OUTPUT.
Exit 0 = consistent, 1 = inconsistent or malformed. Stdlib only.
"""

from __future__ import annotations

import argparse
import os
import sys
import tomllib
import uuid
from pathlib import Path

MARKER = "# vessel-game-support: complete"


def section(note: str, title: str) -> list[str] | None:
    lines, inside, found = [], False, False
    for line in note.splitlines():
        if line.startswith("## "):
            inside = line.strip() == f"## {title}"
            found = found or inside
            continue
        if inside and line.strip():
            lines.append(line.rstrip())
    return lines if found else None


def check(definition: Path, note_path: Path) -> tuple[list[str], str, list[str]]:
    errors: list[str] = []
    text = definition.read_text(encoding="utf-8")
    support = tomllib.loads(text).get("support", "experimental")
    if MARKER not in text.splitlines():
        errors.append(f"{definition}: completion marker '{MARKER}' missing")
    if not note_path.is_file():
        return errors + [f"{note_path}: research note missing"], "", []
    note = note_path.read_text(encoding="utf-8")

    if not section(note, "Sources"):
        errors.append(f"{note_path}: '## Sources' section missing or empty")

    gaps = section(note, "Gaps")
    outcome, items = "", []
    if gaps is None:
        errors.append(f"{note_path}: '## Gaps' section missing")
    elif gaps == ["None"]:
        outcome = "full"
    elif gaps and all(g.startswith("- ") for g in gaps):
        outcome, items = "partial", gaps
    else:
        errors.append(
            f"{note_path}: '## Gaps' must be exactly 'None' or a list of '- ' bullets"
        )

    if outcome == "full" and support != "tested":
        errors.append(f"{definition}: Gaps is None but support = '{support}' (expected 'tested')")
    if outcome == "partial" and support != "experimental":
        errors.append(
            f"{definition}: Gaps lists Rust changes but support = '{support}' (expected 'experimental')"
        )
    return errors, outcome, items


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--definition", type=Path, required=True)
    ap.add_argument("--note", type=Path, required=True)
    args = ap.parse_args()

    errors, outcome, items = check(args.definition, args.note)
    for e in errors:
        print(f"::error::{e}" if os.environ.get("GITHUB_ACTIONS") else f"error: {e}")
    if errors:
        return 1

    label = "full support" if outcome == "full" else "partial (Rust changes needed)"
    gaps = "\n".join(items) if items else "None"
    print(f"outcome: {outcome} = {label}\ngaps:\n{gaps}")
    out = os.environ.get("GITHUB_OUTPUT")
    if out:
        eof = f"EOF_{uuid.uuid4().hex}"
        with open(out, "a", encoding="utf-8") as f:
            f.write(f"outcome={outcome}\noutcome_label={label}\n")
            f.write(f"gaps<<{eof}\n{gaps}\n{eof}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
