// SPDX-License-Identifier: GPL-3.0-or-later
// Modal dialog: a spatial-navigation scope that takes focus on open, traps
// Tab, closes on "back" (Escape / gamepad B) and restores focus on close.

import { useEffect, useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import { BACK_EVENT } from "../nav/actions";
import { focusables } from "../nav/spatial";
import { cx } from "./ui";

export function Dialog({
  title,
  subtitle,
  onBack,
  children,
  wide,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  onBack: () => void;
  children: ReactNode;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const back = useRef(onBack);
  back.current = onBack;
  const titleId = useId();

  useEffect(() => {
    const el = ref.current!;
    const prev = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const first = el.querySelector<HTMLElement>("[data-autofocus]") ?? focusables(el)[0];
    first?.focus();
    const onBackEv = () => back.current();
    el.addEventListener(BACK_EVENT, onBackEv);
    return () => {
      el.removeEventListener(BACK_EVENT, onBackEv);
      if (prev?.isConnected) prev.focus();
    };
  }, []);

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key !== "Tab") return;
    const all = focusables(ref.current!);
    if (!all.length) return;
    const i = all.indexOf(document.activeElement as HTMLElement);
    const j = e.shiftKey ? (i <= 0 ? all.length - 1 : i - 1) : i === all.length - 1 ? 0 : i + 1;
    e.preventDefault();
    all[j].focus();
  };

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/70 p-6">
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        data-nav-scope
        onKeyDown={onKeyDown}
        className={cx("surface max-h-[92vh] w-full overflow-y-auto p-6", wide ? "max-w-5xl" : "max-w-3xl")}
      >
        <h2 id={titleId} className="text-xl font-medium text-zinc-50">
          {title}
        </h2>
        {subtitle && <p className="mt-1 text-sm text-zinc-400">{subtitle}</p>}
        <div className="mt-5">{children}</div>
      </div>
    </div>
  );
}
