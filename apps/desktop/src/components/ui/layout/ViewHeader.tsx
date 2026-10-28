// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";

/** Legacy PageHeader: eyebrow, large title, intro text, actions on the right. */
export function ViewHeader({
  title,
  eyebrow,
  description,
  children,
}: {
  title: string;
  eyebrow?: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
}) {
  return (
    // Wraps instead of squeezing: the title keeps ~24rem, and the actions go below it when both do not fit.
    <header className="flex flex-wrap items-end justify-between gap-x-8 gap-y-5 pb-2">
      <div className="min-w-0 flex-[1_1_24rem]">
        {eyebrow && <div className="eyebrow mb-3">{eyebrow}</div>}
        <h1 className="text-3xl font-medium tracking-tighter text-zinc-50 md:text-display md:leading-none">{title}</h1>
        {description && <div className="mt-3 max-w-prose text-lead leading-relaxed text-zinc-400">{description}</div>}
      </div>
      {children && <div className="flex max-w-full min-w-0 flex-wrap items-center gap-2">{children}</div>}
    </header>
  );
}
