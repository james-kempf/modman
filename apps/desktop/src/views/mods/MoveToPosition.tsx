// SPDX-License-Identifier: GPL-3.0-or-later
import { useState } from "react";
import { Button, TextField } from "../../components/ui";

/** A modMove spec: the backend takes "first", "last" or "#N" (1-based). */
export type MoveSpec = "first" | "last" | `#${number}`;

/** The inline error for a position, or null when it is a whole number in 1..total. */
export function positionError(raw: string, total: number): string | null {
  const v = raw.trim();
  const n = Number(v);
  return /^\d+$/.test(v) && n >= 1 && n <= total ? null : `Enter a position from 1 to ${total}.`;
}

/**
 * Move one load-order row straight to the top, the bottom, or position N.
 * `position` is the row's current 1-based place among `total` rows. The ends
 * are inactive where they would not move; `busy` makes every button inactive.
 * Nothing is native-disabled, so focus never drops to <body>.
 */
export function MoveToPosition({
  name,
  position,
  total,
  busy,
  onMove,
}: {
  name: string;
  position: number;
  total: number;
  busy?: boolean;
  onMove: (spec: MoveSpec) => void;
}) {
  const [value, setValue] = useState("");
  // Validation shows from the first blur (or submit) on, then follows the typing (as profile names do).
  const [touched, setTouched] = useState(false);
  const empty = !value.trim();
  const error = touched && !empty ? positionError(value, total) : null;
  return (
    <form
      noValidate
      className="flex flex-wrap items-end gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (busy || empty) return;
        if (positionError(value, total)) return setTouched(true);
        onMove(`#${Number(value.trim())}`);
        setValue("");
        setTouched(false);
      }}
    >
      <Button
        size="sm"
        aria-label={`Move ${name} to top`}
        inactive={busy || position <= 1}
        onClick={() => onMove("first")}
      >
        Top
      </Button>
      <Button
        size="sm"
        aria-label={`Move ${name} to bottom`}
        inactive={busy || position >= total}
        onClick={() => onMove("last")}
      >
        Bottom
      </Button>
      <TextField
        label="Position"
        type="number"
        inputMode="numeric"
        min={1}
        max={total}
        step={1}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onBlur={() => setTouched(true)}
        helper={`1–${total}`}
        error={error}
        className="w-28"
      />
      <Button type="submit" size="sm" tone="primary" inactive={busy || empty}>
        Move
      </Button>
    </form>
  );
}
