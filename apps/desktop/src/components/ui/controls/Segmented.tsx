// SPDX-License-Identifier: GPL-3.0-or-later
import { Button } from "./Button";

// Pill track and pill segments: concentric at any padding (pill - p-1 = pill), like the Tabs pills.
// gap-1 keeps a visible 4px gap between a hovered or selected segment and its neighbour.
const TRACK = "flex max-w-full flex-wrap gap-1 rounded-pill border border-hairline bg-ink p-1";
// Selected: the Overview Play button's green (Button tone="primary" colours: accent-400 fill, accent-950 text, 8.5:1);
// the fill alone is 10.3:1 against the ink track and the unselected (transparent) segments. Not font-semibold like
// Play: a heavier selected label would change the segment's width when the selection moves.
// A 2px outline in the fill colour is invisible at rest; under the white highlight (an inset box-shadow, which an
// outline does not override) it turns accent-700 (5.2:1; accent-400 would be 1.7:1) and the text keeps accent-700
// (--focus-text, as on Play), so "selected + focused" differs from "focused" by more than hue.
// rounded-pill overrides the sm size's rounded-control (Tailwind orders same-property utilities by name).
const SEGMENT =
  "rounded-pill aria-pressed:bg-accent-400 aria-pressed:text-accent-950 aria-pressed:outline-2 aria-pressed:-outline-offset-2 aria-pressed:outline-accent-400 aria-pressed:highlighted:outline-accent-700 aria-pressed:[--focus-text:var(--color-accent-700)]";

/** A pill switcher (Browse and Mods filters); the selected segment is a solid accent thumb (the Play green). */
export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div role="group" aria-label={label} className="flex flex-wrap items-center gap-3">
      <span className="eyebrow">{label}</span>
      <span className={TRACK}>
        {options.map((o) => (
          <Button
            key={o.value}
            tone="ghost"
            size="sm"
            className={SEGMENT}
            aria-pressed={o.value === value}
            onClick={() => onChange(o.value)}
          >
            {o.label}
          </Button>
        ))}
      </span>
    </div>
  );
}
