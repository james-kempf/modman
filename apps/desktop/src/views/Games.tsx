// SPDX-License-Identifier: GPL-3.0-or-later
// Game library (the landing view): one portrait tile per added game with its
// Steam library art (a data: URL from game_art, else a monogram), support,
// active and deploy-due markers, and an "Add game" tile last. Picking a tile
// makes that game active (game_use) and opens its Overview. Tiles are plain
// buttons in a grid, so spatial navigation moves between them in 2D.

import { CheckIcon, FlaskIcon, PlusIcon, RocketLaunchIcon } from "@phosphor-icons/react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId } from "react";
import { errMsg } from "../app/bridge";
import { useGames } from "../app/queries";
import { useDeployDue, useRun } from "../app/run";
import { useUi } from "../app/store";
import { Badge, ErrorNote, ViewHeader, cx, monogram } from "../components/ui";
import * as ipc from "../ipc/commands";
import type { GameView } from "../ipc/types";
import { openView } from "../nav/actions";

/** One command at a time: a double click (or A pressed twice) switches once. */
const SWITCH = "Switch game";

const TILE =
  "group flex h-full w-full min-w-0 flex-col gap-3 rounded-3xl border p-2.5 pb-3.5 text-left transition-colors duration-200";

export function Games() {
  const games = useGames();
  const list = games.data ?? [];
  return (
    <div className="grid grid-cols-[minmax(0,1fr)] gap-8">
      <ViewHeader title="Games" description="Pick a game to manage its mods. Its Overview opens with it as the active game." />
      {games.error ? (
        <ErrorNote>Could not load games: {errMsg(games.error)}</ErrorNote>
      ) : games.isPending ? (
        <p className="text-[14px] text-zinc-400">Loading games…</p>
      ) : (
        <>
          {!list.length && (
            <p className="text-[15px] text-zinc-400">No games yet. Add one from your Steam library or by its Nexus Mods domain.</p>
          )}
          {/* auto-fill: 2 columns at 900px wide up to 6 at 1920px; tiles stay large gamepad targets. */}
          <ul aria-label="Game library" className="grid grid-cols-[repeat(auto-fill,minmax(176px,1fr))] gap-5">
            {list.map((g) => (
              <li key={g.domain} className="min-w-0">
                <GameTile game={g} />
              </li>
            ))}
            <li className="min-w-0">
              <AddGameTile />
            </li>
          </ul>
        </>
      )}
    </div>
  );
}

function GameTile({ game }: { game: GameView }) {
  const run = useRun();
  const qc = useQueryClient();
  const due = useDeployDue(game.domain);
  const statusId = useId();
  const select = async () => {
    if (!game.active) {
      const r = await run(SWITCH, () => ipc.gameUse(game.domain), { key: SWITCH });
      // undefined: the switch failed (its notice says why); stay here.
      if (r === undefined) return;
      // The Overview opens on the new game at once, not after the refetch.
      qc.setQueryData<GameView[]>(["games"], (old) => old?.map((g) => ({ ...g, active: g.domain === game.domain })));
    }
    openView("overview");
  };
  return (
    <button
      type="button"
      aria-label={game.name}
      aria-describedby={statusId}
      aria-current={game.active ? "true" : undefined}
      data-game={game.domain}
      onClick={() => void select()}
      className={cx(
        TILE,
        game.active ? "border-accent-400/50 bg-white/[0.04]" : "border-white/[0.06] bg-ink-2 hover:border-white/15 hover:bg-white/[0.04]",
      )}
    >
      <Art game={game} />
      <span className="grid min-w-0 gap-2 px-1">
        <span className="truncate text-[15px] font-medium text-zinc-50" title={game.name}>
          {game.name}
        </span>
        <span id={statusId} className="flex min-h-6 flex-wrap items-center gap-1.5">
          {game.active && (
            <Badge tone="accent">
              <CheckIcon aria-hidden size={12} weight="bold" />
              Active
            </Badge>
          )}
          {game.support === "experimental" ? (
            <span data-testid="experimental-badge" className="inline-flex" title="Not tested end to end; may need manual fixes">
              <Badge tone="amber">
                <FlaskIcon aria-hidden size={12} weight="bold" />
                Experimental
              </Badge>
            </span>
          ) : (
            <Badge>Tested</Badge>
          )}
          {due && (
            <span data-testid="deploy-due" className="inline-flex" title="Mod changes are not deployed yet">
              <Badge tone="amber">
                <RocketLaunchIcon aria-hidden size={12} weight="bold" />
                Deploy due
              </Badge>
            </span>
          )}
        </span>
      </span>
    </button>
  );
}

/** Portrait art (2:3), else a flat monogram tile. */
function Art({ game }: { game: GameView }) {
  const q = useQuery({
    // The app id is in the key: setting another one looks the art up again.
    queryKey: ["art", game.domain, game.appid],
    queryFn: () => ipc.gameArt(game.domain),
    enabled: game.appid !== null,
    staleTime: Infinity,
  });
  const src = q.data && q.data.startsWith("data:image/") ? q.data : null;
  const m = monogram(game.name);
  return (
    <span aria-hidden className="relative block aspect-[2/3] w-full overflow-hidden rounded-2xl bg-ink-3">
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

function AddGameTile() {
  const setAddGame = useUi((s) => s.setAddGame);
  return (
    <button
      type="button"
      onClick={() => setAddGame(true)}
      className={cx(TILE, "border-dashed border-white/15 text-zinc-300 hover:border-white/30 hover:bg-white/[0.03] hover:text-zinc-100")}
    >
      <span aria-hidden className="grid aspect-[2/3] w-full place-items-center rounded-2xl bg-white/[0.02]">
        <PlusIcon size={40} />
      </span>
      <span className="px-1 text-[15px] font-medium">Add game</span>
    </button>
  );
}
