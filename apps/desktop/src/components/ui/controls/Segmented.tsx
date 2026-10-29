// SPDX-License-Identifier: GPL-3.0-or-later
import { cx } from "../styles";

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
      <span className="flex max-w-full flex-wrap rounded-control border border-hairline bg-ink p-1">
        {options.map((o) => (
          <button
            key={o.value}
            type="button"
            aria-pressed={o.value === value}
            onClick={() => onChange(o.value)}
            className={cx(
              "min-h-control-sm rounded-control-sm px-4 text-label transition-colors duration-base",
              o.value === value ? "bg-fill-3 text-zinc-50" : "text-zinc-400",
            )}
          >
            {o.label}
          </button>
        ))}
      </span>
    </div>
  );
}
