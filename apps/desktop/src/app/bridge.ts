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
    ]);
    try {
      const info = await appReady();
      if (info.startup_check) ui.setStartupCheck(info.startup_check);
      return info;
    } catch (e) {
      ui.notify("error", `Backend not ready: ${errMsg(e)}`);
      return null;
    }
  })();
  return started;
}

/** Test helper: forget the started bridge and drop its listeners. */
export function resetBridge(): void {
  unlisteners.forEach((u) => u());
  unlisteners = [];
  started = null;
}

/** The message of a `CmdError` rejection (or any thrown value). */
export function errMsg(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}
