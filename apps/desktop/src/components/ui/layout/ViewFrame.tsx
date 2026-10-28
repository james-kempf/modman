// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";
import { VIEW_FRAME } from "../styles";

/**
 * The outer frame of every view: one column of stacked blocks (header, cards).
 * Page padding, max width and scrolling belong to App's <main>, not here.
 */
export function ViewFrame({ children }: { children: ReactNode }) {
  return (
    <div data-view-frame className={VIEW_FRAME}>
      {children}
    </div>
  );
}
