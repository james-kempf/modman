// SPDX-License-Identifier: GPL-3.0-or-later
// Flat look: surfaces are solid fills (no gradients) and buttons have no
// glow (no coloured box-shadow / halo).
import { describe, expect, it } from "vitest";
import css from "./index.css?raw";
import { CARD_SHELL, CARD_TILE, LIST_ROW, PILL, TILE } from "./components/ui/styles";
import { MOD_PICTURE } from "./components/browse/Tile";

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

describe("radius tokens", () => {
  const src = (f: string) => sources[`./components/${f}`];
  const radii = (cls: string) => cls.match(/\brounded-[\w-]+/g) ?? [];
  // Fails when a control loses its full rounding (back to a rounded square) or a container turns into a pill
  // (a pill-shaped card or picture reads as broken): controls round with rounded-pill only, containers never do.
  it("rounds controls fully and containers as rectangles", () => {
    const sizes = src("ui/controls/Button.tsx").match(/const SIZES[\s\S]*?\n\};/)![0];
    expect(sizes.match(/^\s+\w+: "[^"]*"/gm)!.map(radii)).toEqual([["rounded-pill"], ["rounded-pill"], ["rounded-pill"], ["rounded-pill"]]);
    expect(cssFile.match(/\.input \{\s*@apply ([^;]*);/)![1]).toMatch(/\brounded-pill\b/);
    for (const f of ["ui/display/Badge.tsx", "ui/controls/Toggle.tsx", "ui/display/PadGlyph.tsx", "ui/controls/Tabs.tsx", "ui/controls/Segmented.tsx"])
      expect(src(f), f).toMatch(/\brounded-pill\b/);
    // Select: pill options inside a list rounded to match them (option height / 2 + its p-1).
    expect(src("ui/controls/Select.tsx")).toMatch(/role="option"[\s\S]*?\brounded-pill\b/);
    expect(src("ui/controls/Select.tsx")).toMatch(/\brounded-menu p-1\b/);
    expect(radii(PILL)).toEqual(["rounded-pill"]);
    for (const c of [CARD_SHELL, TILE, CARD_TILE, LIST_ROW, MOD_PICTURE]) {
      expect(radii(c), c).toHaveLength(1);
      expect(radii(c)[0], c).not.toBe("rounded-pill");
    }
    expect(src("dialogs/Dialog.tsx")).toMatch(/"glass w-full animate-pop rounded-card /);
  });

  // Fails when the retired control radii come back (rounded squares on controls) or a radius leaves the 4px grid.
  it("keeps the radius scale on the 4px grid with no control-square radius", () => {
    for (const [file, s] of Object.entries(sources)) expect(s, file).not.toMatch(/\brounded-control\b|\brounded-control-sm\b/);
    expect(cssFile).not.toMatch(/--radius-control/);
    for (const name of ["inset", "inner", "card", "card-lg"]) {
      const rem = Number(cssFile.match(new RegExp(`--radius-${name}: ([\\d.]+)rem;`))?.[1]);
      expect((rem * 16) % 4, name).toBe(0);
    }
    expect(cssFile).toMatch(/--radius-menu: calc\(var\(--spacing-control-sm\) \/ 2 \+ 0\.25rem\);/);
  });

  // Fails when a component uses Tailwind's raw radius scale (or an arbitrary radius) instead of the
  // --radius-* tokens, so changing --radius-card in index.css no longer reaches it.
  it("rounds only with the theme radius tokens", () => {
    for (const [file, src] of Object.entries(sources)) {
      expect(src, file).not.toMatch(
        /(?<![\w-])rounded(-[trblse]{1,2})?(-(none|xs|sm|md|lg|xl|2xl|3xl|4xl|full|\[[^\]]*\]))?(?![\w-])/,
      );
    }
  });
});

// vitest.config's `css: false` empties `index.css?raw`; read the file itself (no @types/node, hence the untyped id).
const fsId = "node:fs";
const { readFileSync } = (await import(/* @vite-ignore */ fsId)) as { readFileSync(p: string, enc: "utf8"): string };
// Relative to the vitest root (apps/desktop); import.meta.url is not a file: URL under jsdom.
const cssFile = readFileSync("src/index.css", "utf8");

describe("focus and hover highlight", () => {
  const css = cssFile;
  const token = (name: string) => css.match(new RegExp(`${name}:\\s*([^;]+);`))?.[1] ?? "";
  const variant = (name: string) => css.match(new RegExp(`@custom-variant ${name} \\{[\\s\\S]*?\\n\\}`))?.[0] ?? "";
  const layer = css.slice(css.indexOf("@layer utilities"));
  // The interactive-control list of the hover branch (also the :has() list that hands hover to the innermost control).
  const controls = (v: string) => v.match(/&:is\(([^)]*)\)|:is\(([^)]*)\):not/)?.slice(1).find(Boolean) ?? "";

  // Fails when nav focus and mouse hover stop sharing one definition (the two highlights drift apart), when hover
  // reaches plain containers (no element list), shows on disabled controls, or sticks on touch screens (no hover media).
  it("defines one highlighted variant for nav focus and hover, plus its descendant form", () => {
    for (const name of ["highlighted", "in-highlight"]) {
      const v = variant(name);
      expect(v, name).toMatch(/html\[data-input="nav"\] &?:focus/);
      expect(v, name).toMatch(/@media \(hover: hover\)/);
      expect(v, name).toMatch(/:not\(:disabled, \[aria-disabled="true"\], :has\(:is\(/);
      expect(controls(v), name).toBe('button, a[href], [role="option"], li[tabindex]');
    }
  });

  // Fails when any highlight drifts from the one shared solid white (tints, per-control fills, an outline or ring).
  it("paints the highlight with the one solid white token, no outline", () => {
    expect(token("--color-focus")).toBe("var(--color-zinc-100)");
    expect(css).toMatch(/\* \{\s*@variant highlighted \{\s*box-shadow: inset 0 0 0 100vmax var\(--color-focus\);\s*\}/);
    expect(css).not.toMatch(/--color-focus-fill/);
  });

  // Fails when light text stays light on the white: the control and everything inside turn dark, muted text a readable
  // grey, focused-field placeholders too (all >= 4.5:1 on zinc-100); in the utilities layer to beat text-* utilities.
  it("darkens text inside the highlighted control above the utilities", () => {
    expect(layer).toMatch(/\* \{\s*@variant highlighted \{\s*color: var\(--focus-text, var\(--color-focus-ink\)\);/);
    expect(layer).toMatch(/\* \{\s*@variant in-highlight \{\s*color: inherit;/);
    expect(layer).toMatch(/:is\(\.text-dim, \.text-fg-2, \.text-zinc-400, \.text-zinc-500\) \{\s*@variant in-highlight \{\s*color: var\(--color-focus-ink-muted\);/);
    expect(layer).toMatch(/:focus::placeholder \{\s*color: var\(--color-focus-ink-muted\);/);
    expect(token("--color-focus-ink")).toBe("var(--color-zinc-900)");
    expect(token("--color-focus-ink-muted")).toBe("var(--color-zinc-600)");
  });

  // Fails when a control keeps its own hover look under the shared white (or the old variant names come back).
  it("leaves no per-control hover colours or old focus variants", () => {
    for (const [file, src] of Object.entries(sources)) {
      expect(src, file).not.toMatch(/\bhover:(bg|text|border)-/);
      expect(src, file).not.toMatch(/\b(in-)?nav-focus:/);
    }
  });

  // Fails when the highlight fades in or out (focus or hover, either direction) instead of snapping: colour, fill,
  // border and shadow must not animate in either input mode; transform and opacity still may.
  it("snaps the highlight: no colour or shadow transitions in any input mode", () => {
    expect(layer.match(/html\[data-input\] \* \{\s*transition-property: ([^;]+);/)?.[1]).toBe(
      "transform, translate, scale, rotate, opacity",
    );
    expect(css).not.toMatch(/transition: box-shadow/);
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

  // Fails when nav focus grows controls again.
  it("does not scale on focus", () => {
    for (const [file, src] of Object.entries(sources)) {
      expect(src, file).not.toMatch(/(nav-focus|highlighted):scale-/);
    }
  });
});

describe("form control contrast", () => {
  const css = cssFile;
  const token = (name: string) => css.match(new RegExp(`${name}:\\s*([^;]+);`))?.[1] ?? "";
  const input = css.match(/\.input \{\s*@apply ([^;]+);/)?.[1] ?? "";

  // Fails when the placeholder drops back to zinc-500 (4.05:1 on ink, 3.54:1 on the focused ink-3), below AA.
  it("draws placeholders in --color-dim (>= 4.5:1 on ink, ink-2 and ink-3)", () => {
    expect(input).toMatch(/(?<![\w-])placeholder:text-dim(?![\w-])/);
    expect(input).not.toMatch(/placeholder:text-zinc-/);
  });

  // Fails when the field edge goes back to the 8% white hairline (1.19:1), or the control token is lost or
  // turned translucent (its 3:1 against ink-3 is computed for an opaque colour).
  it("outlines fields with the opaque --color-line-control token (>= 3:1 boundary)", () => {
    expect(input).toMatch(/(?<![\w-])border-line-control(?![\w-])/);
    expect(input).not.toMatch(/(?<![\w-])border-line(?![\w-])/);
    expect(token("--color-line-control")).toBe("oklch(0.53 0.012 286)");
  });
});

describe("activatable card press", () => {
  const cls = CARD_TILE.split(/\s+/);

  // Fails when a game or Browse tile gives no press feedback (buttons and tabs do), when the press stops being a
  // transform (a fill or size change would fight the highlight or shift the grid), or when it reaches an inert
  // tile (the dimmed Browse <div aria-disabled>) or a disabled/aria-disabled card.
  it("presses enabled cards with a small transform only", () => {
    const press = cls.filter((c) => c.includes("active:"));
    expect(press.sort()).toEqual(
      ["enabled:not-aria-disabled:active:scale-99", "enabled:not-aria-disabled:active:translate-y-px"].sort(),
    );
    expect(cls).toContain("transition-[background-color,border-color,color,translate,scale]");
  });
});
