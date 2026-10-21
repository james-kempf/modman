// SPDX-License-Identifier: GPL-3.0-or-later
import { afterEach, describe, expect, it, vi } from "vitest";
import { BUTTON, GamepadNav, startGamepadLoop } from "./gamepad";

type Pad = { connected: boolean; buttons: { pressed: boolean }[]; axes: number[] };
const pad = (pressed: number[] = [], axes: number[] = [0, 0]): Pad => ({
  connected: true,
  buttons: Array.from({ length: 17 }, (_, i) => ({ pressed: pressed.includes(i) })),
  axes,
});

/** navigator.getGamepads mocked to return whatever `state.pads` holds. */
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
});

describe("gamepad mapping (mocked navigator.getGamepads)", () => {
  it("A selects, B goes back, once per press", () => {
    const s = mockPads();
    const nav = new GamepadNav();
    s.pads = [pad([BUTTON.A])];
    expect(nav.poll(0)).toEqual([{ type: "select" }]);
    // Held: no repeat.
    expect(nav.poll(16)).toEqual([]);
    s.pads = [pad([BUTTON.B])];
    expect(nav.poll(32)).toEqual([{ type: "back" }]);
    s.pads = [pad()];
    expect(nav.poll(48)).toEqual([]);
    s.pads = [pad([BUTTON.A])];
    expect(nav.poll(64)).toEqual([{ type: "select" }]);
  });

  it("shoulders switch tabs (LB previous, RB next)", () => {
    const s = mockPads();
    const nav = new GamepadNav();
    s.pads = [pad([BUTTON.RB])];
    expect(nav.poll(0)).toEqual([{ type: "tab", delta: 1 }]);
    s.pads = [pad([BUTTON.LB])];
    expect(nav.poll(16)).toEqual([{ type: "tab", delta: -1 }]);
  });

  it("D-pad moves in four directions", () => {
    const s = mockPads();
    const nav = new GamepadNav();
    const seen = [BUTTON.UP, BUTTON.DOWN, BUTTON.LEFT, BUTTON.RIGHT].map((b, i) => {
      s.pads = [pad([b])];
      const a = nav.poll(i * 100);
      s.pads = [pad()];
      nav.poll(i * 100 + 50);
      return a;
    });
    expect(seen).toEqual([
      [{ type: "move", dir: "up" }],
      [{ type: "move", dir: "down" }],
      [{ type: "move", dir: "left" }],
      [{ type: "move", dir: "right" }],
    ]);
  });

  it("left stick moves past the deadzone only", () => {
    const s = mockPads();
    const nav = new GamepadNav();
    s.pads = [pad([], [0.3, 0.2])];
    expect(nav.poll(0)).toEqual([]);
    s.pads = [pad([], [0.1, 0.9])];
    expect(nav.poll(16)).toEqual([{ type: "move", dir: "down" }]);
    s.pads = [pad([], [-0.8, 0.1])];
    expect(nav.poll(32)).toEqual([{ type: "move", dir: "left" }]);
  });

  it("a held direction repeats after the delay", () => {
    const s = mockPads();
    const nav = new GamepadNav(undefined, { repeatDelay: 400, repeatEvery: 100 });
    s.pads = [pad([BUTTON.DOWN])];
    expect(nav.poll(0)).toHaveLength(1);
    expect(nav.poll(200)).toEqual([]);
    expect(nav.poll(400)).toEqual([{ type: "move", dir: "down" }]);
    expect(nav.poll(450)).toEqual([]);
    expect(nav.poll(500)).toEqual([{ type: "move", dir: "down" }]);
  });

  it("ignores disconnected and empty pad slots", () => {
    const s = mockPads();
    const nav = new GamepadNav();
    s.pads = [null, { ...pad([BUTTON.A]), connected: false }];
    expect(nav.poll(0)).toEqual([]);
  });

  it("the loop polls on animation frames once a pad connects", () => {
    const s = mockPads();
    const frames: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => frames.push(cb));
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
    const got: unknown[] = [];
    const stop = startGamepadLoop((a) => got.push(a));
    expect(frames).toHaveLength(0);
    s.pads = [pad([BUTTON.A])];
    window.dispatchEvent(new Event("gamepadconnected"));
    frames.shift()!(0);
    expect(got).toEqual([{ type: "select" }]);
    stop();
  });
});
