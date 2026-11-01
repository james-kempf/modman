// SPDX-License-Identifier: GPL-3.0-or-later
import { CheckIcon, WarningIcon, XIcon } from "@phosphor-icons/react";
import { cx } from "../styles";

export type IslandOutcome = "running" | "finished" | "cancelled" | "failed";

/**
 * Legacy dynamic-island status pill: a job's title and one detail line,
 * with animated bars while running. Status only (not focusable).
 */
export function JobIsland({
  title,
  detail,
  outcome,
  queued = 0,
}: {
  title: string;
  detail: string;
  outcome: IslandOutcome;
  /** Other running jobs, shown as "+n". */
  queued?: number;
}) {
  const failed = outcome === "failed";
  return (
    <div
      role="status"
      aria-label="Current job"
      className="glass pointer-events-auto relative flex min-h-14 w-full max-w-xl animate-island items-center gap-3 overflow-hidden rounded-pill py-2 pr-5 pl-2.5"
    >
      <span
        aria-hidden
        className={cx(
          "relative grid size-9 shrink-0 place-items-center rounded-pill",
          failed
            ? "bg-rose-400/15 text-rose-300"
            : outcome === "cancelled"
              ? "bg-white/8 text-zinc-300"
              : "bg-accent-400/15 text-accent-300",
        )}
      >
        {outcome === "running" ? (
          <span className="flex gap-rail">
            <span className="h-3.5 w-rail animate-bar rounded-pill bg-current" />
            <span className="h-3.5 w-rail animate-bar rounded-pill bg-current [animation-delay:150ms]" />
            <span className="h-3.5 w-rail animate-bar rounded-pill bg-current [animation-delay:300ms]" />
          </span>
        ) : failed ? (
          <WarningIcon size={17} weight="bold" />
        ) : outcome === "cancelled" ? (
          <XIcon size={17} weight="bold" />
        ) : (
          <CheckIcon size={17} weight="bold" />
        )}
      </span>
      <span className="relative min-w-0 flex-1">
        <span className="block truncate text-[13.5px] font-medium text-zinc-100">{title}</span>
        <span className="block truncate font-mono text-micro text-dim">{detail}</span>
      </span>
      {queued > 0 && <span className="relative font-mono text-micro text-zinc-400">+{queued}</span>}
    </div>
  );
}
