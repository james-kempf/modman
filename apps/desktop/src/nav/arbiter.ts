// SPDX-License-Identifier: GPL-3.0-or-later
// One physical press = one nav action. Steam Input reports a d-pad/button
// press twice: through the Gamepad API and as a keyboard event (arrows,
// Enter, Escape). The gamepad loop and the keyboard handler both consult
// this arbiter so whichever source arrives second is dropped.

/** A physical input both sources can report. */
export type NavInput = "up" | "down" | "left" | "right" | "A" | "B" | "LB" | "RB";

/** Default ms within which the other source reporting the same input is a duplicate. */
export const DUP_WINDOW = 150;
/** Pad state older than this is not trusted (loop stopped or throttled). */
const STALE_POLL = 100;

const within = (t: number | undefined, now: number, span: number) =>
  t !== undefined && now - t >= 0 && now - t < span;

export class NavArbiter {
  dupWindow = DUP_WINDOW;
  private padHeld = new Set<NavInput>();
  private lastPoll = -Infinity;
  private padSent = new Map<NavInput, number>();
  private keySent = new Map<NavInput, number>();

  /** Gamepad loop: the inputs held on any pad at `now`. */
  notePoll(held: ReadonlySet<NavInput>, now: number): void {
    this.padHeld = new Set(held);
    this.lastPoll = now;
  }

  /** Gamepad loop: an action for `input` was dispatched. */
  notePad(input: NavInput, now: number): void {
    this.padSent.set(input, now);
  }

  /** Gamepad loop: a new press is a duplicate if the keyboard just reported it. */
  padIsDuplicate(input: NavInput, now: number): boolean {
    return within(this.keySent.get(input), now, this.dupWindow);
  }

  /** Keyboard: an event for `input` was let through. */
  noteKey(input: NavInput, now: number): void {
    this.keySent.set(input, now);
  }

  /** Keyboard: the pad holds (or just sent) this input, so the key is its echo. */
  keyIsDuplicate(input: NavInput, repeat: boolean, now: number): boolean {
    const fresh = now - this.lastPoll <= STALE_POLL;
    if (fresh && this.padHeld.has(input)) return true;
    if (repeat && fresh && this.padHeld.size > 0) return true;
    return within(this.padSent.get(input), now, this.dupWindow);
  }

  reset(): void {
    this.padHeld.clear();
    this.lastPoll = -Infinity;
    this.padSent.clear();
    this.keySent.clear();
  }
}

/** Shared by installKeyboardNav and startGamepadLoop unless one is passed in. */
export const navArbiter = new NavArbiter();
