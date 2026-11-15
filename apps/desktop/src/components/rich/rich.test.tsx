// SPDX-License-Identifier: GPL-3.0-or-later
// Description renderers: BBCode (mods) and Markdown (collections) become an
// allow-list of React elements; markup they do not know, raw HTML, unsafe
// URLs, over-deep nesting and oversize input stay text; links never navigate
// the webview and open through open_url.
import { fireEvent, screen, waitFor } from "@testing-library/react";
import type { ReactElement } from "react";
import { describe, expect, it } from "vitest";
import { setup } from "../../test/harness";
import { BBCode, MAX_DEPTH as BB_DEPTH, MAX_INPUT as BB_INPUT, decodeEntities } from "./bbcode";
import { MarkdownText, MAX_DEPTH as MD_DEPTH, MAX_INPUT as MD_INPUT } from "./markdown";

async function show(ui: ReactElement) {
  const env = await setup({ open_url: (a) => (a as { url: string }).url });
  const { container } = env.renderIn(<div data-testid="root">{ui}</div>);
  return { ...env, root: container.querySelector("[data-testid=root]") as HTMLElement };
}

/** No element or attribute able to run script, load media or style the page made it into the DOM. */
function expectInert(root: HTMLElement) {
  expect(root.querySelector("script, iframe, img, object, embed, style, video, audio, form, input, button, svg")).toBeNull();
  for (const e of root.querySelectorAll("*")) {
    for (const a of Array.from(e.attributes)) {
      expect(a.name).not.toMatch(/^on|^style$|^src$|^srcdoc$/i);
    }
    const href = e.getAttribute("href");
    if (href !== null) expect(href).toMatch(/^https?:\/\//);
  }
}

describe("BBCode renderer", () => {
  // Fails when an allow-listed tag stops mapping to its element (bold, italic, underline, strike, size, lists,
  // quotes, spoiler collapsed, code, rule) or "\n<br />" stops being one line break.
  it("renders the Nexus formatting tags as elements", async () => {
    const { root } = await show(
      <BBCode
        source={
          "[b]Bold[/b] [i]It[/i] [u]Under[/u] [s]Gone[/s]\n<br />[size=6]Big[/size][color=red]Red[/color][font=Courier New]Mono[/font]\n<br />" +
          "[list]\n<br />[*]One\n<br />[*]Two\n<br />[/list][list=1][*]First[/list]" +
          "[quote]Quoted[/quote][spoiler]Secret[/spoiler][code][b]raw[/b][/code][line][center]Mid[/center]"
        }
      />,
    );
    expect(root.querySelector("strong")).toHaveTextContent("Bold");
    expect(root.querySelector("em")).toHaveTextContent("It");
    expect(root.querySelector("u")).toHaveTextContent("Under");
    expect(root.querySelector("s")).toHaveTextContent("Gone");
    expect(root.querySelectorAll("br").length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText("Big").className).toContain("text-xl");
    expect(screen.getByText("Red").className).toContain("text-rose-300");
    expect(screen.getByText("Mono").tagName).not.toBe("SPAN");
    const items = root.querySelectorAll("ul > li");
    expect(Array.from(items, (li) => li.textContent)).toEqual(["One", "Two"]);
    expect(root.querySelector("ol > li")).toHaveTextContent("First");
    expect(root.querySelector("blockquote")).toHaveTextContent("Quoted");
    const spoiler = root.querySelector("details")!;
    expect(spoiler).not.toHaveAttribute("open");
    expect(spoiler.querySelector("summary")).toHaveTextContent("Spoiler");
    expect(spoiler).toHaveTextContent("Secret");
    // [code] content is verbatim: no bold inside.
    expect(root.querySelector("pre")).toHaveTextContent("[b]raw[/b]");
    expect(root.querySelector("pre strong")).toBeNull();
    expect(root.querySelector("hr")).not.toBeNull();
    expect(screen.getByText("Mid").className).toContain("text-center");
    expect(root.textContent!.replace("[b]raw[/b]", "")).not.toMatch(/\[\/?(b|i|u|s|size|color|font|list|\*|quote|spoiler|line|center)\b/);
    expectInert(root);
  });

  // Fails when bracketed words that are not tags vanish, or entities are parsed as markup instead of text.
  it("keeps unknown tags and decodes entities as text", async () => {
    const { root } = await show(<BBCode source={"Works with [any] [FTM] setup. C:&#92;Steam &lt;b&gt;x&lt;/b&gt; &amp;amp; &#x1F600; &#0; &bogus;"} />);
    expect(root).toHaveTextContent("Works with [any] [FTM] setup. C:\\Steam <b>x</b> &amp; 😀 &#0; &bogus;");
    expect(root.querySelector("b")).toBeNull();
    expect(decodeEntities("&#xD800;")).toBe("&#xD800;");
  });

  // Fails when a link navigates the webview (default not prevented) or does not open through open_url,
  // or when [img] / [youtube] load media instead of being plain links.
  it("links open through open_url and media are links only", async () => {
    const { root, t } = await show(
      <BBCode source={"[url=https://smapi.io]SMAPI[/url] [url]https://example.com/a[/url] [img width=20,height=20]https://i.imgur.com/x.png[/img] [youtube]9DXBti-c6C0[/youtube]"} />,
    );
    const smapi = screen.getByRole("link", { name: "SMAPI" });
    expect(smapi).toHaveAttribute("href", "https://smapi.io/");
    expect(fireEvent.click(smapi)).toBe(false); // preventDefault was called
    await waitFor(() => expect(t.callsOf("open_url")).toEqual([{ url: "https://smapi.io/" }]));
    expect(screen.getByRole("link", { name: "https://example.com/a" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Image: i.imgur.com" })).toHaveAttribute("href", "https://i.imgur.com/x.png");
    expect(screen.getByRole("link", { name: "Video: YouTube 9DXBti-c6C0" })).toHaveAttribute("href", "https://www.youtube.com/watch?v=9DXBti-c6C0");
    expectInert(root);
  });

  // XSS corpus. Fails when any payload becomes a live element, attribute or non-web link.
  it.each([
    ["script tag", "<script>alert(1)</script>", "<script>alert(1)</script>"],
    ["onerror attribute", '<img src=x onerror="alert(1)">', '<img src=x onerror="alert(1)">'],
    ["javascript: url", "[url=javascript:alert(1)]click[/url]", "click"],
    ["javascript: bare url", "[url]javascript:alert(1)[/url]", "javascript:alert(1)"],
    ["data: url", "[url=data:text/html;base64,PHNjcmlwdD4=]x[/url]", "x"],
    ["javascript: img", "[img]javascript:alert(1)[/img]", "Image: javascript:alert(1)"],
    ["quoted arg breakout", '[url="https://a.com" onclick="alert(1)"]y[/url]', "y"],
    ["raw HTML fragment", '<a href="javascript:alert(1)">z</a><iframe src="https://evil"></iframe>', '<a href="javascript:alert(1)">z</a>'],
    ["unclosed tags", "[b][i]open [url=https://a.com]never closed", "[b][i]open [url=https://a.com]never closed"],
    ["misnested tags", "[b]one [i]two[/b] three[/i]", "one [i]two three[/i]"],
    ["bad youtube id", '[youtube]"><script>[/youtube]', '[youtube]"><script>[/youtube]'],
    ["list item outside list", "[*]stray", "[*]stray"],
  ])("%s stays inert", async (_, source, text) => {
    const { root } = await show(<BBCode source={source} />);
    expect(root).toHaveTextContent(text, { normalizeWhitespace: false });
    expectInert(root);
    expect(root.querySelector("a")).toBeNull();
  });

  // Fails when deep nesting throws or keeps nesting elements past the limit instead of degrading to text.
  it("nesting past the depth limit stays text", async () => {
    const n = BB_DEPTH + 50;
    const { root } = await show(<BBCode source={`${"[quote]".repeat(n)}deep${"[/quote]".repeat(n)}`} />);
    expect(root.querySelectorAll("blockquote").length).toBeLessThanOrEqual(BB_DEPTH);
    expect(root).toHaveTextContent("deep");
    expect(root).toHaveTextContent("[quote]");
  });

  // Fails when oversize input is parsed (cost) or throws instead of being shown as plain text.
  it("oversize input is plain text", async () => {
    const big = `[b]x[/b]\n<br />${"a".repeat(BB_INPUT)}`;
    const { root } = await show(<BBCode source={big} />);
    expect(root.querySelector("strong")).toBeNull();
    expect(root.querySelector("br")).toBeNull();
    expect(root.textContent!.startsWith("[b]x[/b]\n")).toBe(true);
  });
});

describe("Markdown renderer", () => {
  // Fails when GFM formatting stops rendering (headings, emphasis, lists, quotes, links, strikethrough) or
  // headings are emitted at page level (h1/h2 inside a dialog).
  it("renders collection Markdown as elements", async () => {
    const { root, t } = await show(
      <MarkdownText source={"# Title\n\n### Steps\n\n**Bold** *it* ~~old~~\n\n- one\n- two\n\n1. first\n\n> quoted\n\n[full list](https://docs.google.com/x)\n\n---"} />,
    );
    expect(root.querySelector("h1, h2")).toBeNull();
    expect(screen.getByRole("heading", { name: "Title" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Steps" })).toBeInTheDocument();
    expect(root.querySelector("strong")).toHaveTextContent("Bold");
    expect(root.querySelector("em")).toHaveTextContent("it");
    expect(root.querySelector("del")).toHaveTextContent("old");
    expect(Array.from(root.querySelectorAll("ul > li"), (l) => l.textContent)).toEqual(["one", "two"]);
    expect(root.querySelector("ol > li")).toHaveTextContent("first");
    expect(root.querySelector("blockquote")).toHaveTextContent("quoted");
    expect(root.querySelector("hr")).not.toBeNull();
    const link = screen.getByRole("link", { name: "full list" });
    expect(fireEvent.click(link)).toBe(false);
    await waitFor(() => expect(t.callsOf("open_url")).toEqual([{ url: "https://docs.google.com/x" }]));
    expectInert(root);
  });

  // Fails when images load (<img>) instead of being links, or an image inside a link nests a second link.
  it("images are links, and only a label inside a link", async () => {
    const { root } = await show(<MarkdownText source={"![shot](https://i.imgur.com/a.png)\n\n[![](https://i.imgur.com/b.png)](https://ko-fi.com/x)"} />);
    expect(screen.getByRole("link", { name: "Image: shot" })).toHaveAttribute("href", "https://i.imgur.com/a.png");
    expect(screen.getByRole("link", { name: "Image" })).toHaveAttribute("href", "https://ko-fi.com/x");
    expect(root.querySelector("a a")).toBeNull();
    expectInert(root);
  });

  // Fails when table alignment comes through as an inline style attribute.
  it("tables carry no inline styles", async () => {
    const { root } = await show(<MarkdownText source={"| a | b |\n|:-:|--:|\n| 1 | 2 |"} />);
    expect(root.querySelector("td")).toHaveTextContent("1");
    expectInert(root);
  });

  // XSS corpus. Fails when raw HTML renders as elements (or vanishes instead of showing as text), or an unsafe
  // URL becomes a link.
  it.each([
    ["script tag", "<script>alert(1)</script>", "<script>alert(1)</script>"],
    ["onerror attribute", 'x <img src=x onerror="alert(1)"> y', '<img src=x onerror="alert(1)">'],
    ["javascript: link", "[click](javascript:alert(1))", "click"],
    ["data: link", "[d](data:text/html,<script>alert(1)</script>)", "d"],
    ["javascript: autolink", "<javascript:alert(1)>", "javascript:alert(1)"],
    ["raw HTML anchor", '<a href="javascript:alert(1)">z</a>', '<a href="javascript:alert(1)">'],
    ["raw button", "<button onclick=\"alert(1)\">b</button>", "<button onclick=\"alert(1)\">"],
    ["unclosed raw tags", "<div><span>open <b>bold", "<b>"],
  ])("%s stays inert", async (_, source, text) => {
    const { root } = await show(<MarkdownText source={source} />);
    expect(root).toHaveTextContent(text, { normalizeWhitespace: false });
    expectInert(root);
    expect(root.querySelector("a")).toBeNull();
  });

  // Fails when <details>/<summary> wrappers show as tags (they are dropped, their text kept).
  it("keeps the text of details/summary wrappers", async () => {
    const { root } = await show(<MarkdownText source={"<details><summary>Load order</summary>\n\nStep text\n\n</details>"} />);
    expect(root).toHaveTextContent("Load order");
    expect(root).toHaveTextContent("Step text");
    expect(root.textContent).not.toMatch(/<\/?(details|summary)/);
    expect(root.querySelector("details")).toBeNull();
  });

  // Fails when deep nesting throws or renders past the limit instead of flattening to text.
  it("nesting past the depth limit becomes text", async () => {
    const n = MD_DEPTH + 40;
    const { root } = await show(<MarkdownText source={`${"> ".repeat(n)}deep`} />);
    expect(root.querySelectorAll("blockquote").length).toBeLessThanOrEqual(MD_DEPTH);
    expect(root).toHaveTextContent("deep");
  });

  // Fails when oversize input is parsed or throws instead of being shown as plain text.
  it("oversize input is plain text", async () => {
    const { root } = await show(<MarkdownText source={`**x** <script>y</script>${"a".repeat(MD_INPUT)}`} />);
    expect(root.querySelector("strong")).toBeNull();
    expect(root.textContent!.startsWith("**x** <script>y</script>")).toBe(true);
    expectInert(root);
  });
});
