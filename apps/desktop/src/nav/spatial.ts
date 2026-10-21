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
 * A candidate must lie wholly beyond `a`'s leading edge (a tall/wide
 * neighbour that merely overlaps is never "below"/"right of" it). Candidates
 * in `a`'s beam (overlapping on the orthogonal axis) always beat ones outside
 * it; within a class: edge gap plus 3x orthogonal misalignment.
 */
const E = 2;
const OUT_OF_BEAM = 1e6;
function score(a: Box, c: Box, dir: Dir): number | null {
  let gap: number;
  switch (dir) {
    case "down":
      if (c.t < a.b - E) return null;
      gap = c.t - a.b;
      break;
    case "up":
      if (c.b > a.t + E) return null;
      gap = a.t - c.b;
      break;
    case "right":
      if (c.l < a.r - E) return null;
      gap = c.l - a.r;
      break;
    case "left":
      if (c.r > a.l + E) return null;
      gap = a.l - c.r;
      break;
  }
  const overlap =
    dir === "up" || dir === "down" ? Math.min(a.r, c.r) - Math.max(a.l, c.l) : Math.min(a.b, c.b) - Math.max(a.t, c.t);
  return (overlap > 0 ? 0 : OUT_OF_BEAM) + Math.max(0, gap) + Math.max(0, -overlap) * 3;
}

/** Up/Down prefer staying in the same landmark (offscreen content beats the header). */
const OTHER_REGION = 2e6;
/** The tab rail is entered/left horizontally; vertically only when nothing else is there. */
const RAIL_CROSS = 4e6;
const region = (el: Element) => el.closest("nav,header,main,aside,[role=dialog]");
const inRail = (el: Element) => !!el.closest("[data-nav-rail]");

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
  const vertical = dir === "up" || dir === "down";
  const fromRegion = region(from);
  const fromRail = inRail(from);
  all.forEach((el, i) => {
    if (el === from) return;
    let s = score(a, boxes[i], dir);
    if (s !== null && vertical) {
      if (region(el) !== fromRegion) s += OTHER_REGION;
      if (inRail(el) !== fromRail) s += RAIL_CROSS;
    }
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

/** Where focus was last lost (viewport box), for recoverFocus(). */
let lastLost: Box | null = null;

/** Focus the focusable nearest to where focus was last lost, else the current tab. */
export function recoverFocus(doc: Document = document): void {
  const all = focusables(navScope(doc));
  let best: HTMLElement | null = null;
  if (lastLost) {
    const lb = lastLost;
    let bd = Infinity;
    for (const el of all) {
      const b = box(el);
      const d = Math.hypot(b.cx - lb.cx, b.cy - lb.cy);
      if (d < bd) {
        bd = d;
        best = el;
      }
    }
  }
  (best ?? doc.querySelector<HTMLElement>("[data-tab][aria-current='page']") ?? all[0])?.focus();
}

/**
 * Safety net: focus that falls to <body> (focused element disabled or
 * unmounted) is put back on the nearest focusable. Dialogs restore focus
 * themselves, so focus leaving a nav scope is ignored.
 */
export function installFocusRecovery(doc: Document = document): () => void {
  const onOut = (e: FocusEvent) => {
    if (e.relatedTarget || !(e.target instanceof HTMLElement) || e.target.closest("[data-nav-scope]")) return;
    lastLost = box(e.target);
    requestAnimationFrame(() => {
      if (doc.activeElement === doc.body && doc.hasFocus()) recoverFocus(doc);
    });
  };
  doc.addEventListener("focusout", onOut, true);
  return () => doc.removeEventListener("focusout", onOut, true);
}
