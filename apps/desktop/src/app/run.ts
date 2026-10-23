// SPDX-License-Identifier: GPL-3.0-or-later
// Running a command from a button: busy state, error notice, query refresh
// and the structured AfterChange result (auto-deploy ran / deploy due), kept
// per game: the game a command ran for is the one whose deploy is due.

import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useCallback } from "react";
import * as ipc from "../ipc/commands";
import type { AfterChange, CollectionResult, DeployReport, GameView } from "../ipc/types";
import { errMsg } from "./bridge";
import { isCancelError } from "./jobs";
import { refreshAll, useMods } from "./queries";
import { useUi } from "./store";

/** The AfterChange inside a command result, if it has one. */
export function afterChangeOf(r: unknown): AfterChange | null {
  if (!r || typeof r !== "object") return null;
  if ("needs_deploy" in r && "deployed" in r) return r as AfterChange;
  if ("after" in r) return afterChangeOf((r as { after: unknown }).after);
  return null;
}

/** A collection install/update result (it reports `deployed` but no `needs_deploy`). */
export function collectionResultOf(r: unknown): CollectionResult | null {
  if (!r || typeof r !== "object") return null;
  if ("slug" in r && "up_to_date" in r && "deployed" in r) return r as CollectionResult;
  if ("collection" in r) return collectionResultOf((r as { collection: unknown }).collection);
  return null;
}

/** The game (domain) a result names, if any. */
function gameOf(r: unknown): string | null {
  return r && typeof r === "object" && "game" in r && typeof r.game === "string" ? r.game : null;
}

/** The active game (domain) per the games query cache. */
export function activeGame(qc: QueryClient): GameView | null {
  return qc.getQueryData<GameView[]>(["games"])?.find((g) => g.active) ?? null;
}

export function applyAfterChange(a: AfterChange, game: string | null): void {
  const ui = useUi.getState();
  if (a.deployed) {
    if (game) ui.clearDeployDue(game);
    ui.notify("success", deployedText(a.deployed));
  } else if (a.needs_deploy && game) {
    ui.markDeployDue(game);
    ui.notify("warn", "Changes are not in the game yet.", game);
  }
}

/**
 * A collection result as an AfterChange: deployed when it says so; else,
 * when the mod set may have changed, the game's mods_list tells whether a
 * deploy is now due (same rule as core after_change: an active deployment).
 */
async function applyCollection(qc: QueryClient, r: CollectionResult, game: string | null): Promise<void> {
  if (r.deployed) return applyAfterChange({ deployed: r.deployed, needs_deploy: false }, game);
  if (!game || r.cancelled || r.download_only || r.up_to_date) return;
  try {
    const mods = await qc.fetchQuery({ queryKey: ["mods", game], queryFn: () => ipc.modsList(game), staleTime: 0 });
    if (mods.changed_since_deploy) applyAfterChange({ deployed: null, needs_deploy: true }, game);
  } catch {
    // The view's own mods query reports read errors.
  }
}

export interface RunOpts {
  /**
   * In-flight key: while a run with this key is pending, another run with
   * it is dropped (a double click sends one command). Also a busy label.
   */
  key?: string;
}

export type Run = <T>(label: string, fn: () => Promise<T>, opts?: RunOpts) => Promise<T | undefined>;

export function useRun(): Run {
  const qc = useQueryClient();
  return useCallback(
    async <T>(label: string, fn: () => Promise<T>, opts: RunOpts = {}) => {
      const ui = useUi.getState();
      if (opts.key && (ui.busy[opts.key] ?? 0) > 0) return undefined;
      const key = opts.key && opts.key !== label ? opts.key : null;
      // Commands act on the active game when they start.
      const started = activeGame(qc)?.domain ?? null;
      ui.setBusy(label, true);
      if (key) ui.setBusy(key, true);
      try {
        const r = await fn();
        const game = gameOf(r) ?? started;
        const after = afterChangeOf(r);
        const coll = collectionResultOf(r);
        if (after && !(coll?.deployed && !after.deployed)) applyAfterChange(after, game);
        else if (coll) await applyCollection(qc, coll, gameOf(coll) ?? game);
        return r;
      } catch (e) {
        const msg = errMsg(e);
        // A cancelled prompt is the user's choice, not an error.
        if (isCancelError(msg)) ui.notify("info", `${label} cancelled.`);
        else ui.notify("error", `${label}: ${msg}`);
        return undefined;
      } finally {
        ui.setBusy(label, false);
        if (key) ui.setBusy(key, false);
        void refreshAll(qc);
      }
    },
    [qc],
  ) as Run;
}

export function useBusy(label: string): boolean {
  return useUi((s) => (s.busy[label] ?? 0) > 0);
}

/**
 * The game's deploy is due: a change made in this session (cleared by a
 * deploy of that game), or the backend's changed_since_deploy.
 */
export function useDeployDue(game: string | null): boolean {
  const mods = useMods(game);
  const pending = useUi((s) => (game ? s.deployDue[game] !== undefined : false));
  return !!game && (pending || !!mods.data?.changed_since_deploy);
}

export function deployedText(r: DeployReport): string {
  const parts = [`Deployed ${r.placed} files from ${r.mods} mods`];
  if (r.conflicts.length) parts.push(`${r.conflicts.length} conflicts`);
  if (r.skipped.length) parts.push(`${r.skipped.length} skipped`);
  if (r.plugins?.state === "failed") parts.push(`plugins.txt failed: ${r.plugins.detail}`);
  return parts.join(", ");
}

/**
 * Deploy a game (default: the active one). Clears that game's "deploy due"
 * state on success, unless it changed again while the deploy ran.
 */
export function useDeploy(): (game?: string) => Promise<DeployReport | undefined> {
  const run = useRun();
  const qc = useQueryClient();
  return useCallback(
    (target?: string) => {
      const g = target ? (qc.getQueryData<GameView[]>(["games"])?.find((x) => x.domain === target) ?? null) : activeGame(qc);
      const domain = target ?? g?.domain;
      return run(
        "Deploy",
        async () => {
          const since = useUi.getState().stamp();
          useUi.getState().setDeployGame(domain ?? null);
          // The game is named: switching games mid-deploy does not retarget it.
          const r = await ipc.deploy({}, domain).finally(() => useUi.getState().setDeployGame(null));
          const ui = useUi.getState();
          if (domain) ui.clearDeployDue(domain, since);
          const text = deployedText(r);
          ui.notify(r.plugins?.state === "failed" ? "warn" : "success", g ? `${g.name}: ${text}` : text);
          return r;
        },
        { key: "Deploy" },
      );
    },
    [run, qc],
  );
}
