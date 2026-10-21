// SPDX-License-Identifier: GPL-3.0-or-later
// Small UI primitives: large (>= 48px) targets, no hover-only affordances,
// no inline styles (CSP style-src 'self').

import { useQuery } from "@tanstack/react-query";
import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode } from "react";
import { useId } from "react";
import { thumbnail } from "../ipc/commands";
import type { Msg, Style, Support } from "../ipc/types";

export const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(" ");

type Tone = "primary" | "default" | "danger" | "ghost";
const TONES: Record<Tone, string> = {
  primary: "bg-accent-500 text-ink font-semibold hover:bg-accent-400",
  default: "bg-ink-3 text-zinc-100 border border-white/10 hover:bg-zinc-700",
  danger: "bg-rose-900/60 text-rose-100 border border-rose-400/30 hover:bg-rose-800/70",
  ghost: "text-zinc-300 hover:bg-white/5",
};

export function Button({
  tone = "default",
  busy,
  className,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { tone?: Tone; busy?: boolean }) {
  return (
    <button
      type="button"
      {...rest}
      disabled={rest.disabled || busy}
      aria-busy={busy || undefined}
      className={cx(
        "inline-flex min-h-12 min-w-12 items-center justify-center gap-2 rounded-xl px-5 text-base transition-colors",
        TONES[tone],
        className,
      )}
    >
      {children}
      {busy && <span className="text-sm opacity-70">…</span>}
    </button>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
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
        "inline-flex min-h-12 items-center gap-3 rounded-xl border px-4 text-base",
        checked ? "border-accent-500/50 bg-accent-950 text-accent-200" : "border-white/10 bg-ink-3 text-zinc-300",
      )}
    >
      <span
        aria-hidden
        className={cx(
          "inline-block h-5 w-9 rounded-full p-0.5 transition-colors",
          checked ? "bg-accent-500" : "bg-zinc-600",
        )}
      >
        <span className={cx("block size-4 rounded-full bg-white transition-transform", checked && "translate-x-4")} />
      </span>
      {label}
    </button>
  );
}

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
    <div role="group" aria-label={label} className="flex flex-wrap items-center gap-2">
      <span className="mr-1 text-sm text-zinc-400">{label}</span>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          aria-pressed={o.value === value}
          onClick={() => onChange(o.value)}
          className={cx(
            "min-h-12 rounded-xl border px-4 text-base",
            o.value === value
              ? "border-accent-500/60 bg-accent-950 text-accent-200"
              : "border-white/10 bg-ink-3 text-zinc-300",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function TextField({
  label,
  hideLabel,
  className,
  ...rest
}: InputHTMLAttributes<HTMLInputElement> & { label: string; hideLabel?: boolean }) {
  const id = useId();
  return (
    <div className={cx("grid gap-1.5", className)}>
      <label htmlFor={id} className={cx("text-sm text-zinc-400", hideLabel && "sr-only")}>
        {label}
      </label>
      <input id={id} type="text" autoComplete="off" spellCheck={false} {...rest} className="input" />
    </div>
  );
}

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
    <section className={cx("surface p-6", className)}>
      {(title || actions) && (
        <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
          {title && <h2 className="text-lg font-medium text-zinc-100">{title}</h2>}
          {actions && <div className="flex flex-wrap gap-2">{actions}</div>}
        </div>
      )}
      {children}
    </section>
  );
}

export function ViewHeader({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <header className="mb-6 flex flex-wrap items-center justify-between gap-4">
      <h1 className="text-3xl font-medium tracking-tight text-zinc-50">{title}</h1>
      {children && <div className="flex flex-wrap items-center gap-2">{children}</div>}
    </header>
  );
}

type BadgeTone = "zinc" | "accent" | "amber" | "rose";
const BADGE: Record<BadgeTone, string> = {
  zinc: "bg-white/5 text-zinc-300 border-white/10",
  accent: "bg-accent-950 text-accent-200 border-accent-500/30",
  amber: "bg-amber-950/60 text-amber-200 border-amber-400/30",
  rose: "bg-rose-950/60 text-rose-200 border-rose-400/30",
};
export function Badge({ tone = "zinc", children }: { tone?: BadgeTone; children: ReactNode }) {
  return (
    <span className={cx("inline-flex items-center rounded-full border px-2.5 py-0.5 text-sm", BADGE[tone])}>
      {children}
    </span>
  );
}

/** "Experimental" label for games outside the tested set (tech stack §9.1). */
export function SupportBadge({ support }: { support: Support }) {
  if (support !== "experimental") return null;
  return (
    <span title="Not tested end to end; may need manual fixes" data-testid="experimental-badge">
      <Badge tone="amber">Experimental</Badge>
    </span>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="rounded-xl border border-dashed border-white/10 p-6 text-zinc-400">{children}</p>;
}

const SPAN: Record<Style, string> = {
  plain: "",
  bold: "font-semibold text-zinc-100",
  dim: "text-zinc-500",
  green: "text-accent-300",
  yellow: "text-amber-300",
};
const LEVEL: Record<Msg["level"], string> = {
  heading: "font-semibold text-zinc-100",
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

/** Mod picture: a data: URL from the backend cache (no remote images under the CSP). */
export function Thumb({ game, modId, className }: { game: string; modId: number | null; className?: string }) {
  const q = useQuery({
    queryKey: ["thumb", game, modId],
    queryFn: () => thumbnail(game, modId!),
    enabled: modId !== null,
    staleTime: Infinity,
  });
  const src = q.data && q.data.startsWith("data:") ? q.data : null;
  return src ? (
    <img src={src} alt="" className={cx("size-12 shrink-0 rounded-lg object-cover", className)} />
  ) : (
    <span aria-hidden className={cx("size-12 shrink-0 rounded-lg bg-ink-3", className)} />
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
