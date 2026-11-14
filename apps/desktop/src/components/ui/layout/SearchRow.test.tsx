// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SearchRow } from "..";

const cell = (text: string) => screen.getByText(text).parentElement!;

describe("SearchRow", () => {
  // Fails when the search stops sitting in the middle column (centered) of a three-column row at xl,
  // or the side slots leave their columns.
  it("centers the search between equal side columns, start left and end right", () => {
    render(<SearchRow start={<span>s</span>} search={<span>q</span>} end={<span>e</span>} />);
    const row = cell("q").parentElement!;
    expect(row.className).toMatch(/xl:grid-cols-\[minmax\(0,1fr\)_auto_minmax\(0,1fr\)\]/);
    expect(cell("q").className).toMatch(/\bjustify-self-center\b/);
    expect(cell("q").className).toMatch(/\bxl:col-start-2\b/);
    expect(cell("s").className).toMatch(/\bxl:col-start-1\b/);
    expect(cell("e").className).toMatch(/\bxl:col-start-3\b/);
  });

  // Fails when the narrow layout stops being one column (the search would collide with the side slots),
  // or a side cell's content can widen the row past the view.
  it("stacks into one column below xl, every cell shrinkable", () => {
    render(<SearchRow start={<span>s</span>} search={<span>q</span>} end={<span>e</span>} />);
    const row = cell("q").parentElement!;
    expect(row.className).toMatch(/(^|\s)grid-cols-\[minmax\(0,1fr\)\]/);
    for (const c of ["s", "q", "e"]) expect(cell(c).className).toMatch(/\bmin-w-0\b/);
  });

  // Fails when an empty side slot leaves an orphaned wrapper in the row.
  it("renders only the slots it is given", () => {
    render(<SearchRow search={<span>q</span>} />);
    expect(cell("q").parentElement!.children).toHaveLength(1);
  });
});
