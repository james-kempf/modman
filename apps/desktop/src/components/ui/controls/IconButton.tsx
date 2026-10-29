// SPDX-License-Identifier: GPL-3.0-or-later
import type { ButtonHTMLAttributes } from "react";
import { cx } from "../styles";

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
        "grid size-control shrink-0 place-items-center rounded-control text-zinc-300 transition-[background-color,color,transform] duration-base active:scale-95",
        className,
      )}
    >
      {children}
    </button>
  );
}
