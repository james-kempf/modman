// SPDX-License-Identifier: GPL-3.0-or-later
// The production CSP is `style-src 'self'; img-src 'self' data:` with no
// remote origins: sources must not rely on inline styles, injected <style>
// tags, raw HTML or remote fonts/images.
import { describe, expect, it } from "vitest";
import indexHtml from "../index.html?raw";
import css from "./index.css?raw";

const sources = import.meta.glob(["./**/*.{ts,tsx}", "!./**/*.test.{ts,tsx}"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

describe("CSP compatibility", () => {
  it("scans the app sources", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(15);
  });

  it.each(Object.entries(sources))("%s has no inline styles, <style> or raw HTML", (_, src) => {
    expect(src).not.toMatch(/\bstyle=\{/);
    expect(src).not.toMatch(/<style[\s>]/);
    expect(src).not.toMatch(/dangerouslySetInnerHTML|innerHTML\s*=|insertAdjacentHTML/);
    expect(src).not.toMatch(/\.style\.(?!\w+\s*[!=]==)/);
    expect(src).not.toMatch(/\beval\(|new Function\(/);
  });

  it("images only from data: URLs (thumbnails), never remote", () => {
    for (const src of Object.values(sources)) {
      expect(src).not.toMatch(/<img[^>]*src=["']https?:/);
      expect(src).not.toMatch(/thumbnailUrl\}/);
    }
  });

  it("index.html and the stylesheet load nothing remote and nothing inline", () => {
    expect(indexHtml).not.toMatch(/https?:\/\//);
    expect(indexHtml).not.toMatch(/<style|style=/);
    expect(indexHtml.match(/<script/g)).toHaveLength(1);
    expect(indexHtml).toMatch(/<script type="module" src="\/src\/main.tsx">/);
    expect(css).not.toMatch(/url\(\s*["']?https?:/);
    expect(css).not.toMatch(/@import\s+["']?https?:/);
  });
});
