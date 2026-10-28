// SPDX-License-Identifier: GPL-3.0-or-later
import { FlaskIcon } from "@phosphor-icons/react";
import type { Support } from "../../../ipc/types";
import { Badge } from "./Badge";

/** "Experimental" label for games outside the tested set (tech stack §9.1). */
export function SupportBadge({ support }: { support: Support }) {
  if (support !== "experimental") return null;
  return (
    <span title="Not tested end to end; may need manual fixes" data-testid="experimental-badge" className="inline-flex">
      <Badge tone="amber">
        <FlaskIcon aria-hidden size={12} weight="bold" />
        Experimental
      </Badge>
    </span>
  );
}
