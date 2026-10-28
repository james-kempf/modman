// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";
import { TILE, cx } from "../styles";

/**
 * Empty state, centred. With a title: a muted icon tile, the title, text and
 * actions stacked (no surface: it sits in a view or a card section as is).
 * Without: one quiet line in a sunken well (TILE), which reads the same in a
 * card, a dialog or directly in a view.
 */
export function Empty({
  icon,
  title,
  children,
  action,
}: {
  icon?: ReactNode;
  title?: string;
  children?: ReactNode;
  action?: ReactNode;
}) {
  if (!title) return <p className={cx(TILE, "px-5 py-6 text-center text-body text-zinc-400")}>{children}</p>;
  return (
    <div className="grid justify-items-center gap-5 py-10 text-center">
      <div aria-hidden className="grid size-tile place-items-center rounded-card border border-hairline bg-ink-2 text-dim">
        {icon}
      </div>
      <div className="max-w-prose-sm">
        <h2 className="text-xl font-medium tracking-tight text-zinc-100">{title}</h2>
        {children && <div className="mt-2 text-lead leading-relaxed text-zinc-400">{children}</div>}
        {action && <div className="mt-6 flex flex-wrap justify-center gap-3">{action}</div>}
      </div>
    </div>
  );
}
