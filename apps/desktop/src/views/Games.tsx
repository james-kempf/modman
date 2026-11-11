// SPDX-License-Identifier: GPL-3.0-or-later
// Game library (the landing view): one portrait tile per added game with its
// Steam library art (a data: URL from game_art, else a monogram), support,
// active and deploy-due markers. Picking a tile makes that game active
// (game_use) and opens its Overview. Below it, a second grid of the same tiles
// lists every game found in Steam but not managed yet (games_detect), with
// its Steam art by app id (steam_art). Picking a supported one adds it
// (game_add); picking one with no preset opens the add-game dialog with its
// folder prefilled, since only the user knows its Nexus domain. Tiles are plain buttons in grids, so spatial navigation
// moves between them in 2D.

import { CheckIcon, FlaskIcon, PlusIcon, LightningIcon } from "@phosphor-icons/react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { errMsg } from "../app/bridge";
import { useGames } from "../app/queries";
import { useDeployDue, useRun } from "../app/run";
import { useUi } from "../app/store";
import { Badge, Button, CARD_TILE, Empty, ErrorNote, SearchField, ViewFrame, ViewHeader, cx, monogram } from "../components/ui";
import * as ipc from "../ipc/commands";
import type { Detected, GameView, Support } from "../ipc/types";
import { openView } from "../nav/actions";

/** One command at a time: a double click (or A pressed twice) switches once. */
const SWITCH = "Switch game";
/** Same label as the add-game dialog: one add at a time across both. */
const ADD = "Add game";

const TILE = cx(CARD_TILE, "flex-col p-2.5 pb-3.5");
/** auto-fill: 2 columns at 900px wide up to 6 at 1920px; tiles stay large gamepad targets. */
const GRID = "grid grid-cols-[repeat(auto-fill,minmax(176px,1fr))] gap-5";

const matches = (q: string, ...fields: (string | null | undefined)[]) =>
  !q || fields.some((f) => !!f && f.toLowerCase().includes(q));

export function Games() {
  const games = useGames();
  const all = games.data ?? [];
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const list = all.filter((g) => matches(q, g.name, g.id, g.nexus_domain));
  return (
    <ViewFrame>
      <ViewHeader title="Games">
        {all.length > 0 && (
          <SearchField
            region="Filter games"
            label="Search games"
            name="game-search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        )}
      </ViewHeader>
      {games.error ? (
        <ErrorNote>Could not load games: {errMsg(games.error)}</ErrorNote>
      ) : games.isPending ? (
        <p className="text-body text-zinc-400">Loading games…</p>
      ) : (
        <>
          {!all.length && (
            <p className="text-lead text-zinc-400">No games yet. Pick one found in Steam below, or add any game by its Nexus Mods domain.</p>
          )}
          {all.length > 0 && !list.length && <p className="text-lead text-zinc-400">No games match “{query.trim()}”.</p>}
          {list.length > 0 && (
            <ul aria-label="Game library" className={GRID}>
              {list.map((g) => (
                <li key={g.id} className="min-w-0">
                  <GameTile game={g} />
                </li>
              ))}
            </ul>
          )}
          <Unmanaged managed={all} q={q} />
        </>
      )}
    </ViewFrame>
  );
}

function GameTile({ game }: { game: GameView }) {
  const run = useRun();
  const qc = useQueryClient();
  const due = useDeployDue(game.id);
  const statusId = useId();
  const select = async () => {
    if (!game.active) {
      const r = await run(SWITCH, () => ipc.gameUse(game.id), { key: SWITCH });
      // undefined: the switch failed (its notice says why); stay here.
      if (r === undefined) return;
      // The Overview opens on the new game at once, not after the refetch.
      qc.setQueryData<GameView[]>(["games"], (old) => old?.map((g) => ({ ...g, active: g.id === game.id })));
    }
    openView("overview");
  };
  return (
    <button
      type="button"
      aria-label={game.name}
      aria-describedby={statusId}
      aria-current={game.active ? "true" : undefined}
      data-game={game.id}
      onClick={() => void select()}
      className={cx(
        TILE,
        game.active ? "border-accent-400/50 bg-white/4" : "border-white/6 bg-ink-2",
      )}
    >
      <Art
        name={game.name}
        // The app id is in the key: setting another one looks the art up again.
        artKey={game.appid === null ? null : ["art", game.id, game.appid]}
        load={() => ipc.gameArt(game.id)}
      />
      <span className="grid min-w-0 gap-2 px-1">
        <span className="truncate text-lead font-medium text-zinc-50" title={game.name}>
          {game.name}
        </span>
        <span id={statusId} className="flex min-h-6 flex-wrap items-center gap-1.5">
          {game.active && (
            <Badge tone="accent">
              <CheckIcon aria-hidden size={12} weight="bold" />
              Active
            </Badge>
          )}
          <SupportMark support={game.support} />
          {due && (
            <span data-testid="deploy-due" className="inline-flex" title="Mod changes are not deployed yet">
              <Badge tone="amber">
                <LightningIcon aria-hidden size={12} weight="bold" />
                Deploy due
              </Badge>
            </span>
          )}
        </span>
      </span>
    </button>
  );
}

/** Portrait art (2:3), else a flat monogram tile. artKey null: no art to look up. */
function Art({ name, artKey, load }: { name: string; artKey: readonly unknown[] | null; load: () => Promise<string | null> }) {
  const q = useQuery({
    queryKey: artKey ?? ["art-none"],
    queryFn: load,
    enabled: artKey !== null,
    staleTime: Infinity,
  });
  const src = q.data && q.data.startsWith("data:image/") ? q.data : null;
  const m = monogram(name);
  return (
    <span aria-hidden className="relative block aspect-2/3 w-full overflow-hidden rounded-card bg-ink-3">
      {src ? (
        <img src={src} alt="" data-testid="game-art" className="size-full object-cover" />
      ) : (
        <span
          data-testid="game-monogram"
          className={cx("grid size-full place-items-center font-mono text-4xl font-semibold text-white/75", m.cls)}
        >
          {m.initials}
        </span>
      )}
    </span>
  );
}

function SupportMark({ support }: { support: Support | null }) {
  if (support === null)
    return (
      <span data-testid="unsupported-badge" className="inline-flex" title="No Vessel preset: you supply the Nexus domain and settings">
        <Badge>Unsupported</Badge>
      </span>
    );
  return support === "experimental" ? (
    <span data-testid="experimental-badge" className="inline-flex" title="Not tested end to end; may need manual fixes">
      <Badge tone="amber">
        <FlaskIcon aria-hidden size={12} weight="bold" />
        Experimental
      </Badge>
    </span>
  ) : (
    <Badge>Tested</Badge>
  );
}

/** Games found in Steam that are not managed yet; picking one adds it. */
function Unmanaged({ managed, q }: { managed: GameView[]; q: string }) {
  const detect = useQuery({ queryKey: ["detect"], queryFn: ipc.gamesDetect });
  const setAddGame = useUi((s) => s.setAddGame);
  // Also drop managed domains: the games list may refetch before detect does.
  const known = new Set(managed.flatMap((g) => [g.id, g.nexus_domain]));
  const fresh = (detect.data ?? []).filter((d) => !d.configured && !known.has(d.domain));
  const list = fresh.filter((d) => matches(q, d.name, d.domain));
  return (
    <section aria-labelledby="games-unmanaged" className="mt-10 grid gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 id="games-unmanaged" className="eyebrow">
          Found in Steam
        </h2>
        <Button size="sm" tone="ghost" icon={<PlusIcon aria-hidden size={15} />} onClick={() => setAddGame(true)}>
          Add game
        </Button>
      </div>
      {detect.isError ? (
        <ErrorNote>Could not look for Steam games: {errMsg(detect.error)}</ErrorNote>
      ) : detect.isPending ? (
        <p className="text-body text-zinc-400">Looking for Steam games…</p>
      ) : !fresh.length ? (
        <Empty>No other games found in Steam.</Empty>
      ) : !list.length ? (
        <p className="text-body text-zinc-400">No found games match “{q}”.</p>
      ) : (
        <ul aria-label="Unmanaged games" className={GRID}>
          {list.map((d) => (
            <li key={d.domain} className="min-w-0">
              <DetectedTile found={d} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function DetectedTile({ found }: { found: Detected }) {
  const run = useRun();
  const setAddGame = useUi((s) => s.setAddGame);
  const statusId = useId();
  const supported = found.support !== null;
  const add = () =>
    supported
      ? // key: a double click (or A pressed twice) adds once; the dialog shares it.
        void run(ADD, () => ipc.gameAdd({ domain: found.domain, path: found.install_dir }), { key: ADD })
      : setAddGame(true, { name: found.name, path: found.install_dir });
  return (
    <button
      type="button"
      aria-label={`Add ${found.name}`}
      aria-haspopup={supported ? undefined : "dialog"}
      aria-describedby={statusId}
      data-detected={found.domain}
      title={found.install_dir}
      onClick={add}
      className={cx(TILE, "border-dashed bg-ink-2/60", supported ? "border-white/15" : "border-white/8 opacity-80")}
    >
      <Art name={found.name} artKey={["steam-art", found.appid]} load={() => ipc.steamArt(found.appid)} />
      <span className="grid min-w-0 gap-2 px-1">
        <span className="truncate text-lead font-medium text-zinc-200" title={found.name}>
          {found.name}
        </span>
        <span id={statusId} className="flex min-h-6 flex-wrap items-center gap-1.5">
          <Badge>
            <PlusIcon aria-hidden size={12} weight="bold" />
            Not managed
          </Badge>
          <SupportMark support={found.support} />
        </span>
      </span>
    </button>
  );
}
