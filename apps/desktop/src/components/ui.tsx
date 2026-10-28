// SPDX-License-Identifier: GPL-3.0-or-later
// UI primitives in the legacy web UI's visual language (soft surfaces, chips,
// Phosphor icons). Large (>= 44px) targets, no hover-only affordances, no
// inline styles (CSP style-src 'self').

import { CaretDownIcon, CheckIcon, CircleNotchIcon, FlaskIcon } from "@phosphor-icons/react";
import { useQuery } from "@tanstack/react-query";
import {
  memo,
  useEffect,
  useId,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type FocusEvent,
  type HTMLAttributes,
  type InputHTMLAttributes,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { BACK_EVENT } from "../nav/actions";
import { thumbnail } from "../ipc/commands";
import type { Msg, Style, Support } from "../ipc/types";

export const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(" ");

type Tone = "primary" | "default" | "danger" | "ghost";
const TONES: Record<Tone, string> = {
  primary: "bg-accent-400 text-accent-950 font-semibold hover:bg-accent-300",
  default: "bg-fill-2 text-zinc-100 border border-hairline hover:bg-fill-hover",
  danger: "bg-rose-500/10 text-rose-200 border border-rose-400/20 hover:bg-rose-500/20",
  ghost: "text-zinc-300 hover:text-zinc-100 hover:bg-fill-2",
};
type Size = "sm" | "md" | "lg";
const SIZES: Record<Size, string> = {
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

export function IconButton({
  label,
  busy,
  className,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; busy?: boolean }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      {...rest}
      aria-disabled={busy || undefined}
      aria-busy={busy || undefined}
      onClick={busy ? (e) => e.preventDefault() : rest.onClick}
      className={cx(
        "grid size-control shrink-0 place-items-center rounded-control text-zinc-300 transition-colors duration-base hover:bg-fill-2 hover:text-zinc-100 active:scale-95",
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
              o.value === value ? "bg-fill-3 text-zinc-50" : "text-zinc-400 hover:text-zinc-200",
            )}
          >
            {o.label}
          </button>
        ))}
      </span>
    </div>
  );
}

/**
 * Dropdown: a button that opens a listbox of options. The open list is a
 * spatial-navigation scope, so the keyboard and the gamepad drive it the
 * same way: Enter/A opens it and picks an option, arrows/D-pad move, Esc/B
 * closes it without a change. Focus returns to the button.
 */
export function Select<T extends string>({
  label,
  hideLabel,
  value,
  options,
  onChange,
  busy,
  className,
}: {
  label: ReactNode;
  hideLabel?: boolean;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (v: T) => void;
  /** A change is being applied: stays focusable, does not open. */
  busy?: boolean;
  className?: string;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const current = options.find((o) => o.value === value);
  const close = (refocus = true) => {
    setOpen(false);
    if (refocus) button.current?.focus();
  };
  const pick = (v: T) => {
    close();
    if (v !== value) onChange(v);
  };
  useEffect(() => {
    if (!open) return;
    const el = list.current;
    (el?.querySelector<HTMLElement>("[aria-selected=true]") ?? el?.querySelector<HTMLElement>("[role=option]"))?.focus();
    // Escape / gamepad B arrive as "back" on the topmost nav scope: this list.
    const onBack = () => close();
    el?.addEventListener(BACK_EVENT, onBack);
    return () => el?.removeEventListener(BACK_EVENT, onBack);
  }, [open]);
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const opts = Array.from(list.current?.querySelectorAll<HTMLElement>("[role=option]") ?? []);
    const i = opts.indexOf(document.activeElement as HTMLElement);
    const go = (j: number) => opts[Math.max(0, Math.min(opts.length - 1, j))]?.focus();
    // Handled here (and marked handled) so the list works the same without the global key handler.
    if (e.key === "ArrowDown") go(i + 1);
    else if (e.key === "ArrowUp") go(i - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(opts.length - 1);
    else if (e.key === "Escape") close();
    else if (e.key === "Tab") return close(false);
    else return;
    e.preventDefault();
  };
  // Focus moving outside (a click elsewhere, Tab) closes the list.
  const onBlur = (e: FocusEvent<HTMLDivElement>) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setOpen(false);
  };
  return (
    <div className={cx("relative grid min-w-0 gap-2", className)} onBlur={open ? onBlur : undefined}>
      <span id={`${id}-label`} className={cx("text-label font-medium wrap-anywhere text-zinc-300", hideLabel && "sr-only")}>
        {label}
      </span>
      <button
        ref={button}
        id={`${id}-button`}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-labelledby={`${id}-label ${id}-button`}
        aria-disabled={busy || undefined}
        aria-busy={busy || undefined}
        onClick={() => !busy && setOpen((o) => !o)}
        className="input flex items-center justify-between gap-3 text-left"
      >
        <span className="min-w-0 truncate">{current?.label ?? value}</span>
        {busy ? (
          <CircleNotchIcon aria-hidden size={16} weight="bold" className="shrink-0 animate-spin" />
        ) : (
          <CaretDownIcon aria-hidden size={16} className="shrink-0 text-zinc-400" />
        )}
      </button>
      {open && (
        <div
          ref={list}
          role="listbox"
          aria-labelledby={`${id}-label`}
          data-nav-scope
          onKeyDown={onKeyDown}
          className="glass absolute top-full right-0 left-0 z-30 mt-1.5 grid max-h-72 gap-0.5 overflow-y-auto rounded-control p-1"
        >
          {options.map((o) => (
            <button
              key={o.value}
              type="button"
              role="option"
              aria-selected={o.value === value}
              onClick={() => pick(o.value)}
              className={cx(
                "flex min-h-control-sm items-center justify-between gap-3 rounded-control-sm px-3 text-left text-body transition-colors duration-fast",
                o.value === value ? "bg-fill-3 text-zinc-50" : "text-zinc-300 hover:bg-fill-2 hover:text-zinc-100",
              )}
            >
              <span className="min-w-0 truncate">{o.label}</span>
              {o.value === value && <CheckIcon aria-hidden size={15} className="shrink-0 text-accent-300" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function TextField({
  label,
  hideLabel,
  helper,
  error,
  className,
  ...rest
}: InputHTMLAttributes<HTMLInputElement> & {
  label: string;
  hideLabel?: boolean;
  helper?: ReactNode;
  /** Inline validation error (marks the input aria-invalid and describes it). */
  error?: string | null;
}) {
  const id = useId();
  const helpId = `${id}-help`;
  const errId = `${id}-err`;
  const described = [error && errId, helper && helpId].filter(Boolean).join(" ") || undefined;
  return (
    <div className={cx("grid gap-2", className)}>
      <label htmlFor={id} className={cx("text-label font-medium text-zinc-300", hideLabel && "sr-only")}>
        {label}
      </label>
      <input
        id={id}
        type="text"
        autoComplete="off"
        spellCheck={false}
        aria-invalid={error ? true : undefined}
        aria-describedby={described}
        {...rest}
        className="input"
      />
      {error && (
        <p id={errId} className="text-label leading-relaxed text-rose-300">
          {error}
        </p>
      )}
      {helper && (
        <p id={helpId} className="text-label leading-relaxed text-dim">
          {helper}
        </p>
      )}
    </div>
  );
}

/** Padding of every Card section (CardSection, Row, card-level list rows): equal on both axes. */
export const SECTION_PAD = "p-section";
/** The inset header: the section padding minus the bottom (the first section's top padding is the gap). */
export const INSET_HEADER_PAD = "px-section pt-section";
/** The card surface: radius, hairline border, raised fill. */
export const CARD_SHELL = "rounded-card border border-hairline bg-ink-2";
/** Hairline dividers between stacked children (card sections, lists). */
export const DIVIDE = "divide-y divide-hairline";
/** A sunken well inside a card or dialog (tiles, typed-text output, side panels); add padding per use. */
export const TILE = "rounded-card border border-hairline bg-ink";
/** A compact status pill / chip row (mono log lines, repair steps). */
export const PILL = "rounded-control border border-hairline bg-ink px-3 py-1.5 text-label";
/** A free-standing list row (jobs, entries) that highlights on hover. */
export const LIST_ROW = "rounded-control px-3 py-2.5 transition-colors duration-base hover:bg-fill-1";
/** Card title (non-inset header). */
export const CARD_TITLE = "text-title font-medium tracking-tight text-zinc-50";
/** Muted secondary line under a title (card intros, row help). */
export const MUTED_NOTE = "text-label leading-snug text-dim";

/**
 * The one card: an optional header (title, intro, actions) above a surface
 * whose children are stacked sections split by dividers. Children are
 * `CardSection`s (padded free content), `Row`s, or lists with their own padding.
 * `inset` puts the header inside the surface as its first section (for cards
 * side by side with an untitled one, so their tops line up).
 */
export function Card({
  title,
  intro,
  actions,
  inset = false,
  children,
  className,
  ...rest
}: Omit<HTMLAttributes<HTMLElement>, "title"> & {
  title?: ReactNode;
  intro?: ReactNode;
  actions?: ReactNode;
  inset?: boolean;
  children: ReactNode;
}) {
  const header = title || intro || actions ? (
    // inset: no bottom padding, the first section's top padding is the gap (no divider line).
    <div className={cx("flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1", inset && INSET_HEADER_PAD)}>
      <div className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-0.5">
        {/* inset titles keep the pre-migration mono eyebrow look. */}
        {title && <h2 className={inset ? "eyebrow" : CARD_TITLE}>{title}</h2>}
        {intro && <div className={MUTED_NOTE}>{intro}</div>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </div>
  ) : null;
  return (
    <section {...rest} className={cx("flex min-w-0 flex-col", className)}>
      {!inset && header}
      {/* flex-1: cards side by side in a grid row share one height. */}
      {inset ? (
        // The header sits outside the divided part, so no line between it and the first section.
        <div className={cx("flex flex-1 flex-col", CARD_SHELL)}>
          {header}
          <div className={cx("flex flex-1 flex-col", DIVIDE)}>{children}</div>
        </div>
      ) : (
        <div
          className={cx(
            "flex flex-1 flex-col",
            DIVIDE,
            CARD_SHELL,
            header && "mt-2.5",
          )}
        >
          {children}
        </div>
      )}
    </section>
  );
}

/** One padded part of a Card (free content: forms, lists, notes). */
export function CardSection({ children, className, ...rest }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div {...rest} className={cx(SECTION_PAD, className)}>
      {children}
    </div>
  );
}

/** One compact row of a Card: title + one muted help line left, controls right (wrapping below when narrow). */
export function Row({ title, desc, children }: { title: ReactNode; desc?: ReactNode; children?: ReactNode }) {
  return (
    <div className={cx("flex flex-wrap items-center justify-between gap-x-4 gap-y-2", SECTION_PAD)}>
      <div className="min-w-0 flex-[1_1_12rem]">
        <div className="text-body text-zinc-100">{title}</div>
        {desc && <div className={MUTED_NOTE}>{desc}</div>}
      </div>
      {children && <div className="ml-auto flex min-w-0 flex-wrap items-center justify-end gap-2">{children}</div>}
    </div>
  );
}

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
    // Wraps instead of squeezing: the title keeps ~24rem, and the actions go below it when both do not fit.
    <header className="flex flex-wrap items-end justify-between gap-x-8 gap-y-5 pb-2">
      <div className="min-w-0 flex-[1_1_24rem]">
        {eyebrow && <div className="eyebrow mb-3">{eyebrow}</div>}
        <h1 className="text-3xl font-medium tracking-tighter text-zinc-50 md:text-display md:leading-none">{title}</h1>
        {description && <div className="mt-3 max-w-prose text-lead leading-relaxed text-zinc-400">{description}</div>}
      </div>
      {children && <div className="flex max-w-full min-w-0 flex-wrap items-center gap-2">{children}</div>}
    </header>
  );
}

type BadgeTone = "zinc" | "accent" | "amber" | "rose";
const BADGE: Record<BadgeTone, string> = {
  zinc: "bg-fill-2 text-zinc-300 border-hairline",
  accent: "bg-accent-400/10 text-accent-300 border-accent-400/15",
  amber: "bg-amber-400/10 text-amber-300 border-amber-400/15",
  rose: "bg-rose-400/10 text-rose-300 border-rose-400/15",
};
/** Legacy Chip: small mono label. */
export function Badge({ tone = "zinc", children }: { tone?: BadgeTone; children: ReactNode }) {
  return (
    <span
      className={cx(
        "inline-flex h-badge items-center gap-1 rounded-control-sm border px-2 font-mono text-micro whitespace-nowrap",
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
    <span aria-hidden className="relative inline-flex size-dot shrink-0">
      {live && <span className={cx("absolute inset-0 animate-breathe rounded-pill", DOT[tone])} />}
      <span className={cx("relative inline-flex size-dot rounded-pill", DOT[tone])} />
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
    return <p className="rounded-card border border-dashed border-line px-5 py-6 text-body text-zinc-400">{children}</p>;
  return (
    <div className="grid gap-5 py-10 md:grid-cols-[auto_1fr] md:items-start md:gap-8">
      <div aria-hidden className="grid size-tile place-items-center rounded-card border border-hairline bg-ink-2 text-dim">
        {icon}
      </div>
      <div className="max-w-prose-sm">
        <h2 className="text-xl font-medium tracking-tight text-zinc-100">{title}</h2>
        {children && <div className="mt-2 text-lead leading-relaxed text-zinc-400">{children}</div>}
        {action && <div className="mt-6 flex flex-wrap gap-3">{action}</div>}
      </div>
    </div>
  );
}

/** Legacy ErrorNote (optional actions, e.g. "Try again"). */
export function ErrorNote({ children, actions }: { children: ReactNode; actions?: ReactNode }) {
  return (
    <div
      role="status"
      className="flex flex-wrap items-center gap-3 rounded-card border border-rose-400/15 bg-rose-500/5 px-4 py-3 text-body text-rose-200"
    >
      <div className="min-w-0 flex-1 wrap-anywhere">{children}</div>
      {actions && <div className="flex flex-wrap gap-2">{actions}</div>}
    </div>
  );
}

/** Legacy fact: eyebrow key, mono value. */
export function Fact({ k, v }: { k: string; v: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="eyebrow">{k}</dt>
      <dd className="mt-1 font-mono text-label break-words text-zinc-300">{v}</dd>
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
          {/* Streamed events stay in the session log: one malformed span must not break Activity for good. */}
          {typeof s.text === "string" ? s.text : JSON.stringify(s.text)}
        </span>
      ))}
    </span>
  );
}

/** Deterministic flat colour class + initials for a name (legacy monogram fallback). */
export function monogram(name: string): { cls: string; initials: string } {
  const h = [...name].reduce((a, c) => (a * 31 + c.charCodeAt(0)) % 997, 7);
  // Words of letters/digits in any script (marks belong to their letter); the
  // initial is a word's first grapheme without its combining marks.
  const words = name.match(/[\p{L}\p{N}][\p{L}\p{M}\p{N}]*/gu) ?? [];
  const initial = (w: string) => {
    const g = graphemes(w)[0] ?? "";
    return g.replace(/\p{M}+/gu, "") || g;
  };
  const initials =
    words
      .slice(0, 2)
      .map(initial)
      .join("")
      .toLocaleUpperCase() ||
    // No letters (e.g. only emoji): the first grapheme as is.
    graphemes(name.trim())[0] ||
    "?";
  return { cls: `mono-${h % 8}`, initials };
}

/** User-perceived characters (an emoji ZWJ sequence or a letter with its marks is one). */
function graphemes(s: string): string[] {
  if (typeof Intl !== "undefined" && "Segmenter" in Intl)
    return Array.from(new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(s), (x) => x.segment);
  return Array.from(s);
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
  if (src) return <img src={src} alt="" className={cx("size-thumb shrink-0 rounded-control object-cover", className)} />;
  const m = monogram(name ?? "");
  return (
    <span
      aria-hidden
      className={cx(
        "grid size-thumb shrink-0 place-items-center rounded-control font-mono text-micro font-semibold text-white/75",
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
