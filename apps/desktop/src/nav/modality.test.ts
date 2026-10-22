// SPDX-License-Identifier: GPL-3.0-or-later
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BUTTON, startGamepadLoop } from "./gamepad";
import { installModalityTracking } from "./modality";

const mode = () => document.documentElement.dataset.input;
const key = (k: string) => document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
const pointer = () => document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));

let stop: () => void;
beforeEach(() => {
  delete document.documentElement.dataset.input;
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
});
