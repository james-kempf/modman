// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";

/** Legacy fact: eyebrow key, mono value. */
export function Fact({ k, v }: { k: string; v: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="eyebrow">{k}</dt>
      <dd className="mt-1 font-mono text-label break-words text-zinc-300">{v}</dd>
    </div>
  );
}
