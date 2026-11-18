// SPDX-License-Identifier: GPL-3.0-or-later
// Mods: load order list (search and filters; enable, move, reinstall,
// uninstall, details), file/plugin conflicts, updates and profiles.

import { showMsg, showToast, type ToastMsg } from "../app/toast";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { errMsg } from "../app/bridge";
import { useActiveGame, useMods } from "../app/queries";
import { useBusy, useDeploy, useRun } from "../app/run";
import {
  NO_FILTER,
  useUi,
  type ModFilter,
  type ModsTab,
  type StateFilter,
} from "../app/store";
import { ModDetailsDialog } from "../components/dialogs/BrowseDetails";
import { PAGE_EVENT, openView, type PageEventDetail } from "../nav/actions";
import {
  ArrowDownIcon,
  ArrowRightIcon,
  ArrowUpIcon,
  ArrowsClockwiseIcon,
  CaretRightIcon,
  CheckIcon,
  DownloadSimpleIcon,
  FloppyDiskIcon,
  FunnelSimpleIcon,
  StackIcon,
  SwordIcon,
} from "@phosphor-icons/react";
import {
  Badge,
  Button,
  Card,
  CardSection,
  Empty,
  ErrorNote,
  IconButton,
  LIST_ROW,
  Pagination,
  SKELETON_DELAY_MS,
  SearchRow,
  Select,
  StatusDot,
  TextField,
  Thumb,
  Tabs,
  Toggle,
  VIEW_FRAME,
  ViewFrame,
  bytes,
  cx,
  when,
} from "../components/ui";
import * as ipc from "../ipc/commands";
import type {
  ConflictData,
  InstalledLoader,
  ModView,
  ProfileLoaded,
  SearchHit,
  UninstallResult,
  UpdateReport,
} from "../ipc/types";
import { NoGame } from "../components/shell/NoGame";
import { BulkBar, type BulkAction } from "./mods/BulkBar";
import { useSelection } from "./mods/useSelection";
import { MODS_PAGE_SIZE, clampPage, pageCount, pageSlice } from "./mods/paginate";

// "Load order", not "Mods": the app's view bar already has a tab named Mods.
const TABS = [
  { value: "list", label: "Load order" },
  { value: "conflicts", label: "Conflicts" },
  { value: "updates", label: "Updates" },
  { value: "profiles", label: "Profiles" },
] as const satisfies readonly { value: ModsTab; label: string }[];

export function Mods() {
  const { game } = useActiveGame();
  // In the store: the tab (and the list's filters) survive leaving the view.
  const tab = useUi((s) => s.modsUi.tab);
  const setTab = useUi((s) => s.setModsTab);
  const panel = useId();
  return (
    <ViewFrame>
      {/* Focusable roving tabs, not LB/RB-bound: Tabs listens for LB/RB only inside a
          data-nav-scope (a dialog), so in a view LB/RB keep switching the app's views. Hence
          controllerFocusable: the D-pad is the only way a controller reaches these tabs. */}
      <Tabs
        label="Mods sections"
        focusable
        controllerFocusable
        tabs={TABS}
        value={tab}
        onChange={setTab}
        controls={panel}
      />
      <div
        id={panel}
        role="tabpanel"
        aria-label={TABS.find((t) => t.value === tab)?.label}
        className={VIEW_FRAME}
      >
      {!game ? (
        <NoGame icon={<StackIcon size={28} weight="duotone" />} />
      ) : tab === "list" ? (
        <ModList game={game.id} />
      ) : tab === "conflicts" ? (
        <Conflicts game={game.id} />
      ) : tab === "updates" ? (
        // key: a report for one game is not shown for another.
        <Updates key={game.id} game={game.id} />
      ) : (
        <Profiles game={game.id} />
      )}
      </div>
    </ViewFrame>
  );
}

/** One move at a time: the load order is one list, and a double click must not move a mod twice. */
const MOVE = "Move mod";
/** One bulk action at a time (the bar's buttons share it). */
const BULK = "Bulk action";
const enableKey = (id: number) => `Enable mod ${id}`;

export { NO_FILTER };
export type { CollectionFilter, ModFilter, StateFilter } from "../app/store";

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
  const deploy = useDeploy();
  const deploying = useBusy("Deploy");
  const moving = useBusy(MOVE);
  const toggling = useUi((s) => s.busy);
  const [detail, setDetail] = useState<ModView | null>(null);
  const filter = useUi((s) => s.modsUi.filters[game]) ?? NO_FILTER;
  const saveFilter = useUi((s) => s.setModsFilter);
  const setFilter = (f: ModFilter) => saveFilter(game, f);
  // Optimistic enabled states (mod id -> shown state) while a change runs; dropped on failure (rollback).
  const [optimistic, setOptimistic] = useState<Record<number, boolean>>({});
  const dropOptimistic = (id: number) =>
    setOptimistic(({ [id]: _, ...rest }) => rest);
  const setEnabled = async (m: ModView, on: boolean) => {
    if ((useUi.getState().busy[enableKey(m.id)] ?? 0) > 0) return;
    setOptimistic((o) => ({ ...o, [m.id]: on }));
    const r = await run(
      "Enable/disable",
      () => ipc.modsSetEnabled([m.id], on),
      { key: enableKey(m.id) },
    );
    // Failed: run showed the error notice; the switch flips back. Done: hold the new state until the list has it.
    if (r !== undefined) await mods.refetch();
    dropOptimistic(m.id);
  };
  // After an uninstall: where focus goes once the mod has left the list (a neighbour's Details, else the heading).
  const [refocus, setRefocus] = useState<{
    gone: string;
    next: string | null;
  } | null>(null);
  useEffect(() => {
    if (!refocus || mods.data?.mods.some((m) => m.uid === refocus.gone))
      return;
    setRefocus(null);
    const next =
      refocus.next &&
      document.querySelector<HTMLElement>(
        `[data-mod-details="${refocus.next}"]`,
      );
    if (next) return next.focus();
    const heading = document.querySelector<HTMLElement>("[data-mods-heading] h2");
    if (!heading) return;
    heading.tabIndex = -1;
    heading.focus();
  }, [refocus, mods.data]);
  const conflicts = useQuery({
    queryKey: ["conflicts", game],
    queryFn: () => ipc.conflicts(game),
    enabled: filter.state === "conflicts",
  });
  const updates = useUpdateReport(game).data;
  const list = mods.data?.mods ?? [];
  const shown = filterMods(list, filter, {
    conflicted: conflictedIds(conflicts.data),
    updated: updates
      ? new Set(updates.updates.map((u) => u.mod.id))
      : undefined,
  });
  // Selection over the shown rows: a row hidden by a filter or removed leaves it.
  const shownKey = shown.map((m) => m.uid).join("\n");
  const shownUids = useMemo(() => (shownKey ? shownKey.split("\n") : []), [shownKey]);
  const sel = useSelection(shownUids);
  // Client-side paging: mods_list returns the whole load order. A changed search or filter starts at page 1
  // (the page is stored with the filter it was chosen under), and the page is clamped on every render so a
  // list that shrank (uninstall, refetch) never shows an empty page.
  const filterKey = [game, filter.query, filter.state, filter.collection].join("\n");
  const [paging, setPaging] = useState({ key: filterKey, page: 0 });
  const pages = pageCount(shown.length, MODS_PAGE_SIZE);
  const page = clampPage(paging.key === filterKey ? paging.page : 0, pages);
  const pageRows = pageSlice(shown, page, MODS_PAGE_SIZE);
  const pageStart = page * MODS_PAGE_SIZE;
  // What takes focus once a page change has rendered (the new page's first row, or a moved row's arrow).
  const pageFocus = useRef<(() => HTMLElement | null) | null>(null);
  const setPage = (to: number, focus?: () => HTMLElement | null) => {
    if (to === page) return;
    pageFocus.current = focus ?? (() => document.querySelector<HTMLElement>("[data-mod-select]"));
    setPaging({ key: filterKey, page: to });
  };
  useEffect(() => {
    const pick = pageFocus.current;
    pageFocus.current = null;
    pick?.()?.focus();
  }, [page]);
  // LT/RT page, as on Browse (the latest closure: the page and list change every render).
  const stepPage = useRef((_: 1 | -1) => {});
  stepPage.current = (delta) => {
    const to = page + delta;
    if (to >= 0 && to < pages) setPage(to);
  };
  useEffect(() => {
    const onPage = (e: Event) => stepPage.current((e as CustomEvent<PageEventDetail>).detail.delta);
    document.addEventListener(PAGE_EVENT, onPage);
    return () => document.removeEventListener(PAGE_EVENT, onPage);
  }, []);
  const [bulkBusy, setBulkBusy] = useState<BulkAction | null>(null);
  // Polite announcements (moves, selection counts) inside the list's status line.
  const [said, say] = useState("");
  // The row focus returns to when the bulk bar hides (the last one toggled, or a neighbour of removed rows).
  const anchor = useRef<string | null>(null);
  const prevCount = useRef(0);
  useEffect(() => {
    const prev = prevCount.current;
    prevCount.current = sel.count;
    if (prev === sel.count) return;
    say(sel.count ? `${sel.count} selected` : "Selection cleared");
    // The bar unmounted with focus in it: focus fell to <body>, so put it back on a row.
    const lost = !document.activeElement || document.activeElement === document.body;
    if (sel.count || !prev || !lost) return;
    const target =
      document.querySelector<HTMLElement>(`[data-mod-select="${anchor.current}"]`) ??
      document.querySelector<HTMLElement>("[data-mod-select]");
    if (target) target.focus();
    else holdListFocus();
  }, [sel.count]);
  // A moved row's control keeps focus: if the reorder dropped it to <body>, focus it again.
  const keep = useRef<HTMLElement | null>(null);
  const keepUid = useRef<string | null>(null);
  useEffect(() => {
    const el = keep.current;
    if (!el) return;
    const lost = !document.activeElement || document.activeElement === document.body;
    if (!lost) return;
    if (el.isConnected) return el.focus();
    // The move carried the row onto another page: follow it there and focus the same arrow.
    const i = shown.findIndex((m) => m.uid === keepUid.current);
    const name = el.getAttribute("aria-label");
    if (i < 0 || !name) return;
    setPage(Math.floor(i / MODS_PAGE_SIZE), () =>
      [...document.querySelectorAll<HTMLElement>("[aria-label]")].find((b) => b.getAttribute("aria-label") === name) ?? null,
    );
  }, [mods.data]);
  const move = async (m: ModView, spec: "before" | "after", arg: string) => {
    keep.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    keepUid.current = m.uid;
    const r = await run("Move", () => ipc.modMove(m.uid, spec, arg, game), { key: MOVE });
    if (r) say(`${m.name} moved to position ${r.to} of ${r.of}`);
  };
  const bulk = async <T,>(action: BulkAction, fn: () => Promise<T>) => {
    if ((useUi.getState().busy[BULK] ?? 0) > 0) return undefined;
    setBulkBusy(action);
    try {
      return await run(action === "uninstall" ? "Uninstall" : "Enable/disable", fn, { key: BULK });
    } finally {
      setBulkBusy(null);
    }
  };
  const bulkEnable = async (on: boolean) => {
    const ids = shown.filter((m) => sel.has(m.uid)).map((m) => m.id);
    const r = await bulk(on ? "enable" : "disable", () => ipc.modsSetEnabled(ids, on, game));
    if (r !== undefined) await mods.refetch();
  };
  const bulkUninstall = async () => {
    const uids = sel.list;
    // Focus lands next to the removed block: the first unselected row after it, else before it.
    const first = shown.findIndex((m) => sel.has(m.uid));
    const rest = (xs: ModView[]) => xs.find((m) => !sel.has(m.uid));
    const near = rest(shown.slice(first + 1)) ?? rest(shown.slice(0, first).reverse());
    // The backend asks: "Uninstall N mods?" with "Delete archive too" (off by default), so never true here.
    const r = await bulk("uninstall", () => ipc.modsUninstall(uids, false, game));
    if (!r) return;
    const one = uids.length === 1 ? list.find((m) => m.uid === uids[0])?.name : undefined;
    showMsg(r.done ? "success" : "info", uninstalledMsg(r, one ?? `${uids.length} mods`));
    if (!r.done) return;
    anchor.current = near?.uid ?? null;
    sel.clear();
  };
  if (mods.isLoading) return <Loading />;
  if (mods.error)
    return (
      <ReadError
        what="the mod list"
        error={mods.error}
        retrying={mods.isFetching}
        onRetry={() => void mods.refetch()}
      />
    );
  // Older mocks and stale caches may lack the field.
  const loaders = mods.data?.loaders ?? [];
  const loaderCard = loaders.length > 0 && (
    <LoaderList game={game} loaders={loaders} />
  );
  if (!list.length)
    return (
      <>
        {loaderCard}
        <div data-mods-heading>
          <Empty
            icon={<StackIcon size={28} weight="duotone" />}
            title="No mods installed yet"
          >
            Install a single mod by its Nexus ID or URL, or a whole collection,
            from the Install tab. They appear here in load order.
          </Empty>
        </div>
      </>
    );
  const on = list.filter((m) => m.enabled).length;
  const filtered = shown.length !== list.length;
  // Under "Has conflicts" a failed conflicts read is an error, not "nothing matches".
  const conflictsFailed = filter.state === "conflicts" && conflicts.isError;
  return (
    <>
      <ModFilters
        mods={list}
        filter={filter}
        onChange={setFilter}
        hasUpdates={!!updates}
      />
      {mods.data?.changed_since_deploy && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-card border border-amber-400/15 bg-amber-400/10 py-2 pr-2 pl-4 text-body text-amber-300">
          <StatusDot tone="amber" live />
          <span className="min-w-0 flex-1">
            The mod set changed since the last deploy.
          </span>
          <Button size="sm" busy={deploying} onClick={() => void deploy(game)}>
            Deploy now
          </Button>
        </div>
      )}
      <div className="flex items-center justify-between px-1 font-mono text-caption text-dim tabular-nums">
        <span role="status">
          {filtered && `${shown.length} of `}
          {list.length} {list.length === 1 ? "mod" : "mods"} · {on} enabled
          <span className="sr-only" data-testid="mods-announce">
            {said && ` · ${said}`}
          </span>
        </span>
        <span>{bytes(list.reduce((n, m) => n + (m.size ?? 0), 0))} staged</span>
      </div>
      {conflictsFailed ? (
        <ReadError
          what="conflicts"
          error={conflicts.error}
          retrying={conflicts.isFetching}
          onRetry={() => void conflicts.refetch()}
        />
      ) : (
        !shown.length && (
          <div data-mods-heading>
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
              : noMatchText(filter, !!updates)}
          </Empty>
          </div>
        )
      )}
      {loaderCard}
      {/* An empty Load order card under the no-match state would only repeat it. */}
      {shown.length > 0 && (
      <Card title="Load order" intro="Lower in the list wins when two mods ship the same file.">
        <ol aria-label="Load order" className="rows stagger min-w-0">
          {pageRows.map((m, j) => {
            // Arrows follow the visible list (screen direction); under a filter
            // a move lands next to the visible neighbor, past hidden mods. Neighbors
            // come from the whole filtered list, so the edge rows of a page move across pages.
            const i = pageStart + j;
            const prev = shown[i - 1];
            const next = shown[i + 1];
            const enabled = optimistic[m.id] ?? m.enabled;
            return (

            <li
              key={m.id}
              className="flex min-h-16 min-w-0 flex-wrap items-center gap-x-3 gap-y-1 py-2 pr-3 pl-2"
            >
              {/* The name keeps about 12rem; on a narrow row the controls wrap below it as one group. */}
              <span className="flex min-w-0 flex-[1_1_18rem] items-center gap-3">
                <span className="w-9 shrink-0 text-right font-mono text-caption text-dim tabular-nums">
                  {m.position}
                </span>
                <Thumb
                  game={game}
                  modId={m.nexus_mod_id}
                  name={m.name}
                  className={cx(
                    "transition-opacity duration-base ease-out",
                    !enabled && "opacity-stale grayscale",
                  )}
                />
                <span className="min-w-0 flex-1">
                  <span
                    className={cx(
                      "block truncate text-lead",
                      enabled ? "text-fg" : "text-fg-2",
                    )}
                    title={m.name}
                  >
                    {m.name}
                  </span>
                  <span className="block truncate font-mono text-caption text-dim tabular-nums">
                    {m.version ? `v${m.version}` : "—"}
                    {m.author ? ` · ${m.author}` : ""} · {m.file_count} files
                  </span>
                </span>
              </span>
              <span className="ml-auto flex shrink-0 items-center gap-1">
                <span className="hidden items-center gap-2 lg:flex">
                  {m.collections.length > 0 && (
                    <Badge>collection</Badge>
                  )}
                  <span className="w-16 text-right font-mono text-caption text-dim tabular-nums">
                    {bytes(m.size)}
                  </span>
                </span>
                <SelectBox
                  name={m.name}
                  uid={m.uid}
                  checked={sel.has(m.uid)}
                  onToggle={() => {
                    anchor.current = m.uid;
                    sel.toggle(m.uid);
                  }}
                />
                {/* Optimistic: the switch flips at once and spins until the job ends. The name
                    follows the confirmed state, so it names the change in flight while busy. */}
                <Toggle
                  label={`${m.enabled ? "Disable" : "Enable"} ${m.name}`}
                  showLabel={false}
                  checked={enabled}
                  busy={(toggling[enableKey(m.id)] ?? 0) > 0}
                  onChange={(v) => void setEnabled(m, v)}
                />
                <IconButton
                  label={`Move ${m.name} up`}
                  icon={ArrowUpIcon}
                  size={20}
                  inactive={!prev}
                  busy={moving}
                  onClick={() => prev && void move(m, "before", prev.uid)}
                />
                <IconButton
                  label={`Move ${m.name} down`}
                  icon={ArrowDownIcon}
                  size={20}
                  inactive={!next}
                  busy={moving}
                  onClick={() => next && void move(m, "after", next.uid)}
                />
                <Button
                  size="sm"
                  tone="ghost"
                  icon={<CaretRightIcon aria-hidden size={14} />}
                  data-mod-details={m.uid}
                  onClick={() => setDetail(m)}
                >
                  Details
                </Button>
              </span>
            </li>
            );
          })}
        </ol>
        {/* The bulk bar and the pager both pin to the bottom of <main>: one sticky footer holds them so they
            stack (the bar above the pager) instead of pinning on top of each other. Their own sticky is inert
            inside it (each already sits within the pinned footer). Rendered only with content: an empty child
            would still draw the card's divider. */}
        {(sel.count > 0 || pages > 1) && (
          <div className="sticky bottom-[calc(var(--main-pb,0px)*-1)] z-10 rounded-b-card">
            <BulkBar
              count={sel.count}
              busy={bulkBusy}
              onEnable={() => void bulkEnable(true)}
              onDisable={() => void bulkEnable(false)}
              onUninstall={() => void bulkUninstall()}
              onClear={sel.clear}
              className={pages > 1 ? undefined : "rounded-b-card"}
            />
            {pages > 1 && (
              <Pagination
                label="Load order pages"
                page={page}
                pages={pages}
                size={MODS_PAGE_SIZE}
                total={shown.length}
                onPage={(to) => setPage(to)}
              />
            )}
          </div>
        )}
      </Card>
      )}
      {detail && (
        <ModDetailDialog
          mod={detail}
          game={game}
          onClose={() => setDetail(null)}
          onUninstalled={(uid) => {
            const i = shown.findIndex((m) => m.uid === uid);
            const near = i < 0 ? undefined : (shown[i + 1] ?? shown[i - 1]);
            setRefocus({ gone: uid, next: near?.uid ?? null });
          }}
        />
      )}
    </>
  );
}

/**
 * Mods installed as a loader (e.g. SMAPI). They sit outside the load order;
 * the Requirements view manages them (repair, uninstall).
 */
function LoaderList({
  game,
  loaders,
}: {
  game: string;
  loaders: InstalledLoader[];
}) {
  return (
    <Card
      title="Loaders"
      actions={
        <Button
          size="sm"
          tone="ghost"
          icon={<ArrowRightIcon aria-hidden size={16} />}
          onClick={() => openView("requirements")}
        >
          Manage in Requirements
        </Button>
      }
    >
      <ul aria-label="Loaders" className="rows min-w-0">
        {loaders.map((l) => (
          <li
            key={l.key}
            className="flex min-h-16 min-w-0 items-center gap-3 py-2 pr-3 pl-2"
          >
            <span className="w-9 shrink-0" />
            <Thumb game={game} modId={l.nexus_mod_id} name={l.name} />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-lead text-fg" title={l.name}>
                {l.name}
              </span>
              <span className="block truncate font-mono text-caption text-dim">
                {l.version ? `v${l.version}` : "—"} · loader
              </span>
            </span>
            <Badge tone={l.status === "installed" ? "accent" : "amber"}>
              {l.status}
            </Badge>
          </li>
        ))}
      </ul>
    </Card>
  );
}

const STATES: { value: StateFilter; label: string }[] = [
  { value: "all", label: "All mods" },
  { value: "enabled", label: "Enabled" },
  { value: "disabled", label: "Disabled" },
  { value: "conflicts", label: "Has conflicts" },
];

/** The no-match copy, naming each active search/filter so the user knows what to loosen. */
export function noMatchText(f: ModFilter, hasUpdates = false): string {
  const parts: string[] = [];
  const q = f.query.trim();
  if (q) parts.push(`search “${q}”`);
  if (f.state !== "all") {
    const label =
      f.state === "updates"
        ? hasUpdates && "Has updates"
        : STATES.find((o) => o.value === f.state)?.label;
    if (label) parts.push(label);
  }
  if (f.collection === "*") parts.push("In a collection");
  else if (f.collection === "-") parts.push("Not in a collection");
  else if (f.collection) parts.push(`collection “${f.collection}”`);
  return parts.length
    ? `Nothing in the load order passes: ${parts.join(", ")}.`
    : "Nothing in the load order matches the search and filters.";
}

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
    // The search is centered in the row (SearchRow); the filters fill the end column.
    <div role="search" aria-label="Filter mods">
      <SearchRow
        search={
          <TextField
            label="Search mods"
            type="search"
            name="mod-search"
            placeholder="Name, author or ID"
            value={filter.query}
            onChange={(e) => onChange({ ...filter, query: e.target.value })}
            className="w-96 max-w-full"
          />
        }
        end={
          <div className="flex flex-wrap items-end gap-3">
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
        }
      />
    </div>
  );
}

/** The uninstall notice; names the deleted archive(s) when the confirm's "Delete archive too" was ticked. */
export function uninstalledMsg(r: UninstallResult, name: string): ToastMsg {
  if (!r.done) return { title: "Kept", detail: `${name} was not uninstalled` };
  const n = r.archives.length;
  if (r.mods === 1) return { title: "Uninstalled", detail: n ? `${name} · archive deleted` : name };
  return {
    title: "Uninstalled",
    detail: n ? `${r.mods} mods · ${n} ${n === 1 ? "archive" : "archives"} deleted` : `${r.mods} mods`,
  };
}

export function profileLoadedMsg(name: string, r: ProfileLoaded): ToastMsg {
  const parts = [`Profile ${name}`];
  if (r.missing) parts.push(`${r.missing} of its mods are no longer installed`);
  if (r.added_disabled)
    parts.push(`${r.added_disabled} mods not in it were disabled`);
  return { title: "Loaded", detail: parts.join(" · ") };
}

export function updatesInstalledMsg(r: UpdateReport): ToastMsg {
  if (!r.installed) return { title: "No updates", detail: "Nothing was installed" };
  return { title: "Updated", detail: `${r.installed} ${r.installed === 1 ? "update" : "updates"} installed` };
}

/** Focus fell out of the list (its rows went away): hold it on the list heading, else the Mods tab. */
function holdListFocus() {
  const heading = document.querySelector<HTMLElement>("[data-mods-heading] h2");
  const target =
    heading ??
    document.querySelector<HTMLElement>('[role="tablist"][aria-label="Mods sections"] [aria-selected="true"]');
  if (!target) return;
  if (target === heading) heading.tabIndex = -1;
  target.focus();
}

/**
 * A row's select control for bulk actions: a checkbox-style button (its own
 * focus stop, pad-reachable), named "Select <mod>".
 */
function SelectBox({
  name,
  uid,
  checked,
  onToggle,
}: {
  name: string;
  uid: string;
  checked: boolean;
  onToggle: () => void;
}) {
  return (
    <Button
      tone="ghost"
      size="icon"
      role="checkbox"
      aria-checked={checked}
      aria-label={`Select ${name}`}
      title={`Select ${name}`}
      data-mod-select={uid}
      onClick={onToggle}
    >
      <span
        aria-hidden
        className={cx(
          "grid size-icon place-items-center rounded-inset border-2",
          checked
            ? "border-accent-400 bg-accent-400 text-accent-950 in-highlight:border-accent-700 in-highlight:bg-accent-700 in-highlight:text-white"
            : "border-zinc-500",
        )}
      >
        {checked && <CheckIcon size={12} weight="bold" />}
      </span>
    </Button>
  );
}

/** An installed mod's details: the Browse mod details dialog, for the mod's row. */
function ModDetailDialog({
  mod,
  game,
  onClose,
  onUninstalled,
}: {
  mod: ModView;
  game: string;
  onClose: () => void;
  /** The mod was uninstalled (the caller moves focus off its row). */
  onUninstalled?: (uid: string) => void;
}) {
  // Only what the mods row knows: the Nexus-only stats (picture, downloads, endorsements, updated) stay unknown.
  const hit: SearchHit = {
    modId: mod.nexus_mod_id,
    name: mod.name,
    author: mod.author,
    version: mod.version,
    summary: null,
    downloads: null,
    endorsements: null,
    thumbnailUrl: null,
    category: mod.category,
    updatedAt: null,
    installed: true,
  };
  return (
    <ModDetailsDialog
      game={game}
      m={hit}
      uid={mod.uid}
      onClose={onClose}
      onRemoved={() => onUninstalled?.(mod.uid)}
    />
  );
}

/** Shows only once a load outlasts SKELETON_DELAY_MS, so a fast answer never flashes it. */
function Loading() {
  const [late, setLate] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setLate(true), SKELETON_DELAY_MS);
    return () => clearTimeout(timer);
  }, []);
  if (!late) return null;
  return <p className="px-1 py-6 text-body text-fg-2">Loading…</p>;
}

/** A failed read: what could not be read, the raw error, and Try again (refetch). */
function ReadError({
  what,
  error,
  retrying,
  onRetry,
}: {
  what: string;
  error: unknown;
  retrying: boolean;
  onRetry: () => void;
}) {
  return (
    <ErrorNote
      actions={
        <Button size="sm" busy={retrying} onClick={onRetry}>
          Try again
        </Button>
      }
    >
      Couldn't read {what}.
      <span className="mt-1 block font-mono text-caption text-rose-300">{errMsg(error)}</span>
    </ErrorNote>
  );
}

/**
 * Opens a mod's Details from a row on another tab. Rows carry only the mod's id, so the
 * ModView comes from the load order; a mod missing from it (stale data) has no action.
 */
function useModDetails(game: string) {
  const mods = useMods(game).data?.mods;
  const [open, setOpen] = useState<ModView | null>(null);
  return {
    find: (id: number | undefined) => mods?.find((m) => m.id === id),
    show: setOpen,
    dialog: open && (
      <ModDetailDialog mod={open} game={game} onClose={() => setOpen(null)} />
    ),
  };
}

/**
 * A highlighted list row that opens a mod's Details (a real button: Enter/A activate it),
 * or the same layout without an action when the mod is unknown.
 */
function DetailsRow({
  mod,
  onOpen,
  className,
  children,
}: {
  mod: ModView | undefined;
  onOpen: (m: ModView) => void;
  className: string;
  children: ReactNode;
}) {
  const layout = cx(LIST_ROW, "flex w-full min-w-0 gap-3 text-left", className);
  if (!mod) return <div className={layout}>{children}</div>;
  return (
    <button
      type="button"
      className={layout}
      onClick={() => onOpen(mod)}
    >
      {children}
      <span className="sr-only">, open details of {mod.name}</span>
      <CaretRightIcon aria-hidden size={14} className="shrink-0 self-center text-dim" />
    </button>
  );
}

function Conflicts({ game }: { game: string }) {
  const q = useQuery({
    queryKey: ["conflicts", game],
    queryFn: () => ipc.conflicts(game),
  });
  const details = useModDetails(game);
  if (q.isLoading) return <Loading />;
  if (!q.data)
    return (
      <ReadError
        what="conflicts"
        error={q.error}
        retrying={q.isFetching}
        onRetry={() => void q.refetch()}
      />
    );
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
                <li key={c.rel_path}>
                  {/* Opens the winner: the mod whose file is deployed. */}
                  <DetailsRow
                    mod={details.find(c.winner.id)}
                    onOpen={details.show}
                    className="items-start"
                  >
                    <SwordIcon
                      aria-hidden
                      size={16}
                      className="mt-0.5 shrink-0 text-dim"
                    />
                    <span className="min-w-0 flex-1">
                      <span className="block font-mono text-label break-all text-fg">
                        {c.rel_path}
                      </span>
                      <span className="text-body text-fg-2">
                        <span className="text-accent-300">{c.winner.name}</span>{" "}
                        wins over {c.losers.map((l) => l.name).join(", ")}
                        {c.plugin && " · plugin"}
                      </span>
                    </span>
                  </DetailsRow>
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
                <li key={p.name}>
                  {/* Opens the providing mod; a plugin no mod provides has no action. */}
                  <DetailsRow
                    mod={details.find(p.provider?.id)}
                    onOpen={details.show}
                    className="flex-wrap items-center"
                  >
                    <span className="w-9 text-right font-mono text-caption text-dim tabular-nums">
                      {p.load_index ?? "–"}
                    </span>
                    <span className="min-w-0 flex-1 font-mono text-label text-fg">
                      {p.name}
                    </span>
                    {!p.active && <Badge>inactive</Badge>}
                    {p.provider && (
                      <span className="text-label text-fg-2">
                        {p.provider.name}
                      </span>
                    )}
                    {p.overridden.length > 0 && (
                      <Badge tone="amber">overrides {p.overridden.length}</Badge>
                    )}
                  </DetailsRow>
                </li>
              ))}
            </ol>
          </CardSection>
        </Card>
      )}
      {details.dialog}
    </div>
  );
}

function Updates({ game }: { game: string }) {
  const run = useRun();
  const qc = useQueryClient();
  const checking = useBusy("Check updates");
  const installing = useBusy("Install updates");
  // Kept in the query cache, so the Mods list can filter on it ("Has updates").
  const report = useUpdateReport(game).data ?? null;
  const setReport = (r: UpdateReport | null) =>
    qc.setQueryData(["updates", game], r);
  const reasonId = useId();
  const details = useModDetails(game);
  // Install stays focusable when there is nothing to install; the reason is shown and describes it.
  const nothingToInstall = !report?.updates.length;
  return (
    <Card
      title="Updates"
      actions={
        <>
          <Button
            size="sm"
            busy={checking}
            icon={<ArrowsClockwiseIcon aria-hidden size={16} />}
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
            icon={<DownloadSimpleIcon aria-hidden size={16} />}
            inactive={nothingToInstall}
            aria-describedby={nothingToInstall ? reasonId : undefined}
            onClick={async () => {
              const r = await run("Install updates", () =>
                ipc.updatesInstall(),
              );
              if (!r) return;
              setReport(r);
              showMsg(r.installed ? "success" : "info", updatesInstalledMsg(r));
            }}
          >
            Install updates
          </Button>
        </>
      }
    >
      <CardSection>
        {!report ? (
          <Empty>
            <span id={reasonId}>
              Check Nexus for newer files of your mods. Install updates
              becomes available once a check finds some.
            </span>
          </Empty>
        ) : (
          <>
            <p className="mb-4 font-mono text-caption text-dim tabular-nums">
              Checked {report.checked} mods
              {report.stopped_early && " (stopped early: rate limit)"}
              {report.installed > 0 && `, installed ${report.installed}`}.
            </p>
            {!report.updates.length ? (
              <Empty>
                <span id={reasonId}>
                  Everything is up to date: nothing to install.
                </span>
              </Empty>
            ) : (
              <ul className="rows">
                {report.updates.map((u) => (
                  <li key={u.mod.id}>
                    <DetailsRow
                      mod={details.find(u.mod.id)}
                      onOpen={details.show}
                      className="flex-wrap items-center"
                    >
                      <span className="text-lead text-fg">{u.mod.name}</span>{" "}
                      <span className="font-mono text-caption text-fg-2">
                        {u.mod.version ?? "?"} → {u.file.version ?? u.file.name}
                      </span>
                      {u.pinned && <Badge tone="amber">pinned</Badge>}
                      <span className="flex-1" />
                    </DetailsRow>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
        {details.dialog}
      </CardSection>
    </Card>
  );
}

/** Mirrors valid_profile_name (crates/vessel-core/src/mods/profiles.rs): legacy rule ^[A-Za-z0-9._-]+$. */
const PROFILE_NAME = /^[A-Za-z0-9._-]+$/;
/** How long a pressed Delete stays armed for its confirming second press. */
export const DELETE_ARM_MS = 4000;

/** The inline error for a profile name, or null when it is valid (or empty: Save is inactive then). */
export function profileNameError(name: string): string | null {
  if (!name || PROFILE_NAME.test(name)) return null;
  const bad = [...new Set(name.replace(/[A-Za-z0-9._-]/g, ""))]
    .map((c) => (c === " " ? "space" : `“${c}”`))
    .join(", ");
  return `Not allowed in a profile name: ${bad}.`;
}

function Profiles({ game }: { game: string }) {
  const q = useQuery({
    queryKey: ["profiles", game],
    queryFn: () => ipc.profilesList(game),
  });
  const run = useRun();
  const saving = useBusy("Save profile");
  const [loading, setLoading] = useState<string | null>(null);
  const [name, setName] = useState("");
  const load = async (source: string) => {
    setLoading(source);
    const r = await run("Load profile", () => ipc.profileLoad(source, game), { key: "Load profile" });
    setLoading(null);
    if (r) showMsg("success", profileLoadedMsg(source, r));
  };
  // Validation shows from the first blur (or submit) on, then follows the typing.
  const [touched, setTouched] = useState(false);
  const nameError = touched ? profileNameError(name.trim()) : null;
  // Deleting is not confirmed by the backend: press twice. The arm lapses after DELETE_ARM_MS or on blur.
  const [armed, setArmed] = useState<string | null>(null);
  useEffect(() => {
    if (armed === null) return;
    const timer = setTimeout(() => setArmed(null), DELETE_ARM_MS);
    return () => clearTimeout(timer);
  }, [armed]);
  return (
    <Card title="Profiles">
      <CardSection>
        <form
          className="mb-5 flex flex-wrap items-end gap-3"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!name.trim()) return;
            const n = name.trim();
            if (profileNameError(n)) return setTouched(true);
            const r = await run("Save profile", () => ipc.profileSave(n), {
              key: "Save profile",
            });
            // undefined = failed (error notice shown), null = overwrite declined: keep the name either way.
            if (r === undefined) return;
            if (r === null)
              return showToast("info", "Kept", { detail: `Profile ${n} was not overwritten` });
            showToast("success", "Saved", { detail: `Profile ${n} · ${r} ${r === 1 ? "mod" : "mods"}` });
            setName("");
            setTouched(false);
          }}
        >
          <TextField
            label="Save the current mod set as"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onBlur={() => setTouched(true)}
            helper="Letters, digits, dot, dash and underscore only (A–Z, 0–9, . _ -)."
            error={nameError}
            className="min-w-0 flex-[1_1_18rem]"
          />
          <Button
            type="submit"
            tone="primary"
            busy={saving}
            inactive={!name.trim()}
            icon={<FloppyDiskIcon aria-hidden size={16} />}
          >
            Save profile
          </Button>
        </form>
        {q.isError ? (
          <ErrorNote>Could not load profiles: {errMsg(q.error)}</ErrorNote>
        ) : q.isPending ? (
          <Loading />
        ) : !q.data.length ? (
          <Empty>No saved profiles.</Empty>
        ) : (
          <ul className="rows">
            {q.data.map((p) => (
              <li
                key={p.name}
                className="flex flex-wrap items-center gap-3 py-3"
              >
                <span className="min-w-0 flex-1">
                  <span className="block text-lead wrap-anywhere text-fg">
                    {p.name}
                  </span>
                  <span className="font-mono text-caption text-dim tabular-nums">
                    {p.enabled}/{p.mods} enabled · saved {when(p.saved_at)}
                  </span>
                </span>
                <Button
                  size="sm"
                  busy={loading === p.name}
                  data-profile-load={p.name}
                  onClick={() => void load(p.name)}
                >
                  Load
                </Button>
                <Button
                  size="sm"
                  // Quiet until armed, then the danger tone for the confirming press.
                  tone={armed === p.name ? "danger" : "ghost"}
                  // Fixed width (and a transparent border, which ghost lacks): arming swaps the
                  // label and tone without moving Load or the row.
                  className="w-44 border border-transparent"
                  onBlur={() => armed === p.name && setArmed(null)}
                  onClick={async () => {
                    if (armed !== p.name) return setArmed(p.name);
                    setArmed(null);
                    const r = await run("Delete profile", () =>
                      ipc.profileDelete(p.name),
                    );
                    // undefined = failed (the error notice is shown).
                    if (r === undefined) return;
                    showToast("success", "Deleted", { detail: `Profile ${p.name}` });
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
