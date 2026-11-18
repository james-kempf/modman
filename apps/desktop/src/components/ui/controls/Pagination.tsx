// SPDX-License-Identifier: GPL-3.0-or-later
import { CaretDoubleLeftIcon, CaretDoubleRightIcon, CaretLeftIcon, CaretRightIcon } from "@phosphor-icons/react";
import { cx } from "../styles";
import { Button } from "./Button";

/** Which pager button asked for a page. */
export type PageStep = "first" | "prev" | "next" | "last";

/**
 * The app's standard pagination toolbar (Browse, Mods load order): First / Previous, "Page 2 of 7 · 25–48 of 160",
 * Next / Last. Use it instead of ad-hoc paging UI. Pages are 0-based; labels are 1-based.
 *
 * With one page it shows only the count. The buttons are inactive (aria-disabled, still focusable) at either end
 * and busy while `loading`; `onPage` is only called for a different page within range, and not while loading.
 * By default it pins to the bottom of its parent card while the card scrolls under <main>: put it last in the
 * card, with no overflow-hidden/auto ancestor between it and <main>. `className` is appended for placement tweaks
 * (e.g. Browse's edge-to-edge margins inside a padded section).
 */
export function Pagination({
  label,
  page,
  pages,
  size,
  total,
  shown = Math.max(0, Math.min(size, total - page * size)),
  loading = false,
  buttonAttrs,
  className,
  onPage,
}: {
  /** Accessible name of the navigation region, e.g. "Mods pages". */
  label: string;
  page: number;
  pages: number;
  /** Items per page. */
  size: number;
  /** Items in the whole list. */
  total: number;
  /** Items on this page (defaults to what `size` and `total` leave for it). */
  shown?: number;
  /** The target page is loading: "Page n · loading…" and busy buttons. */
  loading?: boolean;
  /** Extra attributes per button (e.g. a focus key to park focus on while a page loads). */
  buttonAttrs?: (step: PageStep) => Record<string, string>;
  className?: string;
  onPage: (page: number, step: PageStep) => void;
}) {
  const paged = pages > 1;
  const first = page * size;
  const go = (to: number, step: PageStep) => {
    if (!loading && to >= 0 && to < pages && to !== page) onPage(to, step);
  };
  const btn = (step: PageStep, inactive: boolean, to: number) => ({
    size: "sm" as const,
    ...buttonAttrs?.(step),
    inactive,
    busy: loading,
    onClick: () => go(to, step),
  });
  return (
    // role=navigation, not <nav>: landmarks inside <main> share its spatial-nav region (nav/spatial.ts).
    // Sticky bottom: WebKit insets sticky by <main>'s padding-bottom, so bottom is minus that padding (--main-pb,
    // set by App) to pin flush to <main>'s bottom edge. z-10: the sticky in-flow toolbar layer.
    // Grid 1fr | label | 1fr: equal side columns centre the label in the full width whatever the button groups'
    // widths; minmax(0,auto) lets a long label wrap instead of pushing into the buttons (which keep min-content).
    <div
      role="navigation"
      aria-label={label}
      className={cx(
        "sticky bottom-[calc(var(--main-pb,0px)*-1)] z-10 grid grid-cols-[1fr_minmax(0,auto)_1fr] items-center gap-4 rounded-b-card border-t border-hairline bg-ink-2 px-section py-3",
        className,
      )}
    >
      {/* inactive, not disabled: focus stays on the button at the first/last page. */}
      {paged && (
        <span className="col-start-1 flex gap-2 justify-self-start">
          <Button {...btn("first", page === 0, 0)} icon={<CaretDoubleLeftIcon aria-hidden size={16} />}>
            First
          </Button>
          <Button {...btn("prev", page === 0, page - 1)} icon={<CaretLeftIcon aria-hidden size={16} />}>
            Previous
          </Button>
        </span>
      )}
      <span className="col-start-2 min-w-0 text-center font-mono text-caption text-dim tabular-nums">
        {loading ? (
          // The target page; the total may belong to the previous query.
          <>Page {page + 1} · loading…</>
        ) : (
          <>
            Page {page + 1} of {pages.toLocaleString()} · {(first + 1).toLocaleString()}–
            {(first + shown).toLocaleString()} of {total.toLocaleString()}
          </>
        )}
      </span>
      {paged && (
        <span className="col-start-3 flex gap-2 justify-self-end">
          <Button {...btn("next", page + 1 >= pages, page + 1)}>
            Next
            <CaretRightIcon aria-hidden size={16} />
          </Button>
          <Button {...btn("last", page + 1 >= pages, pages - 1)}>
            Last
            <CaretDoubleRightIcon aria-hidden size={16} />
          </Button>
        </span>
      )}
    </div>
  );
}
