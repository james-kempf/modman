// SPDX-License-Identifier: GPL-3.0-or-later
// A view's search box, top right in its header (Games, Browse). Gamepad Y
// focuses it ([data-view-search]). Enter submits when `onSubmit` is given
// (Browse searches Nexus); without it the field filters as you type.
// `pending` (typed but not submitted yet) shows a trailing "↵ Enter" hint, so
// a submit-on-Enter field does not look like it filters as you type.
import { MagnifyingGlassIcon } from "@phosphor-icons/react";
import type { InputHTMLAttributes } from "react";
import { SEARCH_ATTR } from "../../../nav/spatial";
import { cx } from "../styles";

export function SearchField({
  region,
  label,
  onSubmit,
  pending = false,
  ...rest
}: Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "className"> & {
  /** The search landmark's name. */
  region: string;
  /** The input's accessible name. */
  label: string;
  onSubmit?: () => void;
  /** The text differs from what was last submitted: show the Enter hint (only with `onSubmit`). */
  pending?: boolean;
}) {
  const hint = !!onSubmit && pending;
  return (
    // w-96, capped at the header's width: long enough for real queries, no horizontal scroll when narrow.
    <form
      role="search"
      aria-label={region}
      className="relative w-96 max-w-full"
      onSubmit={(e) => {
        e.preventDefault();
        onSubmit?.();
      }}
    >
      <MagnifyingGlassIcon aria-hidden size={18} className="pointer-events-none absolute top-1/2 left-4 -translate-y-1/2 text-zinc-400" />
      <input
        type="search"
        aria-label={label}
        autoComplete="off"
        spellCheck={false}
        enterKeyHint="search"
        {...{ [SEARCH_ATTR]: "" }}
        {...rest}
        className={cx("peer input w-full pl-11", hint && "pr-20")}
      />
      {/* Not a control: Enter (or the on-screen keyboard's Search key, hence hidden in controller mode) submits.
          On the field's nav-focus highlight (white) it takes the highlight's muted ink, like the placeholder. */}
      {hint && (
        <span
          aria-hidden="true"
          data-search-hint
          className="pointer-events-none absolute top-1/2 right-4 -translate-y-1/2 font-mono text-caption text-dim pad:hidden in-[html[data-input=nav]]:peer-focus:text-focus-ink-muted"
        >
          ↵ Enter
        </span>
      )}
    </form>
  );
}
