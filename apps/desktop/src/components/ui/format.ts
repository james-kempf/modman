// SPDX-License-Identifier: GPL-3.0-or-later
// Display formatting helpers: monogram initials, byte sizes, timestamps.

/** Deterministic flat colour class + initials for a name (legacy monogram fallback). */
export function monogram(name: string): { cls: string; initials: string } {
  const h = [...name].reduce((a, c) => (a * 31 + c.charCodeAt(0)) % 997, 7);
  // Words of letters/digits in any script (marks belong to their letter); the
  // initial is a word's first grapheme without its combining marks.
  const words = name.match(/[\p{L}\p{N}][\p{L}\p{M}\p{N}]*/gu) ?? [];
  const initial = (w: string) => {
    const g = graphemes(w)[0] ?? "";
    return g.replace(/\p{M}+/gu, "") || g;
  };
  const initials =
    words
      .slice(0, 2)
      .map(initial)
      .join("")
      .toLocaleUpperCase() ||
    // No letters (e.g. only emoji): the first grapheme as is.
    graphemes(name.trim())[0] ||
    "?";
  return { cls: `mono-${h % 8}`, initials };
}

/** User-perceived characters (an emoji ZWJ sequence or a letter with its marks is one). */
function graphemes(s: string): string[] {
  if (typeof Intl !== "undefined" && "Segmenter" in Intl)
    return Array.from(new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(s), (x) => x.segment);
  return Array.from(s);
}

export function bytes(n: number | null | undefined): string {
  if (n === null || n === undefined) return "?";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(i && v < 10 ? 1 : 0)} ${u[i]}`;
}

/** Unix seconds -> local date/time. */
export function when(ts: number | null | undefined): string {
  if (!ts) return "never";
  return new Date(ts * 1000).toLocaleString();
}

/** Unix seconds -> "3 min ago" (legacy `ago`). */
export function ago(ts: number | null | undefined, now = Date.now()): string {
  if (!ts) return "never";
  const s = Math.max(0, Math.round(now / 1000 - ts));
  if (s < 60) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h} h ago`;
  return `${Math.round(h / 24)} days ago`;
}
