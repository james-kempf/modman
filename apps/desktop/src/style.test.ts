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
  });

  // Fails when a page brings back its own card shell (the legacy .panel) instead of Card.
  it("has no .panel card shell; cards use Card from ui.tsx", () => {
    expect(css).not.toMatch(/\.panel\b/);
    for (const [file, src] of Object.entries(sources)) {
      expect(src, file).not.toMatch(/className=(\{cx\()?"(panel|[^"]* panel)\b/);
    }
  });
});

// vitest.config's `css: false` empties `index.css?raw`; read the file itself (no @types/node, hence the untyped id).
const fsId = "node:fs";
const { readFileSync } = (await import(/* @vite-ignore */ fsId)) as { readFileSync(p: string, enc: "utf8"): string };
// Relative to the vitest root (apps/desktop); import.meta.url is not a file: URL under jsdom.
const cssFile = readFileSync("src/index.css", "utf8");

describe("navigation focus highlight", () => {
  const css = cssFile;
  const rule = (sel: string) => css.match(new RegExp(`\\n  ${sel.replace(/[[\]().*:"=]/g, "\\$&")} \\{[^}]*\\}`))?.[0] ?? "";
  const navFocus = rule('html[data-input="nav"] :focus');
  const token = (name: string) => css.match(new RegExp(`${name}:\\s*([^;]+);`))?.[1] ?? "";

  // Fails when any nav highlight drifts from the one shared solid white (tints, per-control fills, an outline or ring).
  it("paints every nav focus with the one solid white token, no outline", () => {
    expect(token("--color-focus")).toBe("var(--color-zinc-100)");
    expect(navFocus).toMatch(/box-shadow:\s*inset 0 0 0 100vmax var\(--color-focus\);/);
    expect(navFocus).not.toMatch(/outline/);
    expect(css).not.toMatch(/--color-focus-fill/);
    for (const [file, src] of Object.entries(sources)) {
      expect(src, file).not.toMatch(/(?<!in-)nav-focus:(bg|shadow)-/);
    }
  });

  // Fails when light text stays light on the white fill: the focused control and everything inside it must turn dark,
  // muted text to a readable grey, placeholders too (all >= 4.5:1 on zinc-100).
  it("darkens text inside the focused control above the utilities", () => {
    const layer = css.slice(css.indexOf("@layer utilities"));
    expect(layer).toMatch(/html\[data-input="nav"\] :focus \{\s*color: var\(--focus-text, var\(--color-focus-ink\)\);/);
    expect(layer).toMatch(/html\[data-input="nav"\] :focus \* \{\s*color: inherit;/);
    expect(layer).toMatch(/:focus :is\(\.text-dim, \.text-zinc-400, \.text-zinc-500\) \{\s*color: var\(--color-focus-ink-muted\);/);
    expect(layer).toMatch(/:focus::placeholder \{\s*color: var\(--color-focus-ink-muted\);/);
    expect(token("--color-focus-ink")).toBe("var(--color-zinc-900)");
    expect(token("--color-focus-ink-muted")).toBe("var(--color-zinc-600)");
  });

  // Fails when the nav highlight fades in or out instead of snapping: a transition on the focus rule, or a
  // colour property (transition-colors etc. on the control) still animating in nav mode, where focus-out uses it too.
  it("snaps the nav highlight: no colour or shadow transitions in nav mode", () => {
    expect(navFocus).not.toMatch(/transition/);
    const layer = css.slice(css.indexOf("@layer utilities"));
    const snap = layer.match(/html\[data-input="nav"\] \* \{\s*transition-property: ([^;]+);/)?.[1] ?? "";
    expect(snap).toBe("transform, translate, scale, rotate, opacity");
  });

  // Fails when the descendant variant used by toggles, marks and monograms is not defined (classes dropped silently).
  it("defines the in-nav-focus variant", () => {
    expect(css).toMatch(/@custom-variant in-nav-focus \(html\[data-input="nav"\] :focus &\);/);
  });

  // Fails when nav focus grows controls again.
  it("does not scale on nav focus", () => {
    for (const [file, src] of Object.entries(sources)) {
      expect(src, file).not.toMatch(/nav-focus:scale-/);
    }
  });

  // Fails when outline-color can animate from currentColor (white text) to transparent:
  // Tailwind's transition-colors includes outline-color, so focusing a field or tab flashed a white ring.
  it("keeps outline-color transparent at rest and on focus, so nothing animates", () => {
    expect(css).toMatch(/\n  \* \{[^}]*outline-color: transparent;/);
    expect(css).toMatch(/\n  :focus \{\s*outline: 2px solid transparent;\s*\}/);
  });

  // Fails when focusing a text field turns its border light (a white border on focus).
  it("does not lighten the field border on focus", () => {
    expect(css).not.toMatch(/(?<![\w-])focus:border-/);
    for (const [file, src] of Object.entries(sources)) {
      expect(src, file).not.toMatch(/(?<![\w-])focus(-visible|-within)?:(border|ring|outline)-/);
    }
  });
});
