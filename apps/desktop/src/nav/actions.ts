// SPDX-License-Identifier: GPL-3.0-or-later
// Navigation actions shared by the keyboard and the gamepad.

import { useUi, type ViewId } from "../app/store";
import { navArbiter, type NavArbiter, type NavInput } from "./arbiter";
import { focusables, isTextInput, moveFocus, navScope, type Dir } from "./spatial";

export type NavAction =
  | { type: "move"; dir: Dir }
  | { type: "select" }
  | { type: "back" }
  | { type: "tab"; delta: 1 | -1 };

/** Dispatched on the topmost nav scope (a dialog) for "back"; dialogs close on it. */
export const BACK_EVENT = "modman:back";

export function focusCurrentTab(doc: Document = document): void {
  doc.querySelector<HTMLElement>("[data-tab][aria-current='page']")?.focus();
}

/**
 * Switch to `view` from a control inside the current view: its tab takes
 * focus first, since the control unmounts with the old view.
 */
export function openView(view: ViewId, doc: Document = document): void {
  doc.querySelector<HTMLElement>(`[data-tab='${view}']`)?.focus();
  useUi.getState().setView(view);
}

export function performNav(action: NavAction, doc: Document = document): void {
  switch (action.type) {
    case "move":
      moveFocus(action.dir, doc);
      return;
    case "select": {
      const el = doc.activeElement;
      if (!(el instanceof HTMLElement) || el === doc.body) {
        focusables(navScope(doc))[0]?.focus();
        return;
      }
      if (isTextInput(el) && el instanceof HTMLInputElement) {
        // Gamepad A on a text field: our on-screen keyboard (Steam's opens
        // by itself on focus in Game Mode; this is the fallback).
        const label = el.getAttribute("aria-label") ?? el.labels?.[0]?.textContent ?? "Text";
        useUi.getState().openOsk({ input: el, label });
        return;
      }
      el.click();
      return;
    }
    case "back": {
      const scope = navScope(doc);
      if (scope !== doc.body) {
        scope.dispatchEvent(new CustomEvent(BACK_EVENT));
        return;
      }
      focusCurrentTab(doc);
      return;
    }
    case "tab": {
      // Not while a dialog is open: it owns the input.
      if (navScope(doc) !== doc.body) return;
      const view = useUi.getState().cycleView(action.delta);
      // Tab buttons are always rendered, so focus follows the selection now.
      doc.querySelector<HTMLElement>(`[data-tab='${view}']`)?.focus();
      return;
    }
  }
}

/** True when the caret is collapsed at the start/end of the field, so the arrow has nowhere to go in the text. */
function caretAtEdge(el: HTMLInputElement | HTMLTextAreaElement, edge: "start" | "end"): boolean {
  // Types without selection support (number, email) report null: keep the caret behaviour.
  const { selectionStart: a, selectionEnd: b } = el;
  if (a === null || b === null || a !== b) return false;
  return edge === "start" ? a === 0 : a === el.value.length;
}

/** Keyboard mapping (also the Steam Input keyboard-layout fallback). */
export function keyToNav(e: KeyboardEvent): NavAction | null {
  if (e.altKey || e.ctrlKey || e.metaKey) return null;
  const target = e.target instanceof Element ? e.target : null;
  const inText = isTextInput(target);
  switch (e.key) {
    case "ArrowUp":
    case "ArrowDown":
      if (target instanceof HTMLTextAreaElement) return null;
      return { type: "move", dir: e.key === "ArrowUp" ? "up" : "down" };
    case "ArrowLeft":
    case "ArrowRight":
      // Inside a text field the caret moves, until it sits at the edge the key points to.
      if (inText && !caretAtEdge(target, e.key === "ArrowLeft" ? "start" : "end")) return null;
      return { type: "move", dir: e.key === "ArrowLeft" ? "left" : "right" };
    case "Escape":
      return { type: "back" };
    case "PageUp":
    case "PageDown":
      // Inside a text field the keys keep their editing meaning.
      if (inText) return null;
      return { type: "tab", delta: e.key === "PageUp" ? -1 : 1 };
    default:
      return null;
  }
}

const KEY_INPUT: Record<string, NavInput> = {
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  Enter: "A",
  " ": "A",
  Escape: "B",
  PageUp: "LB",
  PageDown: "RB",
};

/** The pad input a key can echo under Steam Input, if any. */
function keyInput(e: KeyboardEvent): NavInput | null {
  if (e.altKey || e.ctrlKey || e.metaKey) return null;
  // Space in a text field is typing.
  if (e.key === " " && isTextInput(e.target instanceof Element ? e.target : null)) return null;
  return KEY_INPUT[e.key] ?? null;
}

export function installKeyboardNav(doc: Document = document, arbiter: NavArbiter = navArbiter): () => void {
  const swallowed = new Set<string>();
  const swallow = (e: KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };
  // Capture phase: a key echoing a pad press is dropped before any handler
  // (or native Enter/Space activation) sees it.
  const dedup = (e: KeyboardEvent) => {
    const input = keyInput(e);
    if (!input) return;
    const now = (doc.defaultView ?? window).performance.now();
    if (arbiter.keyIsDuplicate(input, e.repeat, now)) {
      swallowed.add(e.key);
      swallow(e);
      return;
    }
    arbiter.noteKey(input, now);
  };
  // Space activates a button on keyup: drop the keyup of a swallowed keydown too.
  const dedupUp = (e: KeyboardEvent) => {
    if (swallowed.delete(e.key)) swallow(e);
  };
  const handler = (e: KeyboardEvent) => {
    if (e.defaultPrevented) return;
    const action = keyToNav(e);
    if (!action) return;
    e.preventDefault();
    performNav(action, doc);
  };
  doc.addEventListener("keydown", dedup, true);
  doc.addEventListener("keyup", dedupUp, true);
  doc.addEventListener("keydown", handler);
  return () => {
    doc.removeEventListener("keydown", dedup, true);
    doc.removeEventListener("keyup", dedupUp, true);
    doc.removeEventListener("keydown", handler);
    arbiter.reset();
  };
}
