// SPDX-License-Identifier: GPL-3.0-or-later
// Modal dialog: a spatial-navigation scope that takes focus on open, traps
// Tab, closes on "back" (Escape / gamepad B) and restores focus on close.
// Rendered in a portal on <body>: a transformed ancestor (the view's entry
// animation) would otherwise be the containing block of its fixed backdrop.
// While open, everything else on <body> (the app, lower dialogs) is inert.

import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { BACK_EVENT } from "../nav/actions";
import { focusables, navScope, recoverFocus } from "../nav/spatial";
import { cx } from "./ui";

/**
 * Each open dialog's fallback openers: when a dialog is opened from inside
 * another one that then closes (back-to-back prompts, e.g. FOMOD then
 * confirm), focus goes back to what opened the first dialog.
 */
const chains = new WeakMap<Element, HTMLElement[]>();
/**
 * Openers of a dialog that just closed and has not restored focus yet: a
 * dialog opening in the next commit (focus is on <body> by then) inherits them.
 */
let restoring: HTMLElement[] | null = null;

/** Open dialog backdrops, bottom to top. */
const stack: HTMLElement[] = [];
/** <body> children this module made inert (others may be inert for their own reasons). */
const madeInert = new Set<Element>();

/** Everything on <body> but the top dialog is inert; nothing when no dialog is open. */
function syncInert(): void {
  const top = stack[stack.length - 1];
  const want = new Set(top ? Array.from(document.body.children).filter((el) => el !== top) : []);
  for (const el of madeInert)
    if (!want.has(el)) {
      el.removeAttribute("inert");
      madeInert.delete(el);
    }
  for (const el of want)
    if (!el.hasAttribute("inert")) {
      el.setAttribute("inert", "");
      madeInert.add(el);
    }
}

/** What has focus now, plus the openers of the dialog it sits in (nearest first). */
function captureOpeners(): HTMLElement[] {
  const a = document.activeElement;
  if (!(a instanceof HTMLElement) || a === document.body) return restoring ?? [];
  const host = a.closest("[role=dialog]");
  return [a, ...((host && chains.get(host)) ?? [])];
}

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
  const backdrop = useRef<HTMLDivElement>(null);
  const back = useRef(onBack);
  back.current = onBack;
  const titleId = useId();
  // Captured during the first render, before this dialog takes focus (an
  // effect would see the dialog's own element on StrictMode's re-run).
  const [openers] = useState(captureOpeners);
  const ret = useRef(returnFocus);
  ret.current = returnFocus;

  // Layout effect: the background is inert before the dialog takes focus.
  useLayoutEffect(() => {
    const el = backdrop.current!;
    stack.push(el);
    syncInert();
    return () => {
      stack.splice(stack.indexOf(el), 1);
      syncInert();
    };
  }, []);

  useEffect(() => {
    const el = ref.current!;
    chains.set(el, openers);
    const first = el.querySelector<HTMLElement>("[data-autofocus]") ?? focusables(el)[0];
    first?.focus();
    const onBackEv = () => back.current();
    el.addEventListener(BACK_EVENT, onBackEv);
    return () => {
      el.removeEventListener(BACK_EVENT, onBackEv);
      const explicit = ret.current;
      const chain = [...(explicit ? [explicit] : []), ...openers];
      restoring = chain;
      // Next frame: after the commit that removes the dialog and lifts the
      // app root's `inert` (focusing an inert element is a no-op).
      requestAnimationFrame(() => {
        if (restoring === chain) restoring = null;
        const a = document.activeElement;
        if (a && a !== document.body && a.isConnected) return; // focus already placed (StrictMode re-run, another dialog)
        const ok = focusables(navScope());
        const target = chain.find((t) => t.isConnected && ok.includes(t));
        if (target) target.focus();
        else recoverFocus();
      });
    };
  }, [openers]);

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key !== "Tab") return;
    const all = focusables(ref.current!);
    if (!all.length) return;
    const i = all.indexOf(document.activeElement as HTMLElement);
    const j = e.shiftKey ? (i <= 0 ? all.length - 1 : i - 1) : i === all.length - 1 ? 0 : i + 1;
    e.preventDefault();
    all[j].focus();
  };

  return createPortal(
    // Fixed to the viewport; the dialog scrolls inside and always fits (also at 800x480).
    <div
      ref={backdrop}
      className="fixed inset-0 z-50 grid animate-fade place-items-center overflow-y-auto overscroll-contain bg-ink/70 p-4 backdrop-blur-sm sm:p-6"
    >
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        data-nav-scope
        onKeyDown={onKeyDown}
        className={cx(
          "glass max-h-[calc(100vh-2rem)] w-full animate-pop overflow-y-auto overscroll-contain rounded-3xl p-5 sm:max-h-[calc(100vh-3rem)] sm:p-7",
          wide ? "max-w-5xl" : "max-w-2xl",
        )}
      >
        <h2 id={titleId} className="text-xl font-medium tracking-tight wrap-anywhere text-zinc-50">
          {title}
        </h2>
        {subtitle && <p className="mt-1.5 text-[14.5px] leading-relaxed wrap-anywhere text-zinc-400">{subtitle}</p>}
        <div className="mt-6">{children}</div>
      </div>
    </div>,
    document.body,
  );
}
