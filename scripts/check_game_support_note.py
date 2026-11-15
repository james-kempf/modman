#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Check that a game-support definition and its research note agree.

Gaps format (in docs/game-support/<domain>.md): the body of the `## Gaps`
section, ignoring blank lines, is either exactly `None` (no Rust changes
needed) or one or more lines that each start with `- ` (one needed Rust
change per bullet). Anything else is an error.

Loader routes format: the body of `## Loader routes` is either exactly
`None` (the game has no loaders, script extenders or frameworks) or a
Markdown table with the header `| Loader | Route | Identifier | Handled by |`
and at least one row. `Handled by` is either the word `Gap` or one or more
backticked definition fields (`profile`, `loaders.nexus_mod_id`,
`loaders.launch.dll_overrides`, ...) separated by `,` or `+`.

Rules:
- Gaps `None`        -> the definition must have support = "tested".
- Gaps bullet list   -> the definition must have support = "experimental".
- The definition must contain the completion marker either way.
- The note must have a non-empty `## Sources` section.
- `## Loader routes` is required. `None` is only allowed when the
  definition has no `[[loaders]]`; otherwise every `[[loaders]]` entry
  needs at least one row (matched by key or name).
- A `loaders.<field>` reference must be set on the `[[loaders]]` entry the
  row names; any other field must be set in the definition.
- A `Gap` row needs a Gaps bullet that names its loader, and makes the
  outcome partial: Gaps `None` with a `Gap` row is an error.

Prints `outcome` (full | partial) and the gaps. Under GitHub Actions also
writes `outcome`, `outcome_label` and a multi-line `gaps` to $GITHUB_OUTPUT.
Exit 0 = consistent, 1 = inconsistent or malformed. Stdlib only.
"""

from __future__ import annotations

import argparse
import os
import re
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


ROUTES_HEADER = ["loader", "route", "identifier", "handled by"]
FIELD = re.compile(r"`([^`]+)`")


def cells(row: str) -> list[str]:
    return [c.strip() for c in row.strip().strip("|").split("|")]


def norm(name: str) -> str:
    return re.sub(r"[^a-z0-9]", "", name.lower())


def loader_entry(loaders: list[dict], name: str) -> dict | None:
    n = norm(name)
    return next(
        (l for l in loaders if n in (norm(l.get("key", "")), norm(l.get("name", "")))), None
    )


def field_set(table: dict, path: list[str]) -> bool:
    node = table
    for part in path:
        if not isinstance(node, dict) or part not in node:
            return False
        node = node[part]
    return True


def check_routes(
    note_path: Path, body: list[str] | None, defn: dict, gap_items: list[str]
) -> tuple[list[str], int]:
    """Errors for `## Loader routes`, and the number of `Gap` rows."""
    where = f"{note_path}: '## Loader routes'"
    if body is None:
        return [f"{where} section missing"], 0
    loaders = defn.get("loaders", [])
    if body == ["None"]:
        if loaders:
            keys = ", ".join(l.get("key", "?") for l in loaders)
            return [f"{where} is None but the definition has [[loaders]] ({keys})"], 0
        return [], 0
    if len(body) < 3 or [c.lower() for c in cells(body[0])] != ROUTES_HEADER:
        return [
            f"{where} must be exactly 'None' or a table with header "
            "'| Loader | Route | Identifier | Handled by |' and at least one row"
        ], 0
    errors, gaps, named = [], 0, set()
    for row in body[2:]:
        c = cells(row)
        if len(c) != 4 or not all(c):
            errors.append(f"{where}: row '{row}' needs 4 non-empty cells")
            continue
        loader, _route, _ident, handled = c
        entry = loader_entry(loaders, loader)
        if entry is not None:
            named.add(id(entry))
        if handled == "Gap":
            gaps += 1
            if not any(norm(loader) in norm(g) for g in gap_items):
                errors.append(f"{where}: '{loader}' row is a Gap but no Gaps bullet names it")
            continue
        fields = FIELD.findall(handled)
        if not fields or re.sub(r"`[^`]+`|[\s,+]", "", handled):
            errors.append(
                f"{where}: handled-by '{handled}' must be 'Gap' or backticked definition fields"
            )
            continue
        for f in fields:
            path = f.split(".")
            if path[0] == "loaders":
                ok = entry is not None and field_set(entry, path[1:]) and len(path) > 1
                if not ok:
                    errors.append(
                        f"{where}: '{f}' is not set on a [[loaders]] entry for '{loader}'"
                    )
            elif not field_set(defn, path):
                errors.append(f"{where}: '{f}' is not set in the definition")
    for l in loaders:
        if id(l) not in named:
            errors.append(f"{where}: no row for [[loaders]] '{l.get('key', '?')}'")
    return errors, gaps


def check(definition: Path, note_path: Path) -> tuple[list[str], str, list[str]]:
    errors: list[str] = []
    text = definition.read_text(encoding="utf-8")
    defn = tomllib.loads(text)
    support = defn.get("support", "experimental")
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

    route_errors, gap_rows = check_routes(
        note_path, section(note, "Loader routes"), defn, items
    )
    errors += route_errors
    if outcome == "full" and gap_rows:
        errors.append(
            f"{note_path}: Gaps is None but {gap_rows} loader route(s) are a Gap"
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
