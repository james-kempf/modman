// SPDX-License-Identifier: GPL-3.0-or-later
// A view's search box, top right in its header (Games, Browse). Gamepad Y
// focuses it ([data-view-search]). Enter submits when `onSubmit` is given
// (Browse searches Nexus); without it the field filters as you type.
import { MagnifyingGlassIcon } from "@phosphor-icons/react";
import type { InputHTMLAttributes } from "react";
import { SEARCH_ATTR } from "../../../nav/spatial";

export function SearchField({
  region,
  label,
  onSubmit,
  ...rest
}: Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "className"> & {
  /** The search landmark's name. */
  region: string;
  /** The input's accessible name. */
  label: string;
  onSubmit?: () => void;
}) {
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
      <MagnifyingGlassIcon aria-hidden size={18} className="pointer-events-none absolute top-1/2 left-3.5 -translate-y-1/2 text-zinc-400" />
      <input
        type="search"
        aria-label={label}
        autoComplete="off"
        spellCheck={false}
        enterKeyHint="search"
        {...{ [SEARCH_ATTR]: "" }}
        {...rest}
        className="input w-full pl-10"
      />
    </form>
  );
}
