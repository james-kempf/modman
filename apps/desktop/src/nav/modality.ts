// SPDX-License-Identifier: GPL-3.0-or-later
// Input modality on <html data-input>: "pointer" after a mouse/touch press or
// a real mouse/pen movement, "nav" after a navigation key or a gamepad action. index.css shows the
// focus ring only under "nav" (:focus-visible cannot be used: WebKit does
// not apply it to the gamepad's programmatic focus).

export type Modality = "pointer" | "nav";

const NAV_KEYS = new Set(["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Tab", "Enter", "Escape", "PageUp", "PageDown", " "]);

/** Mouse travel (px, summed over moves less than MOVE_GAP_MS apart) that leaves nav mode. */
const MOVE_THRESHOLD = 6;
const MOVE_GAP_MS = 500;

export function setModality(m: Modality, doc: Document = document): void {
  // Skip no-op writes: the gamepad loop calls this on every action.
  if (doc.documentElement.dataset.input !== m) doc.documentElement.dataset.input = m;
}

export function installModalityTracking(doc: Document = document): () => void {
  if (!doc.documentElement.dataset.input) setModality("nav", doc);
  const onPointer = () => setModality("pointer", doc);
  const onKey = (e: KeyboardEvent) => {
    if (NAV_KEYS.has(e.key)) setModality("nav", doc);
  };
  // Moving the mouse leaves nav mode, so only the hovered control is highlighted. Focus stays where
  // it is (unstyled outside nav), and the next arrow/gamepad input resumes from it. Screen-position
  // deltas, not movementX: the moves WebKit/Chromium synthesize when content scrolls under a still
  // cursor (scrollIntoView on nav focus) keep the screen position, and slow jitter decays.
  let last: { x: number; y: number; t: number } | null = null;
  let travel = 0;
  const onMove = (e: PointerEvent) => {
    if (e.pointerType === "touch") return;
    const prev = last;
    last = { x: e.screenX, y: e.screenY, t: e.timeStamp };
    if (doc.documentElement.dataset.input !== "nav" || !prev) return;
    if (e.timeStamp - prev.t > MOVE_GAP_MS) travel = 0;
    travel += Math.hypot(e.screenX - prev.x, e.screenY - prev.y);
    if (travel >= MOVE_THRESHOLD) {
      travel = 0;
      setModality("pointer", doc);
    }
  };
  // Capture: runs even when a handler stops propagation.
  doc.addEventListener("pointerdown", onPointer, true);
  doc.addEventListener("pointermove", onMove, { capture: true, passive: true });
  doc.addEventListener("keydown", onKey, true);
  return () => {
    doc.removeEventListener("pointerdown", onPointer, true);
    doc.removeEventListener("pointermove", onMove, true);
    doc.removeEventListener("keydown", onKey, true);
  };
}
