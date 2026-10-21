// SPDX-License-Identifier: GPL-3.0-or-later
// Overview: active game status, deploy/purge/verify, games (switch, add from
// Steam detection, remove) and the start-up auto-update check.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useActiveGame, useGames, useMods } from "../app/queries";
import { useBusy, useDeploy, useRun } from "../app/run";
import { useUi } from "../app/store";
import { Badge, Button, Card, Empty, SupportBadge, ViewHeader, when } from "../components/ui";
import * as ipc from "../ipc/commands";
import type { Detected, Skip, VerifyReport } from "../ipc/types";

export function Overview() {
  const { game } = useActiveGame();
  return (
    <div className="grid gap-6">
      <ViewHeader title="Overview" />
      {game ? <GameStatus /> : <Empty>Add a game below to get started.</Empty>}
      <StartupCheckCard />
      <GamesCard />
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
    <Card
      title={
        <span className="flex flex-wrap items-center gap-3">
          {game.name} <SupportBadge support={game.support} /> <Badge tone={status.tone}>{status.text}</Badge>
        </span>
      }
      actions={
        <>
          <Button tone="primary" busy={deploying} onClick={deploy}>
            Deploy
          </Button>
          <Button busy={verifying} onClick={async () => setVerified((await run("Verify", () => ipc.verify())) ?? null)}>
            Verify
          </Button>
          {/* The backend asks for confirmation through a prompt. */}
          <Button tone="danger" busy={purging} onClick={() => run("Purge", () => ipc.purge())}>
            Purge
          </Button>
        </>
      }
    >
      <dl className="grid grid-cols-2 gap-x-8 gap-y-4 md:grid-cols-4">
        <Stat label="Enabled mods" value={`${enabled} / ${game.mod_count}`} />
        <Stat label="Deployed files" value={String(d.files)} />
        <Stat label="Last deploy" value={when(d.last_deployed_at)} />
        <Stat label="Method" value={game.method ?? "auto"} />
      </dl>
      <p className="mt-4 font-mono text-sm break-all text-zinc-500">{game.folder ?? "no folder set"}</p>
      {verified && (
        <div className="mt-4 rounded-xl border border-white/10 p-4" role="status">
          <p className={verified.issues.length ? "text-amber-300" : "text-accent-300"}>
            {verified.intact} files intact, {verified.issues.length} issues
          </p>
          <ul className="mt-2 grid gap-1 font-mono text-sm text-zinc-400">
            {verified.issues.slice(0, 50).map((i) => (
              <li key={i.target + i.rel_path}>
                {i.kind}: {i.rel_path}
              </li>
            ))}
          </ul>
        </div>
      )}
    </Card>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="eyebrow">{label}</dt>
      <dd className="mt-1 text-lg text-zinc-100">{value}</dd>
    </div>
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
    <Card title="Auto-update check">
      {check.system_busy && (
        <p className="mb-3 text-amber-300">
          {check.system_busy.kind === "already_running"
            ? "An auto-update run is in progress."
            : `The system is updating${check.system_busy.by ? ` (${check.system_busy.by})` : ""}.`}
        </p>
      )}
      <ul className="grid gap-2">
        {check.pending.map((p) => (
          <li key={p.domain} className="text-zinc-300">
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
      actions={
        <Button busy={detect.isFetching} onClick={() => (detected ? detect.refetch() : setDetected(true))}>
          Detect Steam games
        </Button>
      }
    >
      {games.data?.length === 0 && <Empty>No games yet.</Empty>}
      <ul className="grid gap-2">
        {games.data?.map((g) => (
          <li key={g.domain} className="flex flex-wrap items-center gap-3 rounded-xl border border-white/[0.06] p-3">
            <span className="min-w-0 flex-1">
              <span className="flex flex-wrap items-center gap-2 text-zinc-100">
                {g.name} <SupportBadge support={g.support} />
                {g.active && <Badge tone="accent">Active</Badge>}
              </span>
              <span className="block text-sm text-zinc-500">
                {g.mod_count} mods · {g.profile}
              </span>
            </span>
            {!g.active && <Button onClick={() => run("Switch game", () => ipc.gameUse(g.domain))}>Use</Button>}
            <Button tone="danger" onClick={() => run("Remove game", () => ipc.gameRemove(g.domain, false))}>
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
        <ul className="grid gap-2">
          {fresh.map((d) => (
            <li key={d.domain} className="flex flex-wrap items-center gap-3 rounded-xl border border-white/[0.06] p-3">
              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-2 text-zinc-100">
                  {d.name} <SupportBadge support={d.support} />
                </span>
                <span className="block font-mono text-sm break-all text-zinc-500">{d.install_dir}</span>
              </span>
              <Button tone="primary" onClick={() => run("Add game", () => ipc.gameAdd({ domain: d.domain, path: d.install_dir }))}>
                Add
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
