// SPDX-License-Identifier: GPL-3.0-or-later
// Input modality on <html data-input>: "pointer" after a mouse/touch press,
// "nav" after a navigation key or a gamepad action. index.css shows the
// focus ring only under "nav" (:focus-visible cannot be used: WebKit does
// not apply it to the gamepad's programmatic focus).

export type Modality = "pointer" | "nav";

const NAV_KEYS = new Set(["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Tab", "Enter", "Escape", "PageUp", "PageDown", " "]);

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
  // Capture: runs even when a handler stops propagation.
  doc.addEventListener("pointerdown", onPointer, true);
  doc.addEventListener("keydown", onKey, true);
  return () => {
    doc.removeEventListener("pointerdown", onPointer, true);
    doc.removeEventListener("keydown", onKey, true);
  };
}
