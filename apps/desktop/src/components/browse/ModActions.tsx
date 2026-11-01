// SPDX-License-Identifier: GPL-3.0-or-later
// Install and Remove for a Nexus mod or collection item, shared by the browse cards' details dialogs
// and a collection's mods, so both show the same actions. Installing or removing refreshes the browse
// and collection views.

import { useQueryClient } from "@tanstack/react-query";
import { DownloadSimpleIcon, TrashIcon } from "@phosphor-icons/react";
import { useBusy, useRun } from "../../app/run";
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

/** Remove an installed browse item; the backend asks for confirmation (the app's confirm prompt). */
function useRemove(game: string) {
  const run = useRun();
  const qc = useQueryClient();
  const notify = useUi((s) => s.notify);
  const openConsent = useUi((s) => s.openLoaderConsent);
  return async (r: Removable) => {
    const key = removeKey(r);
    switch (r.kind) {
      case "loader": {
        // A loader has no mods row: uninstall it through its consent flow (the consent refreshes every view).
        const preview = await run("Uninstall", () => ipc.loaderPreview({ op: "uninstall", loaderKey: r.loaderKey }, game), { key });
        if (preview) openConsent({ game, preview });
        return;
      }
      case "mod": {
        // A bare Nexus id resolves to the installed mod.
        const res = await run("Uninstall", () => ipc.modsUninstall([String(r.nexusId)], false), { key });
        if (!res) return;
        notify(res.done ? "success" : "info", uninstalledText(res, r.name));
        break;
      }
      case "collection": {
        const res = await run("Remove collection", () => ipc.collectionRemove(r.slug, false), { key });
        if (!res) return;
        notify(res.removed ? "success" : "info", removedText(r.slug, res));
        void qc.invalidateQueries({ queryKey: ["collections", game] });
        break;
      }
    }
    void qc.invalidateQueries({ queryKey: ["browse", game] });
    void qc.invalidateQueries({ queryKey: ["collection-info"] });
  };
}

/** The details dialog's Remove: the backend (or the loader consent) confirms. */
export function RemoveButton({ game, item }: { game: string; item: Removable }) {
  const remove = useRemove(game);
  const busy = useBusy(removeKey(item));
  return (
    <Button
      tone="danger"
      busy={busy}
      aria-label={`Remove ${item.name}`}
      icon={<TrashIcon aria-hidden size={16} />}
      onClick={() => void remove(item)}
    >
      Remove
    </Button>
  );
}


/** Install (or Remove, when the mod is installed) for a Nexus mod, as its details dialog shows them. */
export function ModActions({ game, m }: { game: string; m: SearchHit }) {
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
      />
    );
  }
  const install = async () => {
    const r = await run("Install", () => ipc.install({ target: String(m.modId), fomod: "ask" }), { key });
    if (!r) return;
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
