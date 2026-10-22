// SPDX-License-Identifier: GPL-3.0-or-later
// Collections: installed ones (update, remove) and look up + install by
// URL/slug. Proceed, optional mods, manual downloads and FOMOD choices come
// from the backend as prompt dialogs.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { errMsg } from "../app/bridge";
import { useActiveGame } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { useUi } from "../app/store";
import {
  ArrowsClockwiseIcon,
  BooksIcon,
  DownloadSimpleIcon,
  MagnifyingGlassIcon,
  TrashIcon,
  WarningIcon,
} from "@phosphor-icons/react";
import { Badge, Button, Card, Empty, ErrorNote, Segmented, TextField, Toggle, ViewHeader, bytes, monogram, when } from "../components/ui";
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
    <div className="grid grid-cols-[minmax(0,1fr)] gap-6">
      <ViewHeader
        title="Collections"
        eyebrow={game?.name}
        description="A collection installs a curated set of mods with the curator's installer choices and load order."
      />
      {!game ? (
        <Empty icon={<BooksIcon size={28} weight="duotone" />} title="No active game">
          Add one on the Overview.
        </Empty>
      ) : (
        <>
          {/* key: a looked-up collection belongs to one game. */}
          <Lookup key={game.domain} />
          <Installed game={game.domain} />
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
    <section aria-labelledby="installed-collections" className="grid gap-5">
      <h2 id="installed-collections" className="eyebrow px-1">
        Installed collections
      </h2>
      {q.isError ? (
        <ErrorNote>Could not load installed collections: {errMsg(q.error)}</ErrorNote>
      ) : !list.length ? (
        <Empty icon={<BooksIcon size={28} weight="duotone" />} title="No collections installed">
          Paste a collection link above. modman downloads every file, replays the curator&apos;s FOMOD choices and applies the
          load-order rules.
        </Empty>
      ) : (
        <ul className="stagger grid gap-5">
          {list.map(({ record: c, enabled }) => (
            <li key={c.id} className="panel grid gap-6 p-6 md:grid-cols-[auto_1fr_auto] md:items-center md:p-8">
              <Monogram name={c.name ?? c.slug} />
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <h3 className="truncate text-xl font-medium tracking-tight text-zinc-50">{c.name ?? c.slug}</h3>
                  <Badge>rev {c.revision}</Badge>
                </div>
                <p className="mt-1 text-[13.5px] text-zinc-400">
                  {c.author && `by ${c.author} · `}installed {when(c.installed_at)}
                </p>
                <div className="mt-4 flex items-center gap-3">
                  <progress aria-label={`${c.name ?? c.slug} enabled mods`} className="w-40" max={c.mods.length || 1} value={enabled} />
                  <span className="font-mono text-[12px] text-zinc-400">
                    {enabled}/{c.mods.length} enabled
                  </span>
                </div>
                {c.failed.length > 0 && (
                  <div className="mt-4 rounded-xl border border-amber-400/15 bg-amber-400/[0.05] px-3.5 py-2.5 text-[13px] text-amber-200">
                    <div className="flex items-center gap-2 font-medium">
                      <WarningIcon aria-hidden size={14} /> {c.failed.length} failed
                    </div>
                  </div>
                )}
              </div>
              <div className="flex flex-wrap gap-2 md:flex-col md:items-stretch">
                <Button
                  size="sm"
                  tone="primary"
                  icon={<ArrowsClockwiseIcon aria-hidden size={14} />}
                  onClick={async () => {
                    const r = await run("Update collection", () => ipc.collectionUpdate({ reference: c.slug }), {
                      key: `Collection ${c.slug}`,
                    });
                    if (r) notify(r.failed.length ? "warn" : "success", collectionText(r));
                  }}
                >
                  Update
                </Button>
                <Button
                  size="sm"
                  tone="danger"
                  icon={<TrashIcon aria-hidden size={14} />}
                  onClick={() => run("Remove collection", () => ipc.collectionRemove(c.slug, false), { key: `Collection ${c.slug}` })}
                >
                  Remove
                </Button>
                <Button size="sm" tone="ghost" onClick={() => run("Remove collection", () => ipc.collectionRemove(c.slug, true), { key: `Collection ${c.slug}` })}>
                  Remove, keep mods
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
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
    const r = await run(
      "Install collection",
      () => ipc.collectionInstall({ reference: info.slug, game: info.game, optional, deploy, revision: info.revision.number }),
      { key: "Install collection" },
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
          className="min-w-0 flex-[1_1_18rem]"
          placeholder="https://next.nexusmods.com/skyrimspecialedition/collections/…"
        />
        <Button type="submit" busy={looking} disabled={!ref.trim()} icon={<MagnifyingGlassIcon aria-hidden size={16} />}>
          Look up
        </Button>
      </form>
      {info && (
        <div className="mt-6 grid animate-rise gap-5 rounded-3xl border border-white/[0.06] bg-ink p-6">
          <div className="flex flex-wrap items-start gap-5">
            <Monogram name={info.revision.name} />
            <div className="min-w-0 flex-1">
              <h3 className="text-xl font-medium tracking-tight wrap-anywhere text-zinc-50">
                {info.revision.name} <span className="text-[15px] font-normal text-zinc-400">by {info.revision.author}</span>
              </h3>
              <p className="mt-1 font-mono text-[12px] text-dim">
                Revision {info.revision.number} · {info.revision.mod_count} mods ({info.revision.optional_count} optional) ·{" "}
                {bytes(info.revision.total_size)} · {info.revision.game_name}
              </p>
              {info.installed && (
                <p className="mt-2 text-[13.5px] text-amber-300">
                  Installed at revision {info.installed.revision}
                  {info.update_available ? `; revision ${info.update_available} is available` : ""}.
                </p>
              )}
              {info.revision.summary && <p className="mt-3 text-[14.5px] leading-relaxed text-zinc-300">{info.revision.summary}</p>}
            </div>
          </div>
          <Segmented label="Optional mods" value={optional} options={OPTIONAL} onChange={setOptional} />
          <div className="flex flex-wrap items-center gap-3">
            <Toggle label="Deploy when done" checked={deploy} onChange={setDeploy} />
            <Button
              tone="primary"
              busy={installing}
              onClick={install}
              className="ml-auto"
              icon={<DownloadSimpleIcon aria-hidden size={16} />}
            >
              {info.installed ? "Install this revision" : "Install collection"}
            </Button>
          </div>
        </div>
      )}
    </Card>
  );
}

/** Legacy collection monogram (flat colour tile with initials). */
function Monogram({ name }: { name: string }) {
  const m = monogram(name);
  return (
    <div
      aria-hidden
      className={`grid size-20 shrink-0 place-items-center rounded-3xl text-xl font-semibold tracking-tight text-white/85 ${m.cls}`}
    >
      {m.initials}
    </div>
  );
}
