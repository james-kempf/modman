// SPDX-License-Identifier: GPL-3.0-or-later
// Modal dialog: a spatial-navigation scope that takes focus on open, traps
// Tab, closes on "back" (Escape / gamepad B) and restores focus on close.

import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { BACK_EVENT } from "../nav/actions";
import { focusables, navScope, recoverFocus } from "../nav/spatial";
import { cx } from "./ui";

export function Dialog({
  title,
  subtitle,
  onBack,
  children,
  wide,
  returnFocus,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  onBack: () => void;
  children: ReactNode;
  wide?: boolean;
  /** Where focus goes on close (default: what had focus when the dialog opened). */
  returnFocus?: HTMLElement | null;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const back = useRef(onBack);
  back.current = onBack;
  const titleId = useId();
  // Captured during the first render, before this dialog takes focus (an
  // effect would see the dialog's own element on StrictMode's re-run).
  const [opener] = useState(() => (document.activeElement instanceof HTMLElement ? document.activeElement : null));
  const ret = useRef(returnFocus);
  ret.current = returnFocus;

  useEffect(() => {
    const el = ref.current!;
    const first = el.querySelector<HTMLElement>("[data-autofocus]") ?? focusables(el)[0];
    first?.focus();
    const onBackEv = () => back.current();
    el.addEventListener(BACK_EVENT, onBackEv);
    return () => {
      el.removeEventListener(BACK_EVENT, onBackEv);
      const target = ret.current ?? opener;
      // Next frame: after the commit that removes the dialog and lifts the
      // app root's `inert` (focusing an inert element is a no-op).
      requestAnimationFrame(() => {
        const a = document.activeElement;
        if (a && a !== document.body && a.isConnected) return; // focus already placed (StrictMode re-run, another dialog)
        if (target?.isConnected && focusables(navScope()).includes(target)) target.focus();
        else recoverFocus();
      });
    };
  }, [opener]);

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
    <div className="fixed inset-0 z-50 grid animate-fade place-items-center bg-ink/70 p-6 backdrop-blur-sm">
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        data-nav-scope
        onKeyDown={onKeyDown}
        className={cx("glass max-h-[92vh] w-full animate-pop overflow-y-auto rounded-3xl p-7", wide ? "max-w-5xl" : "max-w-2xl")}
      >
        <h2 id={titleId} className="text-xl font-medium tracking-tight text-zinc-50">
          {title}
        </h2>
        {subtitle && <p className="mt-1.5 text-[14.5px] leading-relaxed text-zinc-400">{subtitle}</p>}
        <div className="mt-6">{children}</div>
      </div>
    </div>
  );
}
