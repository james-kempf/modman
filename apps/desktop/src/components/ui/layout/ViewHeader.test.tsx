// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ViewHeader } from "..";

describe("ViewHeader text", () => {
  // Fails when a title, eyebrow or intro line comes back (the view's name lives on <main>'s aria-label instead).
  it("renders no heading and no text of its own", () => {
    const { container } = render(
      <ViewHeader>
        <button type="button">act</button>
      </ViewHeader>,
    );
    expect(screen.queryByRole("heading")).toBeNull();
    expect(container.textContent).toBe("act");
    expect(container.querySelector(".eyebrow")).toBeNull();
  });

  // Fails when an empty header (nothing left to show) still renders an orphaned wrapper or its spacing.
  it("renders nothing when it has nothing to show", () => {
    const { container } = render(<ViewHeader />);
    expect(container).toBeEmptyDOMElement();
  });
});

describe("ViewHeader actions", () => {
  // Fails when the actions leave the header's end (right) or stop wrapping at narrow widths.
  it("sit at the end of a wrapping row", () => {
    render(
      <ViewHeader>
        <button type="button">act</button>
      </ViewHeader>,
    );
    const header = screen.getByRole("button", { name: "act" }).closest("header")!;
    expect(header.className).toMatch(/\bflex-wrap\b/);
    expect(header.className).toMatch(/\bjustify-end\b/);
  });
});

describe("ViewHeader search", () => {
  // Fails when a view's search stops being centered in the header row (it sat top right before).
  it("goes in the centered slot of a SearchRow, actions at the end", () => {
    render(
      <ViewHeader search={<input type="search" aria-label="q" />}>
        <button type="button">act</button>
      </ViewHeader>,
    );
    const slot = screen.getByRole("searchbox", { name: "q" }).parentElement!;
    expect(slot.className).toMatch(/\bjustify-self-center\b/);
    expect(slot.className).toMatch(/\bxl:col-start-2\b/);
    expect(screen.getByRole("button", { name: "act" }).closest(".xl\\:col-start-3")).not.toBeNull();
    expect(slot.closest("header")).not.toBeNull();
  });
});
