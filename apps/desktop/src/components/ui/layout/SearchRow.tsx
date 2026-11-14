// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";

/**
 * A row with its search bar centered: at xl, three columns with equal sides (so the middle one is the row's
 * center), `start` left and `end` right of the search. Below xl the cells stack in one column, search centered,
 * so nothing collides at narrow widths. The end cell stretches over its column: its content aligns itself.
 */
export function SearchRow({ start, search, end }: { start?: ReactNode; search: ReactNode; end?: ReactNode }) {
  return (
    <div className="grid grid-cols-[minmax(0,1fr)] items-end gap-3 xl:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)]">
      {start && <div className="min-w-0 xl:col-start-1">{start}</div>}
      <div className="min-w-0 max-w-full justify-self-center xl:col-start-2">{search}</div>
      {end && <div className="min-w-0 xl:col-start-3">{end}</div>}
    </div>
  );
}
