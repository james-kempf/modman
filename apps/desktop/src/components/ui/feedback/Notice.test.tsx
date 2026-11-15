// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TOAST_DISC, TOAST_OPEN, TOAST_SHELL, TOAST_TEXT, TOAST_TITLE } from "../styles";
import { Notice } from "./Notice";

const TONES = ["info", "success", "warn", "error"] as const;

describe("Notice is informational only (T05)", () => {
  // Fails when a control (Undo, Deploy now, a close button) comes back inside a toast.
  it("has no focusable element and no button, in any tone", () => {
    const { container } = render(
      <>
        {TONES.map((t) => (
          <Notice key={t} tone={t}>
            {t}
          </Notice>
        ))}
      </>,
    );
    expect(screen.queryAllByRole("button")).toEqual([]);
    expect(container.querySelectorAll("button, a[href], input, [tabindex]")).toHaveLength(0);
    // Still announced: status for most tones, alert for errors.
    expect(screen.getAllByRole("status")).toHaveLength(3);
    expect(screen.getByRole("alert")).toHaveTextContent("error");
  });
});

describe("Notice shape (single toast style)", () => {
  // Fails when results and running-work toasts drift apart in any class (height, padding, gap, glass, radius,
  // animation), e.g. a separate job pill or the squared rounded-card notice coming back.
  it("is one pill, class for class, for every tone and for running work", () => {
    render(
      <>
        {TONES.map((t) => (
          <Notice key={t} tone={t}>
            {t}
          </Notice>
        ))}
        <Notice tone="info" busy detail="Linking files">
          Deploy
        </Notice>
      </>,
    );
    const pills = [...screen.getAllByRole("status"), screen.getByRole("alert")];
    expect(pills).toHaveLength(5);
    for (const p of pills) {
      // The shell plus exactly one motion class (opening, on first mount).
      expect(p.className).toBe(`${TOAST_SHELL} ${TOAST_OPEN}`);
      expect(p.querySelector(".truncate")!.className).toBe(TOAST_TITLE);
      expect((p.firstElementChild as HTMLElement).className.startsWith(TOAST_DISC)).toBe(true);
      expect((p.children[1] as HTMLElement).className).toBe(TOAST_TEXT);
    }
  });

  // Fails when running work loses its rails disc or its detail line, or is not marked busy.
  it("shows running work with the rails disc and a detail line", () => {
    render(
      <Notice tone="info" busy detail="deploying 3/10">
        Deploy
      </Notice>,
    );
    const pill = screen.getByRole("status");
    expect(pill).toHaveAttribute("aria-busy", "true");
    expect(pill.querySelectorAll(".animate-bar")).toHaveLength(3);
    expect(pill).toHaveTextContent("deploying 3/10");
    expect(pill).toHaveAttribute("title", "Deploy: deploying 3/10");
  });

  // Fails when a toast can grow the app bar: a min height (the old min-h-14) or vertical padding that adds to
  // the bar's control height, instead of exactly the bar's control height.
  it("is exactly the bar's control height (h-control), never a min height", () => {
    const cls = TOAST_SHELL.split(" ");
    expect(cls).toContain("h-control");
    expect(cls.some((c) => c.startsWith("min-h-"))).toBe(false);
    // The 36px disc nests concentric in the 48px pill: (48 - 36) / 2 = 6px = pl-1.5.
    expect(cls).toContain("pl-1.5");
    expect(TOAST_DISC.split(" ")).toContain("size-9");
  });

  // Fails when a narrow slot squeezes the text to nothing (or wraps it) instead of a disc-only pill whose text
  // is still read and shown as a tooltip.
  it("closes to the disc in a narrow slot, text still read and in the tooltip", () => {
    render(
      <Notice tone="info">
        Deployed 3 mods
      </Notice>,
    );
    expect(TOAST_TEXT).toMatch(/@max-3xs:sr-only/);
    expect(TOAST_SHELL).toMatch(/@max-3xs:pr-1\.5/);
    const pill = screen.getByRole("status");
    expect(pill).toHaveAttribute("title", "Deployed 3 mods");
    expect(pill).toHaveTextContent("Deployed 3 mods");
  });
});
