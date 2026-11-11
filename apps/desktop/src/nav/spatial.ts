// SPDX-License-Identifier: GPL-3.0-or-later
// Geometric spatial navigation over real DOM focus (so the native focus ring
// and form controls keep working). The scope is the topmost element marked
// `data-nav-scope` (an open dialog), else the whole document.

export type Dir = "up" | "down" | "left" | "right";

/** Marks the current view's search field, focused by the "search" nav action (gamepad Y). */
export const SEARCH_ATTR = "data-view-search";
/** Marks the wrapper of the app's top view tabs (dialog tabs may share `data-tab` values, e.g. "mods"). */
export const VIEW_TABS_ATTR = "data-view-tabs";
/** The selected top view tab. */
export const CURRENT_TAB = `[${VIEW_TABS_ATTR}] [role='tab'][aria-selected='true']`;

/** A roving-tabindex group (focusable Tabs): its tabindex=-1 members stay spatial-nav stops. */
const ROVING = "[data-nav-roving]";

// tabindex=-1 opts a control out (e.g. dialog tabs, switched by LB/RB and the mouse only), except
// inside a roving group, where -1 only means "not the Tab-key stop".
const FOCUSABLE = [
  ...[
    "button:not([disabled])",
    "a[href]",
    "input:not([disabled]):not([type=hidden])",
    "textarea:not([disabled])",
    "select:not([disabled])",
    "[tabindex]",
  ].map((s) => `${s}:not([tabindex='-1'])`),
  `${ROVING} [role='tab']:not([disabled])`,
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

/** Up/Down prefer staying in the same landmark (offscreen content beats the app header). */
const OTHER_REGION = 2e6;
/**
 * A landmark nested in <main> (a view's own <header>, an <aside>) is part of the view: otherwise its
 * controls, scrolled above the viewport, would lose to the app header's tabs and become unreachable.
 */
const region = (el: Element) => {
  const r = el.closest("nav,header,main,aside,[role=dialog]");
  return r && !r.matches("[role=dialog]") ? (r.closest("main") ?? r) : r;
};

/**
 * Card sections, opt-in: inside a `data-nav-sections` container (a Card's
 * divided surface), each direct child and each `data-nav-section` element is
 * a section; the innermost one counts.
 */
const SECTION = "[data-nav-sections] > *, [data-nav-sections] [data-nav-section]";
function sectionOf(el: Element, scope: HTMLElement): Element | null {
  const s = el.closest(SECTION);
  // A section around the scope (e.g. an open listbox inside it) does not apply.
  return s && scope.contains(s) && s !== scope ? s : null;
}

/**
 * Entering a roving group (the top tabs) from outside lands on its current stop (the selected tab),
 * not the geometrically nearest member (APG tablist).
 */
function enter(from: HTMLElement | null, to: HTMLElement | null): HTMLElement | null {
  const group = to?.closest<HTMLElement>(ROVING);
  if (!group || (from && group.contains(from))) return to;
  return group.querySelector<HTMLElement>("[role='tab'][tabindex='0']") ?? to;
}

/** The element focus would move to from `from` in `dir`, or null. */
export function findNext(from: HTMLElement | null, dir: Dir, scope = navScope()): HTMLElement | null {
  return enter(from, pick(from, dir, scope));
}

function pick(from: HTMLElement | null, dir: Dir, scope: HTMLElement): HTMLElement | null {
  const all = focusables(scope);
  if (!all.length) return null;
  if (!from || !scope.contains(from)) return all[0];
  const section = dir === "up" || dir === "down" ? sectionOf(from, scope) : null;
  if (!section) return nearest(from, dir, all);
  // Up/Down inside a section first (stacked controls), then the first control
  // of the previous/next section that has one (no wrap).
  const inside = nearest(from, dir, all.filter((el) => sectionOf(el, scope) === section));
  if (inside) return inside;
  const firsts = new Map<Element, HTMLElement>();
  for (const el of all) {
    const s = sectionOf(el, scope);
    if (s && !firsts.has(s)) firsts.set(s, el);
  }
  const order = [...firsts.keys()];
  const i = order.indexOf(section);
  // `from` itself is not focusable (e.g. just disabled) and alone in its section: plain geometry.
  if (i < 0) return nearest(from, dir, all);
  const next = order[i + (dir === "down" ? 1 : -1)];
  return next ? firsts.get(next)! : null;
}

/** Geometric pick among `all` (which contains `from`). */
function nearest(from: HTMLElement, dir: Dir, all: HTMLElement[]): HTMLElement | null {
  const a = box(from);
  const boxes = all.map(box);
  const vertical = dir === "up" || dir === "down";
  // No layout (jsdom, or a hidden window): fall back to document order.
  if (empty(a) && boxes.every(empty)) {
    // A roving group is a horizontal bar: Up/Down leave it rather than walk it.
    const group = vertical ? from.closest(ROVING) : null;
    const order = group ? all.filter((el) => el === from || !group.contains(el)) : all;
    const i = order.indexOf(from);
    const j = dir === "down" || dir === "right" ? i + 1 : i - 1;
    return order[j] ?? null;
  }
  let best: HTMLElement | null = null;
  let bestScore = Infinity;
  const fromRegion = region(from);
  all.forEach((el, i) => {
    if (el === from) return;
    let s = score(a, boxes[i], dir);
    if (s !== null && vertical && region(el) !== fromRegion) s += OTHER_REGION;
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

/** Focus the focusable nearest to where focus was last lost, else the selected view tab. */
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
  const target = best ?? doc.querySelector<HTMLElement>(CURRENT_TAB) ?? all[0];
  if (!target) return;
  // A plain focus() scrolls the target into view (often far): pointer users must keep their scroll position.
  target.focus({ preventScroll: true });
  if (doc.documentElement.dataset.input !== "pointer") target.scrollIntoView?.({ block: "nearest", inline: "nearest" });
}

/**
 * Safety net: focus that falls to <body> (focused element disabled or
 * unmounted) is put back on the nearest focusable. Dialogs restore focus
 * themselves, so focus leaving a nav scope is ignored.
 */
export function installFocusRecovery(doc: Document = document): () => void {
  const onOut = (e: FocusEvent) => {
    if (e.relatedTarget || !(e.target instanceof HTMLElement) || e.target.closest("[data-nav-scope]")) return;
    const lost = e.target;
    lastLost = box(lost);
    requestAnimationFrame(() => {
      // A pointer click on a non-actionable area is meant to leave the control: only recover when
      // the control itself went away (unmounted or disabled), never pull focus (and scroll) back.
      // Checked after the frame because unmount/disable may land after the focusout event.
      const left = doc.documentElement.dataset.input === "pointer" && lost.isConnected && !lost.matches(":disabled");
      if (!left && doc.activeElement === doc.body && doc.hasFocus()) recoverFocus(doc);
    });
  };
  doc.addEventListener("focusout", onOut, true);
  return () => doc.removeEventListener("focusout", onOut, true);
}
