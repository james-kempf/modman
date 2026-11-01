// SPDX-License-Identifier: GPL-3.0-or-later
// Browse: the game's mods and collections on Nexus (top of all time or a
// recent window, newest, or matching words) with one-click installs, then
// direct installs by id/URL/archive (FOMOD choices come up as the installer
// wizard dialog), the nxm paste box (Game Mode has no browser handoff) and
// the download queue.

import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useLayoutEffect, useRef, useState, type FocusEvent } from "react";
import { errMsg } from "../app/bridge";
import { useActiveGame } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { browseDefaults, useUi, type BrowseSession } from "../app/store";
import { CollectionDetailsDialog, ModDetailsDialog } from "../components/dialogs/BrowseDetails";
import { readClipboard } from "../components/shell/Osk";
import {
  ArrowClockwiseIcon,
  CaretLeftIcon,
  CaretRightIcon,
  CheckIcon,
  ClipboardTextIcon,
  DownloadSimpleIcon,
  HardDrivesIcon,
  LinkIcon,
  PackageIcon,
  ThumbsUpIcon,
  TrashIcon,
} from "@phosphor-icons/react";
import {
  Badge,
  Button,
  Card,
  CardSection,
  Empty,
  ErrorNote,
  Picture,
  Segmented,
  Select,
  StatusDot,
  TextField,
  Toggle,
  SearchField,
  ViewFrame,
  ViewHeader,
  bytes,
  cx,
} from "../components/ui";
import * as ipc from "../ipc/commands";
import type { BrowseKind, BrowsePeriod, BrowseSort, BrowseView, CollectionHit, InstallResult, SearchHit } from "../ipc/types";
import { collectionText, collectionTone, removedText } from "./Collections";
import { uninstalledText } from "./Mods";
import { installKey } from "./Requirements";
import { NoGame } from "../components/shell/NoGame";
import { FOCUS_ATTR, MOD_GRID, MOD_PICTURE, RESULTS_GRID, Stat, Tile, compact } from "../components/browse/Tile";

const FOMOD = [
  { value: "ask", label: "Ask (wizard)" },
  { value: "reuse", label: "Reuse choices" },
  { value: "default", label: "Defaults" },
] as const;

const KINDS = [
  { value: "mods", label: "Mods" },
  { value: "collections", label: "Collections" },
] as const satisfies readonly { value: BrowseKind; label: string }[];

const PERIODS = [
  { value: "all", label: "All time" },
  { value: "year", label: "Year" },
  { value: "month", label: "Month" },
  { value: "week", label: "Week" },
  { value: "day", label: "Today" },
] as const satisfies readonly { value: BrowsePeriod; label: string }[];

const SORTS = [
  { value: "endorsements", label: "Most endorsed" },
  { value: "downloads", label: "Most downloaded" },
  { value: "newest", label: "Newest" },
] as const satisfies readonly { value: BrowseSort; label: string }[];

/** "of all time", "this past month", ... for the result summary. */
const PERIOD_PHRASE: Record<BrowsePeriod, string> = {
  all: "of all time",
  year: "from the past year",
  month: "from the past month",
  week: "from the past week",
  day: "from the past day",
};

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

/** The non-empty lines/words of pasted text (nxm links never contain whitespace). */
export function splitLinks(text: string): string[] {
  return text.split(/\s+/).filter(Boolean);
}

/** Insert `text` over the input's selection; returns the new value. */
function insertAtSelection(input: HTMLInputElement, text: string): string {
  const v = input.value;
  const a = input.selectionStart ?? v.length;
  const b = input.selectionEnd ?? v.length;
  return v.slice(0, a) + text + v.slice(b);
}

export function Browse() {
  const { game, isLoading } = useActiveGame();
  return (
    <ViewFrame>
      <ViewHeader title="Browse" eyebrow={game?.name}>
        {game?.nexus_domain && <BrowseSearch key={game.id} game={game.id} />}
      </ViewHeader>
      {isLoading ? null : !game ? (
        // Every install goes into the active game.
        <NoGame>Pick or add a game in Games first; Browse shows and installs mods for the active game.</NoGame>
      ) : (
        <>
          {/* key: listings belong to one game. */}
          <BrowseCard key={game.id} game={game.id} linked={!!game.nexus_domain} />
          <section className="stagger grid grid-cols-[minmax(0,1fr)] gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
            <InstallCard />
            <NxmCard />
          </section>
          <DownloadsCard />
        </>
      )}
    </ViewFrame>
  );
}

function InstallCard() {
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const busy = useBusy("Install");
  const [target, setTarget] = useState("");
  const [fomod, setFomod] = useState<(typeof FOMOD)[number]["value"]>("ask");
  const [deploy, setDeploy] = useState(false);
  const submit = async () => {
    if (!target.trim()) return;
    const r = await run("Install", () => ipc.install({ target: target.trim(), fomod, deploy }), {
      key: installKey(target.trim()),
    });
    if (r) {
      notify(r.installed || r.collection ? "success" : "info", installedText(r));
      setTarget("");
    }
  };
  return (
    <Card title="Direct install">
      <CardSection>
        <form
          className="grid gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <TextField
            label="Mod id, Nexus URL, collection URL or archive path"
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            onPaste={(e) => {
              // A text input drops line breaks and glues the lines together ("266\n267" -> "266267"):
              // keep the first line and say so.
              const lines = e.clipboardData
                .getData("text")
                .split(/\r?\n/)
                .map((l) => l.trim())
                .filter(Boolean);
              if (lines.length < 2) return;
              e.preventDefault();
              setTarget(insertAtSelection(e.currentTarget, lines[0]));
              notify("warn", `Pasted the first of ${lines.length} lines; install one mod at a time.`);
            }}
            data-osk-submit="Install"
            placeholder="e.g. 266 or https://www.nexusmods.com/skyrimspecialedition/mods/266"
          />
          <Segmented label="FOMOD" value={fomod} options={FOMOD} onChange={setFomod} />
          <div className="flex flex-wrap items-center gap-3">
            <Toggle label="Deploy after install" checked={deploy} onChange={setDeploy} />
            {/* inactive, not disabled: focus returns here after the installer dialogs, when the field is already cleared. */}
            <Button
              type="submit"
              tone="primary"
              busy={busy}
              inactive={!target.trim()}
              className="ml-auto"
              icon={<DownloadSimpleIcon aria-hidden size={16} />}
            >
              Install
            </Button>
          </div>
        </form>
      </CardSection>
    </Card>
  );
}

function NxmCard() {
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const busy = useBusy("Queue link");
  const [url, setUrl] = useState("");
  // Several links (a multi-line paste) are queued one by one; the field shows them space-separated,
  // exactly what is sent, and keeps the ones that failed.
  const submit = async (text: string) => {
    const links = splitLinks(text);
    if (!links.length) return;
    const failed: string[] = [];
    const queued: string[] = [];
    for (const link of links) {
      const r = await run("Queue link", () => ipc.nxmSubmit(link));
      if (r) queued.push(`${r.kind} link ${r.link}`);
      else failed.push(link);
    }
    if (queued.length === 1) notify("success", `Queued ${queued[0]}`);
    else if (queued.length > 1) notify("success", `Queued ${queued.length} links`);
    setUrl(failed.join(" "));
  };
  const setLinks = (text: string) => setUrl(splitLinks(text).join(" "));
  return (
    <Card title="Mod Manager Download link">
      <CardSection>
        <p className="mb-5 text-body leading-relaxed text-zinc-400">
          From the "Mod Manager Download" button on Nexus. Links clicked in a browser arrive here by themselves.
        </p>
        <form
          className="grid gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            void submit(url);
          }}
        >
          <TextField
            label="nxm link"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            onPaste={(e) => {
              const text = e.clipboardData.getData("text");
              if (!/\s/.test(text.trim())) return;
              e.preventDefault();
              setUrl(insertAtSelection(e.currentTarget, splitLinks(text).join(" ")));
            }}
            placeholder="nxm://…"
          />
          <div className="flex flex-wrap gap-3">
            <Button
              icon={<ClipboardTextIcon aria-hidden size={16} />}
              onClick={async () => {
                const t = await readClipboard();
                if (t === null) notify("warn", "Clipboard not readable here; paste into the field with Ctrl+V.");
                else setLinks(t);
              }}
            >
              Paste
            </Button>
            <Button
              type="submit"
              tone="primary"
              busy={busy}
              disabled={!url.trim()}
              className="ml-auto"
              icon={<LinkIcon aria-hidden size={16} />}
            >
              Download
            </Button>
          </div>
        </form>
      </CardSection>
    </Card>
  );
}

/** Items per browse page (the backend's page size). */
export const PAGE_SIZE = 24;

/** Items in one browse page. */
const pageItems = (p: BrowseView) => (p.kind === "mods" ? p.mods.length : p.collections.length);

/** The scrolling view container (App's <main>). */
const scroller = () => document.querySelector("main");

/** The game's Browse session (defaults for a game without one) and a setter that writes through to the store. */
function useBrowseSession(game: string) {
  const stored = useUi((s) => s.browse);
  const saveBrowse = useUi((s) => s.saveBrowse);
  const session = stored?.game === game ? stored : browseDefaults(game);
  return [session, (patch: Partial<Omit<BrowseSession, "game">>) => saveBrowse(game, patch)] as const;
}

/**
 * Remembers <main>'s scroll and the focused Browse control while Browse is
 * shown, saves them when it is left, and puts them back once the listing is
 * rendered again. Focus is restored only when nothing outside the view (the
 * tab rail) holds it, so entering Browse from the rail does not steal focus.
 */
function useBrowsePosition(game: string, ready: boolean) {
  const saveBrowse = useUi((s) => s.saveBrowse);
  const pos = useRef({ scroll: 0, focus: null as string | null });
  const restored = useRef(false);

  // Layout effect: its cleanup runs before the next view's content replaces Browse's (and clamps the scroll).
  useLayoutEffect(() => {
    const saved = useUi.getState().browse;
    if (saved?.game === game) pos.current = { scroll: saved.scroll, focus: saved.focus };
    const main = scroller();
    const onScroll = () => {
      if (restored.current && main) pos.current.scroll = main.scrollTop;
    };
    main?.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      main?.removeEventListener("scroll", onScroll);
      saveBrowse(game, restored.current ? { ...pos.current, scroll: main?.scrollTop ?? pos.current.scroll } : pos.current);
    };
  }, [game, saveBrowse]);

  useEffect(() => {
    if (!ready || restored.current) return;
    restored.current = true;
    const main = scroller();
    const { scroll, focus } = pos.current;
    if (focus) {
      const a = document.activeElement;
      const free = !a || a === document.body || !!main?.contains(a);
      const el = document.querySelector<HTMLElement>(`[${FOCUS_ATTR}="${focus.replace(/["\\]/g, "\\$&")}"]`);
      if (free && el) el.focus({ preventScroll: true });
    }
    if (main && scroll) main.scrollTop = scroll;
  }, [ready]);

  /** onFocus for the Browse card: remember the focused control's key. */
  return (e: FocusEvent) => {
    const key = (e.target as HTMLElement).closest(`[${FOCUS_ATTR}]`)?.getAttribute(FOCUS_ATTR);
    if (key) pos.current.focus = key;
  };
}

/**
 * The Nexus search, top right in the header like the Games filter: Enter (or the on-screen keyboard's Search)
 * lists the matches; emptying the field lists everything again.
 */
function BrowseSearch({ game }: { game: string }) {
  const [s, save] = useBrowseSession(game);
  return (
    <SearchField
      region="Search Nexus"
      label="Search"
      name="browse-search"
      data-osk-submit="Search"
      value={s.draft}
      placeholder={s.kind === "mods" ? "Search mod names" : "Search collections"}
      onChange={(e) => {
        const draft = e.target.value;
        save(!draft.trim() && s.text ? { draft, text: "", page: 0 } : { draft });
      }}
      onSubmit={() => save({ text: s.draft.trim(), page: 0 })}
    />
  );
}

function BrowseCard({ game, linked }: { game: string; linked: boolean }) {
  const [s, save] = useBrowseSession(game);
  const { kind, period, sort, text, page } = s;
  // Any change to what is listed starts again at the first page.
  const refine = (patch: Partial<Omit<BrowseSession, "game">>) => save({ ...patch, page: 0 });
  const q = useQuery({
    queryKey: ["browse", game, kind, period, sort, text, page],
    queryFn: () => ipc.browse({ kind, period, sort, text, offset: page * PAGE_SIZE }),
    enabled: linked,
    staleTime: 5 * 60_000,
    // The previous page stays up while the next one loads: the pager (and its focus) does not jump away.
    placeholderData: keepPreviousData,
  });
  const onFocus = useBrowsePosition(game, q.isSuccess || !linked);
  const data = q.data;
  const total = data?.total ?? 0;
  const shown = data ? pageItems(data) : 0;
  const pages = Math.max(1, Math.ceil(total / PAGE_SIZE));
  // The listing shrank under a remembered page (e.g. fewer matches now): go to its last page.
  useEffect(() => {
    if (q.isSuccess && !q.isPlaceholderData && page > 0 && shown === 0) save({ page: Math.min(page, pages) - 1 });
  }, [q.isSuccess, q.isPlaceholderData, page, shown, pages]); // `save` is a fresh closure each render
  const noun = kind === "mods" ? "mods" : "collections";
  const counted = `${total.toLocaleString()} ${total === 1 ? noun.slice(0, -1) : noun}`;
  const sortLabel = SORTS.find((o) => o.value === sort)!.label.toLowerCase();
  const first = page * PAGE_SIZE;
  return (
    <Card
      onFocus={onFocus}
      title={kind === "mods" ? "Mods on Nexus" : "Collections on Nexus"}
      intro={
        q.isSuccess
          ? `${counted}${text ? ` matching “${text}”` : ""}, ${sortLabel} ${PERIOD_PHRASE[period]}`
          : undefined
      }
    >
      <CardSection>
        <div className="flex flex-wrap items-center gap-x-6 gap-y-4">
          <Segmented label="Show" value={kind} options={KINDS} onChange={(v) => refine({ kind: v })} />
          <Segmented label="Created" value={period} options={PERIODS} onChange={(v) => refine({ period: v })} />
          <Select label="Sort" inline value={sort} options={SORTS} onChange={(v) => refine({ sort: v })} />
        </div>
      </CardSection>
      <CardSection>
        {!linked ? (
          <Empty>This game is not linked to a Nexus Mods game, so there is nothing to browse.</Empty>
        ) : q.isError ? (
          <ErrorNote>
            Could not load {noun}: {errMsg(q.error)}
            <Button size="sm" icon={<ArrowClockwiseIcon aria-hidden size={15} />} onClick={() => void q.refetch()}>
              Retry
            </Button>
          </ErrorNote>
        ) : q.isPending ? (
          <p className="text-body text-zinc-400">Loading {noun}…</p>
        ) : !shown ? (
          <Empty>
            {text
              ? `No ${noun} match “${text}”. Try fewer words, or a longer period.`
              : `No ${noun} were created ${PERIOD_PHRASE[period].replace(/^from /, "in ")}.`}
          </Empty>
        ) : (
          <>
            <ul
              className={kind === "mods" ? MOD_GRID : RESULTS_GRID}
              aria-label={kind === "mods" ? "Mods" : "Collections"}
              aria-busy={q.isPlaceholderData || undefined}
            >
              {data!.kind === "mods"
                ? data!.mods.map((m, i) => <ModTile key={m.modId !== null ? `m${m.modId}` : `i${i}`} game={game} m={m} />)
                : data!.collections.map((c) => <CollectionTile key={c.slug} game={game} c={c} />)}
            </ul>
            <nav className="mt-5 flex flex-wrap items-center gap-4" aria-label={`${kind === "mods" ? "Mods" : "Collections"} pages`}>
              {/* inactive, not disabled: focus stays on the button at the first/last page. */}
              <Button
                size="sm"
                {...{ [FOCUS_ATTR]: "page:prev" }}
                inactive={page === 0}
                busy={q.isPlaceholderData && q.isFetching}
                icon={<CaretLeftIcon aria-hidden size={15} />}
                onClick={() => save({ page: page - 1 })}
              >
                Previous
              </Button>
              <span className="font-mono text-caption text-dim" aria-live="polite">
                Page {page + 1} of {pages.toLocaleString()} · {(first + 1).toLocaleString()}–
                {(first + shown).toLocaleString()} of {total.toLocaleString()}
              </span>
              <Button
                size="sm"
                {...{ [FOCUS_ATTR]: "page:next" }}
                inactive={page + 1 >= pages}
                busy={q.isPlaceholderData && q.isFetching}
                onClick={() => save({ page: page + 1 })}
              >
                Next
                <CaretRightIcon aria-hidden size={15} />
              </Button>
            </nav>
          </>
        )}
      </CardSection>
    </Card>
  );
}

function InstalledBadge() {
  return (
    <Badge tone="accent">
      <CheckIcon aria-hidden size={12} weight="bold" />
      Installed
    </Badge>
  );
}

/** Shown on a card while its install/remove runs (the dialog may be closed by then). */
function WorkingBadge() {
  return <Badge>Working…</Badge>;
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
  };
}

/** The details dialog's Remove: the backend (or the loader consent) confirms. */
function RemoveButton({ game, item }: { game: string; item: Removable }) {
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

/** Open/close state of a card's details dialog, and the card it returns focus to. */
function useDetails() {
  const [open, setOpen] = useState(false);
  const card = useRef<HTMLButtonElement>(null);
  return { open, card, show: () => setOpen(true), hide: () => setOpen(false) };
}

// Mod pictures fill the tile's width at 16:9 (MOD_PICTURE). Collection pictures are 4:5 portrait, left of the
// text, and their width is a mod picture's height. The results <ul> is the container
// (100cqw), and the mod tile's picture width is its column (mods: 1 / sm 2 / xl 3, gap-4 = 1rem) less the tile's
// p-2.5 and 1px border (1.25rem + 2px); times 9/16 is that picture's height. Keep in step with the grid and Tile.
// The monogram fallback takes the same box, so a picture arriving never shifts the grid.
// self-start: the horizontal tile's row stretches its items to the text column's (or grid row's) height, and a
// stretched cross size overrides aspect-ratio when the width is fixed, so the picture would grow past 4:5.
const COLLECTION_PICTURE = cx(
  "aspect-[4/5] shrink-0 self-start rounded-card",
  "w-[calc((100cqw-1.25rem-2px)*9/16)]",
  "sm:w-[calc(((100cqw-1rem)/2-1.25rem-2px)*9/16)]",
  "xl:w-[calc(((100cqw-2rem)/3-1.25rem-2px)*9/16)]",
);

function ModTile({ game, m }: { game: string; m: SearchHit }) {
  const run = useRun();
  const qc = useQueryClient();
  const notify = useUi((s) => s.notify);
  const installing = useUi((s) => s.busy);
  const details = useDetails();
  const name = m.name ?? `Mod ${m.modId ?? "?"}`;
  const key = installKey(String(m.modId));
  const busy = (installing[key] ?? 0) > 0;
  const installed = m.installed && m.modId !== null;
  const install = async () => {
    const r = await run("Install", () => ipc.install({ target: String(m.modId), fomod: "ask" }), { key });
    if (!r) return;
    notify("success", installedText(r));
    void qc.invalidateQueries({ queryKey: ["browse", game] });
  };
  return (
    <Tile
      picture={<Picture url={m.thumbnailUrl} name={name} className={MOD_PICTURE} />}
      name={name}
      byline={[m.version ? `v${m.version.replace(/^v/i, "")}` : null, m.author, m.category].filter(Boolean).join(" · ")}
      summary={m.summary}
      stats={
        <>
          <Stat icon={<DownloadSimpleIcon aria-hidden size={13} />} value={compact(m.downloads)} label="downloads" />
          <Stat icon={<ThumbsUpIcon aria-hidden size={13} />} value={compact(m.endorsements)} label="endorsements" />
        </>
      }
      badges={
        <>
          {busy && <WorkingBadge />}
          {installed && <InstalledBadge />}
        </>
      }
      focusKey={`m${m.modId ?? name}`}
      onOpen={details.show}
      cardRef={details.card}
      dialog={
        details.open && (
          <ModDetailsDialog
            game={game}
            m={m}
            onClose={details.hide}
            returnFocus={details.card.current}
            actions={
              installed ? (
                <RemoveButton
                  game={game}
                  item={m.loader ? { kind: "loader", loaderKey: m.loader, nexusId: m.modId!, name } : { kind: "mod", nexusId: m.modId!, name }}
                />
              ) : (
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
              )
            }
          />
        )
      }
    />
  );
}

function CollectionTile({ game, c }: { game: string; c: CollectionHit }) {
  const run = useRun();
  const qc = useQueryClient();
  const notify = useUi((s) => s.notify);
  const installing = useUi((s) => s.busy);
  const details = useDetails();
  // The same key as the Collections view's actions on this collection.
  const key = `Collection ${c.slug}`;
  const busy = (installing[key] ?? 0) > 0;
  const installed = c.installed_revision !== null;
  const behind = installed && c.revision !== null && c.installed_revision! < c.revision;
  const act = async (update: boolean) => {
    const r = await run(
      update ? "Update collection" : "Install collection",
      () => (update ? ipc.collectionUpdate({ reference: c.slug }) : ipc.collectionInstall({ reference: c.slug, optional: "ask" })),
      { key },
    );
    if (!r) return;
    notify(collectionTone(r), collectionText(r));
    void qc.invalidateQueries({ queryKey: ["browse", game] });
    void qc.invalidateQueries({ queryKey: ["collections", game] });
  };
  return (
    <Tile
      horizontal
      picture={<Picture url={c.image_url} name={c.name} className={COLLECTION_PICTURE} />}
      name={c.name}
      byline={[c.author, c.category, c.revision !== null ? `revision ${c.revision}` : null].filter(Boolean).join(" · ")}
      summary={c.summary}
      stats={
        // 2x2, row by row: likes, installs / size, mods.
        <span className="grid min-w-0 flex-1 grid-cols-2 gap-x-4 gap-y-1">
          <Stat icon={<ThumbsUpIcon aria-hidden size={13} />} value={compact(c.endorsements)} label="endorsements" />
          <Stat icon={<DownloadSimpleIcon aria-hidden size={13} />} value={compact(c.downloads)} label="downloads" />
          <Stat icon={<HardDrivesIcon aria-hidden size={13} />} value={bytes(c.total_size)} label="size" />
          <Stat icon={<PackageIcon aria-hidden size={13} />} value={c.mod_count !== null ? String(c.mod_count) : "?"} label="mods" />
        </span>
      }
      badges={
        <>
          {busy && <WorkingBadge />}
          {installed && <InstalledBadge />}
          {behind && <Badge tone="amber">Update</Badge>}
        </>
      }
      focusKey={`c${c.slug}`}
      onOpen={details.show}
      cardRef={details.card}
      dialog={
        details.open && (
          <CollectionDetailsDialog
            game={game}
            c={c}
            onClose={details.hide}
            returnFocus={details.card.current}
            actions={
              installed ? (
                <>
                  {behind && (
                    <Button tone="primary" busy={busy} aria-label={`Update ${c.name}`} onClick={() => void act(true)}>
                      Update to {c.revision}
                    </Button>
                  )}
                  <RemoveButton game={game} item={{ kind: "collection", slug: c.slug, name: c.name }} />
                </>
              ) : (
                <Button
                  tone="primary"
                  busy={busy}
                  aria-label={`Install ${c.name}`}
                  icon={<DownloadSimpleIcon aria-hidden size={16} />}
                  onClick={() => void act(false)}
                >
                  Install
                </Button>
              )
            }
          />
        )
      }
    />
  );
}

const DOWNLOADS_SHOWN = 10;

function DownloadsCard() {
  const [cap, setCap] = useState(DOWNLOADS_SHOWN);
  const q = useQuery({ queryKey: ["downloads"], queryFn: ipc.downloadsList });
  const live = useUi((s) => s.downloads);
  const rows = q.data ?? [];
  const liveAll = Object.values(live).filter((d) => d.last.type !== "finished" && !rows.some((r) => r.file_name === d.name));
  const total = liveAll.length + rows.length;
  // Running downloads come first, so the cap trims the queue's tail.
  const liveOnly = liveAll.slice(0, cap);
  const shown = rows.slice(0, Math.max(0, cap - liveOnly.length));
  return (
    <Card title="Downloads">
      <CardSection>
        {q.isError && (
          <div className="mb-4">
            <ErrorNote>Could not load the download queue: {errMsg(q.error)}</ErrorNote>
          </div>
        )}
        {!rows.length && !liveOnly.length ? (
          <Empty>No downloads.</Empty>
        ) : (
          <ul className="rows" aria-label="Downloads">
            {liveOnly.map((d) => (
              <li key={d.name} tabIndex={0} className="grid gap-2 rounded-control py-3">
                <span className="flex items-center gap-2.5 text-[14.5px] text-zinc-100">
                  <StatusDot live />
                  {d.name}
                </span>
                <progress aria-label={`${d.name} progress`} max={d.total ?? 1} value={d.total ? d.done : undefined} />
                <span className="font-mono text-[11.5px] text-dim">
                  {bytes(d.done)} / {bytes(d.total)}
                  {d.last.type === "retrying" && ` · retrying (${d.last.reason})`}
                </span>
              </li>
            ))}
            {shown.map((r) => {
              const l = r.file_name ? live[r.file_name] : undefined;
              const done = l?.done ?? r.bytes_done;
              const size = l?.total ?? r.size;
              return (
                <li key={r.id} tabIndex={0} className="grid gap-2 rounded-control py-3">
                  <span className="flex flex-wrap items-center gap-2">
                    <span className="min-w-0 flex-1 truncate text-[14.5px] text-zinc-100">
                      {r.file_name ?? (r.nexus_mod_id ? `mod ${r.nexus_mod_id}` : r.collection_slug ?? `#${r.id}`)}
                    </span>
                    <Badge tone={r.status === "failed" ? "rose" : r.status === "done" ? "accent" : "zinc"}>{r.status}</Badge>
                  </span>
                  {r.status === "downloading" && <progress aria-label="progress" max={size ?? 1} value={size ? done : undefined} />}
                  <span className="font-mono text-[11.5px] text-dim">
                    {bytes(done)} / {bytes(size)} · {r.source}
                    {r.error && ` · ${r.error}`}
                  </span>
                </li>
              );
            })}
          </ul>
        )}
        {total > cap && (
          <Button size="sm" tone="ghost" className="mt-3" onClick={() => setCap(cap + DOWNLOADS_SHOWN)}>
            Show more ({Math.min(DOWNLOADS_SHOWN, total - cap)})
          </Button>
        )}
      </CardSection>
    </Card>
  );
}
