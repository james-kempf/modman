// SPDX-License-Identifier: GPL-3.0-or-later
import { CaretDownIcon } from "@phosphor-icons/react";
import { Button } from "./Button";
import { cx } from "../styles";

/** Disclosure button for a list that is collapsed by default; `name` reads e.g. "Quarantines list (2)". */
export function ListToggle({
  name,
  count,
  open,
  onToggle,
  controls,
}: {
  name: string;
  count: number;
  open: boolean;
  onToggle: () => void;
  controls: string;
}) {
  return (
    <Button
      size="sm"
      aria-expanded={open}
      // Only while open: the collapsed list is not rendered, so the id would dangle.
      aria-controls={open ? controls : undefined}
      aria-label={`${name} list (${count})`}
      icon={<CaretDownIcon aria-hidden size={14} className={cx("transition-transform duration-base", open && "rotate-180")} />}
      onClick={onToggle}
    >
      List ({count})
    </Button>
  );
}
