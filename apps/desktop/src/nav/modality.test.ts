// SPDX-License-Identifier: GPL-3.0-or-later
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BUTTON, startGamepadLoop } from "./gamepad";
import { installModalityTracking } from "./modality";

const mode = () => document.documentElement.dataset.input;
const key = (k: string) => document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
const pointer = () => document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
// jsdom has no PointerEvent: a MouseEvent carries the screen position, pointerType is added by hand.
const move = (screenX: number, screenY: number, pointerType = "mouse") => {
  const e = new MouseEvent("pointermove", { bubbles: true, screenX, screenY });
  Object.defineProperty(e, "pointerType", { value: pointerType });
  document.body.dispatchEvent(e);
};

let stop: () => void;
beforeEach(() => {
  delete document.documentElement.dataset.input;
  delete document.documentElement.dataset.pad;
  stop = installModalityTracking();
});
afterEach(() => {
  stop();
  delete (navigator as { getGamepads?: unknown }).getGamepads;
});

describe("input modality (drives the focus ring)", () => {
  it("defaults to nav so the first D-pad/keyboard use shows focus", () => {
    expect(mode()).toBe("nav");
  });

  it("pointerdown hides the ring; navigation keys bring it back", () => {
    pointer();
    expect(mode()).toBe("pointer");
    key("ArrowDown");
    expect(mode()).toBe("nav");
    for (const k of ["Tab", "Enter", "Escape", "PageUp", "PageDown", " "]) {
      pointer();
      key(k);
      expect(mode(), k).toBe("nav");
    }
  });

  it("typing text after a click does not switch to nav (no ring flash in inputs)", () => {
    pointer();
    key("a");
    key("Shift");
    expect(mode()).toBe("pointer");
  });

  it("a gamepad action switches to nav even though no key event fires", () => {
    pointer();
    const frames: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => frames.push(cb));
    Object.defineProperty(navigator, "getGamepads", {
      configurable: true,
      value: () => [{ connected: true, axes: [0, 0], buttons: Array.from({ length: 17 }, (_, i) => ({ pressed: i === BUTTON.DOWN })) }],
    });
    const stopPad = startGamepadLoop(() => {});
    frames.shift()!(0);
    expect(mode()).toBe("nav");
    stopPad();
  });

  it("an idle gamepad loop (no actions) keeps pointer mode", () => {
    pointer();
    const frames: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => frames.push(cb));
    Object.defineProperty(navigator, "getGamepads", {
      configurable: true,
      value: () => [{ connected: true, axes: [0, 0], buttons: Array.from({ length: 17 }, () => ({ pressed: false })) }],
    });
    const stopPad = startGamepadLoop(() => {});
    frames.shift()!(0);
    expect(mode()).toBe("pointer");
    stopPad();
  });

  // Fails when only a click leaves nav mode: moving the mouse must drop the nav highlight, or the
  // nav-focused control and the hovered one are both white.
  it("a real mouse move switches to pointer mode; nav keys switch back", () => {
    move(100, 100);
    move(108, 100);
    expect(mode()).toBe("pointer");
    key("ArrowDown");
    expect(mode()).toBe("nav");
  });

  // Fails when jitter or the synthetic moves fired when content scrolls under a still cursor
  // (same screen position, e.g. after scrollIntoView on nav focus) kick the user out of controller mode.
  it("stays in nav for sub-threshold jitter and for moves at the same screen position", () => {
    move(100, 100);
    move(102, 101);
    move(100, 100);
    expect(mode()).toBe("nav");
    for (let i = 0; i < 20; i++) move(100, 100);
    expect(mode()).toBe("nav");
  });

  // Fails when touch drags (scrolling a list with a finger) count as a mouse taking over.
  it("ignores touch pointers", () => {
    move(0, 0, "touch");
    move(200, 200, "touch");
    expect(mode()).toBe("nav");
  });

  // Fails when every pointermove rewrites data-input (style recalculation on each mouse event).
  it("writes the attribute once per mode change", () => {
    const seen = new MutationObserver(() => {});
    seen.observe(document.documentElement, { attributes: true, attributeFilter: ["data-input"] });
    for (let x = 0; x <= 200; x += 5) move(x, 0);
    expect(mode()).toBe("pointer");
    expect(seen.takeRecords()).toHaveLength(1);
    seen.disconnect();
  });

  // Fails when the listener outlives teardown.
  it("stops listening on teardown", () => {
    stop();
    move(0, 0);
    move(50, 50);
    expect(mode()).toBe("nav");
    stop = installModalityTracking();
  });
});

// html[data-pad] (controller mode, drives the pad: variant, e.g. LB/RB glyphs) is separate from
// data-input: the keyboard sets data-input="nav" too, but must not show controller glyphs.
describe("controller mode (data-pad)", () => {
  const pad = () => document.documentElement.dataset.pad;
  const runPad = (button: number) => {
    const frames: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => frames.push(cb));
    Object.defineProperty(navigator, "getGamepads", {
      configurable: true,
      value: () => [{ connected: true, axes: [0, 0, 0, 0], buttons: Array.from({ length: 17 }, (_, i) => ({ pressed: i === button })) }],
    });
    const stopPad = startGamepadLoop(() => {});
    frames.shift()!(0);
    stopPad();
  };

  // Fails when data-pad is tied to data-input: keyboard nav would show controller glyphs.
  it("is off by default and keyboard nav keys do not set it", () => {
    expect(pad()).toBeUndefined();
    key("ArrowDown");
    key("PageDown");
    expect(pad()).toBeUndefined();
  });

  // Fails when the gamepad loop does not flag controller mode (glyphs never appear).
  it("a gamepad action sets it (LB included)", () => {
    runPad(BUTTON.LB);
    expect(pad()).toBe("");
  });

  // Fails when switching back to the keyboard keeps showing controller glyphs.
  it("a nav key clears it; typing text does not", () => {
    runPad(BUTTON.DOWN);
    key("a");
    expect(pad()).toBe("");
    key("ArrowLeft");
    expect(pad()).toBeUndefined();
  });

  // Fails when the mouse taking over keeps controller glyphs.
  it("a pointer press or real mouse move clears it; jitter does not", () => {
    runPad(BUTTON.DOWN);
    pointer();
    expect(pad()).toBeUndefined();
    runPad(BUTTON.DOWN);
    move(100, 100);
    move(102, 101);
    expect(pad()).toBe("");
    move(110, 101);
    expect(pad()).toBeUndefined();
  });

  // Fails when every gamepad frame rewrites the attribute (style recalculation per frame).
  it("writes the attribute once per change", () => {
    const seen = new MutationObserver(() => {});
    seen.observe(document.documentElement, { attributes: true, attributeFilter: ["data-pad"] });
    runPad(BUTTON.DOWN);
    runPad(BUTTON.UP);
    expect(seen.takeRecords()).toHaveLength(1);
    key("Tab");
    key("Tab");
    expect(seen.takeRecords()).toHaveLength(1);
    seen.disconnect();
  });
});
