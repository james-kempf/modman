// SPDX-License-Identifier: GPL-3.0-or-later
// Browse: the game's mods and collections on Nexus (top of all time or a
// recent window, newest, or matching words) with one-click installs, then
// direct installs by id/URL/archive (FOMOD choices come up as the installer
// wizard dialog), the nxm paste box (Game Mode has no browser handoff) and
// the download queue.

import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { errMsg } from "../app/bridge";
import { useActiveGame } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { useUi } from "../app/store";
import { readClipboard } from "../components/shell/Osk";
import {
  ArrowClockwiseIcon,
  CheckIcon,
  ClipboardTextIcon,
  DownloadSimpleIcon,
  LinkIcon,
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
  StatusDot,
  TextField,
  Toggle,
  ViewFrame,
  ViewHeader,
  bytes,
} from "../components/ui";
import * as ipc from "../ipc/commands";
import type { BrowseKind, BrowsePeriod, BrowseSort, BrowseView, CollectionHit, InstallResult, SearchHit } from "../ipc/types";
import { collectionText, collectionTone } from "./Collections";
import { installKey } from "./Requirements";
import { NoGame } from "../components/shell/NoGame";

/** 12345 -> "12.3K" (legacy `compact`). */
function compact(n: number | null): string {
  if (n === null) return "0";
  return Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 }).format(n);
}

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
      <ViewHeader
        title="Browse"
        eyebrow={game?.name}
        description="The top mods and collections on Nexus Mods, of all time or lately, or search them. Install by ID or link, or paste a Mod Manager Download link, further down."
      />
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
        <p className="mb-5 text-[14px] leading-relaxed text-zinc-400">
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

/** Items in one browse page. */
const pageItems = (p: BrowseView) => (p.kind === "mods" ? p.mods.length : p.collections.length);

/**
 * The tiles of every loaded page. A ranking that shifts between page loads
 * can list an item again on a later page; it is shown once.
 */
function tiles(pages: BrowseView[], game: string): ReactNode[] {
  const seen = new Set<string>();
  const once = (key: string) => !seen.has(key) && !!seen.add(key);
  return pages.flatMap((p, n) =>
    p.kind === "mods"
      ? p.mods
          .map((m, i) => ({ m, key: m.modId !== null ? `m${m.modId}` : `p${n}-${i}` }))
          .filter(({ key }) => once(key))
          .map(({ m, key }) => <ModTile key={key} game={game} m={m} />)
      : p.collections.filter((c) => once(c.slug)).map((c) => <CollectionTile key={c.slug} game={game} c={c} />),
  );
}

function BrowseCard({ game, linked }: { game: string; linked: boolean }) {
  const [kind, setKind] = useState<BrowseKind>("mods");
  const [period, setPeriod] = useState<BrowsePeriod>("all");
  const [sort, setSort] = useState<BrowseSort>("endorsements");
  const [draft, setDraft] = useState("");
  const [text, setText] = useState("");
  const q = useInfiniteQuery({
    queryKey: ["browse", game, kind, period, sort, text],
    queryFn: ({ pageParam }) => ipc.browse({ kind, period, sort, text, offset: pageParam }),
    initialPageParam: 0,
    getNextPageParam: (last, pages) => {
      const shown = pages.reduce((n, p) => n + pageItems(p), 0);
      return pageItems(last) > 0 && shown < last.total ? shown : undefined;
    },
    enabled: linked,
    staleTime: 5 * 60_000,
  });
  const pages = q.data?.pages ?? [];
  const total = pages[0]?.total ?? 0;
  const shown = pages.reduce((n, p) => n + pageItems(p), 0);
  const noun = kind === "mods" ? "mods" : "collections";
  const counted = `${total.toLocaleString()} ${total === 1 ? noun.slice(0, -1) : noun}`;
  const sortLabel = SORTS.find((o) => o.value === sort)!.label.toLowerCase();
  return (
    <Card
      title={kind === "mods" ? "Mods on Nexus" : "Collections on Nexus"}
      intro={
        q.isSuccess
          ? `${counted}${text ? ` matching “${text}”` : ""}, ${sortLabel} ${PERIOD_PHRASE[period]}`
          : undefined
      }
    >
      <CardSection className="grid gap-5">
        <div className="flex flex-wrap items-end gap-x-6 gap-y-4">
          <Segmented label="Show" value={kind} options={KINDS} onChange={setKind} />
          <Segmented label="Created" value={period} options={PERIODS} onChange={setPeriod} />
          <Select label="Sort" value={sort} options={SORTS} onChange={setSort} />
        </div>
        <form
          className="flex flex-wrap items-end gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            setText(draft.trim());
          }}
        >
          <TextField
            label="Search"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            className="min-w-0 flex-[1_1_18rem]"
            placeholder={kind === "mods" ? "Search mod names" : "Search collections"}
          />
          <Button type="submit" inactive={draft.trim() === text} icon={<MagnifyingGlassIcon aria-hidden size={16} />}>
            Search
          </Button>
          {text && (
            <Button
              tone="ghost"
              icon={<XIcon aria-hidden size={16} />}
              onClick={() => {
                setDraft("");
                setText("");
              }}
            >
              Clear
            </Button>
          )}
        </form>
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
            <ul className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3" aria-label={kind === "mods" ? "Mods" : "Collections"}>
              {tiles(pages, game)}
            </ul>
            <div className="mt-5 flex flex-wrap items-center gap-4">
              <span className="font-mono text-caption text-dim">
                {shown.toLocaleString()} of {total.toLocaleString()}
              </span>
              {q.hasNextPage && (
                <Button busy={q.isFetchingNextPage} onClick={() => void q.fetchNextPage()}>
                  Load more
                </Button>
              )}
            </div>
          </>
        )}
      </CardSection>
    </Card>
  );
}

/** One browse result: picture, name and byline, summary, stats, the install action. */
function Tile({
  picture,
  name,
  byline,
  summary,
  stats,
  action,
}: {
  picture: ReactNode;
  name: string;
  byline: string;
  summary: string | null;
  stats: ReactNode;
  action: ReactNode;
}) {
  return (
    <li className="flex min-w-0 flex-col gap-3 rounded-card-lg border border-hairline bg-ink p-2.5 pb-3.5">
      {picture}
      <div className="flex min-w-0 flex-1 flex-col px-1.5">
        <span className="block truncate text-[15.5px] font-medium text-zinc-100" title={name}>
          {name}
        </span>
        <span className="block truncate font-mono text-[11.5px] text-dim">{byline}</span>
        {summary && <p className="mt-2 line-clamp-2 text-[13px] leading-relaxed text-zinc-400">{summary}</p>}
        <div className="mt-auto flex flex-wrap items-center gap-x-4 gap-y-2 pt-3">
          {stats}
          <span className="ml-auto">{action}</span>
        </div>
      </div>
    </li>
  );
}

function Stat({ icon, value, label }: { icon: ReactNode; value: string; label: string }) {
  return (
    <span className="flex items-center gap-1.5 font-mono text-[11.5px] text-dim">
      {icon}
      {value}
      <span className="sr-only">{label}</span>
    </span>
  );
}

function InstalledMark({ children = "Installed" }: { children?: ReactNode }) {
  return (
    <span className="flex min-h-11 items-center gap-1.5 px-2 text-[13px] text-accent-300">
      <CheckIcon aria-hidden size={14} weight="bold" /> {children}
    </span>
  );
}

const PICTURE = "aspect-video w-full rounded-card";

function ModTile({ game, m }: { game: string; m: SearchHit }) {
  const run = useRun();
  const qc = useQueryClient();
  const notify = useUi((s) => s.notify);
  const installing = useUi((s) => s.busy);
  const name = m.name ?? `Mod ${m.modId ?? "?"}`;
  const key = installKey(String(m.modId));
  return (
    <Tile
      picture={<Picture url={m.thumbnailUrl} name={name} className={PICTURE} />}
      name={name}
      byline={[m.version ? `v${m.version.replace(/^v/i, "")}` : null, m.author, m.category].filter(Boolean).join(" · ")}
      summary={m.summary}
      stats={
        <>
          <Stat icon={<DownloadSimpleIcon aria-hidden size={13} />} value={compact(m.downloads)} label="downloads" />
          <Stat icon={<ThumbsUpIcon aria-hidden size={13} />} value={compact(m.endorsements)} label="endorsements" />
        </>
      }
      action={
        m.installed ? (
          <InstalledMark />
        ) : (
          <Button
            size="sm"
            tone="primary"
            disabled={m.modId === null}
            busy={(installing[key] ?? 0) > 0}
            aria-label={`Install ${name}`}
            onClick={async () => {
              const r = await run("Install", () => ipc.install({ target: String(m.modId), fomod: "ask" }), { key });
              if (!r) return;
              notify("success", installedText(r));
              void qc.invalidateQueries({ queryKey: ["browse", game] });
            }}
          >
            Install
          </Button>
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
  // The same key as the Collections view's actions on this collection.
  const key = `Collection ${c.slug}`;
  const behind = c.installed_revision !== null && c.revision !== null && c.installed_revision < c.revision;
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
      picture={<Picture url={c.image_url} name={c.name} className={PICTURE} />}
      name={c.name}
      byline={[c.author, c.category, c.revision !== null ? `revision ${c.revision}` : null].filter(Boolean).join(" · ")}
      summary={c.summary}
      stats={
        <>
          {c.mod_count !== null && (
            <Stat icon={<PackageIcon aria-hidden size={13} />} value={`${c.mod_count} · ${bytes(c.total_size)}`} label="mods, size" />
          )}
          <Stat icon={<DownloadSimpleIcon aria-hidden size={13} />} value={compact(c.downloads)} label="downloads" />
          <Stat icon={<ThumbsUpIcon aria-hidden size={13} />} value={compact(c.endorsements)} label="endorsements" />
        </>
      }
      action={
        c.installed_revision !== null && !behind ? (
          <InstalledMark />
        ) : (
          <Button
            size="sm"
            tone="primary"
            busy={(installing[key] ?? 0) > 0}
            aria-label={`${behind ? "Update" : "Install"} ${c.name}`}
            onClick={() => void act(behind)}
          >
            {behind ? `Update to ${c.revision}` : "Install"}
          </Button>
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
