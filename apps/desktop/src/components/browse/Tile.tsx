// SPDX-License-Identifier: GPL-3.0-or-later
// The browse result card and its grid, shared by Browse's listing and a
// collection's Mods tab in its details dialog.

import { CheckIcon } from "@phosphor-icons/react";
import { useId, useRef, useState, type ReactNode, type Ref } from "react";
import { Badge, CARD_TILE, cx } from "../ui";

/** Installed status badge (accent). */
export function InstalledBadge() {
  return (
    <Badge tone="accent">
      <CheckIcon aria-hidden size={12} weight="bold" />
      Installed
    </Badge>
  );
}

/** Shown on a card while its install/remove runs (the dialog may be closed by then). */
export function WorkingBadge() {
  return <Badge>Working…</Badge>;
}

/** Open/close state of a card's details dialog, and the card it returns focus to. */
export function useDetails() {
  const [open, setOpen] = useState(false);
  const card = useRef<HTMLButtonElement>(null);
  return { open, card, show: () => setOpen(true), hide: () => setOpen(false) };
}

/** 12345 -> "12.3K" (legacy `compact`). */
export function compact(n: number | null): string {
  if (n === null) return "0";
  return Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 }).format(n);
}

/** Marks a Browse control whose focus is remembered for the session (value: a stable key). */
export const FOCUS_ATTR = "data-browse-focus";

/** Results grid: 1 column, 2 from sm; the <ul> is the container the collection picture width is sized against. */
export const RESULTS_GRID = "@container grid gap-4 sm:grid-cols-2";
/** Mods: 3 columns from xl. */
export const MOD_GRID = cx(RESULTS_GRID, "xl:grid-cols-3");
/** Mod pictures fill the tile's width at 16:9 (Nexus tile art); the monogram fallback takes the same box. */
export const MOD_PICTURE = "aspect-video w-full shrink-0 rounded-card";

/**
 * One browse result: a single focusable card (picture, name and byline,
 * summary, stats, status badges) styled and highlighted like the Games tiles.
 * Activating it opens the item's details dialog, which holds its actions;
 * without `onOpen` it is a dimmed, non-interactive card.
 */
export function Tile({
  picture,
  name,
  byline,
  summary,
  stats,
  badges,
  focusKey,
  onOpen,
  cardRef,
  dialog,
  horizontal = false,
}: {
  horizontal?: boolean;
  picture: ReactNode;
  name: string;
  byline: string;
  summary: string | null;
  stats: ReactNode;
  /** Non-interactive status badges (Installed, ...). */
  badges: ReactNode;
  /** The card's session focus key ([data-browse-focus]). */
  focusKey?: string;
  /** Activation (gets the card, where focus returns from its dialog); absent: a dimmed, non-interactive card. */
  onOpen?: (card: HTMLButtonElement) => void;
  cardRef?: Ref<HTMLButtonElement>;
  /** The open details dialog (portalled), if any. */
  dialog?: ReactNode;
}) {
  const statusId = useId();
  const text = (
    <>
      <span className="block shrink-0 truncate text-[15.5px] font-medium text-zinc-100" title={name}>
        {name}
      </span>
      <span className="block shrink-0 truncate font-mono text-[11.5px] text-dim">{byline}</span>
      {summary && (
        <span
          data-testid="tile-summary"
          className={cx(
            "mt-2 block min-h-0 overflow-hidden text-ellipsis text-label leading-relaxed text-zinc-400",
            horizontal ? "line-clamp-4 shrink" : "line-clamp-2",
          )}
        >
          {summary}
        </span>
      )}
      <span className="mt-auto flex min-w-0 shrink-0 flex-wrap items-center gap-x-4 gap-y-2 pt-3">
        {stats}
        <span id={statusId} className={cx("flex flex-wrap items-center gap-1.5 empty:hidden", !horizontal && "ml-auto")}>
          {badges}
        </span>
      </span>
    </>
  );
  const shell = cx(CARD_TILE, "border-hairline bg-ink", horizontal ? "flex-row" : "flex-col pb-3.5");
  const body = (
    <>
      {picture}
      {horizontal ? (
        // Collections: the picture alone sets the card's height. The text sits absolutely in the column beside
        // it, so a long summary never makes the card taller; it shrinks (clipped, ellipsis) between the fixed
        // title/byline and the stats/badges row.
        <span className="relative min-w-0 flex-1 self-stretch">
          <span className="absolute inset-0 flex flex-col overflow-hidden px-1.5 py-1">{text}</span>
        </span>
      ) : (
        <span className="flex min-w-0 flex-1 flex-col self-stretch px-1.5">{text}</span>
      )}
    </>
  );
  return (
    <li className="min-w-0">
      {onOpen ? (
        <button
          ref={cardRef}
          type="button"
          aria-label={name}
          aria-describedby={statusId}
          aria-haspopup="dialog"
          {...(focusKey && { [FOCUS_ATTR]: focusKey })}
          onClick={(e) => onOpen(e.currentTarget)}
          className={shell}
        >
          {body}
        </button>
      ) : (
        <div aria-disabled="true" aria-describedby={statusId} className={cx(shell, "opacity-60")}>
          {body}
        </div>
      )}
      {dialog}
    </li>
  );
}

export function Stat({ icon, value, label }: { icon: ReactNode; value: string; label: string }) {
  return (
    <span className="flex min-w-0 items-center justify-start gap-1.5 font-mono text-[11.5px] text-dim">
      <span className="shrink-0">{icon}</span>
      <span className="truncate">{value}</span>
      <span className="sr-only">{label}</span>
    </span>
  );
}

