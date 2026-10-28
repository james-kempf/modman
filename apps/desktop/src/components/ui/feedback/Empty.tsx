// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";

/**
 * Empty state. With a title it is the legacy EmptyState (icon tile, title,
 * text, actions); without, a quiet centred line inside a panel.
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
  if (!title)
    return <p className="rounded-card border border-dashed border-line px-5 py-6 text-body text-zinc-400">{children}</p>;
  return (
    <div className="grid gap-5 py-10 md:grid-cols-[auto_1fr] md:items-start md:gap-8">
      <div aria-hidden className="grid size-tile place-items-center rounded-card border border-hairline bg-ink-2 text-dim">
        {icon}
      </div>
      <div className="max-w-prose-sm">
        <h2 className="text-xl font-medium tracking-tight text-zinc-100">{title}</h2>
        {children && <div className="mt-2 text-lead leading-relaxed text-zinc-400">{children}</div>}
        {action && <div className="mt-6 flex flex-wrap gap-3">{action}</div>}
      </div>
    </div>
  );
}
