// SPDX-License-Identifier: GPL-3.0-or-later
// A link from untrusted text (a Nexus description). The webview never
// navigates: the click is cancelled and the backend's open_url opens the URL
// in the browser (it accepts http/https only). Anything that is not an
// http/https URL is not a link at all, only text.

import type { MouseEvent, ReactNode } from "react";
import { useRun } from "../../app/run";
import * as ipc from "../../ipc/commands";

export const LINK_CLASS = "text-accent-300 underline underline-offset-4 wrap-anywhere [--focus-text:var(--color-accent-700)]";

/** The normalized URL when `raw` is an absolute http/https URL with a host, else null. */
export function webUrl(raw: string): string | null {
  try {
    const u = new URL(raw.trim());
    return (u.protocol === "http:" || u.protocol === "https:") && u.hostname ? u.href : null;
  } catch {
    return null;
  }
}

export function ExternalLink({ href, children }: { href: string; children?: ReactNode }) {
  const run = useRun();
  const url = webUrl(href);
  const empty = children === undefined || children === null || children === "" || (Array.isArray(children) && children.length === 0);
  if (!url) return <span>{empty ? href : children}</span>;
  const open = (e: MouseEvent) => {
    e.preventDefault();
    void run("Open link", () => ipc.openUrl(url));
  };
  return (
    <a
      href={url}
      title={url}
      rel="noreferrer noopener"
      className={LINK_CLASS}
      onClick={open}
      // Middle click would open a webview window: cancelled too.
      onAuxClick={(e) => e.preventDefault()}
    >
      {empty ? url : children}
    </a>
  );
}

/** "Image: host" / "Video: host" as a link: pictures and embeds are never loaded from descriptions. */
export function MediaLink({ kind, href, label }: { kind: "Image" | "Video"; href: string; label?: string }) {
  const url = webUrl(href);
  const text = `${kind}: ${label?.trim() || (url ? new URL(url).hostname : href.trim() || "?")}`;
  return url ? <ExternalLink href={url}>{text}</ExternalLink> : <span>{text}</span>;
}
