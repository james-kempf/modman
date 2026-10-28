// SPDX-License-Identifier: GPL-3.0-or-later
import { memo } from "react";
import { cx } from "../styles";

export type DotTone = "accent" | "amber" | "rose" | "zinc";
const DOT: Record<DotTone, string> = {
  accent: "bg-accent-400",
  amber: "bg-amber-400",
  rose: "bg-rose-400",
  zinc: "bg-zinc-500",
};
/** A status dot that breathes while something is live. */
export const StatusDot = memo(function StatusDot({ tone = "accent", live = false }: { tone?: DotTone; live?: boolean }) {
  return (
    <span aria-hidden className="relative inline-flex size-dot shrink-0">
      {live && <span className={cx("absolute inset-0 animate-breathe rounded-pill", DOT[tone])} />}
      <span className={cx("relative inline-flex size-dot rounded-pill", DOT[tone])} />
    </span>
  );
});
