// SPDX-License-Identifier: GPL-3.0-or-later
import { useId, type InputHTMLAttributes, type ReactNode } from "react";
import { cx } from "../styles";

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
