// SPDX-License-Identifier: GPL-3.0-or-later
// Gamepad API polling -> navigation actions (standard mapping):
// A select, B back, LB/RB previous/next tab, D-pad or left stick move.
// Directions repeat while held; buttons fire once per press.

import type { NavAction } from "./actions";
import { setModality } from "./modality";
import type { Dir } from "./spatial";

export const BUTTON = { A: 0, B: 1, LB: 4, RB: 5, UP: 12, DOWN: 13, LEFT: 14, RIGHT: 15 } as const;

export interface GamepadNavOptions {
  /** Stick deflection that counts as a direction. */
  deadzone?: number;
  /** ms before a held direction starts repeating. */
  repeatDelay?: number;
  /** ms between repeats. */
  repeatEvery?: number;
}

type PadLike = Pick<Gamepad, "buttons" | "axes" | "connected">;
export type GetPads = () => ReadonlyArray<PadLike | null>;

const defaultPads: GetPads = () =>
  typeof navigator !== "undefined" && navigator.getGamepads ? navigator.getGamepads() : [];

const pressed = (p: PadLike, i: number) => !!p.buttons[i]?.pressed;

export class GamepadNav {
  private held = new Set<string>();
  private dir: Dir | null = null;
  private dirSince = 0;
  private lastRepeat = 0;
  private readonly deadzone: number;
  private readonly repeatDelay: number;
  private readonly repeatEvery: number;

  constructor(
    private readonly getPads: GetPads = defaultPads,
    opts: GamepadNavOptions = {},
  ) {
    this.deadzone = opts.deadzone ?? 0.5;
    this.repeatDelay = opts.repeatDelay ?? 400;
    this.repeatEvery = opts.repeatEvery ?? 120;
  }

  /** Read every connected pad; returns the actions triggered since the last poll. */
  poll(now: number): NavAction[] {
    const pads = Array.from(this.getPads()).filter((p): p is PadLike => !!p && p.connected);
    const down = new Set<string>();
    let dir: Dir | null = null;
    for (const p of pads) {
      if (pressed(p, BUTTON.A)) down.add("A");
      if (pressed(p, BUTTON.B)) down.add("B");
      if (pressed(p, BUTTON.LB)) down.add("LB");
      if (pressed(p, BUTTON.RB)) down.add("RB");
      dir ??= this.direction(p);
    }

    const out: NavAction[] = [];
    const edge = (k: string) => down.has(k) && !this.held.has(k);
    if (edge("A")) out.push({ type: "select" });
    if (edge("B")) out.push({ type: "back" });
    if (edge("LB")) out.push({ type: "tab", delta: -1 });
    if (edge("RB")) out.push({ type: "tab", delta: 1 });
    this.held = down;

    if (dir !== this.dir) {
      this.dir = dir;
      this.dirSince = now;
      this.lastRepeat = now;
      if (dir) out.push({ type: "move", dir });
    } else if (dir && now - this.dirSince >= this.repeatDelay && now - this.lastRepeat >= this.repeatEvery) {
      this.lastRepeat = now;
      out.push({ type: "move", dir });
    }
    return out;
  }

  private direction(p: PadLike): Dir | null {
    if (pressed(p, BUTTON.UP)) return "up";
    if (pressed(p, BUTTON.DOWN)) return "down";
    if (pressed(p, BUTTON.LEFT)) return "left";
    if (pressed(p, BUTTON.RIGHT)) return "right";
    const x = p.axes[0] ?? 0;
    const y = p.axes[1] ?? 0;
    if (Math.max(Math.abs(x), Math.abs(y)) < this.deadzone) return null;
    if (Math.abs(x) > Math.abs(y)) return x > 0 ? "right" : "left";
    return y > 0 ? "down" : "up";
  }
}

/**
 * Poll on animation frames while a pad is connected. Returns a stop function.
 * WebKitGTK only exposes a pad after its first button press, so the loop
 * starts on `gamepadconnected` (and right away if one is already there).
 */
export function startGamepadLoop(
  dispatch: (a: NavAction) => void,
  win: Window = window,
  nav: GamepadNav = new GamepadNav(),
): () => void {
  let frame = 0;
  let running = false;
  const tick = () => {
    const actions = nav.poll(win.performance.now());
    // Gamepad input fires no key event: switch the focus ring on here.
    if (actions.length) setModality("nav", win.document);
    actions.forEach(dispatch);
    frame = win.requestAnimationFrame(tick);
  };
  const start = () => {
    if (running) return;
    running = true;
    frame = win.requestAnimationFrame(tick);
  };
  const stopLoop = () => {
    const any = defaultPadsOf(win).some((p) => p?.connected);
    if (!any && running) {
      running = false;
      win.cancelAnimationFrame(frame);
    }
  };
  win.addEventListener("gamepadconnected", start);
  win.addEventListener("gamepaddisconnected", stopLoop);
  if (defaultPadsOf(win).some((p) => p?.connected)) start();
  return () => {
    win.removeEventListener("gamepadconnected", start);
    win.removeEventListener("gamepaddisconnected", stopLoop);
    running = false;
    win.cancelAnimationFrame(frame);
  };
}

function defaultPadsOf(win: Window): ReadonlyArray<Gamepad | null> {
  const n = win.navigator;
  return n.getGamepads ? Array.from(n.getGamepads()) : [];
}
