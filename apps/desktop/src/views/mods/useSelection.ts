// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback, useEffect, useMemo, useState } from "react";

export interface Selection<Id> {
  /** Selected ids that are still listed. */
  selected: ReadonlySet<Id>;
  /** Selected ids in list order (the order bulk calls should use). */
  list: Id[];
  count: number;
  has: (id: Id) => boolean;
  /** Select or unselect one listed id; an id that is not listed is ignored. */
  toggle: (id: Id) => void;
  clear: () => void;
}

/**
 * A multi-selection over `ids` (the rows currently listed). Ids that leave the
 * list are pruned on that same render and forgotten, so one that comes back
 * later (e.g. restored by Undo) is not silently reselected.
 */
export function useSelection<Id>(ids: readonly Id[]): Selection<Id> {
  const [raw, setRaw] = useState<ReadonlySet<Id>>(() => new Set());
  const live = useMemo(() => new Set(ids), [ids]);
  // Same object when nothing was pruned, so the effect below settles after one pass.
  const selected = useMemo(() => {
    for (const id of raw) if (!live.has(id)) return new Set([...raw].filter((x) => live.has(x)));
    return raw;
  }, [raw, live]);
  useEffect(() => {
    if (selected !== raw) setRaw(selected);
  }, [selected, raw]);
  const list = useMemo(() => ids.filter((id) => selected.has(id)), [ids, selected]);
  const toggle = useCallback(
    (id: Id) => {
      if (!live.has(id)) return;
      setRaw((s) => {
        const next = new Set(s);
        if (!next.delete(id)) next.add(id);
        return next;
      });
    },
    [live],
  );
  const clear = useCallback(() => setRaw(new Set()), []);
  const has = useCallback((id: Id) => selected.has(id), [selected]);
  return { selected, list, count: selected.size, has, toggle, clear };
}
