// SPDX-License-Identifier: GPL-3.0-or-later
// Shell: a two-row top header (row 1: brand, active game, deploy progress, account, Quit; row 2: the
// view tabs, LB/RB glyphs at their ends in controller mode), the current view (the Games library
// first) scrolling in <main> below it, and a bottom stack with notices and the job island. Prompt
// dialogs and the on-screen keyboard sit on top. Installs keyboard + gamepad nav.

import {
  BooksIcon,
  CompassIcon,
  GameControllerIcon,
  GearSixIcon,
  PowerIcon,
  PuzzlePieceIcon,
  SquaresFourIcon,
  StackIcon,
  TerminalWindowIcon,
} from "@phosphor-icons/react";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useState, type JSX, type ReactNode } from "react";
import vesselLogo from "../assets/vessel-logo.svg";
import { AddGameHost } from "../components/dialogs/AddGameDialog";
import { ViewBoundary } from "../components/shell/ErrorBoundary";
import { OskHost } from "../components/shell/Osk";
import { PromptHost } from "../components/dialogs/PromptDialog";
import { LoaderConsentHost } from "../components/loaders/LoaderConsent";
import { Button, JobIsland, Notice, PadGlyph, Tabs, cx, type TabItem } from "../components/ui";
import * as ipc from "../ipc/commands";
import { focusCurrentTab, installKeyboardNav, performNav } from "../nav/actions";
import { startGamepadLoop } from "../nav/gamepad";
import { installModalityTracking } from "../nav/modality";
import { installFocusRecovery } from "../nav/spatial";
import { Activity } from "../views/Activity";
import { Collections } from "../views/Collections";
import { Games } from "../views/Games";
import { Browse } from "../views/Browse";
import { Mods } from "../views/Mods";
import { HeroBackdrop, Overview } from "../views/Overview";
import { Requirements } from "../views/Requirements";
import { Settings } from "../views/Settings";
import { jobLabel, jobOutcome } from "./jobs";
import { useAccount, useActiveGame } from "./queries";
import { useDeploy } from "./run";
import { VIEWS, useUi, type JobEntry, type ViewId } from "./store";

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

// The selected tab's icon takes the accent (the tab is the Tabs `group`); dark on the white focus highlight.
const ICON_CLASS = "shrink-0 transition-colors duration-150 group-aria-selected:text-accent-300 in-highlight:text-accent-700";
const ICON: Record<ViewId, ReactNode> = {
  games: <GameControllerIcon aria-hidden size={20} weight="duotone" className={ICON_CLASS} />,
  overview: <SquaresFourIcon aria-hidden size={20} weight="duotone" className={ICON_CLASS} />,
  mods: <StackIcon aria-hidden size={20} weight="duotone" className={ICON_CLASS} />,
  browse: <CompassIcon aria-hidden size={20} weight="duotone" className={ICON_CLASS} />,
  collections: <BooksIcon aria-hidden size={20} weight="duotone" className={ICON_CLASS} />,
  requirements: <PuzzlePieceIcon aria-hidden size={20} weight="duotone" className={ICON_CLASS} />,
  activity: <TerminalWindowIcon aria-hidden size={20} weight="duotone" className={ICON_CLASS} />,
  settings: <GearSixIcon aria-hidden size={20} weight="duotone" className={ICON_CLASS} />,
};

/** id of <main>, the element the view tabs control. */
const VIEW_ID = "view";

/**
 * Bottom padding of the view: with nothing in the bottom stack it equals
 * the side padding (px-10); with notices or the island it also leaves room
 * (+ scroll-padding, also kept at the top) for the fixed stack (~82px per notice or island + 24px
 * offset), so a control scrolled into view is not under it.
 */
// --main-pb repeats each pb-* (Tailwind's own value for it): WebKit insets a sticky child by the scroll
// container's padding, so a bar pinned to the bottom (Browse's pager) offsets itself by it. Classes, not a
// style attribute: CSP.
const STACK_ROOM = [
  "pb-10 [--main-pb:calc(var(--spacing)*10)] scroll-pb-32",
  "pb-28 [--main-pb:calc(var(--spacing)*28)] scroll-pb-60",
  "pb-48 [--main-pb:calc(var(--spacing)*48)] scroll-pb-80",
  "pb-72 [--main-pb:calc(var(--spacing)*72)] scroll-pb-96",
  "pb-96 [--main-pb:calc(var(--spacing)*96)] scroll-pb-[30rem]",
];

/** How long a finished job stays in the island (legacy: 5 s). */
const ISLAND_LINGER = 5000;

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

  const noticeCount = useVisibleNotices().length;
  const island = useIslandJob();
  const View = VIEW[view];
  const stack = noticeCount + (island ? 1 : 0);
  return (
    <>
      {/* Dialogs render in a portal on <body> and make everything else inert while open. */}
      <div className="flex h-full flex-col">
        <TopBar />
        {/* The Overview's hero art sits behind the scrolling view, not in it: it stays put while the view scrolls. */}
        <div className="relative isolate min-h-0 min-w-0 flex-1">
          {view === "overview" && <HeroBackdrop />}
          {/* aria-label: the view's name (views show no visible page title). */}
          <main id={VIEW_ID} aria-label={VIEWS.find((v) => v.id === view)?.label ?? view} className={cx("relative h-full scroll-pt-32 overflow-y-auto", STACK_ROOM[stack] ?? STACK_ROOM[4])}>
            {/* key: each view enters with the short legacy fade/rise. */}
            <div key={view} className="mx-auto w-full max-w-content animate-rise px-10 pt-10">
              <ViewBoundary name={VIEWS.find((v) => v.id === view)?.label ?? view}>
                <View />
              </ViewBoundary>
            </div>
          </main>
        </div>
        <BottomStack island={island} />
        <AddGameHost />
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
 * active game (picked in the Games view; plain text, the Games tab is the way to switch) and its deploy
 * progress, the Nexus account, Quit. Row 2: the view tabs. Deploying is done from the Overview's
 * Deployment card.
 */
function TopBar() {
  return (
    // The side padding is on the header, so row 1 and the view tabs share one inset.
    <header aria-label="App" className="shrink-0 border-b border-white/5 bg-ink px-6 lg:px-10">
      <div className="flex min-w-0 items-center gap-4 pt-4 pb-2 lg:gap-6">
        <Brand />
        <span aria-hidden className="h-8 w-px shrink-0 bg-hairline" />
        <GameContext />
        <AccountChip />
        <button
          type="button"
          onClick={() => void ipc.quitApp()}
          className="inline-flex min-h-control shrink-0 items-center gap-2 rounded-pill px-4 text-body text-zinc-400 transition-colors duration-150"
        >
          <PowerIcon aria-hidden size={20} />
          <span>Quit</span>
        </button>
      </div>
      <ViewTabs />
    </header>
  );
}

function GameContext() {
  const { game, isLoading } = useActiveGame();
  // Progress of a deploy of another game is not shown under this one (the job island shows it).
  const progress = useUi((s) => (s.deployGame && s.deployGame !== game?.id ? null : s.deployProgress));
  return (
    <div className="flex min-w-0 flex-1 items-center gap-4">
      {isLoading ? (
        <span className="text-body text-zinc-400">Loading…</span>
      ) : game ? (
        <span className="min-w-0 truncate text-lead font-semibold tracking-tight text-zinc-100" title={game.name}>
          {game.name}
        </span>
      ) : (
        <span className="text-body text-zinc-400">No game selected</span>
      )}
      {progress && (
        <span className="flex shrink-0 items-center gap-2 font-mono text-micro whitespace-nowrap text-dim">
          <progress aria-label="Deploy progress" className="w-24" max={progress.total || 1} value={progress.done} />
          deploying {progress.done}/{progress.total}
        </span>
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
  const running = useUi((s) => Object.values(s.jobs).filter((j) => j.state === "started").length);
  const prompts = useUi((s) => s.prompts.length);
  const tabs: TabItem<ViewId>[] = VIEWS.map((v) => ({
    value: v.id,
    label: v.label,
    icon: ICON[v.id],
    badge:
      v.id === "activity" && running + prompts > 0 ? (
        <span className="min-w-4 rounded-pill bg-accent-400 px-1 text-center font-mono text-micro leading-4 font-semibold text-accent-950">{running + prompts}</span>
      ) : undefined,
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

/** The job shown in the island: the newest running one, else one that finished in the last few seconds. */
function useIslandJob(): JobEntry | null {
  const jobs = useUi((s) => s.jobs);
  const [, tick] = useState(0);
  const list = Object.values(jobs).sort((a, b) => b.id - a.id);
  const job = list.find((j) => j.state === "started") ?? list[0] ?? null;
  const left = job?.finishedAt ? ISLAND_LINGER - (Date.now() - job.finishedAt) : 0;
  useEffect(() => {
    if (!job?.finishedAt || left <= 0) return;
    const t = setTimeout(() => tick((n) => n + 1), left + 20);
    return () => clearTimeout(t);
  }, [job?.id, job?.finishedAt, left]);
  if (!job) return null;
  return job.state === "started" || left > 0 ? job : null;
}

/** Notices to show: a "Deploy now" notice belongs to its game and is hidden while another game is active. */
function useVisibleNotices() {
  const all = useUi((s) => s.notices);
  const { game } = useActiveGame();
  return all.filter((n) => !n.game || n.game === game?.id);
}

/** Fixed bottom stack centred under the content (legacy JobIsland position): notices, then the job island. */
function BottomStack({ island }: { island: JobEntry | null }) {
  const notices = useVisibleNotices();
  const dismiss = useUi((s) => s.dismiss);
  const runNoticeAction = useUi((s) => s.runNoticeAction);
  const holdNotice = useUi((s) => s.holdNotice);
  const deploy = useDeploy();
  if (!notices.length && !island) return null;
  return (
    <div className="pointer-events-none fixed inset-y-auto right-0 bottom-5 left-0 z-40 flex flex-col items-center gap-2 px-6">
      {notices.length > 0 && (
        <aside aria-label="Notices" aria-live="polite" className="grid w-full max-w-xl gap-2">
          {notices.map((n) => (
            <Notice
              key={n.id}
              tone={n.tone}
              act={n.action && { label: n.action.label, onAction: () => runNoticeAction(n.id) }}
              onHold={n.action && ((held) => holdNotice(n.id, held))}
              action={
                n.deploy && (
                  <Button
                    tone="primary"
                    size="sm"
                    onClick={() => {
                      dismiss(n.id);
                      void deploy(n.game);
                    }}
                  >
                    Deploy now
                  </Button>
                )
              }
            >
              {n.text}
            </Notice>
          ))}
        </aside>
      )}
      {island && (
        <aside aria-label="Job status" className="flex w-full justify-center">
          <CurrentJob job={island} />
        </aside>
      )}
    </div>
  );
}

/**
 * The island job's pill: its latest output line while running, then
 * "Finished"/"Failed" for a few seconds. The full output is in Activity.
 */
function CurrentJob({ job }: { job: JobEntry }) {
  const last = useUi((s) => {
    for (let i = s.activity.length - 1; i >= 0; i--) {
      const a = s.activity[i];
      if (a.job === job.id && a.text.trim()) return a.text.trim();
    }
    return null;
  });
  const progress = useUi((s) => (s.deployProgress?.job === job.id ? s.deployProgress : null));
  const queued = useUi((s) => Object.values(s.jobs).filter((j) => j.state === "started" && j.id !== job.id).length);
  const outcome = jobOutcome(job);
  const detail =
    outcome === "running"
      ? progress
        ? `deploying ${progress.done}/${progress.total}`
        : (last ?? "Starting…")
      : outcome === "failed"
        ? `Failed${job.error ? `: ${job.error}` : ""} · see Activity`
        : outcome === "cancelled"
          ? "Cancelled"
          : "Finished";
  return <JobIsland title={jobLabel(job.kind)} detail={detail} outcome={outcome} queued={queued} />;
}
