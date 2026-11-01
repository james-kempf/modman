// SPDX-License-Identifier: GPL-3.0-or-later
// Wires backend events into the UI store and the query cache, then calls
// `app_ready` exactly once (after every listener is registered, so no
// startup-check or forwarded nxm event is lost).

import type { QueryClient } from "@tanstack/react-query";
import { appReady } from "../ipc/commands";
import { on } from "../ipc/events";
import type { Unlisten } from "../ipc/transport";
import type { ReadyInfo } from "../ipc/types";
import { refreshAll } from "./queries";
import { openPendingLoaders } from "./run";
import { useUi } from "./store";

let started: Promise<ReadyInfo | null> | null = null;
let unlisteners: Unlisten[] = [];

export function startBridge(qc: QueryClient): Promise<ReadyInfo | null> {
  if (started) return started;
  const ui = useUi.getState();
  started = (async () => {
    unlisteners = await Promise.all([
      on("vessel:activity", (e) => ui.onActivity(e)),
      on("vessel:job", (e) => {
        ui.onJob(e);
        // Any job may change mods, games, deploy state or downloads.
        if (e.state !== "started") void refreshAll(qc);
      }),
      on("vessel:prompt", (e) => ui.onPrompt(e)),
      on("vessel:prompt-closed", (e) => ui.dropPrompt(e.id)),
      on("vessel:download", (e) => ui.onDownload(e.job, e.name, e.event)),
      on("vessel:deploy-progress", (e) => ui.onDeployProgress(e)),
      on("vessel:nxm", (e) => {
        ui.onNxm(e);
        if (e.status === "rejected") ui.notify("error", `nxm link rejected: ${e.error ?? "unknown"}`);
        void qc.invalidateQueries({ queryKey: ["downloads"] });
      }),
      on("vessel:startup-check", (c) => ui.setStartupCheck(c)),
      on("vessel:loader-log", (e) => ui.onLoaderLog(e)),
      on("vessel:loader-pending", (e) => openPendingLoaders({ loaders_pending: e.pending }, e.game)),
      on("vessel:loader-recovered", (r) => {
        noteRecovered(r.filter((x) => x.action.kind === "rollback_offered").length);
        void qc.invalidateQueries({ queryKey: ["loaders"] });
      }),
    ]);
    try {
      const info = await appReady();
      if (info.startup_check) ui.setStartupCheck(info.startup_check);
      noteRecovered(info.loader_recovered?.filter((r) => r.action.kind === "rollback_offered").length ?? 0);
      return info;
    } catch (e) {
      ui.notify("error", `Backend not ready: ${errMsg(e)}`);
      return null;
    }
  })();
  return started;
}

/** Once per start: an interrupted loader run waits for a rollback decision (Requirements). */
let recoveredNoted = false;
function noteRecovered(n: number): void {
  if (!n || recoveredNoted) return;
  recoveredNoted = true;
  useUi.getState().notify("warn", "A loader install was interrupted. Open Requirements to roll it back or keep it.");
}

/** Test helper: forget the started bridge and drop its listeners. */
export function resetBridge(): void {
  unlisteners.forEach((u) => u());
  unlisteners = [];
  started = null;
  recoveredNoted = false;
}

/** The message of a `CmdError` rejection (or any thrown value). */
export function errMsg(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}
