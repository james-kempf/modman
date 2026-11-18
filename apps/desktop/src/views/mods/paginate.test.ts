// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { clampPage, pageCount, pageSlice } from "./paginate";

const items = Array.from({ length: 112 }, (_, i) => i);

describe("pageCount", () => {
  // Fails when an empty list reports 0 pages: clampPage would then return -1.
  it("is at least 1 and rounds a partial page up", () => {
    expect(pageCount(0, 50)).toBe(1);
    expect(pageCount(50, 50)).toBe(1);
    expect(pageCount(51, 50)).toBe(2);
  });
});

describe("clampPage", () => {
  // Fails when a list that shrank (an uninstall on the last page) leaves the page past the end.
  it("pulls a page past the end back to the last page, and a negative one to 0", () => {
    expect(clampPage(5, 3)).toBe(2);
    expect(clampPage(-1, 3)).toBe(0);
    expect(clampPage(1, 3)).toBe(1);
  });
});

describe("pageSlice", () => {
  // Fails on an off-by-one at the page edges or a short last page.
  it("returns the rows of the page, with a short last page", () => {
    expect(pageSlice(items, 0, 50)).toEqual(items.slice(0, 50));
    expect(pageSlice(items, 1, 50)[0]).toBe(50);
    expect(pageSlice(items, 2, 50)).toEqual([100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111]);
  });
  // Fails when the page past the end renders as an empty list instead of the last page.
  it("clamps a page past the end to the last page", () => {
    expect(pageSlice(items, 9, 50)).toEqual(pageSlice(items, 2, 50));
    expect(pageSlice([], 3, 50)).toEqual([]);
  });
});
