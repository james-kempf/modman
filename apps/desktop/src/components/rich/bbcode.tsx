// SPDX-License-Identifier: GPL-3.0-or-later
// Nexus mod descriptions (BBCode, newlines sent as "\n<br />") as React
// elements. Only the allow-listed tags below become elements; unknown,
// unbalanced or over-deep tags and every other piece of markup (raw HTML
// included) stay literal text. Entities decode into text, never markup.
// Pictures and videos become links (no <img>, no iframe); links go through
// ExternalLink (backend open_url). No inline styles: sizes and colours map
// to theme classes.

import { Fragment, type ReactNode } from "react";
import { cx } from "../ui";
import { ExternalLink, MediaLink } from "./ExternalLink";

/** Larger input is shown as plain text (no parsing). Nexus descriptions run 4 to 30 KB. */
export const MAX_INPUT = 100_000;
/** Tags nested deeper than this stay literal text. */
export const MAX_DEPTH = 12;

type Text = { t: "text"; v: string };
type Br = { t: "br" };
type El = { t: "el"; tag: string; arg: string | null; raw: string; children: Node[] };
type Node = Text | Br | El;

const KNOWN = new Set([
  "b", "i", "u", "s", "size", "color", "font", "center", "left", "right",
  "list", "*", "img", "url", "youtube", "spoiler", "code", "line", "quote",
]);
/** Content is taken verbatim up to the closing tag. */
const VERBATIM = new Set(["code", "img", "youtube"]);

// A tag ([b], [/b], [size=4], [img width=20,height=20], [*]) or a line break ("\n<br />").
const TOKEN = /\[(\/?)(\*|[a-z]+)((?:=|\s)[^[\]\n]*)?\]|\n?<br\s*\/?>/gi;

const NAMED: Record<string, string> = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " " };

/** Decodes &name; / &#N; / &#xH; into text; anything unknown or invalid stays as written. */
export function decodeEntities(s: string): string {
  return s.replace(/&(#x[0-9a-f]{1,6}|#[0-9]{1,7}|[a-z]{2,6});/gi, (all, e: string) => {
    if (e[0] !== "#") return NAMED[e.toLowerCase()] ?? all;
    const cp = e[1] === "x" || e[1] === "X" ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10);
    const ok = cp > 0 && cp <= 0x10ffff && !(cp >= 0xd800 && cp <= 0xdfff);
    return ok ? String.fromCodePoint(cp) : all;
  });
}

/** Plain-text form: line breaks as "\n", entities decoded, tags left as written. */
export function bbcodePlain(src: string): string {
  return decodeEntities(src.replace(/\n?<br\s*\/?>/gi, "\n"));
}

function argOf(rest: string | undefined): string | null {
  if (!rest?.startsWith("=")) return null;
  return rest.slice(1).trim().replace(/^(["'])(.*)\1$/, "$2");
}

const text = (v: string): Text => ({ t: "text", v });

/** An unclosed element back to literal text: its tag as written, then its content (list items too). */
function unwrap(e: El): Node[] {
  const kids = e.tag === "list" ? e.children.flatMap((c) => (c.t === "el" && c.tag === "*" ? unwrap(c) : [c])) : e.children;
  return [text(e.raw), ...kids];
}

/** The tag tree; never throws. */
export function parseBBCode(src: string): Node[] {
  const root: El = { t: "el", tag: "", arg: null, raw: "", children: [] };
  const stack: El[] = [root];
  const lower = src.toLowerCase();
  const top = () => stack[stack.length - 1];
  const add = (n: Node) => top().children.push(n);
  /** Closes the frames above index k: list items stay elements, anything else unclosed turns literal. */
  const popTo = (k: number) => {
    while (stack.length - 1 > k) {
      const f = stack.pop()!;
      if (f.tag === "*") continue;
      const parent = top();
      parent.children.pop();
      parent.children.push(...unwrap(f));
    }
  };
  const lastIndexOf = (tag: string) => {
    for (let k = stack.length - 1; k > 0; k--) if (stack[k].tag === tag) return k;
    return -1;
  };

  const re = new RegExp(TOKEN.source, TOKEN.flags);
  let at = 0;
  for (let m = re.exec(src); m; m = re.exec(src)) {
    if (m.index > at) add(text(src.slice(at, m.index)));
    at = re.lastIndex;
    const raw = m[0];
    if (!raw.startsWith("[")) {
      add({ t: "br" });
      continue;
    }
    const close = m[1] === "/";
    const tag = m[2].toLowerCase();
    if (!KNOWN.has(tag)) {
      add(text(raw));
      continue;
    }
    if (close) {
      const k = lastIndexOf(tag);
      if (k < 0 || tag === "line") add(text(raw));
      else {
        popTo(k);
        stack.pop();
      }
      continue;
    }
    if (tag === "line") {
      add({ t: "el", tag, arg: null, raw, children: [] });
      continue;
    }
    if (tag === "*") {
      const list = lastIndexOf("list");
      if (list < 0) {
        add(text(raw));
        continue;
      }
      popTo(list);
    }
    if (stack.length > MAX_DEPTH) {
      add(text(raw));
      continue;
    }
    const el: El = { t: "el", tag, arg: argOf(m[3]), raw, children: [] };
    if (VERBATIM.has(tag)) {
      const end = lower.indexOf(`[/${tag}]`, at);
      if (end < 0) {
        add(text(raw));
        continue;
      }
      el.children.push(text(src.slice(at, end)));
      at = end + tag.length + 3;
      re.lastIndex = at;
      add(el);
      continue;
    }
    add(el);
    stack.push(el);
  }
  if (at < src.length) add(text(src.slice(at)));
  popTo(0);
  return root.children;
}

const SIZE: Record<string, string> = {
  "1": "text-caption",
  "2": "text-label",
  "3": "text-body",
  "4": "text-lead",
  "5": "text-lg font-semibold",
  "6": "text-xl font-semibold",
};

/** Named colours to readable theme tones; anything else (hex included) keeps the default colour. */
const COLOR: Record<string, string> = {
  red: "text-rose-300", darkred: "text-rose-300", crimson: "text-rose-300",
  orange: "text-orange-300", darkorange: "text-orange-300",
  yellow: "text-amber-200", gold: "text-amber-200",
  green: "text-emerald-300", lime: "text-emerald-300", limegreen: "text-emerald-300", darkgreen: "text-emerald-300",
  blue: "text-sky-300", lightblue: "text-sky-300", cyan: "text-sky-300", aqua: "text-sky-300", teal: "text-sky-300",
  purple: "text-violet-300", violet: "text-violet-300", magenta: "text-violet-300", pink: "text-violet-300",
  gray: "text-zinc-400", grey: "text-zinc-400", silver: "text-zinc-400",
};

const YOUTUBE_ID = /^[A-Za-z0-9_-]{6,20}$/;

const plainOf = (nodes: Node[]): string =>
  nodes.map((n) => (n.t === "text" ? decodeEntities(n.v) : n.t === "br" ? "\n" : plainOf(n.children))).join("");

const blank = (n: Node) => n.t === "br" || (n.t === "text" && !n.v.trim());

/** Without trailing line breaks / blank text (list items end with the "\n<br />" before the next [*]). */
function trimEnd(nodes: Node[]): Node[] {
  let end = nodes.length;
  while (end > 0 && blank(nodes[end - 1])) end--;
  return nodes.slice(0, end);
}

function render(nodes: Node[]): ReactNode[] {
  return nodes.map((n, i) => <Fragment key={i}>{renderNode(n)}</Fragment>);
}

function renderNode(n: Node): ReactNode {
  if (n.t === "text") return decodeEntities(n.v);
  if (n.t === "br") return <br />;
  const kids = () => render(n.children);
  switch (n.tag) {
    case "b":
      return <strong className="font-semibold text-zinc-100">{kids()}</strong>;
    case "i":
      return <em>{kids()}</em>;
    case "u":
      return <u>{kids()}</u>;
    case "s":
      return <s>{kids()}</s>;
    case "size":
      return <span className={SIZE[n.arg ?? ""]}>{kids()}</span>;
    case "color":
      return <span className={COLOR[(n.arg ?? "").toLowerCase()]}>{kids()}</span>;
    case "font":
      return <>{kids()}</>;
    case "center":
    case "left":
    case "right":
      return <div className={cx(n.tag === "center" ? "text-center" : n.tag === "right" ? "text-right" : "text-left")}>{kids()}</div>;
    case "list": {
      const items = render(n.children.filter((c) => !blank(c)));
      return n.arg ? <ol className="my-2 list-decimal pl-6">{items}</ol> : <ul className="my-2 list-disc pl-6">{items}</ul>;
    }
    case "*":
      return <li>{render(trimEnd(n.children))}</li>;
    case "quote":
      return <blockquote className="my-2 border-l-2 border-hairline pl-3 text-zinc-400">{kids()}</blockquote>;
    case "spoiler":
      return (
        <details className="my-2 rounded-card border border-hairline px-3 py-2">
          <summary className="cursor-pointer text-label text-zinc-400">Spoiler</summary>
          <div className="mt-2">{kids()}</div>
        </details>
      );
    case "code":
      return <pre className="my-2 overflow-x-auto rounded-card bg-ink p-3 font-mono text-caption whitespace-pre-wrap">{plainOf(n.children)}</pre>;
    case "line":
      return <hr className="my-3 border-hairline" />;
    case "url": {
      const href = n.arg ?? plainOf(n.children);
      return <ExternalLink href={href}>{n.arg ? kids() : href}</ExternalLink>;
    }
    case "img":
      return <MediaLink kind="Image" href={plainOf(n.children)} />;
    case "youtube": {
      const id = plainOf(n.children).trim();
      return YOUTUBE_ID.test(id) ? <MediaLink kind="Video" href={`https://www.youtube.com/watch?v=${id}`} label={`YouTube ${id}`} /> : `${n.raw}${id}[/youtube]`;
    }
    default:
      return <>{n.raw}{kids()}</>;
  }
}

/** A BBCode description as elements; oversize input as plain text. */
export function BBCode({ source }: { source: string }) {
  if (source.length > MAX_INPUT) return <div className="whitespace-pre-line">{bbcodePlain(source)}</div>;
  return <div className="wrap-anywhere">{render(parseBBCode(source.replace(/\r\n?/g, "\n")))}</div>;
}
