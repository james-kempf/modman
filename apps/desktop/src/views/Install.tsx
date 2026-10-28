// SPDX-License-Identifier: GPL-3.0-or-later
// Install: mod id / URL / archive path (FOMOD choices come up as the
// installer wizard dialog), the nxm paste box (Game Mode has no browser
// handoff), Nexus search and the download queue.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { errMsg } from "../app/bridge";
import { useActiveGame } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { useUi } from "../app/store";
import { readClipboard } from "../components/Osk";
import {
  CheckIcon,
  ClipboardTextIcon,
  DownloadSimpleIcon,
  LinkIcon,
  MagnifyingGlassIcon,
  ThumbsUpIcon,
} from "@phosphor-icons/react";
import { Badge, Button, Card, CardSection, Empty, ErrorNote, Segmented, StatusDot, TextField, Thumb, Toggle, ViewHeader, bytes, cx } from "../components/ui";

/** 12345 -> "12.3K" (legacy `compact`). */
function compact(n: number | null): string {
  if (n === null) return "0";
  return Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 }).format(n);
}
import * as ipc from "../ipc/commands";
import type { InstallResult, SearchView } from "../ipc/types";
import { installKey } from "./Requirements";
import { NoGame } from "../components/NoGame";

const FOMOD = [
  { value: "ask", label: "Ask (wizard)" },
  { value: "reuse", label: "Reuse choices" },
  { value: "default", label: "Defaults" },
] as const;

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

export function Install() {
  const { game, isLoading } = useActiveGame();
  return (
    <div className="grid grid-cols-[minmax(0,1fr)] gap-6">
      <ViewHeader
        title="Install"
        eyebrow={game?.name}
        description="Install by ID or link, paste a Mod Manager Download link, or search Nexus Mods. Installers with options open as a wizard."
      />
      {isLoading ? null : !game ? (
        // Every install goes into the active game.
        <NoGame>Pick or add a game in Games first; mods are installed into the active game.</NoGame>
      ) : (
        <>
          <section className="stagger grid grid-cols-[minmax(0,1fr)] gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
            <InstallCard />
            <NxmCard />
          </section>
          {/* key: search results belong to one game. */}
          <SearchCard key={game.domain} />
          <DownloadsCard />
        </>
      )}
    </div>
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

function SearchCard() {
  const { game } = useActiveGame();
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<SearchView | null>(null);
  const searching = useBusy("Search");
  const installing = useUi((s) => s.busy);
  return (
    <Card title="Search Nexus Mods">
      <CardSection>
        <form
          className="flex flex-wrap items-end gap-3"
          onSubmit={async (e) => {
            e.preventDefault();
            if (query.trim()) setResult((await run("Search", () => ipc.search(query.trim()), { key: "Search" })) ?? null);
          }}
        >
          <TextField
            label="Search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="min-w-0 flex-[1_1_18rem]"
            placeholder={game ? `Search ${game.name} mods` : "Search mods"}
          />
          <Button type="submit" busy={searching} disabled={!query.trim()} icon={<MagnifyingGlassIcon aria-hidden size={16} />}>
            Search
          </Button>
        </form>
        <p className="mt-3 text-[12.5px] text-dim">
          {result ? `${result.total.toLocaleString()} results, best matches first` : "Matches mod names, most relevant first."}
        </p>
        {result && (
          <ul className="stagger mt-6 grid gap-4 md:grid-cols-2" aria-label="Search results">
            {!result.mods.length && (
              <li className="md:col-span-2">
                <Empty>No mods found. Try fewer words, or the mod&apos;s exact name.</Empty>
              </li>
            )}
            {result.mods.map((m, i) => (
              <li
                key={m.modId ?? m.name}
                className={cx("flex gap-4 rounded-3xl border border-white/[0.05] bg-ink p-4", i % 5 === 0 && "md:col-span-2")}
              >
                {game && (
                  <Thumb
                    game={game.domain}
                    modId={m.modId}
                    name={m.name ?? ""}
                    className={cx("rounded-2xl", i % 5 === 0 ? "h-28 w-44" : "size-24")}
                  />
                )}
                <div className="flex min-w-0 flex-1 flex-col">
                  <span className="block truncate text-[15.5px] font-medium text-zinc-100">{m.name}</span>
                  <span className="block truncate font-mono text-[11.5px] text-dim">
                    {m.version ? `v${m.version.replace(/^v/i, "")} · ` : ""}
                    {m.author}
                    {m.category ? ` · ${m.category}` : ""}
                  </span>
                  {m.summary && <p className="mt-2 line-clamp-2 text-[13px] leading-relaxed text-zinc-400">{m.summary}</p>}
                  <div className="mt-auto flex items-center gap-4 pt-3">
                    <span className="flex items-center gap-1.5 font-mono text-[11.5px] text-dim">
                      <DownloadSimpleIcon aria-hidden size={13} />
                      {compact(m.downloads)}
                      <span className="sr-only">downloads</span>
                    </span>
                    <span className="flex items-center gap-1.5 font-mono text-[11.5px] text-dim">
                      <ThumbsUpIcon aria-hidden size={13} />
                      {compact(m.endorsements)}
                      <span className="sr-only">endorsements</span>
                    </span>
                    <span className="ml-auto">
                      {m.installed ? (
                        <span className="flex min-h-11 items-center gap-1.5 px-2 text-[13px] text-accent-300">
                          <CheckIcon aria-hidden size={14} weight="bold" /> Installed
                        </span>
                      ) : (
                        <Button
                          size="sm"
                          tone="primary"
                          disabled={m.modId === null}
                          busy={(installing[installKey(String(m.modId))] ?? 0) > 0}
                          onClick={async () => {
                            const r = await run("Install", () => ipc.install({ target: String(m.modId), fomod: "ask" }), {
                              key: installKey(String(m.modId)),
                            });
                            if (r) notify("success", installedText(r));
                          }}
                        >
                          Install
                        </Button>
                      )}
                    </span>
                  </div>
                </div>
              </li>
            ))}
          </ul>
        )}
      </CardSection>
    </Card>
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
              <li key={d.name} tabIndex={0} className="grid gap-2 rounded-lg py-3">
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
                <li key={r.id} tabIndex={0} className="grid gap-2 rounded-lg py-3">
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
