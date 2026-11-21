// SPDX-License-Identifier: GPL-3.0-or-later
// Shell: a two-row top header (row 1: brand, active game, toast slot, account, the Start-menu hint in
// controller mode; row 2: the
// view tabs, LB/RB glyphs at their ends in controller mode), the current view (the Games library
// first) scrolling in <main> below it; the one toast (results and job progress) in row 1's toast slot. Prompt
// dialogs, the controller Start menu and the on-screen keyboard sit on top. Installs keyboard + gamepad nav.

import {
  BooksIcon,
  CompassIcon,
  GameControllerIcon,
  GearSixIcon,
  PuzzlePieceIcon,
  SquaresFourIcon,
  StackIcon,
  TerminalWindowIcon,
  type Icon,
} from "@phosphor-icons/react";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useState, type JSX } from "react";
import vesselLogo from "../assets/vessel-logo.svg";
import { AddGameHost } from "../components/dialogs/AddGameDialog";
import { ViewBoundary } from "../components/shell/ErrorBoundary";
import { OskHost } from "../components/shell/Osk";
import { PromptHost } from "../components/dialogs/PromptDialog";
import { StartMenuHost } from "../components/dialogs/StartMenu";
import { LoaderConsentHost } from "../components/loaders/LoaderConsent";
import { Notice, PadGlyph, TOAST_CLOSE_MS, Tabs, cx, type TabItem } from "../components/ui";
import * as ipc from "../ipc/commands";
import { focusCurrentTab, installKeyboardNav, performNav } from "../nav/actions";
import { startGamepadLoop } from "../nav/gamepad";
import { installModalityTracking, isPad } from "../nav/modality";
import { focusables, installFocusRecovery, navScope, padSkipped } from "../nav/spatial";
import { Activity } from "../views/Activity";
import { Collections } from "../views/Collections";
import { Games } from "../views/Games";
import { Browse } from "../views/Browse";
import { Mods } from "../views/Mods";
import { HeroBackdrop, Overview } from "../views/Overview";
import { Requirements } from "../views/Requirements";
import { Settings } from "../views/Settings";
import { useAccount, useActiveGame } from "./queries";
import { useToast, type Toast } from "./toast";
import { VIEWS, useUi, type ViewId } from "./store";

const VIEW: Record<ViewId, () => JSX.Element> = {
  games: Games,
  overview: Overview,
  mods: Mods,
  browse: Browse,
  collections: Collections,
  requirements: Requirements,
  activity: Activity,
  settings: Settings,
};

// View tab icons; Tabs sizes and colours them.
const ICON: Record<ViewId, Icon> = {
  games: GameControllerIcon,
  overview: SquaresFourIcon,
  mods: StackIcon,
  browse: CompassIcon,
  collections: BooksIcon,
  requirements: PuzzlePieceIcon,
  activity: TerminalWindowIcon,
  settings: GearSixIcon,
};

/** id of <main>, the element the view tabs control. */
const VIEW_ID = "view";

/**
 * Bottom padding of the view: the side padding (px-10). Nothing floats over the bottom any more (toasts
 * and the job island are in the app bar), so it never grows.
 */
// --main-pb repeats pb-10 (Tailwind's own value for it): WebKit insets a sticky child by the scroll
// container's padding, so a bar pinned to the bottom (Browse's pager) offsets itself by it. Classes, not a
// style attribute: CSP.
const MAIN_PAD_BOTTOM = "pb-10 [--main-pb:calc(var(--spacing)*10)] scroll-pb-32";

export function App() {
  const view = useUi((s) => s.view);

  useEffect(() => {
    const stopKeys = installKeyboardNav();
    const stopPad = startGamepadLoop((a) => performNav(a));
    const stopRecovery = installFocusRecovery();
    const stopModality = installModalityTracking();
    // Start on the selected view tab so arrows/D-pad work right away.
    focusCurrentTab();
    return () => {
      stopKeys();
      stopPad();
      stopRecovery();
      stopModality();
    };
  }, []);

  // Controller mode: LB/RB (or openView) cannot park focus on the view tabs (not focusable there), so
  // focus lost with the old view enters the new one. A dialog keeps its own focus.
  useEffect(() => {
    if (!isPad() || navScope() !== document.body) return;
    const a = document.activeElement;
    if (a && a !== document.body && a.isConnected && !padSkipped(a)) return;
    const main = document.getElementById(VIEW_ID);
    (main && focusables(main)[0])?.focus();
  }, [view]);

  const View = VIEW[view];
  return (
    <>
      {/* Dialogs render in a portal on <body> and make everything else inert while open. */}
      <div className="flex h-full flex-col">
        <TopBar />
        {/* The Overview's hero art sits behind the scrolling view, not in it: it stays put while the view scrolls. */}
        <div className="relative isolate min-h-0 min-w-0 flex-1">
          {view === "overview" && <HeroBackdrop />}
          {/* aria-label: the view's name (views show no visible page title). */}
          <main id={VIEW_ID} aria-label={VIEWS.find((v) => v.id === view)?.label ?? view} className={cx("relative h-full scroll-pt-32 overflow-y-auto", MAIN_PAD_BOTTOM)}>
            {/* key: each view enters with the short legacy fade/rise. */}
            <div key={view} className="mx-auto w-full max-w-content animate-rise px-10 pt-10">
              <ViewBoundary name={VIEWS.find((v) => v.id === view)?.label ?? view}>
                <View />
              </ViewBoundary>
            </div>
          </main>
        </div>
        <AddGameHost />
        <StartMenuHost />
      </div>
      <PromptHost />
      <LoaderConsentHost />
      <OskHost />
    </>
  );
}

function Brand() {
  const info = useQuery({ queryKey: ["appinfo"], queryFn: ipc.appInfo });
  return (
    <div className="flex items-center gap-3">
      <img src={vesselLogo} alt="" aria-hidden className="size-8 shrink-0" />
      <div className="leading-tight">
        <div className="text-lead font-semibold tracking-tight text-zinc-50">Vessel</div>
        <div className="font-mono text-[10.5px] text-dim">v{info.data?.version ?? (info.isError ? "?" : "…")}</div>
      </div>
    </div>
  );
}

/**
 * The app header, above (not in) the scrolling <main>, so it never scrolls away. Row 1: brand, the
 * active game (picked in the Games view; plain text, the Games tab is the way to switch), the toast slot
 * (the only progress indicator: running jobs show there live), the Nexus account, and in controller mode the hint that Start opens the menu (Deploy, Verify, Purge,
 * Quit; keyboard/mouse users close the window instead). Row 2: the view tabs. Deploying is done from the
 * Overview's Deployment card or the Start menu.
 */
function TopBar() {
  return (
    // The side padding is on the header, so row 1 and the view tabs share one inset.
    // Row 1 is a grid so nothing overlaps: the game column shrinks (truncating the name) and the toast slot
    // takes the free space between it and the account (its 1fr column stays even when the slot is empty), never
    // less than one control width (48px: a disc-only toast). Toasts are h-control and the row is at least
    // h-control tall (auto-rows), so row 1 has one height with or without a toast, at every width.
    <header aria-label="App" className="shrink-0 border-b border-white/5 bg-ink px-6 lg:px-10">
      <div className="grid grid-cols-[auto_auto_minmax(0,max-content)_minmax(var(--spacing-control),1fr)_auto_auto] auto-rows-[minmax(var(--spacing-control),auto)] items-center gap-x-4 gap-y-2 pt-4 pb-2 lg:gap-x-6">
        <Brand />
        <span aria-hidden className="h-8 w-px shrink-0 bg-hairline" />
        <GameContext />
        <ToastSlot />
        {/* Fixed columns: the account and the Start hint always end the row. */}
        <div className="col-start-5 flex min-w-0 justify-end">
          <AccountChip />
        </div>
        {/* Controller mode only (pad:), like the LB/RB glyphs: not a control, gamepad Start opens the menu. */}
        <p data-start-hint className="col-start-6 hidden h-control shrink-0 items-center gap-2 text-label text-zinc-400 pad:inline-flex">
          <PadGlyph>Start</PadGlyph>
          <span>Menu</span>
        </p>
      </div>
      <ViewTabs />
    </header>
  );
}

function GameContext() {
  const { game, isLoading } = useActiveGame();
  return (
    <div className="flex min-w-0 items-center gap-4">
      {isLoading ? (
        <span className="text-body text-zinc-400">Loading…</span>
      ) : game ? (
        <span className="max-w-64 min-w-0 truncate text-lead font-semibold tracking-tight text-zinc-100" title={game.name}>
          {game.name}
        </span>
      ) : (
        <span className="text-body text-zinc-400">No game selected</span>
      )}
    </div>
  );
}

function AccountChip() {
  const q = useAccount();
  const a = q.data;
  if (q.isError) return <p className="max-w-56 shrink-0 text-label text-rose-300">Could not read the Nexus account. See Settings.</p>;
  if (!a?.key_source) return <p className="max-w-56 shrink-0 text-label text-zinc-400">Not signed in to Nexus Mods. See Settings.</p>;
  return (
    <div className="flex max-w-56 min-w-0 shrink-0 items-center gap-3">
      <div
        aria-hidden
        className="grid size-8 shrink-0 place-items-center rounded-pill bg-accent-950 font-mono text-caption font-semibold text-accent-50"
      >
        {(a.user ?? "?").slice(0, 2).toUpperCase()}
      </div>
      <div className="min-w-0 leading-tight">
        <div className="truncate text-[13.5px] text-zinc-200">{a.user ?? "Signed in"}</div>
        <div className="font-mono text-[10.5px] text-dim">{a.premium ? "Premium" : "Free account"}</div>
      </div>
    </div>
  );
}

/**
 * The view tabs: one line over the full width (tighter padding first, then a horizontal scroll on narrow
 * windows). A roving-tabindex tablist: the selected tab is the stop; arrows/D-pad move between tabs,
 * A/Enter/click activate; LB/RB and PageUp/PageDown select directly (performNav "tab").
 */
function ViewTabs() {
  const view = useUi((s) => s.view);
  const setView = useUi((s) => s.setView);
  const tabs: TabItem<ViewId>[] = VIEWS.map((v) => ({
    value: v.id,
    label: v.label,
    icon: ICON[v.id],
  }));
  return (
    // data-view-tabs = VIEW_TABS_ATTR: scopes the shell's tab lookups (nav/actions.ts) to these tabs.
    <div data-view-tabs>
      <Tabs
        label="Views"
        focusable
        tabs={tabs}
        value={view}
        onChange={setView}
        controls={VIEW_ID}
        start={<PadGlyph>LB</PadGlyph>}
        end={<PadGlyph>RB</PadGlyph>}
      />
    </div>
  );
}

/**
 * The toast slot in the app bar, between the game and the account, centred. It shows the one toast
 * (app/toast.ts): whoever wrote last, a command result or a running job's progress, replaces it at once and
 * restarts its timer; there is no queue and no second pill. The slot is an @container: the pill shrinks with it
 * and truncates; below @3xs (16rem) the pill shows its disc only (text sr-only, full text in its tooltip). It
 * never leaves row 1 or adds height.
 */
/**
 * Presentational only: keeps the last toast mounted for TOAST_CLOSE_MS after it closes, so it can collapse
 * (reduced motion: 0 ms, it goes at once). A write during that time takes over the same element.
 */
function useClosingToast(toast: Toast | null): { shown: Toast | null; closing: boolean } {
  const [last, setLast] = useState<Toast | null>(toast);
  useEffect(() => {
    if (toast) return setLast(toast);
    const reduced = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const t = setTimeout(() => setLast(null), reduced ? 0 : TOAST_CLOSE_MS);
    return () => clearTimeout(t);
  }, [toast]);
  return { shown: toast ?? last, closing: !toast && !!last };
}

function ToastSlot() {
  const { shown: t, closing } = useClosingToast(useToast());
  return (
    <div data-toast-slot className="@container flex min-w-0 justify-center">
      {/* Always mounted: a live region must exist before the text it announces. */}
      <aside aria-label="Notices" aria-live="polite" className={cx("flex min-w-0 justify-center", t && "w-full")}>
        {/* No key: a new write swaps the content of the same pill (no second pill, no re-entry animation): the
            open motion plays only when the slot goes from empty to a toast, later writes cycle the text. */}
        {t && (
          <Notice tone={t.tone} detail={t.detail} busy={t.busy} cycleKey={t.persist ?? t.id} closing={closing}>
            {t.text}
          </Notice>
        )}
      </aside>
    </div>
  );
}
