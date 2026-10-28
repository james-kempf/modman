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
  const heading = (
    <>
      {eyebrow && <div className="eyebrow mb-3">{eyebrow}</div>}
      <h1 className="text-3xl font-medium tracking-tighter text-zinc-50 md:text-display md:leading-none">{title}</h1>
    </>
  );
  const actions = children && <div className="flex max-w-full min-w-0 flex-wrap items-center gap-2">{children}</div>;
  // Wraps instead of squeezing: the text column keeps ~24rem, and the actions go below it when both do not fit.
  if (!description)
    return (
      // No description: the actions sit beside the title block, bottom-aligned.
      <header className="flex flex-wrap items-end justify-between gap-x-8 gap-y-5 pb-2">
        <div className="min-w-0 flex-[1_1_24rem]">{heading}</div>
        {actions}
      </header>
    );
  return (
    // Title block full width; below it the description (no measure cap: it fills what the actions leave)
    // shares a row with the actions, both top-aligned to the first line's box.
    <header className="pb-2">
      <div className="min-w-0">{heading}</div>
      <div className="mt-3 flex flex-wrap items-start justify-between gap-x-8 gap-y-5">
        <div className="min-w-0 flex-[1_1_24rem] text-lead leading-relaxed text-zinc-400">{description}</div>
        {actions}
      </div>
    </header>
  );
}
