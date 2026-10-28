// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";
import { MUTED_NOTE, SECTION_PAD, cx } from "../styles";

/** One compact row of a Card: title + one muted help line left, controls right (wrapping below when narrow). */
export function Row({ title, desc, children }: { title: ReactNode; desc?: ReactNode; children?: ReactNode }) {
  return (
    <div data-nav-section className={cx("flex flex-wrap items-center justify-between gap-x-4 gap-y-2", SECTION_PAD)}>
      <div className="min-w-0 flex-[1_1_12rem]">
        <div className="text-body text-zinc-100">{title}</div>
        {desc && <div className={MUTED_NOTE}>{desc}</div>}
      </div>
      {children && <div className="ml-auto flex min-w-0 flex-wrap items-center justify-end gap-2">{children}</div>}
    </div>
  );
}
