// SPDX-License-Identifier: GPL-3.0-or-later
// Overview: active game status, deploy/purge/verify, games (switch, add from
// Steam detection, remove) and the start-up auto-update check.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useActiveGame, useGames, useMods } from "../app/queries";
import { useBusy, useDeploy, useRun } from "../app/run";
import { useUi } from "../app/store";
import {
  ArrowsClockwiseIcon,
  BroomIcon,
  GameControllerIcon,
  MagnifyingGlassIcon,
  PlusIcon,
  RocketLaunchIcon,
  ShieldCheckIcon,
} from "@phosphor-icons/react";
import { Badge, Button, Card, Empty, StatusDot, SupportBadge, ago, when } from "../components/ui";
import * as ipc from "../ipc/commands";
import type { Detected, Skip, VerifyReport } from "../ipc/types";

export function Overview() {
  const { game } = useActiveGame();
  return (
    <div className="grid gap-6">
      {/* The legacy Overview has no big page title: the game name leads the hero. */}
      <h1 className="eyebrow">Overview</h1>
      {game ? (
        <GameStatus />
      ) : (
        <Empty icon={<GameControllerIcon size={28} weight="duotone" />} title="No game yet">
          Add a game below to get started. modman keeps every mod in its own folder and links them into the game only when
          you deploy.
        </Empty>
      )}
      <StartupCheckCard />
      <div className="grid gap-6 lg:grid-cols-12">
        <GamesCard />
        {game && <EnabledTile game={game.domain} />}
      </div>
    </div>
  );
}

function GameStatus() {
  const { game } = useActiveGame();
  const mods = useMods(game?.domain ?? null);
  const needsDeploy = useUi((s) => s.needsDeploy);
  const run = useRun();
  const deploy = useDeploy();
  const deploying = useBusy("Deploy");
  const purging = useBusy("Purge");
  const verifying = useBusy("Verify");
  const [verified, setVerified] = useState<VerifyReport | null>(null);
  if (!game) return null;
  const d = game.deploy;
  const enabled = mods.data?.mods.filter((m) => m.enabled).length ?? 0;
  const outOfSync = needsDeploy || mods.data?.changed_since_deploy;
  const status = d.interrupted
    ? { tone: "rose" as const, text: "Last deploy was interrupted" }
    : !d.active
      ? { tone: "zinc" as const, text: "Not deployed" }
      : outOfSync
        ? { tone: "amber" as const, text: "Changes waiting to be deployed" }
        : { tone: "accent" as const, text: "Deployed and in sync" };

  return (
    <>
      <section aria-label={game.name} className="stagger grid gap-6 xl:grid-cols-[1.35fr_1fr]">
        <div className="panel relative overflow-hidden p-7 md:p-9">
          <div aria-hidden className="glow" />
          <div className="relative">
            <div className="flex flex-wrap items-center gap-2.5">
              <StatusDot tone={status.tone} live={d.active} />
              <span className="text-[13.5px] text-zinc-300">{status.text}</span>
              <Badge>{game.profile}</Badge>
              <SupportBadge support={game.support} />
            </div>
            <h2 className="mt-6 text-4xl leading-none font-medium tracking-tighter text-zinc-50 md:text-5xl">{game.name}</h2>
            <p className="mt-4 font-mono text-[12.5px] wrap-anywhere text-dim">{game.folder ?? "no folder set"}</p>
            <dl className="mt-9 grid grid-cols-2 gap-x-8 gap-y-6">
              <Stat label="Enabled mods" value={String(enabled)} sub={`of ${game.mod_count} mods`} />
              <Stat label="Deployed files" value={d.files.toLocaleString()} sub={d.active ? "linked into the game" : "nothing deployed"} />
              <Stat label="Last deploy" value={ago(d.last_deployed_at)} sub={when(d.last_deployed_at)} />
              <Stat label="Method" value={game.method ?? "auto"} sub="deploy" />
            </dl>
          </div>
        </div>

        <div className="panel flex flex-col justify-between gap-8 p-7 md:p-9">
          <div>
            <div className="eyebrow">Deployment</div>
            <p className="mt-3 max-w-[40ch] text-[15px] leading-relaxed text-zinc-400">
              {outOfSync
                ? "You changed mods since the last deploy. Deploy to apply them to the game folder."
                : d.active
                  ? "Your enabled mods are linked into the game. Purge restores the original files."
                  : "Link every enabled mod into the game folder. Original files are backed up first."}
            </p>
          </div>
          <div className="grid gap-3">
            <Button
              tone="primary"
              size="lg"
              className="w-full"
              busy={deploying}
              icon={<RocketLaunchIcon aria-hidden size={19} weight="duotone" />}
              onClick={deploy}
            >
              Deploy
            </Button>
            <div className="grid grid-cols-2 gap-2">
              <Button
                busy={verifying}
                icon={<ShieldCheckIcon aria-hidden size={16} />}
                onClick={async () => setVerified((await run("Verify", () => ipc.verify())) ?? null)}
              >
                Verify
              </Button>
              {/* The backend asks for confirmation through a prompt. */}
              <Button
                tone="danger"
                busy={purging}
                icon={<BroomIcon aria-hidden size={16} />}
                onClick={() => run("Purge", () => ipc.purge())}
              >
                Purge
              </Button>
            </div>
          </div>
        </div>
      </section>
      {verified && (
        <div className="surface p-6" role="status">
          <div className="flex items-center gap-2.5">
            <ShieldCheckIcon
              aria-hidden
              size={18}
              weight="duotone"
              className={verified.issues.length ? "text-amber-300" : "text-accent-300"}
            />
            <p className={verified.issues.length ? "text-amber-300" : "text-accent-300"}>
              {verified.intact} files intact, {verified.issues.length} issues
            </p>
          </div>
          {verified.issues.length > 0 && (
            <ul className="mt-3 grid gap-1 font-mono text-[12.5px] text-zinc-400">
              {verified.issues.slice(0, 50).map((i) => (
                <li key={i.target + i.rel_path}>
                  {i.kind}: {i.rel_path}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </>
  );
}

function Stat({ label, value, sub }: { label: string; value: string; sub: string }) {
  return (
    <div className="min-w-0">
      <dt className="eyebrow">{label}</dt>
      <dd className="mt-2 truncate font-mono text-2xl font-medium tracking-tight text-zinc-50">{value}</dd>
      <dd className="mt-1 truncate text-[12.5px] text-dim">{sub}</dd>
    </div>
  );
}

/** Legacy "Enabled" tile: share of installed mods that are enabled. */
function EnabledTile({ game }: { game: string }) {
  const mods = useMods(game);
  const list = mods.data?.mods ?? [];
  const on = list.filter((m) => m.enabled).length;
  const pct = list.length ? Math.round((on / list.length) * 1000) / 10 : 0;
  return (
    <section className="surface p-7 lg:col-span-4">
      <h2 className="eyebrow">Enabled</h2>
      <div className="mt-5 flex items-baseline gap-2">
        <span className="font-mono text-4xl font-medium tracking-tight text-zinc-50">{pct}</span>
        <span className="text-[14px] text-zinc-400">% of mods enabled</span>
      </div>
      <progress aria-label="Enabled mods" className="mt-5" max={list.length || 1} value={on} />
      <p className="mt-4 text-[14px] text-zinc-400">
        {list.length - on > 0
          ? `${list.length - on} disabled mods stay staged but are not deployed.`
          : list.length
            ? "Every installed mod is enabled."
            : "No mods installed yet."}
      </p>
    </section>
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

function GamesCard() {
  const games = useGames();
  const run = useRun();
  const [detected, setDetected] = useState(false);
  const detect = useQuery({ queryKey: ["detect"], queryFn: ipc.gamesDetect, enabled: detected });
  return (
    <Card
      title="Games"
      className="lg:col-span-8"
      actions={
        <Button
          size="sm"
          busy={detect.isFetching}
          icon={<MagnifyingGlassIcon aria-hidden size={15} />}
          onClick={() => (detected ? detect.refetch() : setDetected(true))}
        >
          Detect Steam games
        </Button>
      }
    >
      {games.data?.length === 0 && <Empty>No games yet.</Empty>}
      <ul className="rows">
        {games.data?.map((g) => (
          <li key={g.domain} className="flex flex-wrap items-center gap-4 py-3">
            <StatusDot tone={g.deploy.active ? "accent" : "zinc"} live={g.active && g.deploy.active} />
            <span className="min-w-0 flex-1">
              <span className="flex flex-wrap items-center gap-2 text-[14.5px] text-zinc-100">
                {g.name} <SupportBadge support={g.support} />
                {g.active && <Badge tone="accent">Active</Badge>}
              </span>
              <span className="block font-mono text-[11.5px] text-dim">
                {g.mod_count} mods · {g.profile}
              </span>
            </span>
            {!g.active && (
              <Button size="sm" onClick={() => run("Switch game", () => ipc.gameUse(g.domain))}>
                Use
              </Button>
            )}
            <Button size="sm" tone="danger" onClick={() => run("Remove game", () => ipc.gameRemove(g.domain, false))}>
              Remove
            </Button>
          </li>
        ))}
      </ul>
      {detect.data && <DetectedList items={detect.data} />}
    </Card>
  );
}

function DetectedList({ items }: { items: Detected[] }) {
  const run = useRun();
  const fresh = items.filter((d) => !d.configured);
  return (
    <div className="mt-5">
      <h3 className="eyebrow mb-2">Found in Steam</h3>
      {!fresh.length ? (
        <Empty>No other supported games found.</Empty>
      ) : (
        <ul className="rows stagger">
          {fresh.map((d) => (
            <li key={d.domain} className="flex flex-wrap items-center gap-4 py-3">
              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-2 text-[14.5px] text-zinc-100">
                  {d.name} <SupportBadge support={d.support} />
                </span>
                <span className="block font-mono text-[11.5px] break-all text-dim">{d.install_dir}</span>
              </span>
              <Button size="sm" tone="primary" icon={<PlusIcon aria-hidden size={14} />} onClick={() => run("Add game", () => ipc.gameAdd({ domain: d.domain, path: d.install_dir }))}>
                Add
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
