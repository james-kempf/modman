// SPDX-License-Identifier: GPL-3.0-or-later
// Install and Remove for a Nexus mod or collection item, shared by the browse cards' details dialogs
// and a collection's mods, so both show the same actions. Installing or removing refreshes the browse
// and collection views.

import { useQueryClient } from "@tanstack/react-query";
import { DownloadSimpleIcon, TrashIcon } from "@phosphor-icons/react";
import { useBusy, useRun } from "../../app/run";
import { PROMPT_ANSWERED, type PromptAnsweredDetail } from "../dialogs/PromptDialog";
import { useUi } from "../../app/store";
import * as ipc from "../../ipc/commands";
import type { InstallResult, SearchHit } from "../../ipc/types";
import { removedText } from "../../views/Collections";
import { uninstalledText } from "../../views/Mods";
import { installKey } from "../../views/Requirements";
import { Button } from "../ui";

export function installedText(r: InstallResult): string {
  if (r.installed) {
    const i = r.installed;
    return `Installed ${i.name} (${i.files} files${i.replaced ? `, replaced ${i.replaced}` : ""})`;
  }
  if (r.collection) {
    const c = r.collection;
    if (c.cancelled) return `Collection ${c.slug}: cancelled`;
    return `Collection ${c.slug}: ${c.installed} installed, ${c.present} present, ${c.failed.length} failed`;
  }
  return "Nothing installed";
}

/**
 * What an installed item's Remove takes away. Routed by kind in `useRemove`
 * (a loader-installed item gets its own kind and route).
 */
export type Removable =
  | { kind: "mod"; nexusId: number; name: string }
  | { kind: "loader"; loaderKey: string; nexusId: number; name: string }
  | { kind: "collection"; slug: string; name: string };

/** The run label/busy key of removing an item (shared with its install where the views share one). */
const removeKey = (r: Removable) => (r.kind === "collection" ? `Collection ${r.slug}` : installKey(String(r.nexusId)));

/** The job kind of a mod/collection removal (its confirmation prompt names the job). */
const REMOVE_JOB = { mod: "mods_uninstall", collection: "collection_remove" } as const;

/**
 * The id of the first `kind` job started after this call (null until it starts): the job a command we are
 * about to run gets, so its prompts are told apart from another job's of the same kind.
 */
function nextJob(kind: string): { id: () => number | null; stop: () => void } {
  const since = Math.max(0, ...Object.keys(useUi.getState().jobs).map(Number));
  let id: number | null = null;
  const find = (jobs: ReturnType<typeof useUi.getState>["jobs"]) => {
    if (id !== null) return;
    const ours = Object.values(jobs).filter((j) => j.kind === kind && j.id > since);
    if (ours.length) id = Math.min(...ours.map((j) => j.id));
  };
  find(useUi.getState().jobs);
  return { id: () => id, stop: useUi.subscribe((s) => find(s.jobs)) };
}

/** Whether a prompt answer picks something (not a Cancel / No). */
const picked = (a: PromptAnsweredDetail["answer"]) => a.type !== "cancel" && !(a.type === "confirm" && !a.yes);

/**
 * Runs `work` (which starts a `kind` job) and calls `accept` on the first answer to one of that job's
 * prompts that picks something (`onlyYes`: a confirmation's Yes); returns `work`'s result.
 */
async function onPicked<T>(
  kind: string,
  accept: (() => void) | undefined,
  work: () => Promise<T>,
  onlyYes = false,
  job = nextJob(kind),
): Promise<T> {
  if (!accept) {
    job.stop();
    return work();
  }
  const listen = (e: Event) => {
    const { job: j, answer } = (e as CustomEvent<PromptAnsweredDetail>).detail;
    if (j === null || j !== job.id() || !picked(answer) || (onlyYes && answer.type !== "confirm")) return;
    accept();
  };
  document.addEventListener(PROMPT_ANSWERED, listen);
  try {
    return await work();
  } finally {
    document.removeEventListener(PROMPT_ANSWERED, listen);
    job.stop();
  }
}

/** Calls `onAccepted` at most once. */
const once = (onAccepted?: () => void) => {
  let done = false;
  return onAccepted && (() => !done && ((done = true), onAccepted()));
};

/**
 * Remove an installed browse item; the backend asks for confirmation (the app's confirm prompt).
 * `onAccepted` (close the details dialog) runs on Yes, or when it was removed without asking; never on No.
 */
function useRemove(game: string) {
  const run = useRun();
  const qc = useQueryClient();
  const notify = useUi((s) => s.notify);
  const openConsent = useUi((s) => s.openLoaderConsent);
  return async (r: Removable, onAccepted?: () => void) => {
    const key = removeKey(r);
    const accept = once(onAccepted);
    switch (r.kind) {
      case "loader": {
        // A loader has no mods row: uninstall it through its consent flow (the consent refreshes every view).
        const preview = await run("Uninstall", () => ipc.loaderPreview({ op: "uninstall", loaderKey: r.loaderKey }, game), { key });
        if (preview) openConsent({ game, preview });
        return;
      }
      case "mod": {
        // A bare Nexus id resolves to the installed mod.
        const res = await onPicked(REMOVE_JOB.mod, accept, () => run("Uninstall", () => ipc.modsUninstall([String(r.nexusId)], false), { key }), true);
        if (!res) return;
        if (res.done) accept?.();
        notify(res.done ? "success" : "info", uninstalledText(res, r.name));
        break;
      }
      case "collection": {
        const res = await onPicked(REMOVE_JOB.collection, accept, () => run("Remove collection", () => ipc.collectionRemove(r.slug, false), { key }), true);
        if (!res) return;
        if (res.removed) accept?.();
        notify(res.removed ? "success" : "info", removedText(r.slug, res));
        void qc.invalidateQueries({ queryKey: ["collections", game] });
        break;
      }
    }
    void qc.invalidateQueries({ queryKey: ["browse", game] });
    void qc.invalidateQueries({ queryKey: ["collection-info"] });
  };
}

/** The details dialog's Remove: the backend (or the loader consent) confirms; `onAccepted` once confirmed. */
export function RemoveButton({ game, item, onAccepted }: { game: string; item: Removable; onAccepted?: () => void }) {
  const remove = useRemove(game);
  const busy = useBusy(removeKey(item));
  return (
    <Button
      tone="danger"
      busy={busy}
      aria-label={`Remove ${item.name}`}
      icon={<TrashIcon aria-hidden size={16} />}
      onClick={() => void remove(item, onAccepted)}
    >
      Remove
    </Button>
  );
}


/**
 * Install (or Remove, when the mod is installed) for a Nexus mod, as its details dialog shows them.
 * `onAccepted` (close the dialog), on Install: once the mod's file list (mod_files, the same rows the backend
 * picks from) shows a single file, i.e. no file choice is coming; with several files, on the pick in the
 * backend's file chooser (the first answer that picks something to this install job's prompts), never on its
 * Cancel. Also once the install succeeds. On Remove: on a confirmed Yes.
 */
export function ModActions({ game, m, onAccepted }: { game: string; m: SearchHit; onAccepted?: () => void }) {
  const run = useRun();
  const qc = useQueryClient();
  const notify = useUi((s) => s.notify);
  const name = m.name ?? `Mod ${m.modId ?? "?"}`;
  const key = installKey(String(m.modId));
  const busy = useBusy(key);
  if (m.installed && m.modId !== null) {
    return (
      <RemoveButton
        game={game}
        item={m.loader ? { kind: "loader", loaderKey: m.loader, nexusId: m.modId, name } : { kind: "mod", nexusId: m.modId, name }}
        onAccepted={onAccepted}
      />
    );
  }
  const install = async () => {
    const accept = once(onAccepted);
    const job = nextJob("install");
    if (accept) {
      const files = qc.fetchQuery({
        queryKey: ["mod-files", game, m.modId],
        queryFn: () => ipc.modFiles(m.modId!),
        staleTime: 5 * 60_000,
      });
      // One file (or no list): no file choice comes, close right away; progress shows in the job island.
      // Never while one of this job's choosers waits for an answer (its pick closes it then).
      const waiting = () => useUi.getState().prompts.some((p) => p.job !== null && p.job === job.id());
      void files.then((f) => f.files.length > 1, () => false).then((options) => options || waiting() || accept());
    }
    const r = await onPicked("install", accept, () => run("Install", () => ipc.install({ target: String(m.modId), fomod: "ask" }), { key }), false, job);
    if (!r) return;
    accept?.();
    notify("success", installedText(r));
    void qc.invalidateQueries({ queryKey: ["browse", game] });
    void qc.invalidateQueries({ queryKey: ["collection-info"] });
  };
  return (
    <Button
      tone="primary"
      disabled={m.modId === null}
      busy={busy}
      aria-label={`Install ${name}`}
      icon={<DownloadSimpleIcon aria-hidden size={16} />}
      onClick={() => void install()}
    >
      Install
    </Button>
  );
}
