// SPDX-License-Identifier: GPL-3.0-or-later
import type { ButtonHTMLAttributes } from "react";
import { Button } from "./Button";

/**
 * A square glyph-only Button; `label` is its accessible name and tooltip.
 * `busy` locks it (focusable, clicks ignored) but keeps the glyph: sibling
 * buttons share one busy flag, so a spinner would spin on every row.
 */
export function IconButton({
  label,
  busy,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; busy?: boolean }) {
  return (
    <Button tone="ghost" size="icon" aria-label={label} title={label} inactive={busy} {...rest}>
      {children}
    </Button>
  );
}
