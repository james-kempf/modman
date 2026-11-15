// SPDX-License-Identifier: GPL-3.0-or-later
import { CircleNotchIcon } from "@phosphor-icons/react";
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
  /** A change is being applied: a spinner in the knob; ignores clicks but stays focusable (aria-disabled). */
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
        // px-2.5 matches the 10px above/below the h-7 track in min-h-control, so the nav highlight pads it evenly;
        // the negative margin keeps the track where it sat before (px-2 with a label, flush without one).
        "inline-flex min-h-control min-w-control shrink-0 items-center justify-center gap-3 rounded-pill px-2.5 text-body text-zinc-200",
        showLabel ? "-mx-0.5" : "-mx-2.5",
      )}
    >
      <span
        aria-hidden
        className={cx(
          "inline-flex h-7 w-12 shrink-0 items-center rounded-pill p-1 transition-colors duration-slow",
          // On the white nav focus fill the at-rest track fades out (the off track is 10% white): >= 3:1 tracks there.
          checked ? "bg-accent-500/90 in-highlight:bg-accent-700" : "bg-fill-hover in-highlight:bg-zinc-500",
        )}
      >
        <span
          className={cx(
            "inline-flex size-icon items-center justify-center rounded-pill transition-[transform,translate,background-color] duration-slow ease-spring",
            checked ? "translate-x-5 bg-white" : "bg-zinc-400 in-highlight:bg-white",
          )}
        >
          {/* Spins only when motion is allowed; under reduced motion the static ring still marks it busy. */}
          {busy && (
            <CircleNotchIcon
              aria-hidden
              data-testid="toggle-spinner"
              size={14}
              weight="bold"
              className="text-zinc-800 motion-safe:animate-spin"
            />
          )}
        </span>
      </span>
      {showLabel && label}
    </button>
  );
}
