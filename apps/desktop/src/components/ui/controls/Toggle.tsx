// SPDX-License-Identifier: GPL-3.0-or-later
import { cx } from "../styles";

export function Toggle({
  checked,
  onChange,
  label,
  disabled,
  busy,
  showLabel = true,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
  /** A change is being applied: ignores clicks but stays focusable (aria-disabled). */
  busy?: boolean;
  /** Show `label` next to the switch (it is always the accessible name). */
  showLabel?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      aria-disabled={busy || undefined}
      aria-busy={busy || undefined}
      onClick={() => !busy && onChange(!checked)}
      className={cx(
        "inline-flex min-h-control min-w-control shrink-0 items-center gap-3 rounded-control text-body text-zinc-200",
        showLabel ? "px-2" : "justify-center",
      )}
    >
      <span
        aria-hidden
        className={cx(
          "inline-flex h-7 w-12 shrink-0 items-center rounded-pill p-1 transition-colors duration-slow",
          checked ? "bg-accent-500/90" : "bg-fill-hover",
        )}
      >
        <span
          className={cx(
            "size-icon rounded-pill transition-[transform,background-color] duration-slow ease-spring",
            checked ? "translate-x-5 bg-white" : "bg-zinc-400",
          )}
        />
      </span>
      {showLabel && label}
    </button>
  );
}
