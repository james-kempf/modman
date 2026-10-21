// SPDX-License-Identifier: GPL-3.0-or-later
// Geometric spatial navigation over real DOM focus (so the native focus ring
// and form controls keep working). The scope is the topmost element marked
// `data-nav-scope` (an open dialog), else the whole document.

export type Dir = "up" | "down" | "left" | "right";

const FOCUSABLE = [
  "button:not([disabled])",
  "a[href]",
  "input:not([disabled]):not([type=hidden])",
  "textarea:not([disabled])",
  "select:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

export function navScope(doc: Document = document): HTMLElement {
  const scopes = doc.querySelectorAll<HTMLElement>("[data-nav-scope]");
  return scopes.length ? scopes[scopes.length - 1] : doc.body;
}

function visible(el: HTMLElement): boolean {
  if (el.closest("[inert],[hidden],[aria-hidden='true']")) return false;
  // jsdom has no layout: treat everything as visible there.
  const style = el.ownerDocument.defaultView?.getComputedStyle(el);
  return !style || (style.visibility !== "hidden" && style.display !== "none");
}

export function focusables(scope: HTMLElement = navScope()): HTMLElement[] {
  return Array.from(scope.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(visible);
}

interface Box {
  l: number;
  r: number;
  t: number;
  b: number;
  cx: number;
  cy: number;
}
const box = (el: Element): Box => {
  const r = el.getBoundingClientRect();
  return {
    l: r.left,
    r: r.right,
    t: r.top,
    b: r.bottom,
    cx: r.left + r.width / 2,
    cy: r.top + r.height / 2,
  };
};
const empty = (b: Box) => b.l === b.r && b.t === b.b;

/**
 * Score of moving from `a` to `c` in `dir`; null = not in that direction.
 * Primary-axis gap plus a penalty for orthogonal misalignment, so the
 * nearest element in the same row/column wins over a diagonal one.
 */
function score(a: Box, c: Box, dir: Dir): number | null {
  const E = 1;
  let gap: number;
  let off: number;
  switch (dir) {
    case "down":
      if (c.cy <= a.cy + E || c.b <= a.b) return null;
      gap = Math.max(0, c.t - a.b);
      off = c.r < a.l ? a.l - c.r : c.l > a.r ? c.l - a.r : 0;
      break;
    case "up":
      if (c.cy >= a.cy - E || c.t >= a.t) return null;
      gap = Math.max(0, a.t - c.b);
      off = c.r < a.l ? a.l - c.r : c.l > a.r ? c.l - a.r : 0;
      break;
    case "right":
      if (c.cx <= a.cx + E || c.r <= a.r) return null;
      gap = Math.max(0, c.l - a.r);
      off = c.b < a.t ? a.t - c.b : c.t > a.b ? c.t - a.b : 0;
      break;
    case "left":
      if (c.cx >= a.cx - E || c.l >= a.l) return null;
      gap = Math.max(0, a.l - c.r);
      off = c.b < a.t ? a.t - c.b : c.t > a.b ? c.t - a.b : 0;
      break;
  }
  return gap + off * 3;
}

/** The element focus would move to from `from` in `dir`, or null. */
export function findNext(from: HTMLElement | null, dir: Dir, scope = navScope()): HTMLElement | null {
  const all = focusables(scope);
  if (!all.length) return null;
  if (!from || !scope.contains(from)) return all[0];
  const a = box(from);
  const boxes = all.map(box);
  // No layout (jsdom, or a hidden window): fall back to document order.
  if (empty(a) && boxes.every(empty)) {
    const i = all.indexOf(from);
    const j = dir === "down" || dir === "right" ? i + 1 : i - 1;
    return all[j] ?? null;
  }
  let best: HTMLElement | null = null;
  let bestScore = Infinity;
  all.forEach((el, i) => {
    if (el === from) return;
    const s = score(a, boxes[i], dir);
    if (s !== null && s < bestScore) {
      best = el;
      bestScore = s;
    }
  });
  return best;
}

/** Move focus; returns the newly focused element (null = nothing there). */
export function moveFocus(dir: Dir, doc: Document = document): HTMLElement | null {
  const scope = navScope(doc);
  const active = doc.activeElement instanceof HTMLElement ? doc.activeElement : null;
  const next = findNext(active === doc.body ? null : active, dir, scope);
  if (next) {
    next.focus();
    next.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }
  return next;
}

const TEXT_TYPES = new Set(["text", "password", "search", "url", "email", "number", "tel", ""]);

export function isTextInput(el: Element | null): el is HTMLInputElement | HTMLTextAreaElement {
  if (el instanceof HTMLTextAreaElement) return true;
  return el instanceof HTMLInputElement && TEXT_TYPES.has(el.type);
}
