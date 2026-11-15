// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { VIEW_FRAME, ViewFrame } from "..";

describe("ViewFrame", () => {
  // Fails when the frame stops using the one shared layout (views drift apart again).
  it("lays its children out with the shared view frame classes", () => {
    render(
      <ViewFrame>
        <p>child</p>
      </ViewFrame>,
    );
    const frame = screen.getByText("child").parentElement!;
    expect(frame.className).toBe(VIEW_FRAME);
    expect(frame).toHaveAttribute("data-view-frame");
  });

  // Fails when the wrapper becomes focusable, a nav scope or a nav section container (breaks nav/spatial.ts).
  it("adds no focus stop or navigation boundary", () => {
    render(
      <ViewFrame>
        <p>child</p>
      </ViewFrame>,
    );
    const frame = screen.getByText("child").parentElement!;
    expect(frame.tagName).toBe("DIV");
    for (const a of ["tabindex", "role", "data-nav-scope", "data-nav-sections", "data-nav-section"]) expect(frame).not.toHaveAttribute(a);
  });
});
