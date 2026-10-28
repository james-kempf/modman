// SPDX-License-Identifier: GPL-3.0-or-later
import { afterEach, describe, expect, it, vi } from "vitest";
import { findNext, installFocusRecovery, moveFocus, navScope } from "./spatial";

/** Button with a fixed layout box (jsdom has no layout). */
function btn(id: string, x: number, y: number, w = 100, h = 50, parent: HTMLElement = document.body, tag = "button") {
  const b = document.createElement(tag) as HTMLButtonElement;
  b.id = id;
  b.textContent = id;
  b.getBoundingClientRect = () => ({ left: x, top: y, right: x + w, bottom: y + h, width: w, height: h, x, y, toJSON() {} }) as DOMRect;
  parent.appendChild(b);
  return b;
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("spatial navigation", () => {
  // Grid:  a b
  //        c d
  //          e (offset to the right)
  const grid = () => ({
    a: btn("a", 0, 0),
    b: btn("b", 200, 0),
    c: btn("c", 0, 100),
    d: btn("d", 200, 100),
    e: btn("e", 400, 200),
  });

  it("moves to the nearest element in the same column/row, not a diagonal one", () => {
    const g = grid();
    expect(findNext(g.a, "down")).toBe(g.c);
    expect(findNext(g.a, "right")).toBe(g.b);
    expect(findNext(g.d, "up")).toBe(g.b);
    expect(findNext(g.d, "left")).toBe(g.c);
  });

  it("returns null at an edge instead of wrapping", () => {
    const g = grid();
    expect(findNext(g.a, "up")).toBeNull();
    expect(findNext(g.a, "left")).toBeNull();
  });

  it("reaches an offset element when nothing is aligned", () => {
    const g = grid();
    expect(findNext(g.d, "down")).toBe(g.e);
  });

  it("skips disabled and inert elements", () => {
    const g = grid();
    g.c.disabled = true;
    const wrap = document.createElement("div");
    wrap.setAttribute("inert", "");
    document.body.appendChild(wrap);
    btn("hidden-below", 0, 300, 100, 50, wrap);
    expect(findNext(g.a, "down")).toBe(g.d);
  });

  it("stays inside the topmost dialog scope", () => {
    grid();
    const dlg = document.createElement("div");
    dlg.setAttribute("data-nav-scope", "");
    document.body.appendChild(dlg);
    const x = btn("x", 0, 500, 100, 50, dlg);
    const y = btn("y", 0, 600, 100, 50, dlg);
    expect(navScope()).toBe(dlg);
    document.getElementById("a")!.focus();
    // Focus outside the scope: enter the scope at its first element.
    expect(moveFocus("down")).toBe(x);
    expect(moveFocus("down")).toBe(y);
    expect(moveFocus("down")).toBeNull();
    expect(document.activeElement).toBe(y);
  });

  it("falls back to document order without layout", () => {
    const p = document.createElement("button");
    const q = document.createElement("button");
    document.body.append(p, q);
    expect(findNext(p, "down")).toBe(q);
    expect(findNext(q, "up")).toBe(p);
    expect(findNext(q, "right")).toBeNull();
  });
});

/** Settings at 1280x800 (signed out, no key typed) as measured by T16; `dy` = main scroll. */
function settings(dy = 0) {
  const nav = document.createElement("nav");
  nav.setAttribute("data-nav-rail", "");
  const main = document.createElement("main");
  document.body.append(nav, main);
  const tabs = ["overview", "mods", "install", "collections", "requirements", "activity", "settings"].map((t, i) =>
    btn(t, 12, 68 + 52 * i, 199, 48, nav),
  );
  return {
    tabs,
    input: btn("key", 281, 313 - dy, 942, 48, main, "input"),
    paste: btn("Paste", 281, 373 - dy, 83, 48, main),
    keyboard: btn("Keyboard", 376, 373 - dy, 114, 48, main),
    toggle: btn("Auto-update off", 1022, 495 - dy, 201, 48, main),
    config: btn("config", 281, 919 - dy, 853, 48, main, "input"),
  };
}

describe("spatial scoring on the Settings layout (T16 F2a/F3)", () => {
  it("Right from Keyboard reaches the toggle, not the overlapping wide field above", () => {
    const s = settings();
    expect(findNext(s.keyboard, "right")).toBe(s.toggle);
  });
  it("Up from a scrolled config field reaches the toggle, not the tab rail", () => {
    const s = settings(511);
    expect(findNext(s.config, "up")).toBe(s.toggle);
  });
  it("Down from Keyboard/Paste stays in content instead of jumping to the rail", () => {
    const s = settings();
    expect(findNext(s.keyboard, "down")).toBe(s.config);
    expect(findNext(s.paste, "down")).toBe(s.config);
  });
  it("Down still moves along the rail and the rail is left horizontally", () => {
    const s = settings();
    expect(findNext(s.tabs[5], "down")).toBe(s.tabs[6]);
    expect(findNext(s.tabs[6], "right")).toBe(s.paste);
  });
});

describe("focus recovery (T16 F1)", () => {
  it("puts focus back near where it was lost when the focused element goes away", async () => {
    const a = btn("a", 0, 0);
    const b = btn("b", 0, 100);
    btn("far", 900, 900);
    // jsdom reports no document focus once nothing is focused; a browser window still has it.
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    const stop = installFocusRecovery();
    try {
      a.focus();
      // A browser drops focus to <body> (focusout, no relatedTarget) when the
      // focused element is disabled or unmounted; jsdom does not, so blur first.
      a.blur();
      a.remove();
      await new Promise((r) => requestAnimationFrame(() => r(null)));
      await new Promise((r) => requestAnimationFrame(() => r(null)));
      expect(document.activeElement).toBe(b);
    } finally {
      stop();
      vi.restoreAllMocks();
    }
  });
});

describe("card sections (opt-in via data-nav-sections)", () => {
  const el = (parent: HTMLElement, attrs: Record<string, string> = {}, tag = "div") => {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
    parent.appendChild(e);
    return e;
  };
  /**
   * Settings-like single column, controls right-aligned (x up to 1000):
   *   card 1: S1 [t1 toggle]  S2 [i2 input][s2 Save]  S3 (text only)
   *   card 2: S4 [b4a far left] [b4b in s2's beam]  S5 [t5]
   */
  function page(opt = true) {
    const cards = [0, 1].map(() => el(el(document.body, {}, "section"), opt ? { "data-nav-sections": "" } : {}));
    const sec = (card: HTMLElement) => el(card, { "data-nav-section": "" });
    const [s1, s2, s3] = [sec(cards[0]), sec(cards[0]), sec(cards[0])];
    s3.textContent = "no controls here";
    const [s4, s5] = [sec(cards[1]), sec(cards[1])];
    return {
      t1: btn("t1", 900, 0, 60, 40, s1),
      i2: btn("i2", 400, 100, 440, 40, s2, "input"),
      s2: btn("s2", 850, 100, 100, 40, s2),
      b4a: btn("b4a", 0, 300, 100, 40, s4),
      b4b: btn("b4b", 850, 300, 100, 40, s4),
      t5: btn("t5", 900, 400, 60, 40, s5),
      s4,
    };
  }

  // Fails when down follows the beam to the in-line Save button instead of the next section's first control.
  it("down moves to the first control of the next section", () => {
    const p = page();
    expect(findNext(p.t1, "down")).toBe(p.i2);
  });

  // Fails when crossing cards picks the aligned control, or stops at a section with no controls.
  it("down crosses into the next card, skipping sections without controls", () => {
    const p = page();
    expect(findNext(p.s2, "down")).toBe(p.b4a);
    expect(findNext(p.i2, "down")).toBe(p.b4a);
  });

  // Fails when up lands on the aligned control instead of the previous section's first one.
  it("up moves to the first control of the previous section or card", () => {
    const p = page();
    expect(findNext(p.b4b, "up")).toBe(p.i2);
    expect(findNext(p.t5, "up")).toBe(p.b4a);
  });

  // Fails when section navigation wraps around at either end.
  it("does not wrap at the first or last section", () => {
    const p = page();
    expect(findNext(p.t1, "up")).toBeNull();
    expect(findNext(p.t5, "down")).toBeNull();
  });

  // Fails when stacked controls inside one section become unreachable with up/down.
  it("moves between stacked controls inside a section first", () => {
    const p = page();
    const below = btn("below", 0, 350, 100, 40, p.s4);
    expect(findNext(p.b4a, "down")).toBe(below);
    expect(findNext(below, "up")).toBe(p.b4a);
  });

  // Fails when the section rule leaks to cards that did not opt in (other pages).
  it("keeps plain geometric navigation without the opt-in", () => {
    const p = page(false);
    expect(findNext(p.t1, "down")).toBe(p.s2);
  });

  // Fails when left/right start following sections.
  it("leaves left/right geometric", () => {
    const p = page();
    expect(findNext(p.i2, "right")).toBe(p.s2);
    expect(findNext(p.b4b, "left")).toBe(p.b4a);
  });

  // Fails when an open overlay inside a section (a Select listbox scope) jumps out by section.
  it("ignores sections outside the current nav scope", () => {
    const p = page();
    const list = el(p.s4, { "data-nav-scope": "" });
    const o1 = btn("o1", 850, 340, 100, 30, list);
    const o2 = btn("o2", 850, 370, 100, 30, list);
    expect(findNext(o1, "down")).toBe(o2);
    expect(findNext(o2, "down")).toBeNull();
  });
});
