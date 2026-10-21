// SPDX-License-Identifier: GPL-3.0-or-later
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { resetUi, useUi } from "../app/store";
import { BACK_EVENT, keyToNav, performNav } from "./actions";

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

  it("left/right inside a text field move the caret, not focus", () => {
    const input = document.createElement("input");
    expect(keyToNav(key("ArrowLeft", input))).toBeNull();
    expect(keyToNav(key("ArrowDown", input))).toEqual({ type: "move", dir: "down" });
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

  it("tab switching is blocked while a dialog is open", () => {
    const dlg = document.createElement("div");
    dlg.setAttribute("data-nav-scope", "");
    document.body.appendChild(dlg);
    performNav({ type: "tab", delta: 1 });
    expect(useUi.getState().view).toBe("overview");
  });
});
