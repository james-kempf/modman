// SPDX-License-Identifier: GPL-3.0-or-later
// Mods: load order list (enable, move, reinstall, uninstall, details),
// file/plugin conflicts, updates and profiles.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useActiveGame, useMods } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { Dialog } from "../components/Dialog";
import {
  ArrowDownIcon,
  ArrowUpIcon,
  ArrowsClockwiseIcon,
  CaretRightIcon,
  DownloadSimpleIcon,
  FloppyDiskIcon,
  StackIcon,
  SwordIcon,
  TrashIcon,
} from "@phosphor-icons/react";
import {
  Badge,
  Button,
  Card,
  Empty,
  Fact,
  IconButton,
  Segmented,
  StatusDot,
  SupportBadge,
  TextField,
  Thumb,
  Toggle,
  ViewHeader,
  bytes,
  cx,
  when,
} from "../components/ui";
import * as ipc from "../ipc/commands";
import type { ModView, UpdateReport } from "../ipc/types";

const TABS = [
  { value: "list", label: "Mods" },
  { value: "conflicts", label: "Conflicts" },
  { value: "updates", label: "Updates" },
  { value: "profiles", label: "Profiles" },
] as const;
type Tab = (typeof TABS)[number]["value"];

export function Mods() {
  const { game } = useActiveGame();
  const [tab, setTab] = useState<Tab>("list");
  return (
    <div className="grid gap-6">
      <ViewHeader
        title="Mods"
        eyebrow={game?.name}
        description="Priority runs top to bottom: when two mods ship the same file, the one lower in the list wins."
      >
        {game && <SupportBadge support={game.support} />}
        <Segmented label="Show" value={tab} options={TABS} onChange={setTab} />
      </ViewHeader>
      {!game ? (
        <Empty icon={<StackIcon size={28} weight="duotone" />} title="No active game">
          Add one on the Overview.
        </Empty>
      ) : tab === "list" ? (
        <ModList game={game.domain} />
      ) : tab === "conflicts" ? (
        <Conflicts game={game.domain} />
      ) : tab === "updates" ? (
        <Updates />
      ) : (
        <Profiles game={game.domain} />
      )}
    </div>
  );
}

function ModList({ game }: { game: string }) {
  const mods = useMods(game);
  const run = useRun();
  const [detail, setDetail] = useState<ModView | null>(null);
  if (mods.isLoading) return <Loading />;
  if (mods.error) return <Empty>Could not load mods: {String((mods.error as { message?: string }).message ?? mods.error)}</Empty>;
  const list = mods.data?.mods ?? [];
  if (!list.length)
    return (
      <Empty icon={<StackIcon size={28} weight="duotone" />} title="No mods installed yet">
        Install a single mod by its Nexus ID or URL, or a whole collection, from the Install tab. They appear here in load
        order.
      </Empty>
    );
  const on = list.filter((m) => m.enabled).length;
  return (
    <>
      {mods.data?.changed_since_deploy && (
        <p className="flex items-center gap-2.5 rounded-2xl border border-amber-400/15 bg-amber-400/[0.05] px-4 py-3 text-[14px] text-amber-200">
          <StatusDot tone="amber" live />
          The mod set changed since the last deploy.
        </p>
      )}
      <div className="flex items-center justify-between px-1 font-mono text-[11.5px] text-dim">
        <span>
          {list.length} {list.length === 1 ? "mod" : "mods"} · {on} enabled
        </span>
        <span>{bytes(list.reduce((n, m) => n + (m.size ?? 0), 0))} staged</span>
      </div>
      <ol aria-label="Load order" className="stagger grid min-w-0 gap-2">
        {list.map((m, i) => (
          <li
            key={m.id}
            className="flex min-h-16 min-w-0 flex-wrap items-center gap-3 rounded-2xl border border-white/[0.05] bg-ink-2 py-2 pr-3 pl-2 transition-colors duration-200 hover:border-white/[0.08]"
          >
            <span className="w-9 shrink-0 text-right font-mono text-[12px] text-dim">{m.position}</span>
            <Thumb game={game} modId={m.nexus_mod_id} name={m.name} className={cx("transition-opacity", !m.enabled && "opacity-40 grayscale")} />
            <span className="min-w-0 flex-1">
              <span className={cx("block truncate text-[15px] transition-colors", m.enabled ? "text-zinc-100" : "text-zinc-400")}>
                {m.name}
              </span>
              <span className="block truncate font-mono text-[11.5px] text-dim">
                {m.version ? `v${m.version}` : "—"}
                {m.author ? ` · ${m.author}` : ""} · {m.file_count} files
              </span>
            </span>
            <span className="hidden items-center gap-2 md:flex">
              {m.collections.length > 0 && <Badge tone="accent">collection</Badge>}
              <span className="w-16 text-right font-mono text-[11.5px] text-dim">{bytes(m.size)}</span>
            </span>
            <Toggle
              label={`${m.enabled ? "Disable" : "Enable"} ${m.name}`}
              showLabel={false}
              checked={m.enabled}
              onChange={(v) => run("Enable/disable", () => ipc.modsSetEnabled([m.id], v))}
            />
            <IconButton label={`Move ${m.name} up`} disabled={i === 0} onClick={() => run("Move", () => ipc.modMove(m.uid, "up"))}>
              <ArrowUpIcon aria-hidden size={18} />
            </IconButton>
            <IconButton
              label={`Move ${m.name} down`}
              disabled={i === list.length - 1}
              onClick={() => run("Move", () => ipc.modMove(m.uid, "down"))}
            >
              <ArrowDownIcon aria-hidden size={18} />
            </IconButton>
            <Button size="sm" tone="ghost" icon={<CaretRightIcon aria-hidden size={14} />} onClick={() => setDetail(m)}>
              Details
            </Button>
          </li>
        ))}
      </ol>
      {detail && <ModDetailDialog mod={detail} game={game} onClose={() => setDetail(null)} />}
    </>
  );
}

function ModDetailDialog({ mod, game, onClose }: { mod: ModView; game: string; onClose: () => void }) {
  const run = useRun();
  const q = useQuery({ queryKey: ["mod", game, mod.uid], queryFn: () => ipc.modDetail(mod.uid, game) });
  const d = q.data;
  return (
    <Dialog title={mod.name} subtitle={`#${mod.position} · ${mod.uid}`} onBack={onClose} wide>
      <dl className="grid grid-cols-2 gap-x-6 gap-y-4 md:grid-cols-3">
        <Fact k="Version" v={mod.version ?? "?"} />
        <Fact k="Author" v={mod.author ?? "?"} />
        <Fact k="Source" v={mod.source} />
        <Fact k="Installed" v={when(mod.installed_at)} />
        <Fact k="Files" v={`${mod.file_count} · ${bytes(mod.size)}`} />
        <Fact k="Conflicts" v={d ? `wins ${d.wins} · loses ${d.loses}` : "…"} />
      </dl>
      {d?.url && <p className="mt-5 font-mono text-[12.5px] break-all text-zinc-400 select-all">{d.url}</p>}
      {d && d.fomod.length > 0 && (
        <div className="mt-5">
          <h3 className="eyebrow mb-2">Installer choices</h3>
          <ul className="grid gap-1 text-[13px] text-zinc-300">
            {d.fomod.map((s) => (
              <li key={s.step}>
                <span className="text-dim">{s.step}:</span> {s.options.join(", ") || "—"}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="mt-7 flex flex-wrap justify-end gap-2">
        <Button
          icon={<ArrowsClockwiseIcon aria-hidden size={15} />}
          onClick={() => {
            onClose();
            void run("Reinstall", () => ipc.modReinstall(mod.uid, { fomod: "ask" }));
          }}
        >
          Reinstall (choose FOMOD options)
        </Button>
        <Button
          tone="danger"
          icon={<TrashIcon aria-hidden size={15} />}
          onClick={() => {
            onClose();
            // The backend asks for confirmation.
            void run("Uninstall", () => ipc.modsUninstall([mod.uid], false));
          }}
        >
          Uninstall
        </Button>
        <Button tone="primary" data-autofocus onClick={onClose}>
          Close
        </Button>
      </div>
    </Dialog>
  );
}

function Loading() {
  return <p className="px-1 py-6 text-[14px] text-zinc-400">Loading…</p>;
}

function Conflicts({ game }: { game: string }) {
  const q = useQuery({ queryKey: ["conflicts", game], queryFn: () => ipc.conflicts(game) });
  if (q.isLoading) return <Loading />;
  if (!q.data) return <Empty>Could not load conflicts.</Empty>;
  const { files, plugins } = q.data;
  return (
    <div className="grid gap-6">
      <Card title={`File conflicts (${files.length})`}>
        {!files.length ? (
          <Empty>No two enabled mods provide the same file.</Empty>
        ) : (
          <ul className="rows" aria-label="File conflicts">
            {files.map((c) => (
              <li key={c.rel_path} tabIndex={0} className="flex items-start gap-3 rounded-lg py-3">
                <SwordIcon aria-hidden size={16} className="mt-0.5 shrink-0 text-dim" />
                <span className="min-w-0 flex-1">
                  <span className="block font-mono text-[12.5px] break-all text-zinc-200">{c.rel_path}</span>
                  <span className="text-[13.5px] text-zinc-400">
                    <span className="text-accent-300">{c.winner.name}</span> wins over {c.losers.map((l) => l.name).join(", ")}
                    {c.plugin && " · plugin"}
                  </span>
                </span>
              </li>
            ))}
          </ul>
        )}
      </Card>
      {plugins.length > 0 && (
        <Card title={`Plugins (${plugins.length})`}>
          <ol className="rows" aria-label="Plugins">
            {plugins.map((p) => (
              <li key={p.name} tabIndex={0} className="flex flex-wrap items-center gap-3 rounded-lg py-2.5">
                <span className="w-9 text-right font-mono text-[12px] text-dim">{p.load_index ?? "–"}</span>
                <span className="min-w-0 flex-1 font-mono text-[13px] text-zinc-200">{p.name}</span>
                {!p.active && <Badge>inactive</Badge>}
                {p.provider && <span className="text-[13px] text-zinc-400">{p.provider.name}</span>}
                {p.overridden.length > 0 && <Badge tone="amber">overrides {p.overridden.length}</Badge>}
              </li>
            ))}
          </ol>
        </Card>
      )}
    </div>
  );
}

function Updates() {
  const run = useRun();
  const checking = useBusy("Check updates");
  const installing = useBusy("Install updates");
  const [report, setReport] = useState<UpdateReport | null>(null);
  return (
    <Card
      title="Updates"
      actions={
        <>
          <Button size="sm" busy={checking} icon={<ArrowsClockwiseIcon aria-hidden size={15} />} onClick={async () => setReport((await run("Check updates", () => ipc.updatesCheck(true))) ?? null)}>
            Check for updates
          </Button>
          <Button
            size="sm"
            tone="primary"
            busy={installing}
            icon={<DownloadSimpleIcon aria-hidden size={15} />}
            disabled={!report?.updates.length}
            onClick={async () => setReport((await run("Install updates", () => ipc.updatesInstall())) ?? null)}
          >
            Install updates
          </Button>
        </>
      }
    >
      {!report ? (
        <Empty>Check Nexus for newer files of your mods.</Empty>
      ) : (
        <>
          <p className="mb-4 font-mono text-[12px] text-dim">
            Checked {report.checked} mods{report.stopped_early && " (stopped early: rate limit)"}
            {report.installed > 0 && `, installed ${report.installed}`}.
          </p>
          {!report.updates.length ? (
            <Empty>Everything is up to date.</Empty>
          ) : (
            <ul className="rows">
              {report.updates.map((u) => (
                <li key={u.mod.id} tabIndex={0} className="flex flex-wrap items-center gap-2 rounded-lg py-3">
                  <span className="text-[14.5px] text-zinc-100">{u.mod.name}</span>{" "}
                  <span className="font-mono text-[12px] text-zinc-400">
                    {u.mod.version ?? "?"} → {u.file.version ?? u.file.name}
                  </span>
                  {u.pinned && <Badge tone="amber">pinned</Badge>}
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </Card>
  );
}

function Profiles({ game }: { game: string }) {
  const q = useQuery({ queryKey: ["profiles", game], queryFn: () => ipc.profilesList(game) });
  const run = useRun();
  const [name, setName] = useState("");
  // Deleting is not confirmed by the backend: press twice.
  const [armed, setArmed] = useState<string | null>(null);
  return (
    <Card title="Profiles">
      <form
        className="mb-5 flex flex-wrap items-end gap-3"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!name.trim()) return;
          await run("Save profile", () => ipc.profileSave(name.trim()));
          setName("");
        }}
      >
        <TextField label="Save the current mod set as" value={name} onChange={(e) => setName(e.target.value)} className="min-w-72 flex-1" />
        <Button type="submit" tone="primary" disabled={!name.trim()} icon={<FloppyDiskIcon aria-hidden size={15} />}>
          Save profile
        </Button>
      </form>
      {!q.data?.length ? (
        <Empty>No saved profiles.</Empty>
      ) : (
        <ul className="rows">
          {q.data.map((p) => (
            <li key={p.name} className="flex flex-wrap items-center gap-3 py-3">
              <span className="min-w-0 flex-1">
                <span className="block text-[14.5px] text-zinc-100">{p.name}</span>
                <span className="font-mono text-[11.5px] text-dim">
                  {p.enabled}/{p.mods} enabled · saved {when(p.saved_at)}
                </span>
              </span>
              <Button size="sm" tone="primary" onClick={() => run("Load profile", () => ipc.profileLoad(p.name))}>
                Load
              </Button>
              <Button
                size="sm"
                tone="danger"
                onClick={() => {
                  if (armed !== p.name) return setArmed(p.name);
                  setArmed(null);
                  void run("Delete profile", () => ipc.profileDelete(p.name));
                }}
              >
                {armed === p.name ? "Press again to delete" : "Delete"}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}
