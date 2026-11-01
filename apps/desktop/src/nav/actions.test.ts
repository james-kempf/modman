// SPDX-License-Identifier: GPL-3.0-or-later
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { resetUi, useUi } from "../app/store";
import { BACK_EVENT, TAB_EVENT, keyToNav, performNav } from "./actions";

const key = (k: string, target: Element = document.body, mods: Partial<KeyboardEventInit> = {}) => {
  const e = new KeyboardEvent("keydown", { key: k, bubbles: true, ...mods });
  Object.defineProperty(e, "target", { value: target });
  return e;
};

beforeEach(() => resetUi());
afterEach(() => {
  document.body.innerHTML = "";
});

describe("keyboard mapping", () => {
  it("arrows move; Escape is back; PageUp/PageDown switch tabs", () => {
    expect(keyToNav(key("ArrowDown"))).toEqual({ type: "move", dir: "down" });
    expect(keyToNav(key("ArrowLeft"))).toEqual({ type: "move", dir: "left" });
    expect(keyToNav(key("Escape"))).toEqual({ type: "back" });
    expect(keyToNav(key("PageDown"))).toEqual({ type: "tab", delta: 1 });
    expect(keyToNav(key("PageUp"))).toEqual({ type: "tab", delta: -1 });
  });

  // CT1 D14: fails when PageUp/PageDown in a text field switch the view instead of editing.
  it("PageUp/PageDown inside a text field are left to the field", () => {
    const input = document.createElement("input");
    expect(keyToNav(key("PageUp", input))).toBeNull();
    expect(keyToNav(key("PageDown", input))).toBeNull();
  });

  it("left/right inside a text field move the caret until it reaches that edge", () => {
    const input = document.createElement("input");
    input.value = "abc";
    input.setSelectionRange(1, 1);
    expect(keyToNav(key("ArrowLeft", input))).toBeNull();
    expect(keyToNav(key("ArrowRight", input))).toBeNull();
    expect(keyToNav(key("ArrowDown", input))).toEqual({ type: "move", dir: "down" });
    input.setSelectionRange(3, 3);
    expect(keyToNav(key("ArrowRight", input))).toEqual({ type: "move", dir: "right" });
    expect(keyToNav(key("ArrowLeft", input))).toBeNull();
    input.setSelectionRange(0, 0);
    expect(keyToNav(key("ArrowLeft", input))).toEqual({ type: "move", dir: "left" });
    input.setSelectionRange(0, 3);
    expect(keyToNav(key("ArrowRight", input))).toBeNull();
  });

  it("leaves modified keys alone", () => {
    expect(keyToNav(key("ArrowDown", document.body, { ctrlKey: true }))).toBeNull();
  });
});

describe("performNav", () => {
  it("select on a text field opens the on-screen keyboard for it", () => {
    const input = document.createElement("input");
    input.setAttribute("aria-label", "Nexus API key");
    document.body.appendChild(input);
    input.focus();
    performNav({ type: "select" });
    expect(useUi.getState().osk).toEqual({ input, label: "Nexus API key" });
  });

  it("select clicks the focused button", () => {
    const b = document.createElement("button");
    const click = vi.fn();
    b.addEventListener("click", click);
    document.body.appendChild(b);
    b.focus();
    performNav({ type: "select" });
    expect(click).toHaveBeenCalledOnce();
  });

  it("back inside a dialog scope notifies that dialog", () => {
    const dlg = document.createElement("div");
    dlg.setAttribute("data-nav-scope", "");
    document.body.appendChild(dlg);
    const back = vi.fn();
    dlg.addEventListener(BACK_EVENT, back);
    performNav({ type: "back" });
    expect(back).toHaveBeenCalledOnce();
  });

  // Fails when LB/RB inside a dialog also switch the app's views, or never reach the dialog (its own tabs).
  it("tab inside a dialog goes to the dialog, not the app's views", () => {
    const dlg = document.createElement("div");
    dlg.setAttribute("data-nav-scope", "");
    document.body.appendChild(dlg);
    const tab = vi.fn();
    dlg.addEventListener(TAB_EVENT, (e) => tab((e as CustomEvent).detail));
    performNav({ type: "tab", delta: 1 });
    performNav({ type: "tab", delta: -1 });
    expect(useUi.getState().view).toBe("games");
    expect(tab.mock.calls).toEqual([[{ delta: 1 }], [{ delta: -1 }]]);
  });

  // Fails when Y does not focus the view's search field, or reaches it behind an open dialog.
  it("search focuses the view's search field, except while a dialog is open", () => {
    const input = document.createElement("input");
    input.setAttribute("data-view-search", "");
    document.body.appendChild(input);
    performNav({ type: "search" });
    expect(document.activeElement).toBe(input);
    input.blur();
    const dlg = document.createElement("div");
    dlg.setAttribute("data-nav-scope", "");
    document.body.appendChild(dlg);
    performNav({ type: "search" });
    expect(document.activeElement).not.toBe(input);
  });
});
