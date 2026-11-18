// SPDX-License-Identifier: GPL-3.0-or-later
import { CaretLeftIcon, CaretRightIcon } from "@phosphor-icons/react";
import { Button } from "../../components/ui";
import { pageList, pageRange } from "./paginate";

// The current page wears the Segmented thumb (accent-400 fill, 10.3:1 against the card; an outline rim that turns
// accent-700 under the white highlight) plus semibold: fill and weight mark it, not hue alone. tabular-nums keeps
// the numbers from shifting the row as they change.
const PAGE =
  "tabular-nums aria-[current=page]:bg-accent-400 aria-[current=page]:font-semibold aria-[current=page]:text-accent-950 aria-[current=page]:outline-2 aria-[current=page]:-outline-offset-2 aria-[current=page]:outline-accent-400 aria-[current=page]:highlighted:outline-accent-700 aria-[current=page]:[--focus-text:var(--color-accent-700)]";

/**
 * Previous / page numbers / Next with the "51–100 of 312" range, for a list
 * paged on the client. Renders nothing with one page. Previous and Next are
 * inactive (still focusable) at the ends.
 */
export function Pager({
  label,
  page,
  pages,
  size,
  total,
  onPage,
}: {
  label: string;
  page: number;
  pages: number;
  size: number;
  total: number;
  onPage: (page: number) => void;
}) {
  if (pages <= 1) return null;
  const { first, last } = pageRange(page, size, total);
  const go = (to: number) => to >= 0 && to < pages && to !== page && onPage(to);
  return (
    <nav aria-label={label} className="flex flex-wrap items-center gap-4 rounded-b-card bg-ink-2 px-section py-3">
      <span className="font-mono text-caption text-dim tabular-nums">
        {first.toLocaleString()}–{last.toLocaleString()} of {total.toLocaleString()}
      </span>
      <span className="ml-auto flex flex-wrap items-center gap-2">
        <Button
          size="sm"
          inactive={page === 0}
          icon={<CaretLeftIcon aria-hidden size={16} />}
          onClick={() => go(page - 1)}
        >
          Previous
        </Button>
        {pageList(page, pages).map((p, i) =>
          p === "gap" ? (
            <span key={`gap${i}`} aria-hidden className="w-6 text-center font-mono text-caption text-dim">
              …
            </span>
          ) : (
            // Keyed by page: the pressed number stays mounted (and focused) as the window moves.
            <Button
              key={p}
              size="sm"
              tone="ghost"
              className={PAGE}
              aria-label={`Page ${p + 1}`}
              aria-current={p === page ? "page" : undefined}
              onClick={() => go(p)}
            >
              {(p + 1).toLocaleString()}
            </Button>
          ),
        )}
        <Button size="sm" inactive={page + 1 >= pages} onClick={() => go(page + 1)}>
          Next
          <CaretRightIcon aria-hidden size={16} />
        </Button>
      </span>
    </nav>
  );
}
