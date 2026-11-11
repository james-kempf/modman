// SPDX-License-Identifier: GPL-3.0-or-later
import { CircleNotchIcon } from "@phosphor-icons/react";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cx } from "../styles";

type Tone = "primary" | "default" | "danger" | "ghost";
const TONES: Record<Tone, string> = {
  // Hover and nav focus are the shared white highlight (index.css); --focus-text keeps the tone in the text (>= 4.5:1).
  primary: "bg-accent-400 text-accent-950 font-semibold [--focus-text:var(--color-accent-700)]",
  default: "bg-fill-2 text-zinc-100 border border-hairline",
  // Tonal: a muted rose wash with rose text (no saturated fill, no coloured border); transparent border keeps the size of default.
  danger: "bg-rose-500/15 text-rose-200 border border-transparent [--focus-text:var(--color-rose-700)]",
  // A toggle (aria-pressed, e.g. a segment) takes the selected tab's look while pressed (accent-200 on an
  // accent-400/15 tint, as Tabs) plus a 1px inset accent-600 edge: 4.8:1 against the ink track (3.7:1 on the
  // white highlight), so the pressed segment is told apart by more than its hue. Under the highlight it keeps
  // accent text (--focus-text) and the edge, apart from a focused unpressed segment.
  ghost:
    "text-zinc-300 aria-pressed:bg-accent-400/15 aria-pressed:text-accent-200 aria-pressed:outline aria-pressed:-outline-offset-1 aria-pressed:outline-accent-600 aria-pressed:[--focus-text:var(--color-accent-700)]",
};
type Size = "icon" | "sm" | "md" | "lg";
const SIZES: Record<Size, string> = {
  // Square, glyph only (IconButton).
  icon: "size-control rounded-control",
  sm: "min-h-control-sm px-3.5 text-label gap-1.5 rounded-control",
  md: "min-h-control px-4 text-body gap-2 rounded-control",
  lg: "min-h-control-lg px-7 text-lead gap-2.5 rounded-card",
};

export function Button({
  tone = "default",
  size = "md",
  icon,
  busy,
  inactive,
  className,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  tone?: Tone;
  size?: Size;
  icon?: ReactNode;
  busy?: boolean;
  /**
   * Unavailable but still focusable (aria-disabled): for a button that focus
   * returns to, where native `disabled` would drop focus to <body>.
   */
  inactive?: boolean;
}) {
  const off = busy || inactive;
  return (
    <button
      type="button"
      {...rest}
      // Busy stays focusable: native `disabled` would drop focus to <body>.
      // preventDefault also cancels a submit button's form submission.
      aria-disabled={off || undefined}
      aria-busy={busy || undefined}
      onClick={off ? (e) => e.preventDefault() : rest.onClick}
      className={cx(
        "inline-flex min-w-control shrink-0 items-center justify-center font-medium whitespace-nowrap transition-[background-color,color,transform,opacity] duration-base active:translate-y-px active:scale-98",
        TONES[tone],
        SIZES[size],
        className,
      )}
    >
      {busy ? <CircleNotchIcon aria-hidden size={16} weight="bold" className="animate-spin" /> : icon}
      {children}
    </button>
  );
}
