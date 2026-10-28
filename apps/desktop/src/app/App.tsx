// SPDX-License-Identifier: GPL-3.0-or-later
// Shell in the legacy layout: left sidebar (brand + active game header, the
// tab rail with icons, account), the current view (the Games library first),
// and a bottom stack with notices and the job island. Prompt dialogs and the on-screen keyboard sit
// on top. Installs keyboard + gamepad nav.

import {
  BooksIcon,
  CheckCircleIcon,
  CheckIcon,
  DownloadSimpleIcon,
  GameControllerIcon,
  GearSixIcon,
  InfoIcon,
  PowerIcon,
  PuzzlePieceIcon,
  SquaresFourIcon,
  StackIcon,
  TerminalWindowIcon,
  WarningIcon,
  XCircleIcon,
  XIcon,
} from "@phosphor-icons/react";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useState, type JSX, type ReactNode } from "react";
import { AddGameHost } from "../components/AddGameDialog";
import { ViewBoundary } from "../components/ErrorBoundary";
import { OskHost } from "../components/Osk";
import { PromptHost } from "../components/PromptDialog";
import { Button, IconButton, StatusDot, SupportBadge, cx, type DotTone } from "../components/ui";
import * as ipc from "../ipc/commands";
import { installKeyboardNav, performNav } from "../nav/actions";
import { startGamepadLoop } from "../nav/gamepad";
import { installModalityTracking } from "../nav/modality";
import { installFocusRecovery } from "../nav/spatial";
import { Activity } from "../views/Activity";
import { Collections } from "../views/Collections";
import { Games } from "../views/Games";
import { Install } from "../views/Install";
import { Mods } from "../views/Mods";
import { HeroBackdrop, Overview } from "../views/Overview";
import { Requirements } from "../views/Requirements";
import { Settings } from "../views/Settings";
import { jobLabel, jobOutcome } from "./jobs";
import { useAccount, useActiveGame } from "./queries";
import { useDeploy, useDeployDue } from "./run";
import { VIEWS, useUi, type JobEntry, type NoticeTone, type ViewId } from "./store";

const VIEW: Record<ViewId, () => JSX.Element> = {
  games: Games,
  overview: Overview,
  mods: Mods,
  install: Install,
  collections: Collections,
  requirements: Requirements,
  activity: Activity,
  settings: Settings,
};

const ICON: Record<ViewId, ReactNode> = {
  games: <GameControllerIcon aria-hidden size={20} weight="duotone" />,
  overview: <SquaresFourIcon aria-hidden size={20} weight="duotone" />,
  mods: <StackIcon aria-hidden size={20} weight="duotone" />,
  install: <DownloadSimpleIcon aria-hidden size={20} weight="duotone" />,
  collections: <BooksIcon aria-hidden size={20} weight="duotone" />,
  requirements: <PuzzlePieceIcon aria-hidden size={20} weight="duotone" />,
  activity: <TerminalWindowIcon aria-hidden size={20} weight="duotone" />,
  settings: <GearSixIcon aria-hidden size={20} weight="duotone" />,
};

/**
 * Bottom padding of the view: with nothing in the bottom stack it equals
 * the side padding (px-10); with notices or the island it also leaves room
 * (+ scroll-padding, also kept at the top) for the fixed stack (~82px per notice or island + 24px
 * offset), so a control scrolled into view is not under it.
 */
const STACK_ROOM = ["pb-10 scroll-pb-32", "pb-28 scroll-pb-60", "pb-48 scroll-pb-80", "pb-72 scroll-pb-96", "pb-96 scroll-pb-[30rem]"];

/** How long a finished job stays in the island (legacy: 5 s). */
const ISLAND_LINGER = 5000;

export function App() {
  const view = useUi((s) => s.view);

  useEffect(() => {
    const stopKeys = installKeyboardNav();
    const stopPad = startGamepadLoop((a) => performNav(a));
    const stopRecovery = installFocusRecovery();
    const stopModality = installModalityTracking();
    // Start on the tab rail so arrows/D-pad work right away.
    document.querySelector<HTMLElement>("[data-tab][aria-current='page']")?.focus();
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
      <div className="flex h-full">
        <Sidebar />
        {/* The Overview's hero art sits behind the scrolling view, not in it: it stays put while the view scrolls. */}
        <div className="relative isolate min-h-0 min-w-0 flex-1">
          {view === "overview" && <HeroBackdrop />}
          <main className={cx("relative h-full scroll-pt-32 overflow-y-auto", STACK_ROOM[stack] ?? STACK_ROOM[4])}>
            {/* key: each view enters with the short legacy fade/rise. */}
            <div key={view} className="mx-auto w-full max-w-[1400px] animate-rise px-10 pt-10">
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
      <OskHost />
    </>
  );
}

function Brand() {
  const info = useQuery({ queryKey: ["appinfo"], queryFn: ipc.appInfo });
  return (
    <div className="flex items-center gap-3">
      <svg viewBox="0 0 32 32" className="size-8 shrink-0" aria-hidden>
        <rect width="32" height="32" rx="9" className="fill-ink-3" />
        <path
          d="M8 22V10l8 7 8-7v12"
          fill="none"
          className="stroke-accent-400"
          strokeWidth="2.6"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
      <div className="leading-tight">
        <div className="text-[15px] font-semibold tracking-tight text-zinc-50">modman</div>
        <div className="font-mono text-[10.5px] text-dim">v{info.data?.version ?? (info.isError ? "?" : "…")}</div>
      </div>
    </div>
  );
}

function Sidebar() {
  return (
    // Scrolls by itself on short windows, so the app never grows past the viewport.
    <aside
      aria-label="Sidebar"
      className="flex h-full min-h-0 w-[268px] shrink-0 flex-col overflow-y-auto border-r border-white/[0.05] bg-ink"
    >
      <GameHeader />
      <TabRail />
      <div data-nav-rail className="mt-auto px-4 pb-4">
        <button
          type="button"
          onClick={() => void ipc.quitApp()}
          className="flex min-h-12 w-full items-center gap-3 rounded-xl px-3 text-left text-[14.5px] text-zinc-400 transition-colors duration-150 hover:bg-white/[0.03] hover:text-zinc-200"
        >
          <PowerIcon aria-hidden size={20} />
          <span className="flex-1">Quit</span>
        </button>
      </div>
      <AccountFooter />
    </aside>
  );
}

/**
 * Brand + the active game (picked in the Games view) and its deploy
 * progress. Deploying is done from the Overview's Deployment card. Part of
 * the rail for spatial navigation: Down from here reaches the tabs, not the view.
 */
function GameHeader() {
  const { game, isLoading } = useActiveGame();
  const needsDeploy = useDeployDue(game?.domain ?? null);
  // Progress of a deploy of another game is not shown under this one (the job island shows it).
  const progress = useUi((s) => (s.deployGame && s.deployGame !== game?.domain ? null : s.deployProgress));
  const d = game?.deploy;
  const tone: DotTone = !d?.active ? "zinc" : d.interrupted ? "rose" : needsDeploy ? "amber" : "accent";
  return (
    <header className="grid grid-cols-[minmax(0,1fr)] gap-5 px-6 pt-6 pb-4">
      <Brand />
      <div data-nav-rail className="grid grid-cols-[minmax(0,1fr)] gap-2">
        {isLoading ? (
          <span className="text-[14px] text-zinc-400">Loading…</span>
        ) : game ? (
          <div className="flex min-h-12 min-w-0 items-center gap-3 rounded-xl border border-white/[0.06] bg-ink-2 px-3">
            <StatusDot tone={tone} live={game.deploy.active} />
            <span className="min-w-0 flex-1 truncate text-[14.5px] text-zinc-50" title={game.name}>
              {game.name}
            </span>
            <span className="font-mono text-[11px] text-dim" title={`${game.mod_count} mods`}>
              {game.mod_count}
            </span>
          </div>
        ) : (
          <span className="text-[14px] text-zinc-400">No game selected</span>
        )}
        {game && <SupportBadge support={game.support} />}
        {progress && (
          <span className="grid gap-1.5 pt-1 font-mono text-[11px] text-dim">
            <progress aria-label="Deploy progress" max={progress.total || 1} value={progress.done} />
            deploying {progress.done}/{progress.total}
          </span>
        )}
      </div>
    </header>
  );
}

function TabRail() {
  const view = useUi((s) => s.view);
  const setView = useUi((s) => s.setView);
  const running = useUi((s) => Object.values(s.jobs).filter((j) => j.state === "started").length);
  const prompts = useUi((s) => s.prompts.length);
  return (
    <nav aria-label="Views" data-nav-rail className="grid gap-0.5 px-4">
      {VIEWS.map((v) => {
        const on = v.id === view;
        return (
          <button
            key={v.id}
            type="button"
            data-tab={v.id}
            aria-current={on ? "page" : undefined}
            onClick={() => setView(v.id)}
            className={cx(
              "flex min-h-12 w-full items-center gap-3 rounded-xl px-3 text-left text-[14.5px] transition-colors duration-150",
              on ? "bg-white/[0.07] text-zinc-50" : "text-zinc-400 hover:bg-white/[0.03] hover:text-zinc-200",
            )}
          >
            <span className={cx("transition-colors duration-150", on && "text-accent-300")}>{ICON[v.id]}</span>
            <span className="flex-1">{v.label}</span>
            {v.id === "activity" && running + prompts > 0 && (
              <span className="rounded-full bg-accent-400 px-2 font-mono text-[11px] font-semibold text-accent-950">
                {running + prompts}
              </span>
            )}
          </button>
        );
      })}
    </nav>
  );
}

function AccountFooter() {
  const q = useAccount();
  const a = q.data;
  const signedIn = !!a?.key_source;
  return (
    <div className="grid gap-4 border-t border-white/[0.05] px-6 py-5">
      {q.isError ? (
        <p className="text-[13px] text-rose-300">Could not read the Nexus account. See Settings.</p>
      ) : signedIn ? (
        <div className="flex items-center gap-3">
          <div
            aria-hidden
            className="grid size-9 place-items-center rounded-xl bg-accent-950 font-mono text-[12px] font-semibold text-accent-50"
          >
            {(a?.user ?? "?").slice(0, 2).toUpperCase()}
          </div>
          <div className="min-w-0 leading-tight">
            <div className="truncate text-[13.5px] text-zinc-200">{a?.user ?? "Signed in"}</div>
            <div className="font-mono text-[10.5px] text-dim">{a?.premium ? "Premium" : "Free account"}</div>
          </div>
        </div>
      ) : (
        <p className="text-[13px] text-zinc-400">Not signed in to Nexus Mods. See Settings.</p>
      )}
      <p className="font-mono text-[10.5px] leading-relaxed text-dim">LB/RB or PgUp/PgDn switch tabs · A/Enter select · B/Esc back</p>
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

const NOTICE_TONE: Record<NoticeTone, { border: string; icon: ReactNode }> = {
  info: { border: "border-white/10", icon: <InfoIcon aria-hidden size={18} weight="duotone" className="text-zinc-300" /> },
  success: {
    border: "border-accent-400/25",
    icon: <CheckCircleIcon aria-hidden size={18} weight="duotone" className="text-accent-300" />,
  },
  warn: { border: "border-amber-400/30", icon: <WarningIcon aria-hidden size={18} weight="duotone" className="text-amber-300" /> },
  error: { border: "border-rose-400/40", icon: <XCircleIcon aria-hidden size={18} weight="duotone" className="text-rose-300" /> },
};

/** Notices to show: a "Deploy now" notice belongs to its game and is hidden while another game is active. */
function useVisibleNotices() {
  const all = useUi((s) => s.notices);
  const { game } = useActiveGame();
  return all.filter((n) => !n.game || n.game === game?.domain);
}

/** Fixed bottom stack centred under the content (legacy JobIsland position): notices, then the job island. */
function BottomStack({ island }: { island: JobEntry | null }) {
  const notices = useVisibleNotices();
  const dismiss = useUi((s) => s.dismiss);
  const deploy = useDeploy();
  if (!notices.length && !island) return null;
  return (
    <div className="pointer-events-none fixed inset-y-auto right-0 bottom-5 left-[268px] z-40 flex flex-col items-center gap-2 px-6">
      {notices.length > 0 && (
        <aside aria-label="Notices" aria-live="polite" className="grid w-full max-w-xl gap-2">
          {notices.map((n) => (
            <div
              key={n.id}
              role={n.tone === "error" ? "alert" : "status"}
              className={cx(
                "glass pointer-events-auto flex animate-island items-center gap-3 rounded-2xl py-2 pr-2 pl-4",
                NOTICE_TONE[n.tone].border,
              )}
            >
              {NOTICE_TONE[n.tone].icon}
              <p className="min-w-0 flex-1 text-[13.5px] break-words text-zinc-100">{n.text}</p>
              {n.deploy && (
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
              )}
              <IconButton label="Dismiss notice" onClick={() => dismiss(n.id)}>
                <XIcon aria-hidden size={16} />
              </IconButton>
            </div>
          ))}
        </aside>
      )}
      {island && (
        <aside aria-label="Job status" className="flex w-full justify-center">
          <JobIsland job={island} />
        </aside>
      )}
    </div>
  );
}

/**
 * Legacy dynamic-island status pill: the running job and its latest line,
 * then "Finished"/"Failed" for a few seconds. Status only (not focusable):
 * the full output is in Activity.
 */
function JobIsland({ job }: { job: JobEntry }) {
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
  const running = outcome === "running";
  const failed = outcome === "failed";
  return (
    <div
      role="status"
      aria-label="Current job"
      className="glass pointer-events-auto relative flex min-h-14 w-full max-w-xl animate-island items-center gap-3 overflow-hidden rounded-full py-2 pr-5 pl-2.5"
    >
      <span
        aria-hidden
        className={cx(
          "relative grid size-9 shrink-0 place-items-center rounded-full",
          failed
            ? "bg-rose-400/15 text-rose-300"
            : outcome === "cancelled"
              ? "bg-white/[0.08] text-zinc-300"
              : "bg-accent-400/15 text-accent-300",
        )}
      >
        {running ? (
          <span className="flex gap-[3px]">
            <span className="h-3.5 w-[3px] animate-bar rounded-full bg-current" />
            <span className="h-3.5 w-[3px] animate-bar rounded-full bg-current [animation-delay:150ms]" />
            <span className="h-3.5 w-[3px] animate-bar rounded-full bg-current [animation-delay:300ms]" />
          </span>
        ) : failed ? (
          <WarningIcon size={17} weight="bold" />
        ) : outcome === "cancelled" ? (
          <XIcon size={17} weight="bold" />
        ) : (
          <CheckIcon size={17} weight="bold" />
        )}
      </span>
      <span className="relative min-w-0 flex-1">
        <span className="block truncate text-[13.5px] font-medium text-zinc-100">{jobLabel(job.kind)}</span>
        <span className="block truncate font-mono text-[11px] text-dim">
          {running
            ? progress
              ? `deploying ${progress.done}/${progress.total}`
              : (last ?? "Starting…")
            : failed
              ? `Failed${job.error ? `: ${job.error}` : ""} · see Activity`
              : outcome === "cancelled"
                ? "Cancelled"
                : "Finished"}
        </span>
      </span>
      {queued > 0 && <span className="relative font-mono text-[11px] text-zinc-400">+{queued}</span>}
    </div>
  );
}
