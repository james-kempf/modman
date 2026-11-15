// SPDX-License-Identifier: GPL-3.0-or-later
import type { Icon } from "@phosphor-icons/react";
import type { ButtonHTMLAttributes } from "react";
import { Button } from "./Button";

/**
 * A square glyph-only Button; `label` is its accessible name and tooltip.
 * `busy` locks it (focusable, clicks ignored) but keeps the glyph: sibling
 * buttons share one busy flag, so a spinner would spin on every row.
 * `inactive` is unavailable but focusable (aria-disabled), never native
 * `disabled`, which would drop focus to <body> (e.g. Move up reaching the top).
 * `size` is the glyph size in px: 18 by default, 20 for a primary row action.
 */
export function IconButton({
  label,
  icon: Glyph,
  size = 18,
  busy,
  inactive,
  ...rest
}: Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> & {
  label: string;
  icon: Icon;
  size?: 18 | 20;
  busy?: boolean;
  inactive?: boolean;
}) {
  return (
    <Button tone="ghost" size="icon" aria-label={label} title={label} inactive={busy || inactive} {...rest}>
      <Glyph aria-hidden size={size} />
    </Button>
  );
}
