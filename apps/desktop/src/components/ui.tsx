// SPDX-License-Identifier: GPL-3.0-or-later
// UI primitives in the legacy web UI's visual language (soft surfaces, chips,
// Phosphor icons). Large (>= 44px) targets, no hover-only affordances, no
// inline styles (CSP style-src 'self').

import { CircleNotchIcon, FlaskIcon } from "@phosphor-icons/react";
import { useQuery } from "@tanstack/react-query";
import { memo, useId, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from "react";
import { thumbnail } from "../ipc/commands";
import type { Msg, Style, Support } from "../ipc/types";

export const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(" ");

type Tone = "primary" | "default" | "danger" | "ghost";
const TONES: Record<Tone, string> = {
  primary: "bg-accent-400 text-accent-950 font-semibold hover:bg-accent-300",
  default: "bg-white/[0.06] text-zinc-100 border border-white/[0.06] hover:bg-white/[0.1]",
  danger: "bg-rose-500/10 text-rose-200 border border-rose-400/20 hover:bg-rose-500/20",
  ghost: "text-zinc-300 hover:text-zinc-100 hover:bg-white/[0.05]",
};
type Size = "sm" | "md" | "lg";
const SIZES: Record<Size, string> = {
  sm: "min-h-11 px-3.5 text-[13.5px] gap-1.5 rounded-xl",
  md: "min-h-12 px-4 text-sm gap-2 rounded-xl",
  lg: "min-h-14 px-7 text-[15px] gap-2.5 rounded-2xl",
};

export function Button({
  tone = "default",
  size = "md",
  icon,
  busy,
  className,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { tone?: Tone; size?: Size; icon?: ReactNode; busy?: boolean }) {
  return (
    <button
      type="button"
      {...rest}
      // Busy stays focusable: native `disabled` would drop focus to <body>.
      // preventDefault also cancels a submit button's form submission.
      aria-disabled={busy || undefined}
      aria-busy={busy || undefined}
      onClick={busy ? (e) => e.preventDefault() : rest.onClick}
      className={cx(
        "inline-flex min-w-12 shrink-0 items-center justify-center font-medium whitespace-nowrap transition-[background-color,color,transform,opacity] duration-200 active:translate-y-px active:scale-[0.98]",
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

export function IconButton({
  label,
  className,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      {...rest}
      className={cx(
        "grid size-12 shrink-0 place-items-center rounded-xl text-zinc-300 transition-colors duration-200 hover:bg-white/[0.06] hover:text-zinc-100 active:scale-95",
        className,
      )}
    >
      {children}
    </button>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  disabled,
  showLabel = true,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
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
      onClick={() => onChange(!checked)}
      className={cx(
        "inline-flex min-h-12 min-w-12 shrink-0 items-center gap-3 rounded-xl text-sm text-zinc-200",
        showLabel ? "px-2" : "justify-center",
      )}
    >
      <span
        aria-hidden
        className={cx(
          "inline-flex h-7 w-12 shrink-0 items-center rounded-full p-1 transition-colors duration-300",
          checked ? "bg-accent-500/90" : "bg-white/[0.1]",
        )}
      >
        <span
          className={cx(
            "size-5 rounded-full transition-[transform,background-color] duration-300 ease-spring",
            checked ? "translate-x-5 bg-white" : "bg-zinc-400",
          )}
        />
      </span>
      {showLabel && label}
    </button>
  );
}

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
      <span className="flex rounded-xl border border-white/[0.06] bg-ink p-1">
        {options.map((o) => (
          <button
            key={o.value}
            type="button"
            aria-pressed={o.value === value}
            onClick={() => onChange(o.value)}
            className={cx(
              "min-h-11 rounded-lg px-4 text-[13.5px] transition-colors duration-200",
              o.value === value ? "bg-white/[0.08] text-zinc-50" : "text-zinc-400 hover:text-zinc-200",
            )}
          >
            {o.label}
          </button>
        ))}
      </span>
    </div>
  );
}

export function TextField({
  label,
  hideLabel,
  helper,
  className,
  ...rest
}: InputHTMLAttributes<HTMLInputElement> & { label: string; hideLabel?: boolean; helper?: ReactNode }) {
  const id = useId();
  return (
    <div className={cx("grid gap-2", className)}>
      <label htmlFor={id} className={cx("text-[13px] font-medium text-zinc-300", hideLabel && "sr-only")}>
        {label}
      </label>
      <input id={id} type="text" autoComplete="off" spellCheck={false} {...rest} className="input" />
      {helper && <p className="text-[12.5px] leading-relaxed text-dim">{helper}</p>}
    </div>
  );
}

/** A legacy panel: eyebrow title, optional actions, generous padding. */
export function Card({
  title,
  actions,
  children,
  className,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={cx("panel p-6 md:p-8", className)}>
      {(title || actions) && (
        <div className="mb-5 flex flex-wrap items-center justify-between gap-3">
          {title && <h2 className="eyebrow">{title}</h2>}
          {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
        </div>
      )}
      {children}
    </section>
  );
}

/** Settings-style section: title above, rows in a divided surface. */
export function Section({
  title,
  intro,
  actions,
  children,
}: {
  title: ReactNode;
  intro?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section>
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-[19px] font-medium tracking-tight text-zinc-50">{title}</h2>
          {intro && <div className="mt-1.5 max-w-[65ch] text-[14px] leading-relaxed text-dim">{intro}</div>}
        </div>
        {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
      </div>
      <div className="rows mt-5 rounded-3xl border border-white/[0.05] bg-ink-2">{children}</div>
    </section>
  );
}

/** One row of a Section: title + description left, controls right. */
export function Row({ title, desc, children }: { title: ReactNode; desc?: ReactNode; children?: ReactNode }) {
  return (
    <div className="grid gap-4 px-6 py-5 md:grid-cols-[1fr_auto] md:items-center">
      <div className="min-w-0">
        <div className="text-[14.5px] text-zinc-100">{title}</div>
        {desc && <div className="mt-1 text-[13px] leading-relaxed text-dim">{desc}</div>}
      </div>
      {children && <div className="flex min-w-0 flex-wrap items-center gap-2 md:justify-end">{children}</div>}
    </div>
  );
}

/** Legacy PageHeader: eyebrow, large title, intro text, actions on the right. */
export function ViewHeader({
  title,
  eyebrow,
  description,
  children,
}: {
  title: string;
  eyebrow?: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <header className="flex flex-col gap-5 pb-2 md:flex-row md:items-end md:justify-between">
      <div className="min-w-0">
        {eyebrow && <div className="eyebrow mb-3">{eyebrow}</div>}
        <h1 className="text-3xl font-medium tracking-tighter text-zinc-50 md:text-[40px] md:leading-none">{title}</h1>
        {description && <div className="mt-3 max-w-[65ch] text-[15px] leading-relaxed text-zinc-400">{description}</div>}
      </div>
      {children && <div className="flex shrink-0 flex-wrap items-center gap-2">{children}</div>}
    </header>
  );
}

type BadgeTone = "zinc" | "accent" | "amber" | "rose";
const BADGE: Record<BadgeTone, string> = {
  zinc: "bg-white/[0.05] text-zinc-300 border-white/[0.06]",
  accent: "bg-accent-400/10 text-accent-300 border-accent-400/15",
  amber: "bg-amber-400/10 text-amber-300 border-amber-400/15",
  rose: "bg-rose-400/10 text-rose-300 border-rose-400/15",
};
/** Legacy Chip: small mono label. */
export function Badge({ tone = "zinc", children }: { tone?: BadgeTone; children: ReactNode }) {
  return (
    <span
      className={cx(
        "inline-flex h-6 items-center gap-1 rounded-lg border px-2 font-mono text-[11px] whitespace-nowrap",
        BADGE[tone],
      )}
    >
      {children}
    </span>
  );
}

/** "Experimental" label for games outside the tested set (tech stack §9.1). */
export function SupportBadge({ support }: { support: Support }) {
  if (support !== "experimental") return null;
  return (
    <span title="Not tested end to end; may need manual fixes" data-testid="experimental-badge" className="inline-flex">
      <Badge tone="amber">
        <FlaskIcon aria-hidden size={12} weight="bold" />
        Experimental
      </Badge>
    </span>
  );
}

export type DotTone = "accent" | "amber" | "rose" | "zinc";
const DOT: Record<DotTone, string> = {
  accent: "bg-accent-400",
  amber: "bg-amber-400",
  rose: "bg-rose-400",
  zinc: "bg-zinc-500",
};
/** A status dot that breathes while something is live. */
export const StatusDot = memo(function StatusDot({ tone = "accent", live = false }: { tone?: DotTone; live?: boolean }) {
  return (
    <span aria-hidden className="relative inline-flex size-2.5 shrink-0">
      {live && <span className={cx("absolute inset-0 animate-breathe rounded-full", DOT[tone])} />}
      <span className={cx("relative inline-flex size-2.5 rounded-full", DOT[tone])} />
    </span>
  );
});

/**
 * Empty state. With a title it is the legacy EmptyState (icon tile, title,
 * text, actions); without, a quiet centred line inside a panel.
 */
export function Empty({
  icon,
  title,
  children,
  action,
}: {
  icon?: ReactNode;
  title?: string;
  children?: ReactNode;
  action?: ReactNode;
}) {
  if (!title)
    return <p className="rounded-2xl border border-dashed border-white/[0.08] px-5 py-6 text-[14px] text-zinc-400">{children}</p>;
  return (
    <div className="grid gap-5 py-10 md:grid-cols-[auto_1fr] md:items-start md:gap-8">
      <div aria-hidden className="grid size-16 place-items-center rounded-2xl border border-white/[0.06] bg-ink-2 text-dim">
        {icon}
      </div>
      <div className="max-w-[52ch]">
        <h2 className="text-xl font-medium tracking-tight text-zinc-100">{title}</h2>
        {children && <div className="mt-2 text-[15px] leading-relaxed text-zinc-400">{children}</div>}
        {action && <div className="mt-6 flex flex-wrap gap-3">{action}</div>}
      </div>
    </div>
  );
}

/** Legacy ErrorNote. */
export function ErrorNote({ children }: { children: ReactNode }) {
  return (
    <div role="status" className="rounded-2xl border border-rose-400/15 bg-rose-500/[0.06] px-4 py-3 text-[14px] text-rose-200">
      {children}
    </div>
  );
}

/** Legacy fact: eyebrow key, mono value. */
export function Fact({ k, v }: { k: string; v: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="eyebrow">{k}</dt>
      <dd className="mt-1 font-mono text-[12.5px] break-words text-zinc-300">{v}</dd>
    </div>
  );
}

const SPAN: Record<Style, string> = {
  plain: "",
  bold: "font-semibold text-zinc-100",
  dim: "text-dim",
  green: "text-accent-300",
  yellow: "text-amber-300",
};
const LEVEL: Record<Msg["level"], string> = {
  heading: "font-sans font-medium text-zinc-100",
  info: "text-zinc-300",
  success: "text-accent-300",
  warn: "text-amber-300",
  error: "text-rose-300",
  line: "text-zinc-300",
  err_line: "text-rose-300",
};
export function MsgLine({ msg }: { msg: Msg }) {
  return (
    <span className={LEVEL[msg.level]}>
      {msg.spans.map((s, i) => (
        <span key={i} className={SPAN[s.style]}>
          {s.text}
        </span>
      ))}
    </span>
  );
}

/** Deterministic flat colour class + initials for a name (legacy monogram fallback). */
export function monogram(name: string): { cls: string; initials: string } {
  const h = [...name].reduce((a, c) => (a * 31 + c.charCodeAt(0)) % 997, 7);
  const initials =
    name
      .replace(/[^A-Za-z0-9 ]/g, " ")
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0])
      .join("")
      .toUpperCase() || "?";
  return { cls: `mono-${h % 8}`, initials };
}

/** Mod picture: a data: URL from the backend cache (no remote images under the CSP), else a monogram. */
export function Thumb({
  game,
  modId,
  name,
  className,
}: {
  game: string;
  modId: number | null;
  name?: string;
  className?: string;
}) {
  const q = useQuery({
    queryKey: ["thumb", game, modId],
    queryFn: () => thumbnail(game, modId!),
    enabled: modId !== null,
    staleTime: Infinity,
  });
  const src = q.data && q.data.startsWith("data:") ? q.data : null;
  if (src) return <img src={src} alt="" className={cx("size-11 shrink-0 rounded-xl object-cover", className)} />;
  const m = monogram(name ?? "");
  return (
    <span
      aria-hidden
      className={cx(
        "grid size-11 shrink-0 place-items-center rounded-xl font-mono text-[11px] font-semibold text-white/75",
        m.cls,
        className,
      )}
    >
      {name ? m.initials : ""}
    </span>
  );
}

export function bytes(n: number | null | undefined): string {
  if (n === null || n === undefined) return "?";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(i && v < 10 ? 1 : 0)} ${u[i]}`;
}

/** Unix seconds -> local date/time. */
export function when(ts: number | null | undefined): string {
  if (!ts) return "never";
  return new Date(ts * 1000).toLocaleString();
}

/** Unix seconds -> "3 min ago" (legacy `ago`). */
export function ago(ts: number | null | undefined, now = Date.now()): string {
  if (!ts) return "never";
  const s = Math.max(0, Math.round(now / 1000 - ts));
  if (s < 60) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h} h ago`;
  return `${Math.round(h / 24)} days ago`;
}
