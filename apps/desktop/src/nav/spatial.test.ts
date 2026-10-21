// SPDX-License-Identifier: GPL-3.0-or-later
import { afterEach, describe, expect, it } from "vitest";
import { findNext, moveFocus, navScope } from "./spatial";

/** Button with a fixed layout box (jsdom has no layout). */
function btn(id: string, x: number, y: number, w = 100, h = 50, parent: HTMLElement = document.body) {
  const b = document.createElement("button");
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
