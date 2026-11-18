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
