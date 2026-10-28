// SPDX-License-Identifier: GPL-3.0-or-later
import type { HTMLAttributes, ReactNode } from "react";
import { CARD_SHELL, CARD_TITLE, DIVIDE, INSET_HEADER_PAD, MUTED_NOTE, cx } from "../styles";

/**
 * The one card: an optional header (title, intro, actions) above a surface
 * whose children are stacked sections split by dividers. Children are
 * `CardSection`s (padded free content), `Row`s, or lists with their own padding.
 * `inset` puts the header inside the surface as its first section (for cards
 * side by side with an untitled one, so their tops line up).
 */
export function Card({
  title,
  intro,
  actions,
  inset = false,
  children,
  className,
  ...rest
}: Omit<HTMLAttributes<HTMLElement>, "title"> & {
  title?: ReactNode;
  intro?: ReactNode;
  actions?: ReactNode;
  inset?: boolean;
  children: ReactNode;
}) {
  const header = title || intro || actions ? (
    // inset: no bottom padding, the first section's top padding is the gap (no divider line).
    <div className={cx("flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1", inset && INSET_HEADER_PAD)}>
      <div className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-0.5">
        {/* inset titles keep the pre-migration mono eyebrow look. */}
        {title && <h2 className={inset ? "eyebrow" : CARD_TITLE}>{title}</h2>}
        {intro && <div className={MUTED_NOTE}>{intro}</div>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </div>
  ) : null;
  return (
    <section {...rest} className={cx("flex min-w-0 flex-col", className)}>
      {!inset && header}
      {/* flex-1: cards side by side in a grid row share one height. */}
      {inset ? (
        // The header sits outside the divided part, so no line between it and the first section.
        <div className={cx("flex flex-1 flex-col", CARD_SHELL)}>
          {header}
          <div className={cx("flex flex-1 flex-col", DIVIDE)}>{children}</div>
        </div>
      ) : (
        <div
          className={cx(
            "flex flex-1 flex-col",
            DIVIDE,
            CARD_SHELL,
            header && "mt-2.5",
          )}
        >
          {children}
        </div>
      )}
    </section>
  );
}
