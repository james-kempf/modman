// SPDX-License-Identifier: GPL-3.0-or-later
// Collections: installed ones (update, remove) and look up + install by
// URL/slug. Proceed, optional mods, manual downloads and FOMOD choices come
// from the backend as prompt dialogs.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useActiveGame } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { useUi } from "../app/store";
import { Badge, Button, Card, Empty, Segmented, TextField, Toggle, ViewHeader, bytes, when } from "../components/ui";
import * as ipc from "../ipc/commands";
import type { CollectionInfo, CollectionResult } from "../ipc/types";

const OPTIONAL = [
  { value: "ask", label: "Ask" },
  { value: "all", label: "All" },
  { value: "none", label: "None" },
] as const;

export function collectionText(r: CollectionResult): string {
  if (r.cancelled) return `${r.slug}: cancelled`;
  if (r.up_to_date) return `${r.slug} is up to date`;
  const parts = [`${r.slug}: ${r.installed} installed`, `${r.present} already present`];
  if (r.removed) parts.push(`${r.removed} removed`);
  if (r.failed.length) parts.push(`${r.failed.length} failed`);
  return parts.join(", ");
}

export function Collections() {
  const { game } = useActiveGame();
  return (
    <div className="grid gap-6">
      <ViewHeader title="Collections" />
      {!game ? (
        <Empty>No active game.</Empty>
      ) : (
        <>
          <Installed game={game.domain} />
          <Lookup />
        </>
      )}
    </div>
  );
}

function Installed({ game }: { game: string }) {
  const q = useQuery({ queryKey: ["collections", game], queryFn: () => ipc.collectionsList(game) });
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const list = q.data ?? [];
  return (
    <Card title="Installed collections">
      {!list.length ? (
        <Empty>No collections installed.</Empty>
      ) : (
        <ul className="grid gap-2">
          {list.map(({ record: c, enabled }) => (
            <li key={c.id} className="flex flex-wrap items-center gap-3 rounded-xl border border-white/[0.06] p-3">
              <span className="min-w-0 flex-1">
                <span className="flex flex-wrap items-center gap-2 text-zinc-100">
                  {c.name ?? c.slug} <Badge>rev {c.revision}</Badge>
                  {c.failed.length > 0 && <Badge tone="rose">{c.failed.length} failed</Badge>}
                </span>
                <span className="block text-sm text-zinc-500">
                  {c.author && `by ${c.author} · `}
                  {enabled}/{c.mods.length} enabled · installed {when(c.installed_at)}
                </span>
              </span>
              <Button
                onClick={async () => {
                  const r = await run("Update collection", () => ipc.collectionUpdate({ reference: c.slug }));
                  if (r) notify(r.failed.length ? "warn" : "success", collectionText(r));
                }}
              >
                Update
              </Button>
              <Button tone="danger" onClick={() => run("Remove collection", () => ipc.collectionRemove(c.slug, false))}>
                Remove
              </Button>
              <Button tone="danger" onClick={() => run("Remove collection", () => ipc.collectionRemove(c.slug, true))}>
                Remove, keep mods
              </Button>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

function Lookup() {
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const [ref, setRef] = useState("");
  const [info, setInfo] = useState<CollectionInfo | null>(null);
  const [optional, setOptional] = useState<(typeof OPTIONAL)[number]["value"]>("ask");
  const [deploy, setDeploy] = useState(true);
  const looking = useBusy("Look up");
  const installing = useBusy("Install collection");
  const install = async () => {
    if (!info) return;
    const r = await run("Install collection", () =>
      ipc.collectionInstall({ reference: info.slug, game: info.game, optional, deploy, revision: info.revision.number }),
    );
    if (r) notify(r.failed.length ? "warn" : "success", collectionText(r));
  };
  return (
    <Card title="Add a collection">
      <form
        className="flex flex-wrap items-end gap-3"
        onSubmit={async (e) => {
          e.preventDefault();
          if (ref.trim()) setInfo((await run("Look up", () => ipc.collectionInfo(ref.trim()))) ?? null);
        }}
      >
        <TextField
          label="Collection URL, nxm link or slug"
          value={ref}
          onChange={(e) => setRef(e.target.value)}
          className="min-w-72 flex-1"
          placeholder="https://next.nexusmods.com/skyrimspecialedition/collections/…"
        />
        <Button type="submit" busy={looking} disabled={!ref.trim()}>
          Look up
        </Button>
      </form>
      {info && (
        <div className="mt-5 grid gap-4 rounded-xl border border-white/10 p-4">
          <div>
            <h3 className="text-lg text-zinc-100">
              {info.revision.name} <span className="text-zinc-400">by {info.revision.author}</span>
            </h3>
            <p className="text-sm text-zinc-500">
              Revision {info.revision.number} · {info.revision.mod_count} mods ({info.revision.optional_count} optional) ·{" "}
              {bytes(info.revision.total_size)} · {info.revision.game_name}
            </p>
            {info.installed && (
              <p className="mt-1 text-sm text-amber-300">
                Installed at revision {info.installed.revision}
                {info.update_available ? `; revision ${info.update_available} is available` : ""}.
              </p>
            )}
            {info.revision.summary && <p className="mt-2 text-zinc-300">{info.revision.summary}</p>}
          </div>
          <Segmented label="Optional mods" value={optional} options={OPTIONAL} onChange={setOptional} />
          <div className="flex flex-wrap items-center gap-3">
            <Toggle label="Deploy when done" checked={deploy} onChange={setDeploy} />
            <Button tone="primary" busy={installing} onClick={install} className="ml-auto">
              {info.installed ? "Install this revision" : "Install collection"}
            </Button>
          </div>
        </div>
      )}
    </Card>
  );
}
