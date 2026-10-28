// SPDX-License-Identifier: GPL-3.0-or-later
import { CaretDownIcon, CheckIcon, CircleNotchIcon } from "@phosphor-icons/react";
import { useEffect, useId, useRef, useState, type FocusEvent, type KeyboardEvent, type ReactNode } from "react";
import { BACK_EVENT } from "../../../nav/actions";
import { cx } from "../styles";

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
