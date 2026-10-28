// SPDX-License-Identifier: GPL-3.0-or-later
// Overview of the active game: play and its folders, deployment
// (deploy/purge/verify and how many enabled mods are deployed), mods,
// collections, requirements, plugins (Bethesda games) and the start-up
// auto-update check, over the game's Steam hero art.
// Games are picked and added in the Games view.

import {
  ArrowRightIcon,
  ArrowsClockwiseIcon,
  BroomIcon,
  FolderOpenIcon,
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
  CardSection,
  ErrorNote,
  StatusDot,
  SupportBadge,
  ago,
  cx,
  when,
  ViewFrame,
} from "../components/ui";
import * as ipc from "../ipc/commands";
import type {
  DeployStatus,
  GameFolder,
  GameView,
  ModView,
  PurgeReport,
  ReqRow,
  Skip,
  VerifyReport,
} from "../ipc/types";
import { NoGame } from "../components/shell/NoGame";

export function Overview() {
  const { game, data: games, error } = useActiveGame();
  return (
    // minmax(0, 1fr): long unbroken names truncate inside the cards instead of widening the page.
    <ViewFrame>
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
    </ViewFrame>
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
          // Three cards in a row from xl; with Plugins, two rows of two until there is room for four.
          bethesda
            ? "2xl:grid-cols-[repeat(4,minmax(0,1fr))]"
            : "xl:grid-cols-[repeat(3,minmax(0,1fr))]",
        )}
      >
        <ModsCard
          mods={mods.data?.mods}
          deploy={game.deploy}
          outOfSync={outOfSync}
        />
        <CollectionsCard game={game.domain} />
        <RequirementsCard game={game.domain} />
        {bethesda && <PluginsCard game={game.domain} />}
      </div>
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
    <Card>
      <CardSection className="flex flex-1 flex-col justify-between gap-8">
        <div>
          <div
            data-testid="game-tags"
            className="flex flex-wrap items-center gap-2.5"
          >
            {game.appid !== null && <Badge>App {game.appid}</Badge>}
            <Badge>{game.profile}</Badge>
            {game.folder && <Badge>{game.prefix ? "Proton" : "Native"}</Badge>}
            <SupportBadge support={game.support} />
            <span className="flex items-center gap-2.5">
              <StatusDot tone={status.tone} live={d.active} />
              <span className="text-[13.5px] text-zinc-300">{status.text}</span>
            </span>
          </div>
          <h2 className="mt-6 text-4xl leading-none font-medium tracking-tighter wrap-anywhere text-zinc-50 md:text-5xl">
            {game.name}
          </h2>
          <div className="mt-4 flex flex-wrap gap-2">
            {game.folder ? (
              <FolderButton game={game} which="game" label="Game folder" />
            ) : (
              <p className="font-mono text-[12.5px] text-dim">no folder set</p>
            )}
            {game.prefix && (
              <FolderButton game={game} which="prefix" label="Proton folder" />
            )}
            <FolderButton game={game} which="mods" label="Mods folder" />
          </div>
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
          {(game.appid === null || outOfSync) && (
            <p className="text-[13px] leading-relaxed text-dim">
              {game.appid === null
                ? "No Steam app id is set for this game, so it cannot be started from here (modman game set appid <id>)."
                : "Your mod changes are not deployed yet: Play asks to deploy them first."}
            </p>
          )}
        </div>
      </CardSection>
    </Card>
  );
}

/** Opens a folder of the game in the file manager; the backend resolves the path from the game's settings. */
function FolderButton({
  game,
  which,
  label,
}: {
  game: GameView;
  which: GameFolder;
  label: string;
}) {
  const run = useRun();
  const key = `Open ${label}`;
  const busy = useBusy(key);
  return (
    <Button
      size="sm"
      tone="ghost"
      busy={busy}
      title={
        which === "game"
          ? (game.folder ?? undefined)
          : which === "prefix"
            ? (game.prefix ?? undefined)
            : undefined
      }
      icon={<FolderOpenIcon aria-hidden size={16} />}
      onClick={() =>
        void run(key, () => ipc.openFolder(game.domain, which), { key })
      }
    >
      {label}
    </Button>
  );
}

function RemoveGame({ game }: { game: GameView }) {
  const run = useRun();
  // The backend asks for confirmation (and refuses while deployed).
  return (
    <Button
      tone="ghost"
      size="lg"
      icon={<TrashIcon aria-hidden size={17} />}
      onClick={() =>
        run("Remove game", () => ipc.gameRemove(game.domain, false), {
          key: "Remove game",
        })
      }
    >
      Remove game
    </Button>
  );
}

function DeploymentCard({
  game,
  outOfSync,
}: {
  game: GameView;
  outOfSync: boolean;
}) {
  const run = useRun();
  const deploy = useDeploy();
  const deploying = useBusy("Deploy");
  const purging = useBusy("Purge");
  const verifying = useBusy("Verify");
  const [verified, setVerified] = useState<VerifyReport | null>(null);
  const d = game.deploy;
  return (
    // inset: the title sits inside the box, so its top lines up with the untitled Play card beside it.
    <Card title="Deployment" inset>
      <CardSection className="flex flex-1 flex-col justify-between gap-7">
        <div>
          {(outOfSync || !d.active) && (
            <p className="max-w-[44ch] text-[15px] leading-relaxed text-zinc-400">
              {outOfSync
                ? "You changed mods since the last deploy. Deploy to apply them to the game folder."
                : "Link every enabled mod into the game folder. Original files are backed up first."}
            </p>
          )}
          <dl className="mt-4 grid grid-cols-[repeat(auto-fit,minmax(7.5rem,1fr))] gap-4">
            <Stat label="Files" value={d.files.toLocaleString()} />
            <Stat label="Method" value={game.method ?? "auto"} />
            <Stat
              label="Last deploy"
              value={ago(d.last_deployed_at)}
              title={when(d.last_deployed_at)}
            />
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
              onClick={async () =>
                setVerified(
                  (await run("Verify", () => ipc.verify(), {
                    key: "Verify",
                  })) ?? null,
                )
              }
            >
              Verify
            </Button>
            {/* The backend asks for confirmation through a prompt. */}
            <Button
              tone="danger"
              busy={purging}
              icon={<BroomIcon aria-hidden size={16} />}
              onClick={async () => {
                const r = await run("Purge", () => ipc.purge(), {
                  key: "Purge",
                });
                // null: declined at the confirmation.
                if (r)
                  useUi
                    .getState()
                    .notify(
                      r.nothing_deployed ? "info" : "success",
                      purgedText(r),
                    );
              }}
            >
              Purge
            </Button>
          </div>
          {verified && <VerifyResult r={verified} />}
        </div>
      </CardSection>
    </Card>
  );
}

/**
 * How many enabled mods are in the game: none before a deploy; all of them
 * when in sync (a mod whose every file lost a conflict is still part of
 * the deploy); else the ones with files in the current deployment.
 */
export function deployTracker(
  d: DeployStatus,
  outOfSync: boolean,
): { deployed: number; enabled: number; pct: number } {
  const enabled = d.enabled_mods ?? 0;
  if (!d.active) return { deployed: 0, enabled, pct: 0 };
  if (!outOfSync) return { deployed: enabled, enabled, pct: 100 };
  const deployed = Math.min(d.deployed_mods ?? 0, enabled);
  // Out of sync with nothing enabled (all mods disabled since): nothing of the current set is deployed.
  const pct = enabled ? Math.floor((deployed / enabled) * 100) : 0;
  return { deployed, enabled, pct };
}

function DeployTracker({
  d,
  outOfSync,
}: {
  d: DeployStatus;
  outOfSync: boolean;
}) {
  const t = deployTracker(d, outOfSync);
  return (
    <div data-testid="deploy-tracker" className="mt-5 grid gap-2">
      <div className="flex items-center gap-3">
        <progress
          aria-label="Enabled mods deployed"
          className="flex-1"
          max={100}
          value={t.pct}
        />
        <span className="w-12 text-right font-mono text-[13px] text-zinc-200">
          {t.pct}%
        </span>
      </div>
      <p className="font-mono text-[12px] text-zinc-400">
        {t.deployed} of {t.enabled} enabled {t.enabled === 1 ? "mod" : "mods"}{" "}
        deployed
      </p>
    </div>
  );
}

export function purgedText(r: PurgeReport): string {
  if (r.nothing_deployed) return "Nothing was deployed; nothing to purge.";
  const parts = [
    `Purged: removed ${r.removed} files`,
    `restored ${r.restored} originals`,
  ];
  if (r.missing) parts.push(`${r.missing} already missing`);
  if (r.changed) parts.push(`${r.changed} changed in the game folder`);
  if (r.kept) parts.push(`${r.kept} kept`);
  if (r.plugins?.state === "failed")
    parts.push(`plugins.txt failed: ${r.plugins.detail}`);
  return parts.join(", ");
}

function VerifyResult({ r }: { r: VerifyReport }) {
  const bad = r.issues.length > 0;
  return (
    <div role="status" className="grid gap-2 pt-1">
      <p
        className={cx(
          "flex items-center gap-2 text-[14px]",
          bad ? "text-amber-300" : "text-accent-300",
        )}
      >
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

function Stat({
  label,
  value,
  title,
}: {
  label: string;
  value: string;
  title?: string;
}) {
  return (
    <div className="min-w-0" title={title}>
      <dt className="eyebrow">{label}</dt>
      <dd className="mt-2 font-mono text-lg font-medium tracking-tight break-words text-zinc-50">
        {value}
      </dd>
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
    <Card aria-label={title} title={title} inset>
      {/* flex-1 pushes the link to the bottom when tiles in a row share one height. */}
      <CardSection className="flex flex-1 flex-col gap-5">
        <div className="flex-1">{children}</div>
        <Button
          size="sm"
          tone="ghost"
          className="self-start"
          icon={<ArrowRightIcon aria-hidden size={15} />}
          onClick={() => setView(view)}
        >
          {linkLabel}
        </Button>
      </CardSection>
    </Card>
  );
}

function Count({
  n,
  label,
  tone,
}: {
  n: number | string;
  label: string;
  tone?: string;
}) {
  return (
    <div className="min-w-0">
      <div
        className={cx(
          "font-mono text-3xl font-medium tracking-tight",
          tone ?? "text-zinc-50",
        )}
      >
        {n}
      </div>
      <div className="mt-1 text-[13px] text-dim">{label}</div>
    </div>
  );
}

function ModsCard({
  mods,
  deploy,
  outOfSync,
}: {
  mods: ModView[] | undefined;
  deploy: DeployStatus;
  outOfSync: boolean;
}) {
  const list = mods ?? [];
  const on = list.filter((m) => m.enabled).length;
  return (
    <SummaryCard title="Mods" view="mods" linkLabel="Open Mods">
      <div className="grid grid-cols-2 gap-4">
        <Count n={list.length} label="installed" />
        <Count n={on} label="enabled" />
      </div>
      <DeployTracker d={deploy} outOfSync={outOfSync} />
      <p className="mt-5 flex items-center gap-2 text-[13.5px] text-zinc-400">
        <StatusDot tone={outOfSync ? "amber" : "zinc"} />
        {outOfSync
          ? "Changes since the last deploy"
          : "No changes since the last deploy"}
      </p>
    </SummaryCard>
  );
}

function CollectionsCard({ game }: { game: string }) {
  // Same query as the Collections view, so both share the cache.
  const q = useQuery({
    queryKey: ["collections", game],
    queryFn: () => ipc.collectionsList(game),
  });
  const list = q.data ?? [];
  const members = list.reduce((n, c) => n + c.record.mods.length, 0);
  const enabled = list.reduce((n, c) => n + c.enabled, 0);
  return (
    <SummaryCard
      title="Collections"
      view="collections"
      linkLabel="Open Collections"
    >
      {q.isError ? (
        <ErrorNote>{errMsg(q.error)}</ErrorNote>
      ) : q.isPending ? (
        <p className="text-[14px] text-zinc-400">Reading collections…</p>
      ) : !list.length ? (
        <p className="text-[14px] text-zinc-400">
          No collections installed. Add one from the Collections tab.
        </p>
      ) : (
        <>
          <div className="grid grid-cols-2 gap-4">
            <Count n={list.length} label="installed" />
            <Count n={`${enabled}/${members}`} label="mods enabled" />
          </div>
          <progress
            aria-label="Collection mods enabled"
            className="mt-5"
            max={members || 1}
            value={enabled}
          />
        </>
      )}
    </SummaryCard>
  );
}

/** A requirement still needs the user (acknowledged external tools count as done). */
const open = (r: ReqRow) =>
  !r.acked && r.status !== "ok" && r.status !== "gone";

function RequirementsCard({ game }: { game: string }) {
  // Same query as the Requirements view, so both share the cache.
  const q = useQuery({
    queryKey: ["deps", game],
    queryFn: () => ipc.deps({}, game),
  });
  const rows = q.data ?? [];
  const external = rows.filter(
    (r) => open(r) && r.status === "external",
  ).length;
  // Open external tools can't be verified, so they count as missing until acknowledged.
  const missing =
    rows.filter(
      (r) => open(r) && (r.status === "missing" || r.status === "disabled"),
    ).length + external;
  const ok = rows.length - missing;
  return (
    <SummaryCard
      title="Requirements"
      view="requirements"
      linkLabel="Open Requirements"
    >
      {q.isError ? (
        <ErrorNote>{errMsg(q.error)}</ErrorNote>
      ) : q.isPending ? (
        <p className="text-[14px] text-zinc-400">Checking requirements…</p>
      ) : (
        <>
          <div className="grid grid-cols-2 gap-4">
            <Count
              n={missing}
              label="missing"
              tone={missing ? "text-rose-300" : undefined}
            />
            <Count n={ok} label="ok" />
          </div>
          <p className="mt-5 text-[13.5px] text-zinc-400">
            {external
              ? `Includes ${external} external tool${external === 1 ? "" : "s"} to check by hand.`
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
  const q = useQuery({
    queryKey: ["plugins", game],
    queryFn: () => ipc.pluginsList(game),
  });
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
        <p className="text-[14px] text-zinc-400">
          No plugins.txt yet. Deploy mods, or start the game once.
        </p>
      ) : (
        <>
          <div className="grid grid-cols-2 gap-4">
            <Count n={active} label="active" />
            <Count n={lines.length} label="in plugins.txt" />
          </div>
          <p className="mt-5 text-[13.5px] text-zinc-400">
            {managed} written by modman for your mods.
          </p>
        </>
      )}
    </SummaryCard>
  );
}

/**
 * The active game's Steam hero art, faint and flat behind the Overview
 * (none without an app id or art). Decorative: hidden from assistive tech.
 */
export function HeroBackdrop() {
  const { game } = useActiveGame();
  const q = useQuery({
    queryKey: ["art", game?.domain, game?.appid, "hero"],
    queryFn: () => ipc.gameArt(game!.domain, "library_hero"),
    enabled: !!game && game.appid !== null,
    staleTime: Infinity,
  });
  const src =
    game && q.data && q.data.startsWith("data:image/") ? q.data : null;
  if (!src) return null;
  return (
    <img
      src={src}
      alt=""
      aria-hidden
      data-testid="hero-art"
      className="pointer-events-none absolute inset-0 size-full object-cover object-top opacity-[0.09] select-none"
    />
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
      inset
    >
      <CardSection>
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
              <span className="text-zinc-100">{p.name}</span>:{" "}
              {p.reasons.join(", ")}
              {p.blocked
                ? ` (waiting: ${skipText(p.blocked)})`
                : " (the next run will update and redeploy)"}
            </li>
          ))}
        </ul>
      </CardSection>
    </Card>
  );
}
