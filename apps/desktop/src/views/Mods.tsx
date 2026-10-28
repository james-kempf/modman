// SPDX-License-Identifier: GPL-3.0-or-later
// Mods: load order list (search and filters; enable, move, reinstall,
// uninstall, details), file/plugin conflicts, updates and profiles.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errMsg } from "../app/bridge";
import { useActiveGame, useMods } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { useUi } from "../app/store";
import { Dialog } from "../components/dialogs/Dialog";
import {
  ArrowDownIcon,
  ArrowUpIcon,
  ArrowsClockwiseIcon,
  CaretRightIcon,
  DownloadSimpleIcon,
  FloppyDiskIcon,
  FunnelSimpleIcon,
  StackIcon,
  SwordIcon,
  TrashIcon,
} from "@phosphor-icons/react";
import {
  Badge,
  Button,
  Card,
  CardSection,
  Empty,
  ErrorNote,
  Fact,
  IconButton,
  Segmented,
  Select,
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
import type {
  ConflictData,
  ModView,
  ProfileLoaded,
  UninstallResult,
  UpdateReport,
} from "../ipc/types";
import { NoGame } from "../components/shell/NoGame";

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
    <div className="grid grid-cols-[minmax(0,1fr)] gap-6">
      <ViewHeader
        title="Mods"
        eyebrow={game?.name}
        description="Priority runs top to bottom: when two mods ship the same file, the one lower in the list wins."
      >
        {game && <SupportBadge support={game.support} />}
        <Segmented label="Show" value={tab} options={TABS} onChange={setTab} />
      </ViewHeader>
      {!game ? (
        <NoGame icon={<StackIcon size={28} weight="duotone" />} />
      ) : tab === "list" ? (
        <ModList game={game.domain} />
      ) : tab === "conflicts" ? (
        <Conflicts game={game.domain} />
      ) : tab === "updates" ? (
        // key: a report for one game is not shown for another.
        <Updates key={game.domain} game={game.domain} />
      ) : (
        <Profiles game={game.domain} />
      )}
    </div>
  );
}

/** One move at a time: the load order is one list, and a double click must not move a mod twice. */
const MOVE = "Move mod";
const enableKey = (id: number) => `Enable mod ${id}`;

export type StateFilter =
  "all" | "enabled" | "disabled" | "conflicts" | "updates";
/** "" = any collection membership, "-" = in none, "*" = in any, else a collection slug. */
export type CollectionFilter = string;
export interface ModFilter {
  query: string;
  state: StateFilter;
  collection: CollectionFilter;
}
export const NO_FILTER: ModFilter = { query: "", state: "all", collection: "" };

/**
 * Mods matching a filter: the query (case-insensitive) in the name, author
 * or uid, or the Nexus id; the state (conflicts and updates need their
 * data: mod ids in a conflict / with an update); the collection.
 */
export function filterMods(
  mods: ModView[],
  f: ModFilter,
  data: { conflicted?: Set<number>; updated?: Set<number> } = {},
): ModView[] {
  const q = f.query.trim().toLocaleLowerCase();
  return mods.filter((m) => {
    if (
      q &&
      ![m.name, m.author ?? "", m.uid].some((t) =>
        t.toLocaleLowerCase().includes(q),
      ) &&
      String(m.nexus_mod_id ?? "") !== q
    )
      return false;
    switch (f.state) {
      case "enabled":
        if (!m.enabled) return false;
        break;
      case "disabled":
        if (m.enabled) return false;
        break;
      case "conflicts":
        if (!data.conflicted?.has(m.id)) return false;
        break;
      case "updates":
        // No update check yet: the filter is not offered, so it does not narrow.
        if (data.updated && !data.updated.has(m.id)) return false;
        break;
    }
    if (f.collection === "*") return m.collections.length > 0;
    if (f.collection === "-") return m.collections.length === 0;
    return !f.collection || m.collections.includes(f.collection);
  });
}

/** Ids of the mods that win or lose a file, or provide/override a plugin. */
export function conflictedIds(c: ConflictData | undefined): Set<number> {
  const ids = new Set<number>();
  for (const f of c?.files ?? [])
    [f.winner, ...f.losers].forEach((m) => ids.add(m.id));
  for (const p of c?.plugins ?? [])
    if (p.overridden.length)
      [p.provider, ...p.overridden].forEach((m) => m && ids.add(m.id));
  return ids;
}

/** The last update check of a game (set by the Updates tab; never fetched here: it needs the network). */
const useUpdateReport = (game: string) =>
  useQuery<UpdateReport | null>({
    queryKey: ["updates", game],
    queryFn: () => null,
    enabled: false,
    staleTime: Infinity,
  });

function ModList({ game }: { game: string }) {
  const mods = useMods(game);
  const run = useRun();
  const moving = useBusy(MOVE);
  const toggling = useUi((s) => s.busy);
  const [detail, setDetail] = useState<ModView | null>(null);
  const [filter, setFilter] = useState<ModFilter>(NO_FILTER);
  const conflicts = useQuery({
    queryKey: ["conflicts", game],
    queryFn: () => ipc.conflicts(game),
    enabled: filter.state === "conflicts",
  });
  const updates = useUpdateReport(game).data;
  if (mods.isLoading) return <Loading />;
  if (mods.error)
    return (
      <Empty>
        Could not load mods:{" "}
        {String((mods.error as { message?: string }).message ?? mods.error)}
      </Empty>
    );
  const list = mods.data?.mods ?? [];
  if (!list.length)
    return (
      <Empty
        icon={<StackIcon size={28} weight="duotone" />}
        title="No mods installed yet"
      >
        Install a single mod by its Nexus ID or URL, or a whole collection, from
        the Install tab. They appear here in load order.
      </Empty>
    );
  const on = list.filter((m) => m.enabled).length;
  const shown = filterMods(list, filter, {
    conflicted: conflictedIds(conflicts.data),
    updated: updates
      ? new Set(updates.updates.map((u) => u.mod.id))
      : undefined,
  });
  const filtered = shown.length !== list.length;
  return (
    <>
      <ModFilters
        mods={list}
        filter={filter}
        onChange={setFilter}
        hasUpdates={!!updates}
      />
      {mods.data?.changed_since_deploy && (
        <p className="flex items-center gap-2.5 rounded-2xl border border-amber-400/15 bg-amber-400/[0.05] px-4 py-3 text-[14px] text-amber-200">
          <StatusDot tone="amber" live />
          The mod set changed since the last deploy.
        </p>
      )}
      <div className="flex items-center justify-between px-1 font-mono text-[11.5px] text-dim">
        <span role="status">
          {filtered && `${shown.length} of `}
          {list.length} {list.length === 1 ? "mod" : "mods"} · {on} enabled
        </span>
        <span>{bytes(list.reduce((n, m) => n + (m.size ?? 0), 0))} staged</span>
      </div>
      {!shown.length && (
        <Empty
          icon={<FunnelSimpleIcon size={28} weight="duotone" />}
          title="No mods match"
          action={
            <Button
              onClick={() => setFilter(NO_FILTER)}
              data-testid="clear-filters"
            >
              Clear search and filters
            </Button>
          }
        >
          {filter.state === "conflicts" && conflicts.isPending
            ? "Reading conflicts…"
            : "Nothing in the load order matches the search and filters."}
        </Empty>
      )}
      <Card title="Load order">
        <ol aria-label="Load order" className="rows stagger min-w-0">
          {shown.map((m) => (
            <li
              key={m.id}
              className="flex min-h-16 min-w-0 flex-wrap items-center gap-x-3 gap-y-1 py-2 pr-3 pl-2 transition-colors duration-200 hover:bg-white/[0.02]"
            >
              {/* The name keeps about 12rem; on a narrow row the controls wrap below it as one group. */}
              <span className="flex min-w-0 flex-[1_1_18rem] items-center gap-3">
                <span className="w-9 shrink-0 text-right font-mono text-[12px] text-dim">
                  {m.position}
                </span>
                <Thumb
                  game={game}
                  modId={m.nexus_mod_id}
                  name={m.name}
                  className={cx(
                    "transition-opacity",
                    !m.enabled && "opacity-40 grayscale",
                  )}
                />
                <span className="min-w-0 flex-1">
                  <span
                    className={cx(
                      "block truncate text-[15px] transition-colors",
                      m.enabled ? "text-zinc-100" : "text-zinc-400",
                    )}
                    title={m.name}
                  >
                    {m.name}
                  </span>
                  <span className="block truncate font-mono text-[11.5px] text-dim">
                    {m.version ? `v${m.version}` : "—"}
                    {m.author ? ` · ${m.author}` : ""} · {m.file_count} files
                  </span>
                </span>
              </span>
              <span className="ml-auto flex shrink-0 items-center gap-1">
                <span className="hidden items-center gap-2 lg:flex">
                  {m.collections.length > 0 && (
                    <Badge tone="accent">collection</Badge>
                  )}
                  <span className="w-16 text-right font-mono text-[11.5px] text-dim">
                    {bytes(m.size)}
                  </span>
                </span>
                <Toggle
                  label={`${m.enabled ? "Disable" : "Enable"} ${m.name}`}
                  showLabel={false}
                  checked={m.enabled}
                  busy={(toggling[enableKey(m.id)] ?? 0) > 0}
                  onChange={(v) =>
                    run("Enable/disable", () => ipc.modsSetEnabled([m.id], v), {
                      key: enableKey(m.id),
                    })
                  }
                />
                <IconButton
                  label={`Move ${m.name} up`}
                  disabled={m.position <= 1}
                  busy={moving}
                  onClick={() =>
                    run("Move", () => ipc.modMove(m.uid, "up"), { key: MOVE })
                  }
                >
                  <ArrowUpIcon aria-hidden size={18} />
                </IconButton>
                <IconButton
                  label={`Move ${m.name} down`}
                  disabled={m.position >= list.length}
                  busy={moving}
                  onClick={() =>
                    run("Move", () => ipc.modMove(m.uid, "down"), { key: MOVE })
                  }
                >
                  <ArrowDownIcon aria-hidden size={18} />
                </IconButton>
                <Button
                  size="sm"
                  tone="ghost"
                  icon={<CaretRightIcon aria-hidden size={14} />}
                  onClick={() => setDetail(m)}
                >
                  Details
                </Button>
              </span>
            </li>
          ))}
        </ol>
      </Card>
      {detail && (
        <ModDetailDialog
          mod={detail}
          game={game}
          onClose={() => setDetail(null)}
        />
      )}
    </>
  );
}

const STATES: { value: StateFilter; label: string }[] = [
  { value: "all", label: "All mods" },
  { value: "enabled", label: "Enabled" },
  { value: "disabled", label: "Disabled" },
  { value: "conflicts", label: "Has conflicts" },
];

/** Search box and filters over the load order (instant; the list stays in load order). */
function ModFilters({
  mods,
  filter,
  onChange,
  hasUpdates,
}: {
  mods: ModView[];
  filter: ModFilter;
  onChange: (f: ModFilter) => void;
  hasUpdates: boolean;
}) {
  const slugs = [...new Set(mods.flatMap((m) => m.collections))].sort();
  const states = hasUpdates
    ? [...STATES, { value: "updates" as const, label: "Has updates" }]
    : STATES;
  return (
    <div
      role="search"
      aria-label="Filter mods"
      className="flex flex-wrap items-end gap-3"
    >
      <TextField
        label="Search mods"
        type="search"
        name="mod-search"
        placeholder="Name, author or ID"
        value={filter.query}
        onChange={(e) => onChange({ ...filter, query: e.target.value })}
        className="min-w-0 flex-[2_1_16rem]"
      />
      <Select
        label="Filter"
        value={
          states.some((o) => o.value === filter.state) ? filter.state : "all"
        }
        options={states}
        onChange={(state) => onChange({ ...filter, state })}
        className="min-w-0 flex-[1_1_11rem]"
      />
      {slugs.length > 0 && (
        <Select
          label="Collection"
          value={filter.collection}
          options={[
            { value: "", label: "Any" },
            { value: "*", label: "In a collection" },
            { value: "-", label: "Not in a collection" },
            ...slugs.map((c) => ({ value: c, label: c })),
          ]}
          onChange={(collection) => onChange({ ...filter, collection })}
          className="min-w-0 flex-[1_1_11rem]"
        />
      )}
    </div>
  );
}

export function uninstalledText(r: UninstallResult, name: string): string {
  if (!r.done) return `${name} was not uninstalled.`;
  return r.mods === 1 ? `Uninstalled ${name}` : `Uninstalled ${r.mods} mods`;
}

export function profileLoadedText(name: string, r: ProfileLoaded): string {
  const parts = [`Loaded profile ${name}`];
  if (r.missing) parts.push(`${r.missing} of its mods are no longer installed`);
  if (r.added_disabled)
    parts.push(`${r.added_disabled} mods not in it were disabled`);
  return parts.join("; ");
}

export function updatesInstalledText(r: UpdateReport): string {
  if (!r.installed) return "No updates were installed.";
  return `Installed ${r.installed} ${r.installed === 1 ? "update" : "updates"}`;
}

function ModDetailDialog({
  mod,
  game,
  onClose,
}: {
  mod: ModView;
  game: string;
  onClose: () => void;
}) {
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const q = useQuery({
    queryKey: ["mod", game, mod.uid],
    queryFn: () => ipc.modDetail(mod.uid, game),
  });
  const d = q.data;
  return (
    <Dialog
      title={mod.name}
      subtitle={`#${mod.position} · ${mod.uid}`}
      onBack={onClose}
      wide
    >
      <dl className="grid grid-cols-2 gap-x-6 gap-y-4 md:grid-cols-3">
        <Fact k="Version" v={mod.version ?? "?"} />
        <Fact k="Author" v={mod.author ?? "?"} />
        <Fact k="Source" v={mod.source} />
        <Fact k="Installed" v={when(mod.installed_at)} />
        <Fact k="Files" v={`${mod.file_count} · ${bytes(mod.size)}`} />
        <Fact k="Conflicts" v={d ? `wins ${d.wins} · loses ${d.loses}` : "…"} />
      </dl>
      {d?.url && (
        <p className="mt-5 font-mono text-[12.5px] break-all">
          <a
            href={d.url}
            className="text-accent-300 underline underline-offset-4 hover:text-accent-200"
            onClick={(e) => {
              // The webview must not navigate: the backend opens the page in the browser.
              e.preventDefault();
              void run(
                "Open Nexus page",
                () => ipc.modOpenPage(mod.uid, game),
                { key: "Open Nexus page" },
              );
            }}
          >
            {d.url}
          </a>
        </p>
      )}
      {d && d.fomod.length > 0 && (
        <div className="mt-5">
          <h3 className="eyebrow mb-2">Installer choices</h3>
          <ul className="grid gap-1 text-[13px] text-zinc-300">
            {d.fomod.map((s) => (
              <li key={s.step}>
                <span className="text-dim">{s.step}:</span>{" "}
                {s.options.join(", ") || "—"}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="mt-7 flex flex-wrap justify-end gap-2">
        <Button
          icon={<ArrowsClockwiseIcon aria-hidden size={15} />}
          onClick={async () => {
            onClose();
            const r = await run("Reinstall", () =>
              ipc.modReinstall(mod.uid, { fomod: "ask" }),
            );
            if (r)
              notify(
                "success",
                `Reinstalled ${r.installed.name} (${r.installed.files} files)`,
              );
          }}
        >
          Reinstall (choose FOMOD options)
        </Button>
        <Button
          tone="danger"
          icon={<TrashIcon aria-hidden size={15} />}
          onClick={async () => {
            onClose();
            // The backend asks for confirmation.
            const r = await run("Uninstall", () =>
              ipc.modsUninstall([mod.uid], false),
            );
            if (r)
              notify(r.done ? "success" : "info", uninstalledText(r, mod.name));
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
  const q = useQuery({
    queryKey: ["conflicts", game],
    queryFn: () => ipc.conflicts(game),
  });
  if (q.isLoading) return <Loading />;
  if (!q.data) return <Empty>Could not load conflicts.</Empty>;
  const { files, plugins } = q.data;
  return (
    <div className="grid gap-6">
      <Card title={`File conflicts (${files.length})`}>
        <CardSection>
          {!files.length ? (
            <Empty>No two enabled mods provide the same file.</Empty>
          ) : (
            <ul className="rows" aria-label="File conflicts">
              {files.map((c) => (
                <li
                  key={c.rel_path}
                  tabIndex={0}
                  className="flex items-start gap-3 rounded-lg py-3"
                >
                  <SwordIcon
                    aria-hidden
                    size={16}
                    className="mt-0.5 shrink-0 text-dim"
                  />
                  <span className="min-w-0 flex-1">
                    <span className="block font-mono text-[12.5px] break-all text-zinc-200">
                      {c.rel_path}
                    </span>
                    <span className="text-[13.5px] text-zinc-400">
                      <span className="text-accent-300">{c.winner.name}</span>{" "}
                      wins over {c.losers.map((l) => l.name).join(", ")}
                      {c.plugin && " · plugin"}
                    </span>
                  </span>
                </li>
              ))}
            </ul>
          )}
        </CardSection>
      </Card>
      {plugins.length > 0 && (
        <Card title={`Plugins (${plugins.length})`}>
          <CardSection>
            <ol className="rows" aria-label="Plugins">
              {plugins.map((p) => (
                <li
                  key={p.name}
                  tabIndex={0}
                  className="flex flex-wrap items-center gap-3 rounded-lg py-2.5"
                >
                  <span className="w-9 text-right font-mono text-[12px] text-dim">
                    {p.load_index ?? "–"}
                  </span>
                  <span className="min-w-0 flex-1 font-mono text-[13px] text-zinc-200">
                    {p.name}
                  </span>
                  {!p.active && <Badge>inactive</Badge>}
                  {p.provider && (
                    <span className="text-[13px] text-zinc-400">
                      {p.provider.name}
                    </span>
                  )}
                  {p.overridden.length > 0 && (
                    <Badge tone="amber">overrides {p.overridden.length}</Badge>
                  )}
                </li>
              ))}
            </ol>
          </CardSection>
        </Card>
      )}
    </div>
  );
}

function Updates({ game }: { game: string }) {
  const run = useRun();
  const qc = useQueryClient();
  const notify = useUi((s) => s.notify);
  const checking = useBusy("Check updates");
  const installing = useBusy("Install updates");
  // Kept in the query cache, so the Mods list can filter on it ("Has updates").
  const report = useUpdateReport(game).data ?? null;
  const setReport = (r: UpdateReport | null) =>
    qc.setQueryData(["updates", game], r);
  return (
    <Card
      title="Updates"
      actions={
        <>
          <Button
            size="sm"
            busy={checking}
            icon={<ArrowsClockwiseIcon aria-hidden size={15} />}
            onClick={async () =>
              setReport(
                (await run("Check updates", () => ipc.updatesCheck(true))) ??
                  null,
              )
            }
          >
            Check for updates
          </Button>
          <Button
            size="sm"
            tone="primary"
            busy={installing}
            icon={<DownloadSimpleIcon aria-hidden size={15} />}
            disabled={!report?.updates.length}
            onClick={async () => {
              const r = await run("Install updates", () =>
                ipc.updatesInstall(),
              );
              if (!r) return;
              setReport(r);
              notify(r.installed ? "success" : "info", updatesInstalledText(r));
            }}
          >
            Install updates
          </Button>
        </>
      }
    >
      <CardSection>
        {!report ? (
          <Empty>Check Nexus for newer files of your mods.</Empty>
        ) : (
          <>
            <p className="mb-4 font-mono text-[12px] text-dim">
              Checked {report.checked} mods
              {report.stopped_early && " (stopped early: rate limit)"}
              {report.installed > 0 && `, installed ${report.installed}`}.
            </p>
            {!report.updates.length ? (
              <Empty>Everything is up to date.</Empty>
            ) : (
              <ul className="rows">
                {report.updates.map((u) => (
                  <li
                    key={u.mod.id}
                    tabIndex={0}
                    className="flex flex-wrap items-center gap-2 rounded-lg py-3"
                  >
                    <span className="text-[14.5px] text-zinc-100">
                      {u.mod.name}
                    </span>{" "}
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
      </CardSection>
    </Card>
  );
}

function Profiles({ game }: { game: string }) {
  const q = useQuery({
    queryKey: ["profiles", game],
    queryFn: () => ipc.profilesList(game),
  });
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const saving = useBusy("Save profile");
  const [name, setName] = useState("");
  // Deleting is not confirmed by the backend: press twice.
  const [armed, setArmed] = useState<string | null>(null);
  return (
    <Card title="Profiles">
      <CardSection>
        <form
          className="mb-5 flex flex-wrap items-end gap-3"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!name.trim()) return;
            const n = name.trim();
            const r = await run("Save profile", () => ipc.profileSave(n), {
              key: "Save profile",
            });
            // undefined = failed (error notice shown), null = overwrite declined: keep the name either way.
            if (r === undefined) return;
            if (r === null)
              return notify("info", `Profile ${n} was not overwritten.`);
            notify(
              "success",
              `Saved profile ${n} (${r} ${r === 1 ? "mod" : "mods"})`,
            );
            setName("");
          }}
        >
          <TextField
            label="Save the current mod set as"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="min-w-0 flex-[1_1_18rem]"
          />
          <Button
            type="submit"
            tone="primary"
            busy={saving}
            inactive={!name.trim()}
            icon={<FloppyDiskIcon aria-hidden size={15} />}
          >
            Save profile
          </Button>
        </form>
        {q.isError ? (
          <ErrorNote>Could not load profiles: {errMsg(q.error)}</ErrorNote>
        ) : !q.data?.length ? (
          <Empty>No saved profiles.</Empty>
        ) : (
          <ul className="rows">
            {q.data.map((p) => (
              <li
                key={p.name}
                className="flex flex-wrap items-center gap-3 py-3"
              >
                <span className="min-w-0 flex-1">
                  <span className="block text-[14.5px] wrap-anywhere text-zinc-100">
                    {p.name}
                  </span>
                  <span className="font-mono text-[11.5px] text-dim">
                    {p.enabled}/{p.mods} enabled · saved {when(p.saved_at)}
                  </span>
                </span>
                <Button
                  size="sm"
                  tone="primary"
                  onClick={async () => {
                    const r = await run(
                      "Load profile",
                      () => ipc.profileLoad(p.name),
                      { key: "Load profile" },
                    );
                    if (r) notify("success", profileLoadedText(p.name, r));
                  }}
                >
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
      </CardSection>
    </Card>
  );
}
