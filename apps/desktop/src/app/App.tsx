// SPDX-License-Identifier: GPL-3.0-or-later
// Shell: tab rail (7 views), active-game header, the current view, notices,
// prompt dialogs and the on-screen keyboard. Installs keyboard + gamepad nav.

import { useEffect, type JSX } from "react";
import { OskHost } from "../components/Osk";
import { PromptHost } from "../components/PromptDialog";
import { Button, SupportBadge, cx } from "../components/ui";
import { installKeyboardNav, performNav } from "../nav/actions";
import { startGamepadLoop } from "../nav/gamepad";
import { Activity } from "../views/Activity";
import { Collections } from "../views/Collections";
import { Install } from "../views/Install";
import { Mods } from "../views/Mods";
import { Overview } from "../views/Overview";
import { Requirements } from "../views/Requirements";
import { Settings } from "../views/Settings";
import { useActiveGame } from "./queries";
import { useBusy, useDeploy } from "./run";
import { VIEWS, useUi, type ViewId } from "./store";

const VIEW: Record<ViewId, () => JSX.Element> = {
  overview: Overview,
  mods: Mods,
  install: Install,
  collections: Collections,
  requirements: Requirements,
  activity: Activity,
  settings: Settings,
};

export function App() {
  const view = useUi((s) => s.view);
  const modal = useUi((s) => s.prompts.length > 0 || s.osk !== null);

  useEffect(() => {
    const stopKeys = installKeyboardNav();
    const stopPad = startGamepadLoop((a) => performNav(a));
    // Start on the tab rail so arrows/D-pad work right away.
    document.querySelector<HTMLElement>("[data-tab][aria-current='page']")?.focus();
    return () => {
      stopKeys();
      stopPad();
    };
  }, []);

  const View = VIEW[view];
  return (
    <>
      <div inert={modal} className="flex h-full">
        <TabRail />
        <div className="flex min-w-0 flex-1 flex-col">
          <GameHeader />
          <main className="min-h-0 flex-1 overflow-y-auto px-8 pt-6 pb-24">
            <View />
          </main>
        </div>
        <Notices />
      </div>
      <PromptHost />
      <OskHost />
    </>
  );
}

function TabRail() {
  const view = useUi((s) => s.view);
  const setView = useUi((s) => s.setView);
  const running = useUi((s) => Object.values(s.jobs).filter((j) => j.state === "started").length);
  const prompts = useUi((s) => s.prompts.length);
  return (
    <nav aria-label="Views" className="flex w-56 shrink-0 flex-col gap-1 border-r border-white/[0.06] bg-ink-2 p-3">
      <div className="px-3 pt-2 pb-4 text-xl font-semibold tracking-tight text-zinc-50">modman</div>
      {VIEWS.map((v) => (
        <button
          key={v.id}
          type="button"
          data-tab={v.id}
          aria-current={v.id === view ? "page" : undefined}
          onClick={() => setView(v.id)}
          className={cx(
            "flex min-h-12 items-center justify-between rounded-xl px-4 text-left text-base",
            v.id === view ? "bg-accent-950 text-accent-200" : "text-zinc-300 hover:bg-white/5",
          )}
        >
          {v.label}
          {v.id === "activity" && running + prompts > 0 && (
            <span className="rounded-full bg-accent-500 px-2 text-sm text-ink">{running + prompts}</span>
          )}
        </button>
      ))}
      <p className="mt-auto px-3 text-xs leading-relaxed text-zinc-500">
        LB/RB or PgUp/PgDn switch tabs · A/Enter select · B/Esc back
      </p>
    </nav>
  );
}

function GameHeader() {
  const { game, isLoading } = useActiveGame();
  const needsDeploy = useUi((s) => s.needsDeploy);
  const progress = useUi((s) => s.deployProgress);
  const deploy = useDeploy();
  const busy = useBusy("Deploy");
  return (
    <header className="flex min-h-16 flex-wrap items-center gap-3 border-b border-white/[0.06] px-8 py-3">
      {game ? (
        <>
          <span className="text-lg font-medium text-zinc-100">{game.name}</span>
          <SupportBadge support={game.support} />
          {progress && (
            <span className="flex w-56 items-center gap-2 text-sm text-zinc-400">
              <progress aria-label="Deploy progress" max={progress.total || 1} value={progress.done} />
              {progress.done}/{progress.total}
            </span>
          )}
          {needsDeploy && !progress && (
            <Button tone="primary" className="ml-auto" busy={busy} onClick={deploy}>
              Deploy changes
            </Button>
          )}
        </>
      ) : (
        <span className="text-zinc-400">{isLoading ? "Loading…" : "No game selected"}</span>
      )}
    </header>
  );
}

const NOTICE_TONE = {
  info: "border-white/10",
  success: "border-accent-500/40",
  warn: "border-amber-400/40",
  error: "border-rose-400/50",
} as const;

function Notices() {
  const notices = useUi((s) => s.notices);
  const dismiss = useUi((s) => s.dismiss);
  const deploy = useDeploy();
  if (!notices.length) return null;
  return (
    <aside aria-label="Notices" aria-live="polite" className="fixed right-6 bottom-6 z-40 grid w-[26rem] gap-2">
      {notices.map((n) => (
        <div key={n.id} role={n.tone === "error" ? "alert" : "status"} className={cx("surface flex items-center gap-3 border p-3 pl-4", NOTICE_TONE[n.tone])}>
          <p className="min-w-0 flex-1 text-sm break-words text-zinc-200">{n.text}</p>
          {n.deploy && (
            <Button
              tone="primary"
              onClick={() => {
                dismiss(n.id);
                void deploy();
              }}
            >
              Deploy now
            </Button>
          )}
          <Button tone="ghost" aria-label="Dismiss notice" onClick={() => dismiss(n.id)}>
            ✕
          </Button>
        </div>
      ))}
    </aside>
  );
}
