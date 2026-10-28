// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";

/** Legacy ErrorNote (optional actions, e.g. "Try again"). */
export function ErrorNote({ children, actions }: { children: ReactNode; actions?: ReactNode }) {
  return (
    <div
      role="status"
      className="flex flex-wrap items-center gap-3 rounded-card border border-rose-400/15 bg-rose-500/5 px-4 py-3 text-body text-rose-200"
    >
      <div className="min-w-0 flex-1 wrap-anywhere">{children}</div>
      {actions && <div className="flex flex-wrap gap-2">{actions}</div>}
    </div>
  );
}
