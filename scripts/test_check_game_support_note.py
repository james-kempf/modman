#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Fixture tests for check_game_support_note.py (`## Loader routes` rules).

Run: python3 -m unittest scripts/test_check_game_support_note.py
"""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "check_game_support_note", Path(__file__).with_name("check_game_support_note.py")
)
checker = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(checker)

MELON = """
[[loaders]]
key = "melonloader"
name = "MelonLoader"
nexus_mod_id = 1196
required_paths = ["version.dll", "MelonLoader"]
[loaders.launch]
dll_overrides = ["version"]
"""

HEADER = "| Loader | Route | Identifier | Handled by |\n|---|---|---|---|\n"


def definition(support: str, loaders: str = "") -> str:
    return (
        "# SPDX-License-Identifier: GPL-3.0-or-later\n"
        f"{checker.MARKER}\n"
        'domain = "g"\nname = "G"\nprofile = "melonloader"\n'
        f'support = "{support}"\n{loaders}'
    )


def note(routes: str | None, gaps: str = "None") -> str:
    parts = ["# G (`g`): game support\n", "## Summary\nx\n"]
    if routes is not None:
        parts.append(f"## Loader routes\n{routes}\n")
    parts += [f"## Gaps\n{gaps}\n", "## Sources\n1. https://example.org (2026-11-15)\n"]
    return "\n".join(parts)


class LoaderRoutes(unittest.TestCase):
    def run_check(self, defn: str, text: str) -> tuple[list[str], str]:
        with tempfile.TemporaryDirectory() as d:
            dp, np = Path(d, "g.toml"), Path(d, "g.md")
            dp.write_text(defn, encoding="utf-8")
            np.write_text(text, encoding="utf-8")
            errors, outcome, _ = checker.check(dp, np)
        return errors, outcome

    def assert_error(self, errors: list[str], fragment: str) -> None:
        self.assertTrue(any(fragment in e for e in errors), errors)

    # Catches: old-format notes (no section) still passing.
    def test_missing_section_fails(self):
        errors, _ = self.run_check(definition("tested"), note(None))
        self.assert_error(errors, "'## Loader routes' section missing")

    # Catches: `None` being rejected for a game without loaders.
    def test_none_without_loaders_passes(self):
        errors, outcome = self.run_check(definition("tested"), note("None"))
        self.assertEqual((errors, outcome), ([], "full"))

    # Catches: a game with [[loaders]] skipping the route analysis via `None`.
    def test_none_with_loaders_fails(self):
        errors, _ = self.run_check(definition("tested", MELON), note("None"))
        self.assert_error(errors, "is None but the definition has [[loaders]] (melonloader)")

    # Catches: a header-only table counting as a route list.
    def test_table_without_rows_fails(self):
        errors, _ = self.run_check(definition("tested", MELON), note(HEADER))
        self.assert_error(errors, "at least one row")

    # Catches: a wrong or reordered header being accepted.
    def test_wrong_header_fails(self):
        routes = "| Loader | Handled by |\n|---|---|\n| MelonLoader | `loaders.nexus_mod_id` |"
        errors, _ = self.run_check(definition("tested", MELON), note(routes))
        self.assert_error(errors, "must be exactly 'None' or a table")

    # Catches: a correct, fully mapped table being rejected.
    def test_mapped_routes_pass(self):
        routes = HEADER + (
            "| MelonLoader | Nexus page | schedule1 1196 | "
            "`loaders.nexus_mod_id` + `loaders.required_paths` |\n"
            "| melonloader | Proton launch | version.dll | `loaders.launch.dll_overrides` |"
        )
        errors, outcome = self.run_check(definition("tested", MELON), note(routes))
        self.assertEqual((errors, outcome), ([], "full"))

    # Catches: the schedule1 case, a Nexus route "handled" by a field the
    # loader entry does not set.
    def test_field_not_set_on_loader_fails(self):
        loaders = MELON.replace("nexus_mod_id = 1196\n", "")
        routes = HEADER + "| MelonLoader | Nexus page | 1196 | `loaders.nexus_mod_id` |"
        errors, _ = self.run_check(definition("tested", loaders), note(routes))
        self.assert_error(errors, "'loaders.nexus_mod_id' is not set on a [[loaders]] entry")

    # Catches: `loaders.*` fields credited to a loader with no entry.
    def test_field_on_unknown_loader_fails(self):
        routes = HEADER + (
            "| MelonLoader | Nexus page | 1196 | `loaders.nexus_mod_id` |\n"
            "| BepInEx | Nexus page | 1 | `loaders.nexus_mod_id` |"
        )
        errors, _ = self.run_check(definition("tested", MELON), note(routes))
        self.assert_error(errors, "not set on a [[loaders]] entry for 'BepInEx'")

    # Catches: a top-level field reference that does not exist.
    def test_unknown_top_level_field_fails(self):
        routes = HEADER + (
            "| MelonLoader | Nexus page | 1196 | `loaders.nexus_mod_id` |\n"
            "| SMAPI | Nexus page | 2400 | `smapi_source` |"
        )
        errors, _ = self.run_check(definition("tested", MELON), note(routes))
        self.assert_error(errors, "'smapi_source' is not set in the definition")

    # Catches: built-in loaders (selected by a top-level field) being rejected.
    def test_top_level_field_passes(self):
        routes = HEADER + "| SMAPI | Nexus page | stardewvalley 2400 | `domain` |"
        errors, outcome = self.run_check(definition("tested"), note(routes))
        self.assertEqual((errors, outcome), ([], "full"))

    # Catches: free text in handled-by passing as a field.
    def test_free_text_handled_by_fails(self):
        routes = HEADER + "| MelonLoader | Nexus page | 1196 | config |"
        errors, _ = self.run_check(definition("tested", MELON), note(routes))
        self.assert_error(errors, "must be 'Gap' or backticked definition fields")

    # Catches: a [[loaders]] entry whose routes were never analysed.
    def test_loader_without_row_fails(self):
        routes = HEADER + "| SMAPI | Nexus page | 2400 | `domain` |"
        errors, _ = self.run_check(definition("tested", MELON), note(routes))
        self.assert_error(errors, "no row for [[loaders]] 'melonloader'")

    # Catches: claiming full support while a loader route is a Gap.
    def test_gap_row_with_full_outcome_fails(self):
        routes = HEADER + (
            "| MelonLoader | Nexus page | 1196 | `loaders.nexus_mod_id` |\n"
            "| MelonLoader | Off-site requirement | melonwiki.xyz | Gap |"
        )
        errors, _ = self.run_check(definition("tested", MELON), note(routes))
        self.assert_error(errors, "Gaps is None but 1 loader route(s) are a Gap")

    # Catches: a Gap row with no Rust change recorded for its loader.
    def test_gap_row_without_matching_bullet_fails(self):
        routes = HEADER + "| MelonLoader | Off-site requirement | melonwiki.xyz | Gap |"
        gaps = "- requirements.rs: something unrelated"
        errors, _ = self.run_check(definition("experimental", MELON), note(routes, gaps))
        self.assert_error(errors, "'MelonLoader' row is a Gap but no Gaps bullet names it")

    # Catches: a correctly recorded Gap being rejected.
    def test_gap_row_with_bullet_and_experimental_passes(self):
        routes = HEADER + (
            "| MelonLoader | Nexus page (installer exe) | schedule1 1196 | Gap |\n"
            "| MelonLoader | Off-site requirement | melonwiki.xyz | Gap |"
        )
        gaps = "- loaders: Melon Loader needs a GitHub release source (declarative.rs)"
        errors, outcome = self.run_check(definition("experimental", MELON), note(routes, gaps))
        self.assertEqual((errors, outcome), ([], "partial"))


if __name__ == "__main__":
    unittest.main()
