// SPDX-License-Identifier: GPL-3.0-or-later
// Right stick scrolling, the d-pad's hat-axis fallback, and d-pad/left-stick parity.
import { afterEach, describe, expect, it, vi } from "vitest";
import { performNav, scrollTarget } from "./actions";
import { BUTTON, GamepadNav } from "./gamepad";

type Pad = { connected: boolean; buttons: { pressed: boolean }[]; axes: number[] };
const pad = (pressed: number[] = [], axes: number[] = [0, 0, 0, 0]): Pad => ({
  connected: true,
  buttons: Array.from({ length: 17 }, (_, i) => ({ pressed: pressed.includes(i) })),
  axes,
});

function mockPads() {
  const state: { pads: (Pad | null)[] } = { pads: [] };
  Object.defineProperty(navigator, "getGamepads", {
    configurable: true,
    value: vi.fn(() => state.pads),
  });
  return state;
}

afterEach(() => {
  delete (navigator as { getGamepads?: unknown }).getGamepads;
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

const scrolled = (actions: { type: string }[]) =>
  actions.filter((a): a is { type: "scroll"; dy: number } => a.type === "scroll").reduce((s, a) => s + a.dy, 0);

describe("right stick scrolls", () => {
  it("scrolls in proportion to the deflection, frame by frame", () => {
    const s = mockPads();
    const nav = new GamepadNav();
    s.pads = [pad([], [0, 0, 0, 1])];
    expect(nav.poll(0)).toEqual([]); // first frame: no time has passed yet
    expect(nav.poll(16)).toEqual([{ type: "scroll", dy: 16 }]);
    // 100 more frames at full deflection: 1600 ms of scrolling at 1 px/ms, none lost to rounding.
    let total = 0;
    for (let t = 32; t <= 32 + 99 * 16; t += 16) total += scrolled(nav.poll(t));
    expect(total).toBe(1600);
  });

  it("is zero inside the deadzone and scrolls up when pushed up", () => {
    const s = mockPads();
    const nav = new GamepadNav();
    s.pads = [pad([], [0, 0, 0, 0.1])];
    nav.poll(0);
    expect(nav.poll(16)).toEqual([]);
    s.pads = [pad([], [0, 0, 0, -1])];
    expect(nav.poll(32)).toEqual([{ type: "scroll", dy: -16 }]);
  });

  it("does not switch the input to keyboard focus by itself", () => {
    const s = mockPads();
    const nav = new GamepadNav();
    s.pads = [pad([], [0, 0, 0, 1])];
    nav.poll(0);
    // Only scroll actions: the loop checks this to keep the focus ring hidden.
    expect(nav.poll(16).every((a) => a.type === "scroll")).toBe(true);
  });

  it("performNav scrolls the open dialog, else the view's main pane", () => {
    document.body.innerHTML = `<main><p>content</p></main>`;
    const main = document.querySelector("main") as HTMLElement;
    main.scrollBy = vi.fn();
    performNav({ type: "scroll", dy: 7 });
    expect(main.scrollBy).toHaveBeenCalledWith({ top: 7 });
    expect(scrollTarget()).toBe(main);

    document.body.insertAdjacentHTML("beforeend", `<div role="dialog" data-nav-scope><button>x</button></div>`);
    const dialog = document.querySelector("[data-nav-scope]") as HTMLElement;
    dialog.scrollBy = vi.fn();
    performNav({ type: "scroll", dy: -3 });
    expect(dialog.scrollBy).toHaveBeenCalledWith({ top: -3 });
    expect(main.scrollBy).toHaveBeenCalledTimes(1);
  });
});

describe("d-pad", () => {
  it("reads a hat axis pair as a direction when the pad reports no d-pad buttons", () => {
    const s = mockPads();
    const nav = new GamepadNav();
    s.pads = [pad([], [0, 0, 0, 0, 0, 0, 1, 0])];
    expect(nav.poll(0)).toEqual([{ type: "move", dir: "right" }]);
    s.pads = [pad([], [0, 0, 0, 0, 0, 0, 0, -1])];
    expect(nav.poll(16)).toEqual([{ type: "move", dir: "up" }]);
  });

  it("moves exactly like the left stick over the same timeline (press, hold, release)", () => {
    const timeline = [1, 1, 1, 1, 1, 1, 1, 1, 0, 0];
    // Frames 0..: hold for 8 frames, then release; first at 0 ms, 16 ms apart, long enough to repeat.
    const run = (input: (on: boolean) => Pad) => {
      const s = mockPads();
      const nav = new GamepadNav(undefined, { repeatDelay: 40, repeatEvery: 32 });
      const out: unknown[] = [];
      timeline.forEach((on, i) => {
        s.pads = [input(on === 1)];
        out.push(nav.poll(i * 16));
      });
      return out;
    };
    const dpad = run((on) => pad(on ? [BUTTON.DOWN] : []));
    const stick = run((on) => pad([], on ? [0, 1, 0, 0] : [0, 0, 0, 0]));
    expect(dpad).toEqual(stick);
    expect(dpad.flat()).toContainEqual({ type: "move", dir: "down" });
  });
});
