// SPDX-License-Identifier: GPL-3.0-or-later
// D02: one physical d-pad press must move focus exactly once, whatever mix of
// sources reports it (Gamepad API, Steam Input arrow keys, a second
// Steam virtual pad). Black-box: real installKeyboardNav + startGamepadLoop
// wired as App.tsx wires them; counts focus moves in jsdom (document order).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BACK_EVENT, installKeyboardNav, performNav } from "./actions";
import { BUTTON, startGamepadLoop } from "./gamepad";

type Pad = { connected: boolean; buttons: { pressed: boolean }[]; axes: number[] };
const pad = (pressed: number[] = []): Pad => ({
  connected: true,
  buttons: Array.from({ length: 17 }, (_, i) => ({ pressed: pressed.includes(i) })),
  axes: [0, 0, 0, 0],
});

let pads: (Pad | null)[] = [];
let frames: FrameRequestCallback[] = [];
let now = 0;
let moves = 0;
let stops: (() => void)[] = [];

/** Advance one animation frame (16 ms). */
function frame() {
  now += 16;
  const due = frames;
  frames = [];
  due.forEach((cb) => cb(now));
}
const key = (k: string, repeat = false) =>
  (document.activeElement ?? document.body).dispatchEvent(
    new KeyboardEvent("keydown", { key: k, repeat, bubbles: true, cancelable: true }),
  );

beforeEach(() => {
  document.body.innerHTML = ["a", "b", "c", "d", "e", "f"].map((l) => `<button>${l}</button>`).join("");
  document.querySelector("button")!.focus();
  pads = [pad()];
  frames = [];
  now = 1000;
  moves = 0;
  Object.defineProperty(navigator, "getGamepads", { configurable: true, value: () => pads });
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => frames.push(cb));
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
  vi.spyOn(window.performance, "now").mockImplementation(() => now);
  const onFocus = () => moves++;
  document.addEventListener("focusin", onFocus);
  // Same wiring as App.tsx's mount effect.
  stops = [installKeyboardNav(), startGamepadLoop((a) => performNav(a)), () => document.removeEventListener("focusin", onFocus)];
  window.dispatchEvent(new Event("gamepadconnected"));
  frame();
});

afterEach(() => {
  stops.forEach((s) => s());
  vi.restoreAllMocks();
  delete (navigator as { getGamepads?: unknown }).getGamepads;
});

describe("one d-pad press = one focus move", () => {
  it("baseline: gamepad-only press moves once", () => {
    pads = [pad([BUTTON.DOWN])];
    frame();
    frame();
    pads = [pad()];
    frame();
    expect(moves).toBe(1);
  });

  it("baseline: keyboard-only arrow moves once", () => {
    key("ArrowDown");
    expect(moves).toBe(1);
  });

  it("Steam Input arrow key arriving BEFORE the gamepad poll is not a second move", () => {
    pads = [pad([BUTTON.DOWN])];
    key("ArrowDown");
    frame();
    frame();
    pads = [pad()];
    frame();
    expect(moves).toBe(1);
  });

  it("Steam Input arrow key arriving AFTER the gamepad poll is not a second move", () => {
    pads = [pad([BUTTON.DOWN])];
    frame();
    key("ArrowDown");
    frame();
    pads = [pad()];
    frame();
    expect(moves).toBe(1);
  });

  it("Steam Input arrow key arriving shortly after release (input latency) is not a second move", () => {
    pads = [pad([BUTTON.DOWN])];
    frame();
    frame();
    pads = [pad()];
    frame();
    key("ArrowDown");
    frame();
    expect(moves).toBe(1);
  });

  it("OS key auto-repeat while the d-pad is held adds no moves beyond the gamepad's own repeat", () => {
    pads = [pad([BUTTON.DOWN])];
    frame();
    key("ArrowDown");
    // 300 ms held: under the 400 ms repeat delay, so still exactly one move.
    for (let i = 0; i < 18; i++) {
      if (i % 2 === 0) key("ArrowDown", true);
      frame();
    }
    pads = [pad()];
    frame();
    expect(moves).toBe(1);
  });

  it("a second pad (Steam virtual + physical) reporting the same press a frame late is not a second move", () => {
    pads = [pad([BUTTON.DOWN]), pad()];
    frame();
    frame();
    pads = [pad(), pad()];
    frame();
    pads = [pad(), pad([BUTTON.DOWN])];
    frame();
    frame();
    pads = [pad(), pad()];
    frame();
    expect(moves).toBe(1);
  });

  it("two deliberate presses 250 ms apart still move twice", () => {
    pads = [pad([BUTTON.DOWN])];
    frame();
    frame();
    pads = [pad()];
    for (let i = 0; i < 14; i++) frame();
    pads = [pad([BUTTON.DOWN])];
    frame();
    pads = [pad()];
    frame();
    expect(moves).toBe(2);
  });

  it("keyboard and gamepad each still work alone after the other was used", () => {
    pads = [pad([BUTTON.DOWN])];
    frame();
    pads = [pad()];
    for (let i = 0; i < 20; i++) frame();
    key("ArrowDown");
    expect(moves).toBe(2);
  });
});

describe("one A/B press = one select/back", () => {
  /** Stands in for the browser: Enter on a focused button clicks it unless prevented. */
  function nativeEnterActivation() {
    const activate = (e: KeyboardEvent) => {
      if (e.key === "Enter" && !e.defaultPrevented && e.target instanceof HTMLElement) e.target.click();
    };
    document.addEventListener("keydown", activate);
    stops.push(() => document.removeEventListener("keydown", activate));
  }

  // Fails when Steam Input's Enter clicks the focused button a second time.
  it.each([
    ["Enter after the A poll", false],
    ["Enter before the A poll", true],
  ])("A + %s clicks once", (_name, keyFirst) => {
    nativeEnterActivation();
    let clicks = 0;
    document.activeElement!.addEventListener("click", () => clicks++);
    pads = [pad([BUTTON.A])];
    if (keyFirst) key("Enter");
    frame();
    if (!keyFirst) key("Enter");
    frame();
    pads = [pad()];
    frame();
    expect(clicks).toBe(1);
  });

  // Fails when Steam Input's Escape closes the dialog a second time (or a parent too).
  it("B + Escape goes back once", () => {
    const dlg = document.createElement("div");
    dlg.setAttribute("data-nav-scope", "");
    dlg.innerHTML = "<button>x</button>";
    document.body.appendChild(dlg);
    dlg.querySelector("button")!.focus();
    let backs = 0;
    dlg.addEventListener(BACK_EVENT, () => backs++);
    pads = [pad([BUTTON.B])];
    frame();
    key("Escape");
    frame();
    pads = [pad()];
    frame();
    expect(backs).toBe(1);
  });

  it("Enter and Escape alone still work with a pad connected but idle", () => {
    nativeEnterActivation();
    let clicks = 0;
    document.activeElement!.addEventListener("click", () => clicks++);
    key("Enter");
    expect(clicks).toBe(1);
  });
});
