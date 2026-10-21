// SPDX-License-Identifier: GPL-3.0-or-later
// Mods: load order list (enable, move, reinstall, uninstall, details),
// file/plugin conflicts, updates and profiles.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useActiveGame, useMods } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { Dialog } from "../components/Dialog";
import { Badge, Button, Card, Empty, Segmented, SupportBadge, TextField, Thumb, ViewHeader, bytes, when } from "../components/ui";
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
      <ViewHeader title="Mods">
        {game && <SupportBadge support={game.support} />}
        <Segmented label="Show" value={tab} options={TABS} onChange={setTab} />
      </ViewHeader>
      {!game ? (
        <Empty>No active game. Add one on the Overview.</Empty>
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
  if (mods.isLoading) return <p className="text-zinc-400">Loading…</p>;
  if (mods.error) return <Empty>Could not load mods: {String((mods.error as { message?: string }).message ?? mods.error)}</Empty>;
  const list = mods.data?.mods ?? [];
  if (!list.length) return <Empty>No mods installed yet. Use the Install tab.</Empty>;
  return (
    <>
      {mods.data?.changed_since_deploy && (
        <p className="rounded-xl border border-amber-400/30 bg-amber-950/40 px-4 py-3 text-amber-200">
          The mod set changed since the last deploy.
        </p>
      )}
      <ol aria-label="Load order" className="grid gap-2">
        {list.map((m, i) => (
          <li key={m.id} className="surface flex flex-wrap items-center gap-3 p-3">
            <span className="w-10 text-right font-mono text-zinc-500">{m.position}</span>
            <Thumb game={game} modId={m.nexus_mod_id} />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-zinc-100">{m.name}</span>
              <span className="block text-sm text-zinc-500">
                {m.version ? `v${m.version} · ` : ""}
                {m.file_count} files · {bytes(m.size)}
                {m.collections.length > 0 && ` · ${m.collections.join(", ")}`}
              </span>
            </span>
            <Button
              aria-label={`${m.enabled ? "Disable" : "Enable"} ${m.name}`}
              aria-pressed={m.enabled}
              tone={m.enabled ? "primary" : "default"}
              onClick={() => run("Enable/disable", () => ipc.modsSetEnabled([m.id], !m.enabled))}
            >
              {m.enabled ? "Enabled" : "Disabled"}
            </Button>
            <Button aria-label={`Move ${m.name} up`} disabled={i === 0} onClick={() => run("Move", () => ipc.modMove(m.uid, "up"))}>
              ↑
            </Button>
            <Button
              aria-label={`Move ${m.name} down`}
              disabled={i === list.length - 1}
              onClick={() => run("Move", () => ipc.modMove(m.uid, "down"))}
            >
              ↓
            </Button>
            <Button onClick={() => setDetail(m)}>Details</Button>
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
      <dl className="grid grid-cols-2 gap-3 text-sm md:grid-cols-3">
        <Field k="Version" v={mod.version ?? "?"} />
        <Field k="Author" v={mod.author ?? "?"} />
        <Field k="Source" v={mod.source} />
        <Field k="Installed" v={when(mod.installed_at)} />
        <Field k="Files" v={`${mod.file_count} (${bytes(mod.size)})`} />
        <Field k="Conflicts" v={d ? `wins ${d.wins} · loses ${d.loses}` : "…"} />
      </dl>
      {d?.url && <p className="mt-3 font-mono text-sm break-all text-zinc-400 select-all">{d.url}</p>}
      {d && d.fomod.length > 0 && (
        <div className="mt-4">
          <h3 className="eyebrow mb-2">FOMOD choices</h3>
          <ul className="grid gap-1 text-sm text-zinc-300">
            {d.fomod.map((s) => (
              <li key={s.step}>
                {s.step}: {s.options.join(", ")}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="mt-6 flex flex-wrap justify-end gap-3">
        <Button
          onClick={() => {
            onClose();
            void run("Reinstall", () => ipc.modReinstall(mod.uid, { fomod: "ask" }));
          }}
        >
          Reinstall (choose FOMOD options)
        </Button>
        <Button
          tone="danger"
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

function Field({ k, v }: { k: string; v: string }) {
  return (
    <div>
      <dt className="text-zinc-500">{k}</dt>
      <dd className="text-zinc-200">{v}</dd>
    </div>
  );
}

function Conflicts({ game }: { game: string }) {
  const q = useQuery({ queryKey: ["conflicts", game], queryFn: () => ipc.conflicts(game) });
  if (q.isLoading) return <p className="text-zinc-400">Loading…</p>;
  if (!q.data) return <Empty>Could not load conflicts.</Empty>;
  const { files, plugins } = q.data;
  return (
    <div className="grid gap-6">
      <Card title={`File conflicts (${files.length})`}>
        {!files.length ? (
          <Empty>No two enabled mods provide the same file.</Empty>
        ) : (
          <ul className="grid gap-2" aria-label="File conflicts">
            {files.map((c) => (
              <li key={c.rel_path} tabIndex={0} className="rounded-xl border border-white/[0.06] p-3">
                <span className="block font-mono text-sm break-all text-zinc-200">{c.rel_path}</span>
                <span className="text-sm text-zinc-400">
                  <span className="text-accent-300">{c.winner.name}</span> wins over {c.losers.map((l) => l.name).join(", ")}
                  {c.plugin && " · plugin"}
                </span>
              </li>
            ))}
          </ul>
        )}
      </Card>
      {plugins.length > 0 && (
        <Card title={`Plugins (${plugins.length})`}>
          <ol className="grid gap-1" aria-label="Plugins">
            {plugins.map((p) => (
              <li key={p.name} tabIndex={0} className="flex flex-wrap items-center gap-3 rounded-lg px-3 py-2">
                <span className="w-10 text-right font-mono text-zinc-500">{p.load_index ?? "–"}</span>
                <span className="min-w-0 flex-1 text-zinc-200">{p.name}</span>
                {!p.active && <Badge>inactive</Badge>}
                {p.provider && <span className="text-sm text-zinc-400">{p.provider.name}</span>}
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
          <Button busy={checking} onClick={async () => setReport((await run("Check updates", () => ipc.updatesCheck(true))) ?? null)}>
            Check for updates
          </Button>
          <Button
            tone="primary"
            busy={installing}
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
          <p className="mb-3 text-zinc-400">
            Checked {report.checked} mods{report.stopped_early && " (stopped early: rate limit)"}
            {report.installed > 0 && `, installed ${report.installed}`}.
          </p>
          {!report.updates.length ? (
            <Empty>Everything is up to date.</Empty>
          ) : (
            <ul className="grid gap-2">
              {report.updates.map((u) => (
                <li key={u.mod.id} tabIndex={0} className="rounded-xl border border-white/[0.06] p-3">
                  <span className="text-zinc-100">{u.mod.name}</span>{" "}
                  <span className="text-zinc-400">
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
        <Button type="submit" tone="primary" disabled={!name.trim()}>
          Save profile
        </Button>
      </form>
      {!q.data?.length ? (
        <Empty>No saved profiles.</Empty>
      ) : (
        <ul className="grid gap-2">
          {q.data.map((p) => (
            <li key={p.name} className="flex flex-wrap items-center gap-3 rounded-xl border border-white/[0.06] p-3">
              <span className="min-w-0 flex-1">
                <span className="block text-zinc-100">{p.name}</span>
                <span className="text-sm text-zinc-500">
                  {p.enabled}/{p.mods} enabled · saved {when(p.saved_at)}
                </span>
              </span>
              <Button tone="primary" onClick={() => run("Load profile", () => ipc.profileLoad(p.name))}>
                Load
              </Button>
              <Button
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
