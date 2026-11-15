#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Rename the app (display name, id, app id) across the repository.

Repeats the modman -> Vessel rebrand for any OLD -> NEW pair. Dry-run by
default; --apply performs the changes. Never commits.

Example (dry run, then apply):

  scripts/rebrand.py --from-display Vessel --to-display Foo \\
      --from-id vessel --to-id foo \\
      --from-appid org.vessel.app --to-appid org.foo.app
  scripts/rebrand.py ...same args... --apply --update-legacy

What it does:
  * Renames tracked paths whose name contains the old id or app id with
    `git mv` (deepest paths first), e.g. crates/vessel-core,
    packaging/rpm/vessel.spec, packaging/common/org.vessel.app.desktop.
  * Rewrites file contents, case-aware: app id (org.vessel.app), D-Bus
    object path (/org/vessel/app), Display name (Vessel), PascalCase
    identifiers (VesselFoo), UPPER_SNAKE (VESSEL_ env prefix), snake_case
    (vessel_core) and the lowercase id (vessel, vessel-cli, .vessel.tmp).
  * Skips .git, target, node_modules, dist, legacy/, docs/plan/,
    Cargo.lock (regenerated) and binary / non-UTF-8 files.
  * Protects legacy-aware areas (not rewritten; hits are reported):
      - crates/*/src/migrate.rs and crates/*/src/migrate/**
        (rewritten only with --rewrite-migrate)
      - `pub const LEGACY_*: &str = ...` lines in crates/*/src/legacy.rs
        (rewritten to the OUTGOING names only with --update-legacy)
      - packaging rename metadata: provides=/conflicts=/replaces= (AUR),
        Obsoletes:/Provides:/Conflicts: (RPM), X-Flatpak-RenamedFrom,
        metainfo <replaces>/<provides> blocks
      - Markdown README sections headed "Migrating from ..." or
        "Upgrading from ..." (up to the next heading of the same level)
  * After --apply: cargo fmt --all, cargo update -w (regenerates
    Cargo.lock), then a residual grep for the old id. Exit code 3 when
    hits remain outside the protected areas.

Exit codes: 0 ok, 1 runtime error, 2 invalid input / refused, 3 residual
hits outside protected areas after --apply.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

EXCLUDED_DIRS = {".git", "target", "node_modules", "dist"}
EXCLUDED_PREFIXES = ("legacy/", "docs/plan/")
EXCLUDED_FILES = {"Cargo.lock"}
ID_RE = re.compile(r"^[a-z][a-z0-9-]*$")
APPID_RE = re.compile(r"^[a-zA-Z][a-zA-Z0-9_-]*(\.[a-zA-Z0-9_-]+)+$")

# Line-level protection for package rename metadata.
PKG_META_LINE = re.compile(
    r"^\s*(provides|conflicts|replaces)\s*=\s*\("  # AUR PKGBUILD
    r"|^\s*(Obsoletes|Provides|Conflicts)\s*:"  # RPM spec
    r"|X-Flatpak-RenamedFrom"  # desktop entry
    r"|<(replaces|provides)>.*</(replaces|provides)>",  # one-line metainfo
)
META_BLOCK_OPEN = re.compile(r"<(replaces|provides)>\s*$")
META_BLOCK_CLOSE = re.compile(r"</(replaces|provides)>")
LEGACY_CONST = re.compile(r'^(\s*pub\s+const\s+(LEGACY_\w+)\s*:\s*&str\s*=\s*")([^"]*)(".*)$')
MD_HEADING = re.compile(r"^(#{1,6})\s+(.*)$")
MIGRATION_HEADING = re.compile(r"^(Migrating|Upgrading) from\b", re.I)


def die(msg: str, code: int = 2) -> None:
    print(f"rebrand: error: {msg}", file=sys.stderr)
    sys.exit(code)


def pascal(ident: str) -> str:
    return "".join(p[:1].upper() + p[1:] for p in ident.split("-") if p)


def dbus_path(appid: str) -> str:
    return "/" + appid.replace(".", "/").replace("-", "_")


def git(root: Path, *args: str, check: bool = True) -> str:
    r = subprocess.run(["git", "-C", str(root), *args], capture_output=True, text=True)
    if check and r.returncode != 0:
        die(f"git {' '.join(args)} failed: {r.stderr.strip()}", 1)
    return r.stdout


@dataclass
class Names:
    from_display: str
    to_display: str
    from_id: str
    to_id: str
    from_appid: str
    to_appid: str

    def variants(self) -> list[tuple[str, str, str, bool]]:
        """(label, old, new, needs_identifier_boundaries), highest priority first."""
        snake_f, snake_t = self.from_id.replace("-", "_"), self.to_id.replace("-", "_")
        v = [
            ("dbus-path", dbus_path(self.from_appid), dbus_path(self.to_appid), False),
            ("app-id", self.from_appid, self.to_appid, False),
            ("display", self.from_display, self.to_display, True),
            ("pascal", pascal(self.from_id), pascal(self.to_id), False),
            ("upper", snake_f.upper(), snake_t.upper(), False),
            ("id", self.from_id, self.to_id, False),
        ]
        if snake_f != self.from_id:
            v.insert(-1, ("snake", snake_f, snake_t, False))
        seen, out = set(), []
        for label, old, new, bound in v:
            key = (old, bound)
            if old and key not in seen:
                seen.add(key)
                out.append((label, old, new, bound))
        return out


class Replacer:
    """One-pass, non-cascading replacement of every name variant."""

    def __init__(self, names: Names):
        self.table: dict[str, tuple[str, str]] = {}
        parts = []
        for i, (label, old, new, bound) in enumerate(names.variants()):
            g = f"g{i}"
            self.table[g] = (label, new)
            pat = re.escape(old)
            if bound:
                pat = rf"(?<![A-Za-z0-9_]){pat}(?![A-Za-z0-9_])"
            parts.append(f"(?P<{g}>{pat})")
        self.rx = re.compile("|".join(parts))

    def sub(self, text: str, counts: dict[str, int] | None = None) -> str:
        def repl(m: re.Match) -> str:
            label, new = self.table[m.lastgroup]
            if counts is not None:
                counts[label] = counts.get(label, 0) + 1
            return new

        return self.rx.sub(repl, text)

    def hits(self, text: str) -> bool:
        return self.rx.search(text) is not None


@dataclass
class FilePlan:
    path: str
    counts: dict[str, int] = field(default_factory=dict)
    protected: list[tuple[int, str, str]] = field(default_factory=list)  # (line, reason, text)
    new_text: str | None = None


def is_excluded(rel: str) -> bool:
    parts = rel.split("/")
    if any(p in EXCLUDED_DIRS for p in parts[:-1]):
        return True
    if parts[-1] in EXCLUDED_FILES:
        return True
    return rel.startswith(EXCLUDED_PREFIXES)


def list_files(root: Path) -> list[str]:
    out = git(root, "ls-files", "-z", "--cached", "--others", "--exclude-standard")
    files = sorted({f for f in out.split("\0") if f})
    return [f for f in files if not is_excluded(f) and (root / f).is_file() and not (root / f).is_symlink()]


def read_text(p: Path) -> str | None:
    data = p.read_bytes()
    if b"\0" in data[:8192]:
        return None
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return None


def is_migrate_file(rel: str) -> bool:
    return re.match(r"^crates/[^/]+/src/migrate(\.rs$|/)", rel) is not None


def is_legacy_file(rel: str) -> bool:
    return re.match(r"^crates/[^/]+/src/legacy\.rs$", rel) is not None


def is_packaging_file(rel: str) -> bool:
    name = rel.rsplit("/", 1)[-1]
    return (
        rel.startswith("packaging/")
        or name == "PKGBUILD"
        or name.endswith((".spec", ".desktop", ".metainfo.xml", ".appdata.xml"))
    )


def is_readme(rel: str) -> bool:
    return re.search(r"(^|/)README[^/]*\.md$", rel, re.I) is not None


def protected_lines(rel: str, lines: list[str], args) -> dict[int, str]:
    """Map line index -> reason, for lines that must not be rewritten."""
    prot: dict[int, str] = {}
    if is_migrate_file(rel) and not args.rewrite_migrate:
        return {i: "migrate module" for i in range(len(lines))}
    if is_legacy_file(rel):
        for i, ln in enumerate(lines):
            if LEGACY_CONST.match(ln.rstrip("\r\n")):
                prot[i] = "legacy constant"
    if is_packaging_file(rel):
        in_block = False
        for i, ln in enumerate(lines):
            if in_block:
                prot[i] = "package rename metadata"
                if META_BLOCK_CLOSE.search(ln):
                    in_block = False
            elif META_BLOCK_OPEN.search(ln):
                prot[i] = "package rename metadata"
                in_block = True
            elif PKG_META_LINE.search(ln):
                prot[i] = "package rename metadata"
    if is_readme(rel):
        level = 0
        in_code = False
        for i, ln in enumerate(lines):
            if ln.lstrip().startswith("```"):
                in_code = not in_code
            m = None if in_code else MD_HEADING.match(ln.rstrip("\r\n"))
            if m:
                lvl = len(m.group(1))
                if level and lvl <= level:
                    level = 0
                if not level and MIGRATION_HEADING.match(m.group(2)):
                    level = lvl
            if level:
                prot[i] = "README migration section"
    return prot


def plan_content(root: Path, files: list[str], rep: Replacer, args) -> tuple[list[FilePlan], list[str]]:
    plans, skipped = [], []
    for rel in files:
        text = read_text(root / rel)
        if text is None:
            skipped.append(rel)
            continue
        if not rep.hits(text):
            continue
        lines = text.splitlines(keepends=True)
        prot = protected_lines(rel, lines, args)
        fp = FilePlan(rel)
        out = []
        for i, ln in enumerate(lines):
            if i in prot:
                if rep.hits(ln):
                    fp.protected.append((i + 1, prot[i], ln.rstrip("\r\n")))
                out.append(ln)
            else:
                out.append(rep.sub(ln, fp.counts))
        new = "".join(out)
        if new != text:
            fp.new_text = new
        if fp.counts or fp.protected:
            plans.append(fp)
    return plans, skipped


def plan_renames(files: list[str], rep: Replacer) -> list[tuple[str, str]]:
    """(old path, new path) per renamed component, deepest first.

    Each old path has unrenamed parents, which is valid at the time it runs
    because deeper entries are moved before their parents.
    """
    ops: dict[str, str] = {}
    for rel in files:
        parts = rel.split("/")
        for i, comp in enumerate(parts):
            new = rep.sub(comp)
            if new != comp:
                old = "/".join(parts[: i + 1])
                ops[old] = "/".join(parts[:i] + [new])
    return sorted(ops.items(), key=lambda kv: (-kv[0].count("/"), kv[0]))


def check_new_name_absent(root: Path, files: list[str], names: Names) -> list[str]:
    needles = [names.to_id.lower(), names.to_appid.lower(), names.to_display.lower()]
    needles += [names.to_id.replace("-", "_").lower()]
    rx = re.compile("|".join(re.escape(n) for n in set(needles)))
    hits = []
    for rel in files:
        if rx.search(rel.lower()):
            hits.append(f"{rel} (path)")
            continue
        text = read_text(root / rel)
        if text is None:
            continue
        for n, ln in enumerate(text.splitlines(), 1):
            if rx.search(ln.lower()):
                hits.append(f"{rel}:{n}: {ln.strip()[:120]}")
    return hits


def update_legacy(root: Path, names: Names, apply: bool) -> list[str]:
    """Point LEGACY_* constants at the outgoing (FROM) names."""
    report = []
    for lf in sorted(root.glob("crates/*/src/legacy.rs")):
        text = lf.read_text("utf-8")
        consts = {m.group(2): m.group(3) for m in map(LEGACY_CONST.match, text.splitlines()) if m}
        old_id, old_appid = consts.get("LEGACY_APP_DIR"), consts.get("LEGACY_APP_ID")
        if not old_id or not old_appid:
            report.append(f"{lf.relative_to(root)}: LEGACY_APP_DIR/LEGACY_APP_ID not found; skipped")
            continue
        legacy = Names(pascal(old_id), names.from_display, old_id, names.from_id, old_appid, names.from_appid)
        lrep = Replacer(legacy)
        out = []
        for ln in text.splitlines(keepends=True):
            body = ln.rstrip("\r\n")
            m = LEGACY_CONST.match(body)
            if m:
                name, val = m.group(2), m.group(3)
                new_val = names.from_display if name == "LEGACY_STEAM_APP_NAME" else lrep.sub(val)
                if new_val != val:
                    report.append(f"{lf.relative_to(root)}: {name} {val!r} -> {new_val!r}")
                out.append(m.group(1) + new_val + m.group(4) + ln[len(body):])
            else:
                # Doc comments ("~/.config/<old>", "old \"<old>\" names").
                out.append(lrep.sub(ln) if ln.lstrip().startswith(("//", "///", "//!")) else ln)
        if apply:
            lf.write_text("".join(out), "utf-8")
    return report


def residual(root: Path, needle: str, args) -> tuple[list[str], list[str]]:
    rx = re.compile(re.escape(needle), re.I)
    expected, unexpected = [], []
    for rel in list_files(root):
        text = read_text(root / rel)
        if text is None:
            continue
        lines = text.splitlines(keepends=True)
        prot = None
        for i, ln in enumerate(lines):
            if not rx.search(ln):
                continue
            if prot is None:
                prot = protected_lines(rel, lines, argparse.Namespace(rewrite_migrate=False))
            entry = f"{rel}:{i + 1}: {ln.strip()[:140]}"
            if i in prot or is_legacy_file(rel):
                expected.append(f"[{prot.get(i, 'legacy.rs')}] {entry}")
            else:
                unexpected.append(entry)
        if rx.search(rel):
            unexpected.append(f"{rel} (path)")
    return expected, unexpected


def run(cmd: list[str], root: Path) -> bool:
    print(f"$ {' '.join(cmd)}")
    return subprocess.run(cmd, cwd=root).returncode == 0


def post_apply(root: Path) -> None:
    if not (root / "Cargo.toml").exists():
        return
    if not shutil.which("cargo"):
        print("WARNING: cargo not found; run `cargo fmt --all` and `cargo update -w` yourself.")
        return
    if not run(["cargo", "fmt", "--all"], root):
        print("WARNING: cargo fmt failed (see above).")
    if not (run(["cargo", "update", "-w", "--offline"], root) or run(["cargo", "update", "-w"], root)):
        print("WARNING: could not regenerate Cargo.lock; run `cargo update -w`.")


def checklist(names: Names, args) -> str:
    f, t = names, names
    legacy_done = "DONE by --update-legacy" if args.update_legacy and args.apply else "TODO (or rerun with --update-legacy)"
    migrate_done = (
        "DONE by --rewrite-migrate (review it)" if args.rewrite_migrate and args.apply else "TODO (or rerun with --rewrite-migrate)"
    )
    return f"""
Manual follow-ups ({f.from_display} -> {t.to_display}):
  1. legacy.rs: point every LEGACY_* constant at the OUTGOING names
     ({f.from_id}, {f.from_id}.db, {f.from_id}-autoupdate, {f.from_appid}, {f.from_id.upper().replace('-', '_')}_,
     Steam AppName "{f.from_display}"). [{legacy_done}]
  2. Migration module (crates/*/src/migrate*): rewrite references to the
     current name to {t.to_display} and re-check assumptions that were
     specific to the previous legacy name (e.g. bash-tool-only env vars,
     paths::LEGACY_CONFIG_DIR). [{migrate_done}]
  3. Packaging rename metadata for the outgoing name '{f.from_id}':
     AUR provides/conflicts/replaces=('{f.from_id}'); RPM
     'Obsoletes: {f.from_id} < %{{version}}-%{{release}}' and
     'Provides: {f.from_id} = %{{version}}-%{{release}}'; metainfo
     <replaces><id>{f.from_appid}</id></replaces> and <provides>;
     desktop entry X-Flatpak-RenamedFrom={f.from_appid}.desktop;
     (decide whether to keep or drop the entries for older names).
  4. Flatpak: publish an end-of-life rebase for {f.from_appid}
     (end-of-life-rebase={t.to_appid}) in the old app's repo/Flathub.
  5. README 'Migrating from ...' and packaging/README 'Upgrading from ...'
     sections: rewrite for {f.from_display} -> {t.to_display}.
  6. Review the residual grep below and the full diff, run the test suites
     (cargo test, npm test, npm run build), then commit yourself.

WARNING: the migration only supports ONE previous name. After this rename
users still on a pre-{f.from_display} name (e.g. the name in the current legacy.rs)
who skip the {f.from_display} release will NOT be migrated automatically.
"""


def main() -> int:
    ap = argparse.ArgumentParser(
        prog="rebrand.py",
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    ap.add_argument("--from-display", required=True, help="current display name, e.g. Vessel")
    ap.add_argument("--to-display", required=True, help="new display name, e.g. Foo")
    ap.add_argument("--from-id", required=True, help="current lowercase id, e.g. vessel")
    ap.add_argument("--to-id", required=True, help="new lowercase id, e.g. foo")
    ap.add_argument("--from-appid", required=True, help="current app id, e.g. org.vessel.app")
    ap.add_argument("--to-appid", required=True, help="new app id, e.g. org.foo.app")
    ap.add_argument("--apply", action="store_true", help="perform the changes (default: dry run)")
    ap.add_argument("--allow-dirty", action="store_true", help="allow --apply on a dirty git tree")
    ap.add_argument("--force", action="store_true", help="proceed even if the new name already appears")
    ap.add_argument("--update-legacy", action="store_true", help="rewrite LEGACY_* values to the outgoing names")
    ap.add_argument("--rewrite-migrate", action="store_true", help="also rewrite crates/*/src/migrate*")
    ap.add_argument("--no-post", action="store_true", help="skip cargo fmt / cargo update after --apply")
    ap.add_argument("--root", default=None, help="repository root (default: git toplevel of cwd)")
    args = ap.parse_args()

    for k in ("from_id", "to_id"):
        if not ID_RE.match(getattr(args, k)):
            die(f"--{k.replace('_', '-')} {getattr(args, k)!r} must match {ID_RE.pattern}")
    for k in ("from_appid", "to_appid"):
        if not APPID_RE.match(getattr(args, k)):
            die(f"--{k.replace('_', '-')} {getattr(args, k)!r} is not a reverse-DNS app id")
    for k in ("from_display", "to_display"):
        v = getattr(args, k)
        if not v.strip() or any(c in v for c in "\"'`\\/\n"):
            die(f"--{k.replace('_', '-')} {v!r} is empty or contains quotes/slashes")
    if args.from_id == args.to_id or args.from_appid == args.to_appid:
        die("FROM and TO must differ")
    if args.to_id in args.from_id or args.from_id in args.to_id:
        die("one id contains the other; the replacement would be ambiguous")

    names = Names(args.from_display, args.to_display, args.from_id, args.to_id, args.from_appid, args.to_appid)
    root = Path(args.root or git(Path.cwd(), "rev-parse", "--show-toplevel").strip()).resolve()
    os.chdir(root)

    files = list_files(root)
    if not args.force:
        hits = check_new_name_absent(root, files, names)
        if hits:
            print("\n".join(hits[:20]), file=sys.stderr)
            die(f"the new name already appears {len(hits)} time(s) (first 20 above); use --force to proceed")

    if args.apply and not args.allow_dirty and git(root, "status", "--porcelain").strip():
        die("working tree is not clean; commit/stash first or pass --allow-dirty")

    rep = Replacer(names)
    plans, skipped = plan_content(root, files, rep, args)
    renames = plan_renames(files, rep)
    mode = "APPLY" if args.apply else "DRY RUN (pass --apply to perform)"
    print(f"== rebrand {names.from_display}/{names.from_id}/{names.from_appid} -> "
          f"{names.to_display}/{names.to_id}/{names.to_appid}: {mode}")

    print(f"\n== Path renames ({len(renames)}), deepest first")
    for old, new in renames:
        print(f"git mv {old} {new}")

    total = sum(sum(p.counts.values()) for p in plans)
    print(f"\n== Content replacements ({total} in {sum(1 for p in plans if p.counts)} files)")
    for p in plans:
        if p.counts:
            detail = ", ".join(f"{k}={v}" for k, v in sorted(p.counts.items()))
            print(f"{sum(p.counts.values()):5d}  {p.path}  ({detail})")

    prot = [(p.path, ln, why, txt) for p in plans for ln, why, txt in p.protected]
    print(f"\n== Protected, NOT rewritten ({len(prot)} lines)")
    for path, ln, why, txt in prot:
        print(f"[{why}] {path}:{ln}: {txt.strip()[:120]}")
    if skipped:
        print(f"\n== Skipped binary/non-UTF-8 files: {len(skipped)}")

    if args.update_legacy:
        print("\n== --update-legacy (after the rename; values become the outgoing names)")
        if not args.apply:
            print("(dry run: preview against the current legacy.rs)")
        for line in update_legacy(root, names, apply=False):
            print(line)

    if args.apply:
        for p in plans:
            if p.new_text is not None:
                (root / p.path).write_text(p.new_text, "utf-8")
        tracked = set(git(root, "ls-files", "-z").split("\0"))
        for old, new in renames:
            is_tracked = old in tracked or any(t.startswith(old + "/") for t in tracked)
            if is_tracked:
                git(root, "mv", old, new)
            else:
                os.rename(root / old, root / new)
        if args.update_legacy:
            update_legacy(root, names, apply=True)
        if not args.no_post:
            print("\n== Post-apply")
            post_apply(root)

    print(checklist(names, args))

    if args.apply:
        expected, unexpected = residual(root, names.from_id, args)
        print(f"== Residual grep -i '{names.from_id}' (excluding .git, target, node_modules, dist, legacy/, docs/plan/, Cargo.lock)")
        print(f"-- in protected areas ({len(expected)}):")
        for e in expected:
            print(e)
        print(f"-- OUTSIDE protected areas ({len(unexpected)}):")
        for u in unexpected:
            print(u)
        print("\nDone. Nothing was committed; review with `git status` and `git diff`.")
        return 3 if unexpected else 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
