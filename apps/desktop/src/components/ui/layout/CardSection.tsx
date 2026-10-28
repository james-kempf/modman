// SPDX-License-Identifier: GPL-3.0-or-later
import type { HTMLAttributes } from "react";
import { SECTION_PAD, cx } from "../styles";

/** One padded part of a Card (free content: forms, lists, notes). */
export function CardSection({ children, className, ...rest }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div data-nav-section {...rest} className={cx(SECTION_PAD, className)}>
      {children}
    </div>
  );
}
