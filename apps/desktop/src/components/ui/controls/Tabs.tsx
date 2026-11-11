// SPDX-License-Identifier: GPL-3.0-or-later
// A tab strip (role=tablist) with an optional panel for the selected tab.
// One look everywhere (the app's view bar and the dialogs): left-aligned tabs on one line (horizontal
// scroll when narrow), icons, badges. Tabs are pills; the selected one sits on an accent-tinted pill
// that animates scale + opacity only, driven by aria-selected classes (no inline styles: CSP).
// Tabs are not focus stops by default (tabindex -1); `focusable` makes them a roving-tabindex group
// following the APG manual-activation pattern. Start/end adornments (e.g. LB/RB glyphs) are siblings
// of the tablist, never children, so the tablist owns only role=tab elements.

import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { TAB_EVENT, type TabEventDetail } from "../../../nav/actions";
import { focusables } from "../../../nav/spatial";
import { cx } from "../styles";

export interface TabItem<T extends string> {
  value: T;
  label: string;
  /** Leading glyph (decorative; give it aria-hidden). */
  icon?: ReactNode;
  /** Trailing badge, e.g. a count. */
  badge?: ReactNode;
}

// One line, never wraps: overflows into a horizontal scroll (scrollbar hidden) at narrow widths.
const LIST = "flex min-w-0 flex-1 items-stretch gap-1 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden";
// Tighter padding below xl (1280) so the 8 view tabs fit a 1024 window before the list has to scroll.
// Selected: accent text on an accent tint (the Badge accent look), apart from the app's white nav-focus/hover
// highlight. Under that highlight the selected tab keeps accent text (--focus-text, AA on white, as primary
// buttons do) and its pill turns into an accent ring, so "selected + focused" differs from "focused".
const TAB =
  "group relative inline-flex min-h-control-xs shrink-0 items-center rounded-pill px-3 text-body font-medium whitespace-nowrap text-zinc-400 transition-[color,transform] duration-base active:scale-98 aria-selected:text-accent-200 aria-selected:[--focus-text:var(--color-accent-700)] xl:px-4";
// The selected tab's pill: scale + opacity only, spring easing, none under reduced motion.
const HIGHLIGHT =
  "pointer-events-none absolute inset-0 scale-95 rounded-pill bg-accent-400/15 opacity-0 transition-[transform,opacity] duration-[250ms] ease-[cubic-bezier(0.16,1,0.3,1)] group-aria-selected:scale-100 group-aria-selected:opacity-100 motion-reduce:transition-none in-highlight:ring-2 in-highlight:ring-accent-600 in-highlight:ring-inset";
// Positioned so it paints above the (absolute) pill.
const CONTENT = "relative inline-flex items-center gap-2";

/**
 * Tabs, and the selected tab's panel when `children` is given. LB/RB (the enclosing dialog's TAB_EVENT)
 * switch tabs with wrap-around; the listener attaches only inside a `data-nav-scope` (never document/body).
 * Focus inside the old panel moves into the new one.
 */
export function Tabs<T extends string>({
  label,
  tabs,
  value,
  onChange,
  focusable = false,
  start,
  end,
  controls,
  children,
}: {
  label: string;
  tabs: readonly TabItem<T>[];
  value: T;
  onChange: (t: T) => void;
  /** Roving tabindex (selected tab 0, others -1; APG manual activation) with the spatial-nav marker. */
  focusable?: boolean;
  /** Decorative adornments before/after the tablist (aria-hidden, not focusable). */
  start?: ReactNode;
  end?: ReactNode;
  /** id of the element the tabs control, when there is no `children` panel. */
  controls?: string;
  /** The selected tab's panel content. */
  children?: ReactNode;
}) {
  const id = useId();
  const list = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const refocus = useRef(false);
  const latest = useRef({ tabs, value, onChange });
  latest.current = { tabs, value, onChange };
  const [select] = useState(() => (t: T) => {
    const cur = latest.current;
    if (t === cur.value) return;
    // The focused control goes away with its panel: focus the new panel once it is rendered.
    refocus.current = !!panel.current?.contains(document.activeElement);
    cur.onChange(t);
  });
  const hasPanel = children !== undefined;

  useEffect(() => {
    const scope = list.current?.closest<HTMLElement>("[data-nav-scope]");
    if (!scope || scope === document.body) return;
    const onTab = (e: Event) => {
      const { tabs: all, value: cur } = latest.current;
      const i = all.findIndex((t) => t.value === cur);
      const { delta } = (e as CustomEvent<TabEventDetail>).detail;
      select(all[(i + delta + all.length) % all.length].value);
    };
    scope.addEventListener(TAB_EVENT, onTab);
    return () => scope.removeEventListener(TAB_EVENT, onTab);
  }, [select]);

  useEffect(() => {
    if (!refocus.current) return;
    refocus.current = false;
    const scope = list.current?.closest<HTMLElement>("[data-nav-scope]");
    const target = (panel.current && focusables(panel.current)[0]) ?? scope?.querySelector<HTMLElement>("[data-autofocus]");
    target?.focus();
  }, [value]);

  // Horizontal-scroll fallback: keep the selected tab in view (jsdom has no scrollIntoView).
  useEffect(() => {
    list.current?.querySelector<HTMLElement>("[aria-selected='true']")?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [value]);

  // APG manual activation: arrows/Home/End move focus only; tabindex 0 stays on the selected tab.
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const all = Array.from(list.current?.querySelectorAll<HTMLElement>("[role='tab']") ?? []);
    const i = all.indexOf(e.target as HTMLElement);
    if (i < 0) return;
    const to =
      e.key === "ArrowRight" ? (i + 1) % all.length
      : e.key === "ArrowLeft" ? (i - 1 + all.length) % all.length
      : e.key === "Home" ? 0
      : e.key === "End" ? all.length - 1
      : -1;
    if (to < 0) return;
    // Handled here: the app's spatial-nav keydown handler skips prevented events.
    e.preventDefault();
    all[to].focus();
  };

  const adornment = (node: ReactNode) =>
    node == null ? null : (
      <span aria-hidden="true" className="shrink-0">
        {node}
      </span>
    );

  const bar = (
    // pb-1.5: the pills clear the separator under the bar. A panel below gets a hairline (the view bar's
    // separator is the app header's border).
    <div className={cx("flex w-full shrink-0 items-center gap-2 pb-1.5", hasPanel && "border-b border-hairline")}>
      {adornment(start)}
      <div
        ref={list}
        role="tablist"
        aria-label={label}
        data-nav-roving={focusable || undefined}
        onKeyDown={focusable ? onKeyDown : undefined}
        className={LIST}
      >
        {tabs.map((t) => {
          const selected = t.value === value;
          const panelId = hasPanel ? `${id}-panel-${t.value}` : controls;
          return (
            <button
              key={t.value}
              type="button"
              role="tab"
              data-tab={t.value}
              tabIndex={focusable && selected ? 0 : -1}
              id={`${id}-tab-${t.value}`}
              aria-selected={selected}
              aria-controls={panelId}
              // Non-focusable tabs stay out of the focus order on click too.
              onMouseDown={focusable ? undefined : (e) => e.preventDefault()}
              onFocus={(e) => e.currentTarget.scrollIntoView?.({ block: "nearest", inline: "nearest" })}
              onClick={() => select(t.value)}
              className={TAB}
            >
              <span aria-hidden="true" data-tab-highlight className={HIGHLIGHT} />
              <span className={CONTENT}>
                {t.icon}
                <span>{t.label}</span>
                {t.badge}
              </span>
            </button>
          );
        })}
      </div>
      {adornment(end)}
    </div>
  );

  if (!hasPanel) return bar;
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {bar}
      {/* The tab's contents are the only part that scrolls (the tabs, title and actions stay in place). */}
      <div
        ref={panel}
        data-scroll-pane
        role="tabpanel"
        id={`${id}-panel-${value}`}
        aria-labelledby={`${id}-tab-${value}`}
        className="mt-5 min-h-0 flex-1 overflow-y-auto overscroll-contain"
      >
        {children}
      </div>
    </div>
  );
}
