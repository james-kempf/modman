// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";
import { SearchRow } from "./SearchRow";

/**
 * A view's header: the view's search bar centered and actions at the end. No text of its own (no title,
 * eyebrow or intro): the view is named by <main>'s aria-label (App). Renders nothing when empty.
 */
export function ViewHeader({ search, children }: { search?: ReactNode; children?: ReactNode }) {
  const actions = children && <div className="flex max-w-full min-w-0 flex-wrap items-center gap-2">{children}</div>;
  if (search)
    return (
      <header className="pb-2">
        <SearchRow search={search} end={actions} />
      </header>
    );
  if (!actions) return null;
  return <header className="flex flex-wrap items-center justify-end gap-2 pb-2">{actions}</header>;
}
