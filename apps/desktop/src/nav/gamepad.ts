// SPDX-License-Identifier: GPL-3.0-or-later
// Gamepad API polling -> navigation actions (standard mapping):
// A select, B back, LB/RB previous/next tab, D-pad or left stick move.
// Directions repeat while held; buttons fire once per press.

import type { NavAction } from "./actions";
import { navArbiter, type NavArbiter, type NavInput } from "./arbiter";
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
  /** ms within which a key and a pad reporting the same input count as one press (sets the arbiter's window). */
  dupWindow?: number;
  /** ms an input may read as released (pads handing over) and still continue the same hold. */
  releaseGrace?: number;
  /** Shared with installKeyboardNav; defaults to the app-wide one. */
  arbiter?: NavArbiter;
}

type PadLike = Pick<Gamepad, "buttons" | "axes" | "connected">;
export type GetPads = () => ReadonlyArray<PadLike | null>;

const defaultPads: GetPads = () =>
  typeof navigator !== "undefined" && navigator.getGamepads ? navigator.getGamepads() : [];

const pressed = (p: PadLike, i: number) => !!p.buttons[i]?.pressed;

export class GamepadNav {
  private held = new Map<NavInput, Set<number>>();
  private released = new Map<NavInput, { at: number; on: Set<number> }>();
  private dir: Dir | null = null;
  private dirSince = 0;
  private lastRepeat = 0;
  private readonly deadzone: number;
  private readonly repeatDelay: number;
  private readonly repeatEvery: number;
  private readonly releaseGrace: number;
  readonly arbiter: NavArbiter;

  constructor(
    private readonly getPads: GetPads = defaultPads,
    opts: GamepadNavOptions = {},
  ) {
    this.deadzone = opts.deadzone ?? 0.5;
    this.repeatDelay = opts.repeatDelay ?? 400;
    this.repeatEvery = opts.repeatEvery ?? 120;
    this.releaseGrace = opts.releaseGrace ?? 50;
    this.arbiter = opts.arbiter ?? navArbiter;
    if (opts.dupWindow !== undefined) this.arbiter.dupWindow = opts.dupWindow;
  }

  /** Read every connected pad; returns the actions triggered since the last poll. */
  poll(now: number): NavAction[] {
    // Input -> indices of the pads holding it.
    const down = new Map<NavInput, Set<number>>();
    const hold = (k: NavInput, i: number) => down.set(k, (down.get(k) ?? new Set()).add(i));
    let dir: Dir | null = null;
    for (const [i, p] of Array.from(this.getPads()).entries()) {
      if (!p?.connected) continue;
      for (const b of ["A", "B", "LB", "RB"] as const) if (pressed(p, BUTTON[b])) hold(b, i);
      if (dir) continue;
      dir = this.direction(p);
      if (dir) hold(dir, i);
    }
    for (const [k, on] of this.held) if (!down.has(k)) this.released.set(k, { at: now, on });
    // Steam's virtual pad and the physical pad can hand a press over with a
    // frame of neither: the same input back on another pad within the grace
    // continues the hold. Same pad = a real second press.
    const resumed = (k: NavInput) => {
      const r = this.released.get(k);
      if (!r || now - r.at >= this.releaseGrace) return false;
      return [...(down.get(k) ?? [])].some((i) => !r.on.has(i));
    };
    const edge = (k: NavInput) => down.has(k) && !this.held.has(k) && !resumed(k);

    const out: NavAction[] = [];
    // A new press the keyboard (Steam Input keys) already reported is dropped.
    const emit = (k: NavInput, a: NavAction) => {
      if (this.arbiter.padIsDuplicate(k, now)) return;
      this.arbiter.notePad(k, now);
      out.push(a);
    };
    if (edge("A")) emit("A", { type: "select" });
    if (edge("B")) emit("B", { type: "back" });
    if (edge("LB")) emit("LB", { type: "tab", delta: -1 });
    if (edge("RB")) emit("RB", { type: "tab", delta: 1 });
    this.held = down;
    this.arbiter.notePoll(new Set(down.keys()), now);

    if (dir !== this.dir) {
      this.dir = dir;
      if (dir && !resumed(dir)) {
        this.dirSince = now;
        this.lastRepeat = now;
        emit(dir, { type: "move", dir });
      }
    } else if (dir && now - this.dirSince >= this.repeatDelay && now - this.lastRepeat >= this.repeatEvery) {
      // Own repeats are never dropped; they only refresh the arbiter.
      this.lastRepeat = now;
      this.arbiter.notePad(dir, now);
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
      // No pad left: nothing may keep swallowing keys.
      nav.arbiter.reset();
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
    nav.arbiter.reset();
  };
}

function defaultPadsOf(win: Window): ReadonlyArray<Gamepad | null> {
  const n = win.navigator;
  return n.getGamepads ? Array.from(n.getGamepads()) : [];
}
