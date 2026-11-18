// SPDX-License-Identifier: GPL-3.0-or-later
// Client-side paging of the load order (pages are 0-based; labels are 1-based).

/** Rows per load order page. */
export const MODS_PAGE_SIZE = 50;

/** Pages needed for `total` rows; never 0, so an empty list still has page 0. */
export const pageCount = (total: number, size: number) => Math.max(1, Math.ceil(total / size));

/** `page` within 0..pages-1: a list that shrank under it lands on its last page. */
export const clampPage = (page: number, pages: number) => Math.min(Math.max(0, page), pages - 1);

/** The rows of `page` (clamped). */
export function pageSlice<T>(items: readonly T[], page: number, size: number): T[] {
  const start = clampPage(page, pageCount(items.length, size)) * size;
  return items.slice(start, start + size);
}

/** 1-based first and last row numbers of `page`, for "51–100 of 312" (0–0 when empty). */
export function pageRange(page: number, size: number, total: number) {
  if (!total) return { first: 0, last: 0 };
  const first = page * size + 1;
  return { first, last: Math.min(total, first + size - 1) };
}

const SLOTS = 7;

/**
 * The page buttons to show: all of them up to 7 pages, else always 7 slots
 * (first, last, current ±1, "gap" for the skipped runs), so the pager keeps
 * its width while paging.
 */
export function pageList(page: number, pages: number): (number | "gap")[] {
  const range = (from: number, n: number) => Array.from({ length: n }, (_, i) => from + i);
  if (pages <= SLOTS) return range(0, pages);
  if (page < 4) return [...range(0, 5), "gap", pages - 1];
  if (page > pages - 5) return [0, "gap", ...range(pages - 5, 5)];
  return [0, "gap", page - 1, page, page + 1, "gap", pages - 1];
}
