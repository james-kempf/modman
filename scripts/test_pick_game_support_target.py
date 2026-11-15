#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Fixture tests for pick_game_support_target.py (`--rerun` and forced picks).

Each test builds a throwaway repo (registry + definitions) and a bare git
repo as the remote; `open_prs` is replaced so `gh` is never called.

Run: python3 -m unittest scripts/test_pick_game_support_target.py
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import os
import subprocess
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path
from unittest import mock

_spec = importlib.util.spec_from_file_location(
    "pick_game_support_target", Path(__file__).with_name("pick_game_support_target.py")
)
picker = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(picker)

NOW = datetime(2026, 11, 15, 9, 7, tzinfo=timezone.utc)
REGISTRY = """
const BUILTIN: &[(&str, &str)] = &[
    def!("pending1"),
    def!("tested1"),
    def!("marked1"),
];
"""


def toml(domain: str, support: str, marker: bool = False) -> str:
    head = "# SPDX-License-Identifier: GPL-3.0-or-later\n"
    if marker:
        head += f"{picker.MARKER}\n"
    return f'{head}domain = "{domain}"\nname = "{domain.title()}"\nsupport = "{support}"\n'


def git(cwd: Path, *args: str) -> None:
    subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True)


class Picker(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name, "repo")
        self.remote = Path(tmp.name, "remote.git")
        reg = self.root / picker.REGISTRY
        reg.parent.mkdir(parents=True)
        reg.write_text(REGISTRY, encoding="utf-8")
        defs = self.root / picker.DEFS_DIR
        defs.mkdir(parents=True)
        (defs / "pending1.toml").write_text(toml("pending1", "experimental"))
        (defs / "tested1.toml").write_text(toml("tested1", "tested", marker=True))
        (defs / "marked1.toml").write_text(toml("marked1", "experimental", marker=True))
        git(self.root.parent, "init", "-q", "--bare", str(self.remote))
        git(self.root, "init", "-q")
        git(self.root, "-c", "user.name=t", "-c", "user.email=t@t", "commit",
            "-q", "--allow-empty", "-m", "init")
        self.prs: dict[str, str] = {}

    def push_branch(self, suffix: str) -> None:
        git(self.root, "push", "-q", str(self.remote), f"HEAD:refs/heads/{picker.BRANCH_PREFIX}{suffix}")

    def pick(self, *args: str) -> tuple[int, dict | None, str]:
        out, err = io.StringIO(), io.StringIO()
        env = {k: v for k, v in os.environ.items() if k != "GITHUB_OUTPUT"}
        with mock.patch.object(picker, "open_prs", lambda _root: self.prs), \
                mock.patch.dict(os.environ, env, clear=True), \
                contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            rc = picker.main(["--root", str(self.root), "--remote", str(self.remote), *args],
                             now=NOW)
        return rc, (json.loads(out.getvalue()) if out.getvalue() else None), err.getvalue()

    # Catches: --rerun silently ignored when no game is forced.
    def test_rerun_without_game_fails(self):
        rc, _, err = self.pick("--rerun")
        self.assertEqual(rc, 2)
        self.assertIn("--rerun needs --game", err)

    # Catches: --rerun loosening the plain forced pick (tested stays rejected).
    def test_tested_rejected_without_rerun(self):
        rc, _, err = self.pick("--game", "tested1")
        self.assertEqual(rc, 2)
        self.assertIn("not experimental", err)

    # Catches: a tested game still being refused under --rerun.
    def test_tested_accepted_with_rerun(self):
        rc, res, _ = self.pick("--game", "tested1", "--rerun")
        self.assertEqual(rc, 0)
        self.assertEqual(res["domain"], "tested1")
        self.assertEqual(res["branch"], "vessel/game-support-tested1")
        self.assertTrue(res["rerun"])

    # Catches: a completed (marker) experimental game refused under --rerun.
    def test_marker_accepted_with_rerun(self):
        rc, res, _ = self.pick("--game", "marked1", "--rerun")
        self.assertEqual((rc, res["domain"]), (0, "marked1"))

    # Catches: reusing / force-pushing the old remote branch on a re-run.
    def test_existing_branch_gives_rerun_name(self):
        self.push_branch("tested1")
        rc, res, _ = self.pick("--game", "tested1", "--rerun")
        self.assertEqual(rc, 0)
        self.assertEqual(res["branch"], "vessel/game-support-tested1-rerun-202611150907")

    # Catches: plain forced pick no longer blocked by an existing branch.
    def test_existing_branch_blocks_without_rerun(self):
        self.push_branch("pending1")
        rc, _, err = self.pick("--game", "pending1")
        self.assertEqual(rc, 2)
        self.assertIn("already has a branch or open PR", err)

    # Catches: a re-run racing an open PR from the base or a -rerun- branch.
    def test_open_pr_rejected_with_url(self):
        url = "https://github.com/o/r/pull/7"
        for suffix in ("tested1", "tested1-rerun-202601010000"):
            with self.subTest(suffix=suffix):
                self.prs = {suffix: url}
                rc, _, err = self.pick("--game", "tested1", "--rerun")
                self.assertEqual(rc, 2)
                self.assertIn(url, err)

    # Catches: an open PR for another domain sharing the prefix blocking this one.
    def test_open_pr_of_other_domain_ignored(self):
        self.prs = {"tested1x": "https://github.com/o/r/pull/8"}
        rc, _, _ = self.pick("--game", "tested1", "--rerun")
        self.assertEqual(rc, 0)

    # Catches: --rerun accepting a domain with no built-in definition.
    def test_unknown_domain_rejected(self):
        rc, _, err = self.pick("--game", "nosuchgame", "--rerun")
        self.assertEqual(rc, 2)
        self.assertIn("no built-in definition", err)

    # Catches: the default (unforced) pick changing (order, skip marker/tested).
    def test_default_pick_unchanged(self):
        rc, res, _ = self.pick()
        self.assertEqual(rc, 0)
        self.assertEqual(res["domain"], "pending1")
        self.assertEqual(res["branch"], "vessel/game-support-pending1")
        self.assertNotIn("rerun", res)


if __name__ == "__main__":
    unittest.main()
