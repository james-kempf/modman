// SPDX-License-Identifier: GPL-3.0-or-later
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installFocusRecovery } from "./spatial";

/** Button with a fixed layout box (jsdom has no layout). */
function btn(id: string, y: number) {
  const b = document.createElement("button");
  b.id = id;
  b.getBoundingClientRect = () => ({ left: 0, top: y, right: 100, bottom: y + 50, width: 100, height: 50, x: 0, y, toJSON() {} }) as DOMRect;
  document.body.appendChild(b);
  return b;
}

const frames = async () => {
  await new Promise((r) => requestAnimationFrame(() => r(null)));
  await new Promise((r) => requestAnimationFrame(() => r(null)));
};

let stop: () => void;
let focusSpy: ReturnType<typeof vi.spyOn>;
let scrollSpy: ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  scrollSpy = vi.fn();
  HTMLElement.prototype.scrollIntoView = scrollSpy as unknown as HTMLElement["scrollIntoView"];
  stop = installFocusRecovery();
});

afterEach(() => {
  stop();
  vi.restoreAllMocks();
  delete (HTMLElement.prototype as Partial<HTMLElement>).scrollIntoView;
  delete document.documentElement.dataset.input;
  document.body.innerHTML = "";
});

/** Focus `a`, then let it lose focus to <body>; optionally unmount it. */
async function loseFocus(a: HTMLElement, unmount: boolean) {
  a.focus();
  focusSpy = vi.spyOn(HTMLElement.prototype, "focus");
  a.blur();
  if (unmount) a.remove();
  await frames();
}

describe("focus recovery never scrolls on pointer clicks", () => {
  // Fails when a click on a non-actionable area refocuses (and scrolls back to) the previous control.
  it("pointer click on a non-actionable area leaves focus and scroll alone", async () => {
    document.documentElement.dataset.input = "pointer";
    const a = btn("a", 0);
    btn("b", 100);
    await loseFocus(a, false);
    expect(focusSpy).not.toHaveBeenCalled();
    expect(scrollSpy).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(document.body);
  });

  // Fails when recovery after an unmount in pointer mode uses a scrolling focus().
  it("pointer-mode recovery after unmount focuses without scrolling", async () => {
    document.documentElement.dataset.input = "pointer";
    const a = btn("a", 0);
    const b = btn("b", 100);
    await loseFocus(a, true);
    expect(document.activeElement).toBe(b);
    expect(focusSpy).toHaveBeenCalledWith({ preventScroll: true });
    expect(scrollSpy).not.toHaveBeenCalled();
  });

  // Fails when keyboard/controller recovery stops bringing the recovered control into view.
  it("nav-mode recovery scrolls the recovered control into view minimally", async () => {
    document.documentElement.dataset.input = "nav";
    const a = btn("a", 0);
    const b = btn("b", 100);
    await loseFocus(a, true);
    expect(document.activeElement).toBe(b);
    expect(focusSpy).toHaveBeenCalledWith({ preventScroll: true });
    expect(scrollSpy).toHaveBeenCalledWith({ block: "nearest", inline: "nearest" });
  });
});
