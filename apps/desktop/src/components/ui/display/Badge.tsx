// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";
import { cx } from "../styles";

type BadgeTone = "zinc" | "accent" | "amber" | "rose";
const BADGE: Record<BadgeTone, string> = {
  zinc: "bg-fill-2 text-zinc-300 border-hairline",
  accent: "bg-accent-400/10 text-accent-300 border-accent-400/15",
  amber: "bg-amber-400/10 text-amber-300 border-amber-400/15",
  rose: "bg-rose-400/10 text-rose-300 border-rose-400/15",
};
/** Legacy Chip: small mono label. */
export function Badge({ tone = "zinc", children }: { tone?: BadgeTone; children: ReactNode }) {
  return (
    <span
      className={cx(
        "inline-flex h-badge items-center gap-1 rounded-control-sm border px-2 font-mono text-micro whitespace-nowrap",
        BADGE[tone],
      )}
    >
      {children}
    </span>
  );
}
