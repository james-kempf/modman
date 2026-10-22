// SPDX-License-Identifier: GPL-3.0-or-later
// Running a command from a button: busy state, error notice, query refresh
// and the structured AfterChange result (auto-deploy ran / deploy due).

import { useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";
import { deploy } from "../ipc/commands";
import type { AfterChange, DeployReport } from "../ipc/types";
import { errMsg } from "./bridge";
import { isCancelError } from "./jobs";
import { useUi } from "./store";

/** The AfterChange inside a command result, if it has one. */
export function afterChangeOf(r: unknown): AfterChange | null {
  if (!r || typeof r !== "object") return null;
  if ("needs_deploy" in r && "deployed" in r) return r as AfterChange;
  if ("after" in r) return afterChangeOf((r as { after: unknown }).after);
  return null;
}

export function applyAfterChange(a: AfterChange): void {
  const ui = useUi.getState();
  if (a.deployed) {
    ui.setNeedsDeploy(false);
    ui.notify("success", deployedText(a.deployed));
  } else if (a.needs_deploy) {
    ui.setNeedsDeploy(true);
    ui.notify("warn", "Changes are not in the game yet.", true);
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
      ui.setBusy(label, true);
      if (key) ui.setBusy(key, true);
      try {
        const r = await fn();
        const after = afterChangeOf(r);
        if (after) applyAfterChange(after);
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
        void qc.invalidateQueries();
      }
    },
    [qc],
  ) as Run;
}

export function useBusy(label: string): boolean {
  return useUi((s) => (s.busy[label] ?? 0) > 0);
}

export function deployedText(r: DeployReport): string {
  const parts = [`Deployed ${r.placed} files from ${r.mods} mods`];
  if (r.conflicts.length) parts.push(`${r.conflicts.length} conflicts`);
  if (r.skipped.length) parts.push(`${r.skipped.length} skipped`);
  if (r.plugins?.state === "failed") parts.push(`plugins.txt failed: ${r.plugins.detail}`);
  return parts.join(", ");
}

/** Deploy the active game; clears the "deploy due" state on success. */
export function useDeploy(): () => Promise<DeployReport | undefined> {
  const run = useRun();
  return useCallback(
    () =>
      run("Deploy", async () => {
        const r = await deploy();
        const ui = useUi.getState();
        ui.setNeedsDeploy(false);
        ui.notify(r.plugins?.state === "failed" ? "warn" : "success", deployedText(r));
        return r;
      }, { key: "Deploy" }),
    [run],
  );
}
