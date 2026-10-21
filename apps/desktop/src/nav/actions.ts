// SPDX-License-Identifier: GPL-3.0-or-later
// Navigation actions shared by the keyboard and the gamepad.

import { useUi } from "../app/store";
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
      // Inside a text field the caret moves.
      if (inText) return null;
      return { type: "move", dir: e.key === "ArrowLeft" ? "left" : "right" };
    case "Escape":
      return { type: "back" };
    case "PageUp":
      return { type: "tab", delta: -1 };
    case "PageDown":
      return { type: "tab", delta: 1 };
    default:
      return null;
  }
}

export function installKeyboardNav(doc: Document = document): () => void {
  const handler = (e: KeyboardEvent) => {
    if (e.defaultPrevented) return;
    const action = keyToNav(e);
    if (!action) return;
    e.preventDefault();
    performNav(action, doc);
  };
  doc.addEventListener("keydown", handler);
  return () => doc.removeEventListener("keydown", handler);
}
