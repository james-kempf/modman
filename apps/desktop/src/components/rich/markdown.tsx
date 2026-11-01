// SPDX-License-Identifier: GPL-3.0-or-later
// Nexus collection descriptions (GFM Markdown) as React elements through
// react-markdown, never raw HTML: a remark step turns raw HTML into literal
// text (only harmless wrapper tags such as <details>/<summary> are dropped,
// keeping their text) and flattens anything nested deeper than MAX_DEPTH to
// text. Only the allow-listed elements below render, each by a component of
// ours that passes no attributes through (no style, no event handlers).
// Images become links; links go through ExternalLink (backend open_url).

import { createContext, useContext, type ReactNode } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { ExternalLink, MediaLink } from "./ExternalLink";

/** Larger input is shown as plain text (no parsing). */
export const MAX_INPUT = 100_000;
/** Deeper nodes are flattened to their text. */
export const MAX_DEPTH = 12;

interface MdNode {
  type: string;
  value?: string;
  alt?: string | null;
  children?: MdNode[];
}

// Structural tags whose text is worth keeping; the tags themselves are dropped.
const WRAPPER_TAG = /<\/?(?:details|summary|p|div|span|center|br|hr)\b[^<>]*>/gi;

const textOf = (n: MdNode): string => n.value ?? n.alt ?? (n.children ?? []).map(textOf).join("");

/** Remark step: raw HTML -> literal text, over-deep subtrees -> their text. */
function remarkSafe() {
  const walk = (n: MdNode, depth: number) => {
    if (!n.children) return;
    n.children = n.children.flatMap((c): MdNode[] => {
      if (c.type === "html") {
        const rest = (c.value ?? "").replace(WRAPPER_TAG, "");
        return rest.trim() ? [{ type: "text", value: rest }] : [];
      }
      if (depth >= MAX_DEPTH && c.children) return [{ type: "text", value: textOf(c) }];
      walk(c, depth + 1);
      return [c];
    });
  };
  return (tree: MdNode) => walk(tree, 0);
}

/** Inside a link: an image is a label there (no link in a link). */
const InLink = createContext(false);

type Kids = { children?: ReactNode };
const el =
  (Tag: "p" | "strong" | "em" | "del" | "ul" | "ol" | "li" | "blockquote" | "pre" | "code" | "table" | "thead" | "tbody" | "tr" | "th" | "td" | "h3" | "h4" | "h5", className?: string) =>
  ({ children }: Kids) => <Tag className={className}>{children}</Tag>;

const components: Components = {
  p: el("p", "my-2"),
  strong: el("strong", "font-semibold text-zinc-100"),
  em: el("em"),
  del: el("del"),
  ul: el("ul", "my-2 list-disc pl-6"),
  ol: el("ol", "my-2 list-decimal pl-6"),
  li: el("li"),
  blockquote: el("blockquote", "my-2 border-l-2 border-hairline pl-3 text-zinc-400"),
  pre: el("pre", "my-2 overflow-x-auto rounded-card bg-ink p-3 font-mono text-caption whitespace-pre-wrap"),
  code: el("code", "font-mono text-caption"),
  table: el("table", "my-2 border-collapse text-label"),
  thead: el("thead"),
  tbody: el("tbody"),
  tr: el("tr"),
  th: el("th", "border border-hairline px-2 py-1 text-left font-semibold"),
  td: el("td", "border border-hairline px-2 py-1"),
  h1: el("h3", "mt-4 mb-2 text-xl font-semibold text-zinc-100"),
  h2: el("h3", "mt-4 mb-2 text-lg font-semibold text-zinc-100"),
  h3: el("h4", "mt-3 mb-2 text-title font-semibold text-zinc-100"),
  h4: el("h5", "mt-3 mb-1 text-lead font-semibold text-zinc-100"),
  h5: el("h5", "mt-2 mb-1 text-body font-semibold text-zinc-100"),
  h6: el("h5", "mt-2 mb-1 text-body font-semibold text-zinc-300"),
  hr: () => <hr className="my-3 border-hairline" />,
  br: () => <br />,
  a: ({ href, children }) => (
    <InLink.Provider value={true}>
      <ExternalLink href={href ?? ""}>{children}</ExternalLink>
    </InLink.Provider>
  ),
  img: function Img({ src, alt }) {
    const inLink = useContext(InLink);
    const href = typeof src === "string" ? src : "";
    return inLink ? <span>{`Image${alt ? `: ${alt}` : ""}`}</span> : <MediaLink kind="Image" href={href} label={alt} />;
  },
};

const ALLOWED = Object.keys(components);

/** A Markdown description as elements; oversize input as plain text. */
export function MarkdownText({ source }: { source: string }) {
  if (source.length > MAX_INPUT) return <div className="whitespace-pre-line">{source}</div>;
  return (
    <div className="wrap-anywhere">
      <Markdown remarkPlugins={[remarkGfm, remarkSafe]} components={components} allowedElements={ALLOWED} unwrapDisallowed skipHtml={false}>
        {source}
      </Markdown>
    </div>
  );
}
