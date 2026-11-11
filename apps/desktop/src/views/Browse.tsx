// SPDX-License-Identifier: GPL-3.0-or-later
// Browse: the game's mods and collections on Nexus (top of all time or a
// recent window, newest, or matching words) with one-click installs. The
// download queue is on Activity.

import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useLayoutEffect, useRef, useState, type FocusEvent } from "react";
import { errMsg } from "../app/bridge";
import { useActiveGame } from "../app/queries";
import { useRun } from "../app/run";
import { browseDefaults, useUi, type BrowseSession } from "../app/store";
import { CollectionDetailsDialog, ModTile } from "../components/dialogs/BrowseDetails";
import { RemoveButton } from "../components/browse/ModActions";
import {
  ArrowClockwiseIcon,
  CalendarBlankIcon,
  CaretDoubleLeftIcon,
  CaretDoubleRightIcon,
  CaretLeftIcon,
  CaretRightIcon,
  DownloadSimpleIcon,
  HardDrivesIcon,
  LinkBreakIcon,
  MagnifyingGlassIcon,
  PackageIcon,
  ThumbsUpIcon,
  XIcon,
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
  SearchField,
  ViewFrame,
  ViewHeader,
  bytes,
  cx,
} from "../components/ui";
import * as ipc from "../ipc/commands";
import { PAGE_EVENT, type PageEventDetail } from "../nav/actions";
import { focusables } from "../nav/spatial";
import type { BrowseKind, BrowsePeriod, BrowseSort, BrowseView, CollectionHit } from "../ipc/types";
import { collectionText, collectionTone } from "./Collections";
import { NoGame } from "../components/shell/NoGame";
import {
  FOCUS_ATTR,
  InstalledBadge,
  MOD_GRID,
  MOD_PICTURE,
  RESULTS_GRID,
  Stat,
  Tile,
  TileSkeleton,
  WorkingBadge,
  compact,
  useDetails,
} from "../components/browse/Tile";

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

/** "in the past month", ... for the empty state of a recent window (all time has its own sentence). */
const PERIOD_PHRASE: Record<Exclude<BrowsePeriod, "all">, string> = {
  year: "in the past year",
  month: "in the past month",
  week: "in the past week",
  day: "today",
};

/** How long a page change keeps the previous page up (dimmed) before skeletons replace it: a fast answer never flashes them. */
const SKELETON_DELAY_MS = 300;

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
      ) : !game.nexus_domain ? (
        // No filters either: they would change nothing.
        <NoGame icon={<LinkBreakIcon size={28} weight="duotone" />} title="Nothing to browse">
          This game is not linked to a Nexus Mods game, so Browse cannot list or install its mods. Pick a game that
          Nexus Mods lists in Games.
        </NoGame>
      ) : (
        // key: listings belong to one game.
        <BrowseCard key={game.id} game={game.id} />
      )}
    </ViewFrame>
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
      placeholder={s.kind === "mods" ? "Search mods" : "Search collections"}
      onChange={(e) => {
        const draft = e.target.value;
        save(!draft.trim() && s.text ? { draft, text: "", page: 0 } : { draft });
      }}
      onSubmit={() => save({ text: s.draft.trim(), page: 0 })}
      pending={s.draft.trim() !== s.text}
    />
  );
}

function BrowseCard({ game }: { game: string }) {
  const [s, save] = useBrowseSession(game);
  const { kind, period, sort, text, page } = s;
  const qc = useQueryClient();
  // Set by a page change: once the new page is rendered, go to the top and focus its first result.
  const toFirst = useRef(false);
  const results = useRef<HTMLUListElement>(null);
  // Any change to what is listed starts again at the first page (focus stays on the filter).
  const refine = (patch: Partial<Omit<BrowseSession, "game">>) => {
    toFirst.current = false;
    save({ ...patch, page: 0 });
  };
  const clearSearch = () => refine({ draft: "", text: "" });
  const q = useQuery({
    queryKey: ["browse", game, kind, period, sort, text, page],
    queryFn: () => ipc.browse({ kind, period, sort, text, offset: page * PAGE_SIZE }),
    staleTime: 5 * 60_000,
    // The previous page stays up while the next one loads: the pager (and its focus) does not jump away.
    placeholderData: keepPreviousData,
  });
  const onFocus = useBrowsePosition(game, q.isSuccess);
  const data = q.data;
  const total = data?.total ?? 0;
  const shown = data ? pageItems(data) : 0;
  const pages = Math.max(1, Math.ceil(total / PAGE_SIZE));
  // The listing shrank under a remembered page (e.g. fewer matches now): go to its last page.
  useEffect(() => {
    if (q.isSuccess && !q.isPlaceholderData && page > 0 && shown === 0) save({ page: Math.min(page, pages) - 1 });
  }, [q.isSuccess, q.isPlaceholderData, page, shown, pages]); // `save` is a fresh closure each render
  useEffect(() => {
    if (!toFirst.current || !q.isSuccess || q.isPlaceholderData) return;
    // An empty page past the end is replaced by the last page above: keep waiting for that one.
    if (!shown && page > 0) return;
    toFirst.current = false;
    const main = scroller();
    if (main) main.scrollTop = 0;
    const first = results.current && focusables(results.current)[0];
    if (first) first.focus({ preventScroll: true });
  }, [q.isSuccess, q.isPlaceholderData, data, page, shown]);
  const noun = kind === "mods" ? "mods" : "collections";
  const first = page * PAGE_SIZE;
  // Nothing on screen belongs to this query yet (first load, or the previous query's placeholder data).
  const loading = !q.isError && (q.isPending || q.isPlaceholderData);
  // A load that outlasts SKELETON_DELAY_MS swaps in skeleton tiles; until then the previous listing stays up,
  // dimmed and inert (or nothing, on a first load). A cached query has its data at once and never shows them.
  const [late, setLate] = useState(false);
  useEffect(() => {
    setLate(false);
    if (!loading) return;
    const timer = setTimeout(() => setLate(true), SKELETON_DELAY_MS);
    return () => clearTimeout(timer);
  }, [loading, kind, period, sort, text, page]);
  const skeleton = loading && late;
  const dim = loading && !late && !!data;
  // As many skeletons as the page will hold when another page of these filters told the total.
  const knownTotal = qc
    .getQueriesData<BrowseView>({ queryKey: ["browse", game, kind, period, sort, text] })
    .find(([, d]) => d)?.[1]?.total;
  const skeletons = knownTotal === undefined ? PAGE_SIZE : Math.min(PAGE_SIZE, Math.max(1, knownTotal - first));
  // Results (or a loading page) are on screen: the grid, its summary line and the pager.
  const listed = !q.isError && (loading || shown > 0);
  // The pager is shown (results listed, or loading with a listing already up): it is the card's bottom edge,
  // flush with the view's.
  const hasPager = !q.isError && (loading ? !!data : shown > 0);
  // One page: no paging buttons, only the count.
  const paged = pages > 1;
  // One rule for the buttons and LT/RT: nothing while a page loads, with no results, or past either end.
  // `park`: the pager button that holds focus while the page loads when a tile had it (LT/RT).
  const goTo = (to: number, park: "first" | "prev" | "next" | "last") => {
    if (loading || !shown || to < 0 || to >= pages || to === page) return;
    toFirst.current = true;
    // A tile with focus (LT/RT) goes away with its page: park focus on the pager button until the new page's
    // first tile takes it (T3).
    if (results.current?.contains(document.activeElement))
      document.querySelector<HTMLElement>(`[${FOCUS_ATTR}="page:${park}"]`)?.focus({ preventScroll: true });
    save({ page: to });
  };
  const goPage = (delta: 1 | -1) => goTo(page + delta, delta > 0 ? "next" : "prev");
  const goPageRef = useRef(goPage);
  goPageRef.current = goPage;
  useEffect(() => {
    const onPage = (e: Event) => goPageRef.current((e as CustomEvent<PageEventDetail>).detail.delta);
    document.addEventListener(PAGE_EVENT, onPage);
    return () => document.removeEventListener(PAGE_EVENT, onPage);
  }, []);
  const count = (n: number) => `${n.toLocaleString()} ${n === 1 ? noun.slice(0, -1) : noun}`;
  // The one live result summary (screen readers hear each new listing); shown above the grid while results are.
  const summary = q.isError ? "" : loading ? `Loading ${noun}…` : `${text ? `Results for “${text}” · ` : ""}${count(total)}`;
  const emptyActions = (text || period !== "all") && (
    <>
      {text && (
        <Button tone="primary" icon={<XIcon aria-hidden size={16} />} onClick={clearSearch}>
          Clear search
        </Button>
      )}
      {period !== "all" && (
        <Button tone={text ? "default" : "primary"} icon={<CalendarBlankIcon aria-hidden size={16} />} onClick={() => refine({ period: "all" })}>
          Show all time
        </Button>
      )}
    </>
  );
  return (
    <Card onFocus={onFocus}>
      <CardSection>
        {/* Sort stays a dropdown: as a third Segmented the row no longer fits one line at 1024px (DESIGN.md). */}
        <div className="flex flex-wrap items-center gap-x-6 gap-y-4">
          <Segmented label="Show" value={kind} options={KINDS} onChange={(v) => refine({ kind: v })} />
          <Segmented label="Created" value={period} options={PERIODS} onChange={(v) => refine({ period: v })} />
          <Select label="Sort" inline value={sort} options={SORTS} onChange={(v) => refine({ sort: v })} />
        </div>
      </CardSection>
      <CardSection className={hasPager ? "pb-0" : undefined}>
        {/* Always mounted (a live region must exist before it changes); visually hidden when the empty state or
            the error says it instead. */}
        <div className={listed ? "mb-4 flex flex-wrap items-center gap-3" : "sr-only"}>
          <p aria-live="polite" className="font-mono text-caption text-dim tabular-nums">
            {summary}
          </p>
          {listed && text && (
            <Button size="sm" tone="ghost" icon={<XIcon aria-hidden size={16} />} onClick={clearSearch}>
              Clear search
            </Button>
          )}
        </div>
        {q.isError ? (
          <ErrorNote
            actions={
              <Button size="sm" busy={q.isFetching} icon={<ArrowClockwiseIcon aria-hidden size={16} />} onClick={() => void q.refetch()}>
                Try again
              </Button>
            }
          >
            <span className="block">Couldn’t reach Nexus to list {noun}.</span>
            <span className="mt-1 block font-mono text-caption text-rose-300">{errMsg(q.error)}</span>
          </ErrorNote>
        ) : !listed ? (
          <Empty
            icon={
              text ? (
                <MagnifyingGlassIcon size={28} weight="duotone" />
              ) : period !== "all" ? (
                <CalendarBlankIcon size={28} weight="duotone" />
              ) : (
                <PackageIcon size={28} weight="duotone" />
              )
            }
            title={
              text ? `No ${noun} match “${text}”` : period === "all" ? `No ${noun} listed` : `No ${noun} created ${PERIOD_PHRASE[period]}`
            }
            action={emptyActions || undefined}
          >
            {text
              ? period === "all"
                ? "Try fewer or different words."
                : "Try fewer words, or a longer period."
              : period === "all"
                ? `Nexus lists no ${noun} for this game.`
                : "Try a longer period."}
          </Empty>
        ) : (
          <>
            <ul
              ref={results}
              className={cx(kind === "mods" ? MOD_GRID : RESULTS_GRID, "stagger", dim && "opacity-stale transition-opacity duration-base")}
              aria-label={kind === "mods" ? "Mods" : "Collections"}
              aria-busy={loading || undefined}
              // The previous page, kept up while the next loads, cannot be opened.
              inert={dim || undefined}
            >
              {skeleton
                ? Array.from({ length: skeletons }, (_, i) => (
                    <TileSkeleton key={`sk${i}`} horizontal={kind === "collections"} picture={kind === "mods" ? MOD_PICTURE : COLLECTION_PICTURE} />
                  ))
                : !data
                ? null
                : data.kind === "mods"
                ? data.mods.map((m, i) => (
                    <ModTile
                      key={m.modId !== null ? `m${m.modId}` : `i${i}`}
                      game={game}
                      m={m}
                      focusKey={`m${m.modId ?? m.name}`}
                      // No Nexus id: nothing to download or describe; a dimmed card that says why (as a collection's mods).
                      inert={m.modId === null}
                      extraBadges={m.modId === null && <Badge>Not downloadable: no Nexus id</Badge>}
                    />
                  ))
                : data.collections.map((c) => <CollectionTile key={c.slug} game={game} c={c} />)}
            </ul>
            {/* role=navigation, not <nav> (landmarks inside <main> share its spatial-nav region, nav/spatial.ts).
                A toolbar stuck to the bottom of <main>: the last child of the
                results section, so it pins while the grid is on screen and rests below the last row (in flow, it
                never covers it). -mx-section: edge to edge across the card. While scrolled up it pins flush to
                <main>'s bottom edge: WebKit insets sticky by <main>'s padding-bottom, so bottom is minus that
                padding (--main-pb, set by App). At the end it rests here with <main>'s usual bottom padding below. Nothing of the card shows under it:
                the section has no bottom padding (pb-0) and -mb-px covers the card's bottom border. */}
            {hasPager && (
              <div
                role="navigation"
                className="sticky bottom-[calc(var(--main-pb,0px)*-1)] z-10 -mx-section mt-5 -mb-px flex flex-wrap items-center gap-4 rounded-b-card border-t border-hairline bg-ink-2 px-section py-3"
                aria-label={`${kind === "mods" ? "Mods" : "Collections"} pages`}
              >
                {/* inactive, not disabled: focus stays on the button at the first/last page. */}
                {paged && (
                  <span className="flex gap-2">
                    <Button
                      size="sm"
                      {...{ [FOCUS_ATTR]: "page:first" }}
                      inactive={page === 0}
                      busy={loading}
                      icon={<CaretDoubleLeftIcon aria-hidden size={16} />}
                      onClick={() => goTo(0, "first")}
                    >
                      First
                    </Button>
                    <Button
                      size="sm"
                      {...{ [FOCUS_ATTR]: "page:prev" }}
                      inactive={page === 0}
                      busy={loading}
                      icon={<CaretLeftIcon aria-hidden size={16} />}
                      onClick={() => goPage(-1)}
                    >
                      Previous
                    </Button>
                  </span>
                )}
                <span className="font-mono text-caption text-dim tabular-nums">
                  {loading ? (
                    // The target page; the total may belong to the previous query.
                    <>Page {page + 1} · loading…</>
                  ) : (
                    <>
                      Page {page + 1} of {pages.toLocaleString()} · {(first + 1).toLocaleString()}–
                      {(first + shown).toLocaleString()} of {total.toLocaleString()}
                    </>
                  )}
                </span>
                {paged && (
                  <span className="ml-auto flex gap-2">
                    <Button
                      size="sm"
                      {...{ [FOCUS_ATTR]: "page:next" }}
                      inactive={page + 1 >= pages}
                      busy={loading}
                      onClick={() => goPage(1)}
                    >
                      Next
                      <CaretRightIcon aria-hidden size={16} />
                    </Button>
                    <Button
                      size="sm"
                      {...{ [FOCUS_ATTR]: "page:last" }}
                      inactive={page + 1 >= pages}
                      busy={loading}
                      onClick={() => goTo(pages - 1, "last")}
                    >
                      Last
                      <CaretDoubleRightIcon aria-hidden size={16} />
                    </Button>
                  </span>
                )}
              </div>
            )}
          </>
        )}
      </CardSection>
    </Card>
  );
}

// Mod pictures fill the tile's width at 16:9 (MOD_PICTURE). Collection pictures are 4:5 portrait, left of the
// text, and their width is a mod picture's height. The results <ul> is the container
// (100cqw), and the mod tile's picture width is its column (mods: 1 / sm 2 / xl 3, gap-4 = 1rem) less the tile's
// p-3 and 1px border (1.5rem + 2px); times 9/16 is that picture's height. Keep in step with the grid and Tile.
// The monogram fallback takes the same box, so a picture arriving never shifts the grid.
// self-start: the horizontal tile's row stretches its items to the text column's (or grid row's) height, and a
// stretched cross size overrides aspect-ratio when the width is fixed, so the picture would grow past 4:5.
// rounded-control-sm: one step below the tile's rounded-card-lg (as MOD_PICTURE), so the corners nest.
const COLLECTION_PICTURE = cx(
  "aspect-[4/5] shrink-0 self-start rounded-control-sm",
  "w-[calc((100cqw-1.5rem-2px)*9/16)]",
  "sm:w-[calc(((100cqw-1rem)/2-1.5rem-2px)*9/16)]",
  "xl:w-[calc(((100cqw-2rem)/3-1.5rem-2px)*9/16)]",
);

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
    // Accepted: the dialog closes at once (focus back on the card); progress shows in the job island.
    details.hide();
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
        // 2x2, row by row: likes, installs / size, mods. An unknown (null) count is left out, as on mod tiles.
        <span className="grid min-w-0 flex-1 grid-cols-2 gap-x-4 gap-y-1">
          {c.endorsements !== null && <Stat icon={<ThumbsUpIcon aria-hidden size={12} />} value={compact(c.endorsements)} label="endorsements" />}
          {c.downloads !== null && <Stat icon={<DownloadSimpleIcon aria-hidden size={12} />} value={compact(c.downloads)} label="downloads" />}
          {c.total_size !== null && <Stat icon={<HardDrivesIcon aria-hidden size={12} />} value={bytes(c.total_size)} label="size" />}
          {c.mod_count !== null && <Stat icon={<PackageIcon aria-hidden size={12} />} value={String(c.mod_count)} label="mods" />}
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
                  <RemoveButton game={game} item={{ kind: "collection", slug: c.slug, name: c.name }} onAccepted={details.hide} />
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
