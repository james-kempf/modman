// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ViewHeader } from "..";

const actionsOf = () => screen.getByRole("button", { name: "act" }).parentElement!;

describe("ViewHeader description", () => {
  // Fails when the description gets a measure cap (e.g. max-w-prose) again and wraps before the header's free width.
  it("is not width-capped, so it wraps only at the space left beside the actions", () => {
    render(
      <ViewHeader title="T" description="desc">
        <button type="button">act</button>
      </ViewHeader>,
    );
    const desc = screen.getByText("desc");
    expect(desc.className).not.toMatch(/(^|\s)max-w-/);
    // The text column grows into the free space; the actions keep their content size.
    expect(desc.className).toMatch(/\bflex-\[1_1_24rem\]/);
    expect(actionsOf().className).not.toMatch(/\bflex-1\b|\bgrow\b/);
  });
});

describe("ViewHeader actions", () => {
  // Fails when the actions leave the description's row (back beside the title block) or stop top-aligning with its first line.
  it("share the description's row, top-aligned, below a full-width title block", () => {
    render(
      <ViewHeader title="T" eyebrow="E" description="desc">
        <button type="button">act</button>
      </ViewHeader>,
    );
    const row = screen.getByText("desc").parentElement!;
    expect(actionsOf().parentElement).toBe(row);
    expect(row.className).toMatch(/\bflex-wrap\b/);
    expect(row.className).toMatch(/\bitems-start\b/);
    // Title and eyebrow are not in that row, so the actions never narrow the title.
    const h1 = screen.getByRole("heading", { level: 1, name: "T" });
    expect(row.contains(h1)).toBe(false);
    expect(row.contains(screen.getByText("E"))).toBe(false);
  });

  // Fails when a header without a description loses the bottom-aligned title + actions layout.
  it("sit beside the title, bottom-aligned, when there is no description", () => {
    render(
      <ViewHeader title="T">
        <button type="button">act</button>
      </ViewHeader>,
    );
    const header = screen.getByRole("heading", { level: 1, name: "T" }).closest("header")!;
    expect(actionsOf().parentElement).toBe(header);
    expect(header.className).toMatch(/\bflex-wrap\b/);
    expect(header.className).toMatch(/\bitems-end\b/);
  });
});
