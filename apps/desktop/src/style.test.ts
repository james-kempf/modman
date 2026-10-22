// SPDX-License-Identifier: GPL-3.0-or-later
// Flat look: surfaces are solid fills (no gradients) and buttons have no
// glow (no coloured box-shadow / halo).
import { describe, expect, it } from "vitest";
import css from "./index.css?raw";

const sources = import.meta.glob(["./**/*.tsx", "!./**/*.test.tsx"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

describe("flat look", () => {
  // Fails when a gradient comes back on a card, panel, monogram or the island.
  it("uses no gradients in the stylesheet or class names", () => {
    expect(css).not.toMatch(/gradient\(/);
    for (const [file, src] of Object.entries(sources)) {
      expect(src, file).not.toMatch(/bg-gradient-|bg-linear-|bg-radial-|from-\w+-\d+\/|via-\w+-\d+\//);
    }
  });

  // Fails when a button variant regains a glow (shadow classes or the hero .glow disc).
  it("gives buttons and surfaces no glow or shadow classes", () => {
    for (const [file, src] of Object.entries(sources)) {
      expect(src, file).not.toMatch(/\bshadow-\[|\bshadow-(sm|md|lg|xl|2xl)\b|className="glow"/);
    }
    expect(css).not.toMatch(/\.glow\b/);
    for (const cls of ["surface", "panel"]) {
      const rule = new RegExp(`\\.${cls}\\s*\\{([^}]*)\\}`).exec(css)?.[1] ?? "";
      expect(rule, cls).not.toMatch(/box-shadow/);
    }
  });
});
