// SPDX-License-Identifier: GPL-3.0-or-later
// Overview of the active game: play, deployment (deploy/purge/verify), mods,
// requirements, recently installed mods, plugins (Bethesda games) and the
// start-up auto-update check. Games are picked and added in the Games view.

import {
  ArrowRightIcon,
  ArrowsClockwiseIcon,
  BroomIcon,
  PlayIcon,
  RocketLaunchIcon,
  ShieldCheckIcon,
  TrashIcon,
} from "@phosphor-icons/react";
import { useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { errMsg } from "../app/bridge";
import { useActiveGame, useMods } from "../app/queries";
import { useBusy, useDeploy, useDeployDue, useRun } from "../app/run";
import { useUi, type ViewId } from "../app/store";
import {
  Badge,
  Button,
  Card,
  Empty,
  ErrorNote,
  StatusDot,
  SupportBadge,
  Thumb,
  ago,
  cx,
  when,
} from "../components/ui";
import * as ipc from "../ipc/commands";
import type { GameView, ModView, PurgeReport, ReqRow, Skip, VerifyReport } from "../ipc/types";
import { NoGame } from "../components/NoGame";

/** How many mods "Recently installed" lists. */
const RECENT = 5;

export function Overview() {
  const { game, data: games, error } = useActiveGame();
  return (
    // minmax(0, 1fr): long unbroken names truncate inside the cards instead of widening the page.
    <div className="grid grid-cols-[minmax(0,1fr)] gap-6">
      {/* The legacy Overview has no big page title: the game name leads the hero. */}
      <h1 className="eyebrow">Overview</h1>
      <StartupCheckCard />
      {error ? (
        <ErrorNote>Could not load games: {errMsg(error)}</ErrorNote>
      ) : game ? (
        // key: per-game results (Verify) do not carry over to another game.
        <GameCards key={game.domain} game={game} />
      ) : (
        <NoGame title={games?.length ? "No game selected" : "No game yet"}>
          {games?.length
            ? "Pick a game in Games, or add another one."
            : "Add a game in Games to get started. modman keeps every mod in its own folder and links them into the game only when you deploy."}
        </NoGame>
      )}
    </div>
  );
}

function GameCards({ game }: { game: GameView }) {
  const mods = useMods(game.domain);
  const outOfSync = useDeployDue(game.domain);
  const bethesda = game.profile === "bethesda";
  return (
    <>
      <section
        aria-label={game.name}
        className="stagger grid grid-cols-[minmax(0,1fr)] gap-6 xl:grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)]"
      >
        <PlayCard game={game} outOfSync={outOfSync} />
        <DeploymentCard game={game} outOfSync={outOfSync} />
      </section>
      <div
        className={cx(
          "stagger grid grid-cols-[minmax(0,1fr)] gap-6 md:grid-cols-[repeat(2,minmax(0,1fr))]",
          bethesda && "xl:grid-cols-[repeat(3,minmax(0,1fr))]",
        )}
      >
        <ModsCard mods={mods.data?.mods} outOfSync={outOfSync} />
        <RequirementsCard game={game.domain} />
        {bethesda && <PluginsCard game={game.domain} />}
      </div>
      <RecentCard game={game.domain} mods={mods.data?.mods} />
    </>
  );
}

/** Hero: the game, its state and Play (offers to deploy pending changes first). */
function PlayCard({ game, outOfSync }: { game: GameView; outOfSync: boolean }) {
  const run = useRun();
  const launching = useBusy("Play");
  const d = game.deploy;
  const status = d.interrupted
    ? { tone: "rose" as const, text: "Last deploy was interrupted" }
    : !d.active
      ? { tone: "zinc" as const, text: "Not deployed" }
      : outOfSync
        ? { tone: "amber" as const, text: "Changes waiting to be deployed" }
        : { tone: "accent" as const, text: "Deployed and in sync" };
  const play = async () => {
    const r = await run("Play", () => ipc.gameLaunch(), { key: "Play" });
    if (!r) return;
    const ui = useUi.getState();
    if (r.deployed) ui.clearDeployDue(game.domain);
    ui.notify("success", `Starting ${game.name} through Steam.`);
  };
  return (
    <div className="panel flex min-w-0 flex-col justify-between gap-8 p-7 md:p-9">
      <div>
        <div className="flex flex-wrap items-center gap-2.5">
          <StatusDot tone={status.tone} live={d.active} />
          <span className="text-[13.5px] text-zinc-300">{status.text}</span>
          <Badge>{game.profile}</Badge>
          <SupportBadge support={game.support} />
        </div>
        <h2 className="mt-6 text-4xl leading-none font-medium tracking-tighter wrap-anywhere text-zinc-50 md:text-5xl">{game.name}</h2>
        <p className="mt-4 font-mono text-[12.5px] wrap-anywhere text-dim">{game.folder ?? "no folder set"}</p>
      </div>
      <div className="grid gap-3">
        <div className="flex flex-wrap items-center gap-3">
          <Button
            tone="primary"
            size="lg"
            busy={launching}
            disabled={game.appid === null}
            icon={<PlayIcon aria-hidden size={19} weight="fill" />}
            onClick={play}
          >
            Play
          </Button>
          <RemoveGame game={game} />
        </div>
        <p className="text-[13px] leading-relaxed text-dim">
          {game.appid === null
            ? "No Steam app id is set for this game, so it cannot be started from here (modman game set appid <id>)."
            : outOfSync
              ? "Starts the game through Steam. Your mod changes are not deployed yet: you will be asked to deploy first."
              : `Starts the game through Steam (app ${game.appid}).`}
        </p>
      </div>
    </div>
  );
}

function RemoveGame({ game }: { game: GameView }) {
  const run = useRun();
  // The backend asks for confirmation (and refuses while deployed).
  return (
    <Button
      tone="ghost"
      size="sm"
      icon={<TrashIcon aria-hidden size={15} />}
      onClick={() => run("Remove game", () => ipc.gameRemove(game.domain, false), { key: "Remove game" })}
    >
      Remove game
    </Button>
  );
}

function DeploymentCard({ game, outOfSync }: { game: GameView; outOfSync: boolean }) {
  const run = useRun();
  const deploy = useDeploy();
  const deploying = useBusy("Deploy");
  const purging = useBusy("Purge");
  const verifying = useBusy("Verify");
  const [verified, setVerified] = useState<VerifyReport | null>(null);
  const d = game.deploy;
  return (
    <div className="panel flex min-w-0 flex-col justify-between gap-7 p-7 md:p-9">
      <div>
        <h2 className="eyebrow">Deployment</h2>
        <p className="mt-3 max-w-[44ch] text-[15px] leading-relaxed text-zinc-400">
          {outOfSync
            ? "You changed mods since the last deploy. Deploy to apply them to the game folder."
            : d.active
              ? "Your enabled mods are linked into the game. Purge restores the original files."
              : "Link every enabled mod into the game folder. Original files are backed up first."}
        </p>
        <dl className="mt-6 grid grid-cols-[repeat(auto-fit,minmax(7.5rem,1fr))] gap-4">
          <Stat label="Files" value={d.files.toLocaleString()} />
          <Stat label="Method" value={game.method ?? "auto"} />
          <Stat label="Last deploy" value={ago(d.last_deployed_at)} title={when(d.last_deployed_at)} />
        </dl>
      </div>
      <div className="grid gap-3">
        <Button
          tone="primary"
          size="lg"
          className="w-full"
          busy={deploying}
          icon={<RocketLaunchIcon aria-hidden size={19} weight="duotone" />}
          onClick={() => void deploy()}
        >
          Deploy
        </Button>
        <div className="grid grid-cols-2 gap-2">
          <Button
            busy={verifying}
            icon={<ShieldCheckIcon aria-hidden size={16} />}
            onClick={async () => setVerified((await run("Verify", () => ipc.verify(), { key: "Verify" })) ?? null)}
          >
            Verify
          </Button>
          {/* The backend asks for confirmation through a prompt. */}
          <Button
            tone="danger"
            busy={purging}
            icon={<BroomIcon aria-hidden size={16} />}
            onClick={async () => {
              const r = await run("Purge", () => ipc.purge(), { key: "Purge" });
              // null: declined at the confirmation.
              if (r) useUi.getState().notify(r.nothing_deployed ? "info" : "success", purgedText(r));
            }}
          >
            Purge
          </Button>
        </div>
        {verified && <VerifyResult r={verified} />}
      </div>
    </div>
  );
}

export function purgedText(r: PurgeReport): string {
  if (r.nothing_deployed) return "Nothing was deployed; nothing to purge.";
  const parts = [`Purged: removed ${r.removed} files`, `restored ${r.restored} originals`];
  if (r.missing) parts.push(`${r.missing} already missing`);
  if (r.changed) parts.push(`${r.changed} changed in the game folder`);
  if (r.kept) parts.push(`${r.kept} kept`);
  if (r.plugins?.state === "failed") parts.push(`plugins.txt failed: ${r.plugins.detail}`);
  return parts.join(", ");
}

function VerifyResult({ r }: { r: VerifyReport }) {
  const bad = r.issues.length > 0;
  return (
    <div role="status" className="grid gap-2 pt-1">
      <p className={cx("flex items-center gap-2 text-[14px]", bad ? "text-amber-300" : "text-accent-300")}>
        <ShieldCheckIcon aria-hidden size={17} weight="duotone" />
        {r.intact} files intact, {r.issues.length} issues
      </p>
      {bad && (
        <ul className="grid max-h-48 gap-1 overflow-y-auto font-mono text-[12px] break-all text-zinc-400">
          {r.issues.slice(0, 50).map((i) => (
            <li key={i.target + i.rel_path}>
              {i.kind}: {i.rel_path}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Stat({ label, value, title }: { label: string; value: string; title?: string }) {
  return (
    <div className="min-w-0" title={title}>
      <dt className="eyebrow">{label}</dt>
      <dd className="mt-2 font-mono text-lg font-medium tracking-tight break-words text-zinc-50">{value}</dd>
    </div>
  );
}

/** A small summary card: eyebrow title, content, and a link to the full view. */
function SummaryCard({
  title,
  view,
  linkLabel,
  children,
}: {
  title: string;
  view: ViewId;
  linkLabel: string;
  children: ReactNode;
}) {
  const setView = useUi((s) => s.setView);
  return (
    <section aria-label={title} className="surface flex min-w-0 flex-col gap-5 p-6">
      <h2 className="eyebrow">{title}</h2>
      <div className="flex-1">{children}</div>
      <Button
        size="sm"
        tone="ghost"
        className="justify-self-start self-start"
        icon={<ArrowRightIcon aria-hidden size={15} />}
        onClick={() => setView(view)}
      >
        {linkLabel}
      </Button>
    </section>
  );
}

function Count({ n, label, tone }: { n: number | string; label: string; tone?: string }) {
  return (
    <div className="min-w-0">
      <div className={cx("font-mono text-3xl font-medium tracking-tight", tone ?? "text-zinc-50")}>{n}</div>
      <div className="mt-1 text-[13px] text-dim">{label}</div>
    </div>
  );
}

function ModsCard({ mods, outOfSync }: { mods: ModView[] | undefined; outOfSync: boolean }) {
  const list = mods ?? [];
  const on = list.filter((m) => m.enabled).length;
  return (
    <SummaryCard title="Mods" view="mods" linkLabel="Open Mods">
      <div className="grid grid-cols-2 gap-4">
        <Count n={list.length} label="installed" />
        <Count n={on} label="enabled" />
      </div>
      <p className="mt-5 flex items-center gap-2 text-[13.5px] text-zinc-400">
        <StatusDot tone={outOfSync ? "amber" : "zinc"} />
        {outOfSync ? "Changes since the last deploy" : "No changes since the last deploy"}
      </p>
    </SummaryCard>
  );
}

/** A requirement still needs the user (acknowledged external tools count as done). */
const open = (r: ReqRow) => !r.acked && r.status !== "ok" && r.status !== "gone";

function RequirementsCard({ game }: { game: string }) {
  // Same query as the Requirements view, so both share the cache.
  const q = useQuery({ queryKey: ["deps", game], queryFn: () => ipc.deps({}, game) });
  const rows = q.data ?? [];
  const missing = rows.filter((r) => open(r) && (r.status === "missing" || r.status === "disabled")).length;
  const external = rows.filter((r) => open(r) && r.status === "external").length;
  const ok = rows.length - missing - external;
  return (
    <SummaryCard title="Requirements" view="requirements" linkLabel="Open Requirements">
      {q.isError ? (
        <ErrorNote>{errMsg(q.error)}</ErrorNote>
      ) : q.isPending ? (
        <p className="text-[14px] text-zinc-400">Checking requirements…</p>
      ) : (
        <>
          <div className="grid grid-cols-2 gap-4">
            <Count n={missing} label="missing" tone={missing ? "text-rose-300" : undefined} />
            <Count n={ok} label="ok" />
          </div>
          <p className="mt-5 text-[13.5px] text-zinc-400">
            {external
              ? `${external} external tool${external === 1 ? "" : "s"} to check by hand.`
              : rows.length
                ? "Nothing left to check by hand."
                : "Your enabled mods list no requirements."}
          </p>
        </>
      )}
    </SummaryCard>
  );
}

function PluginsCard({ game }: { game: string }) {
  const q = useQuery({ queryKey: ["plugins", game], queryFn: () => ipc.pluginsList(game) });
  const lines = q.data ?? [];
  const active = lines.filter((p) => p.active).length;
  const managed = lines.filter((p) => p.managed).length;
  return (
    <SummaryCard title="Plugins" view="mods" linkLabel="Open Mods">
      {q.isError ? (
        <ErrorNote>{errMsg(q.error)}</ErrorNote>
      ) : q.isPending ? (
        <p className="text-[14px] text-zinc-400">Reading plugins.txt…</p>
      ) : q.data === null ? (
        <p className="text-[14px] text-zinc-400">No plugins.txt yet. Deploy mods, or start the game once.</p>
      ) : (
        <>
          <div className="grid grid-cols-2 gap-4">
            <Count n={active} label="active" />
            <Count n={lines.length} label="in plugins.txt" />
          </div>
          <p className="mt-5 text-[13.5px] text-zinc-400">{managed} written by modman for your mods.</p>
        </>
      )}
    </SummaryCard>
  );
}

function RecentCard({ game, mods }: { game: string; mods: ModView[] | undefined }) {
  const setView = useUi((s) => s.setView);
  const recent = [...(mods ?? [])].sort((a, b) => b.installed_at - a.installed_at).slice(0, RECENT);
  return (
    <Card
      title="Recently installed"
      actions={
        <Button size="sm" tone="ghost" icon={<ArrowRightIcon aria-hidden size={15} />} onClick={() => setView("install")}>
          Install mods
        </Button>
      }
    >
      {!recent.length ? (
        <Empty>No mods installed yet.</Empty>
      ) : (
        <ul className="rows" aria-label="Recently installed mods">
          {recent.map((m) => (
            <li key={m.id} className="flex items-center gap-4 py-3">
              <Thumb game={game} modId={m.nexus_mod_id} name={m.name} />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[14.5px] text-zinc-100">{m.name}</span>
                <span className="block truncate font-mono text-[11.5px] text-dim">
                  {m.version ?? "no version"} · installed {ago(m.installed_at)}
                </span>
              </span>
              {!m.enabled && <Badge>disabled</Badge>}
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

export function skipText(s: Skip): string {
  switch (s.kind) {
    case "not_deployed":
      return "not deployed";
    case "baseline":
      return "first run (baseline recorded)";
    case "no_reasons":
      return "nothing changed";
    case "back_off":
      return `backing off until ${when(s.retry_at)}`;
    case "folder_missing":
      return "game folder missing";
    case "steam_updating":
      return "Steam is updating the game";
    case "game_running":
      return "the game is running";
  }
}

function StartupCheckCard() {
  const check = useUi((s) => s.startupCheck);
  if (!check || (!check.pending.length && !check.system_busy)) return null;
  return (
    <Card
      title={
        <span className="flex items-center gap-2">
          <ArrowsClockwiseIcon aria-hidden size={14} /> Auto-update check
        </span>
      }
    >
      {check.system_busy && (
        <p className="mb-3 flex items-center gap-2 text-[14px] text-amber-300">
          <StatusDot tone="amber" live />
          {check.system_busy.kind === "already_running"
            ? "An auto-update run is in progress."
            : `The system is updating${check.system_busy.by ? ` (${check.system_busy.by})` : ""}.`}
        </p>
      )}
      <ul className="rows">
        {check.pending.map((p) => (
          <li key={p.domain} className="py-3 text-[14px] text-zinc-400">
            <span className="text-zinc-100">{p.name}</span>: {p.reasons.join(", ")}
            {p.blocked ? ` (waiting: ${skipText(p.blocked)})` : " (the next run will update and redeploy)"}
          </li>
        ))}
      </ul>
    </Card>
  );
}
