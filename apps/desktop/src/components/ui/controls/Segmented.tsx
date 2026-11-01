// SPDX-License-Identifier: GPL-3.0-or-later
import { Button } from "./Button";

/** Legacy pill switcher (Mods filter / Activity tabs). */
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
      {/* rounded-card: one step above the segments' rounded-control, so the corners nest. */}
      <span className="flex max-w-full flex-wrap rounded-card border border-hairline bg-ink p-1">
        {options.map((o) => (
          <Button
            key={o.value}
            tone="ghost"
            size="sm"
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
