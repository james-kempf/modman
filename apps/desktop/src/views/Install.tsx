// SPDX-License-Identifier: GPL-3.0-or-later
// Install: mod id / URL / archive path (FOMOD choices come up as the
// installer wizard dialog), the nxm paste box (Game Mode has no browser
// handoff), Nexus search and the download queue.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useActiveGame } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { useUi } from "../app/store";
import { readClipboard } from "../components/Osk";
import { Badge, Button, Card, Empty, Segmented, TextField, Thumb, Toggle, ViewHeader, bytes } from "../components/ui";
import * as ipc from "../ipc/commands";
import type { InstallResult, SearchView } from "../ipc/types";

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

export function Install() {
  return (
    <div className="grid gap-6">
      <ViewHeader title="Install" />
      <div className="grid gap-6 xl:grid-cols-2">
        <InstallCard />
        <NxmCard />
      </div>
      <SearchCard />
      <DownloadsCard />
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
    const r = await run("Install", () => ipc.install({ target: target.trim(), fomod, deploy }));
    if (r) {
      notify(r.installed || r.collection ? "success" : "info", installedText(r));
      setTarget("");
    }
  };
  return (
    <Card title="Install a mod">
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
          placeholder="e.g. 266 or https://www.nexusmods.com/skyrimspecialedition/mods/266"
        />
        <Segmented label="FOMOD" value={fomod} options={FOMOD} onChange={setFomod} />
        <div className="flex flex-wrap items-center gap-3">
          <Toggle label="Deploy after install" checked={deploy} onChange={setDeploy} />
          <Button type="submit" tone="primary" busy={busy} disabled={!target.trim()} className="ml-auto">
            Install
          </Button>
        </div>
      </form>
    </Card>
  );
}

function NxmCard() {
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const busy = useBusy("Queue link");
  const [url, setUrl] = useState("");
  const submit = async (link: string) => {
    if (!link.trim()) return;
    const r = await run("Queue link", () => ipc.nxmSubmit(link.trim()));
    if (r) {
      notify("success", `Queued ${r.kind} link ${r.link}`);
      setUrl("");
    }
  };
  return (
    <Card title="Paste an nxm:// link">
      <p className="mb-4 text-sm text-zinc-400">
        From the "Mod Manager Download" button on Nexus. Links clicked in a browser arrive here by themselves.
      </p>
      <form
        className="grid gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          void submit(url);
        }}
      >
        <TextField label="nxm link" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="nxm://…" />
        <div className="flex flex-wrap gap-3">
          <Button
            onClick={async () => {
              const t = await readClipboard();
              if (t === null) notify("warn", "Clipboard not readable here; paste into the field with Ctrl+V.");
              else setUrl(t.trim());
            }}
          >
            Paste
          </Button>
          <Button type="submit" tone="primary" busy={busy} disabled={!url.trim()} className="ml-auto">
            Download
          </Button>
        </div>
      </form>
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
  return (
    <Card title="Search Nexus Mods">
      <form
        className="flex flex-wrap items-end gap-3"
        onSubmit={async (e) => {
          e.preventDefault();
          if (query.trim()) setResult((await run("Search", () => ipc.search(query.trim()))) ?? null);
        }}
      >
        <TextField label="Search" value={query} onChange={(e) => setQuery(e.target.value)} className="min-w-72 flex-1" />
        <Button type="submit" busy={searching} disabled={!query.trim()}>
          Search
        </Button>
      </form>
      {result && (
        <ul className="mt-5 grid gap-2" aria-label="Search results">
          {!result.mods.length && <Empty>No matches.</Empty>}
          {result.mods.map((m) => (
            <li key={m.modId ?? m.name} className="flex flex-wrap items-center gap-3 rounded-xl border border-white/[0.06] p-3">
              {game && <Thumb game={game.domain} modId={m.modId} />}
              <span className="min-w-0 flex-1">
                <span className="block text-zinc-100">{m.name}</span>
                <span className="block truncate text-sm text-zinc-500">
                  {m.author} · {m.downloads ?? 0} downloads · {m.summary}
                </span>
              </span>
              {m.installed ? (
                <Badge tone="accent">Installed</Badge>
              ) : (
                <Button
                  tone="primary"
                  disabled={m.modId === null}
                  onClick={async () => {
                    const r = await run("Install", () => ipc.install({ target: String(m.modId), fomod: "ask" }));
                    if (r) notify("success", installedText(r));
                  }}
                >
                  Install
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

function DownloadsCard() {
  const q = useQuery({ queryKey: ["downloads"], queryFn: ipc.downloadsList });
  const live = useUi((s) => s.downloads);
  const rows = q.data ?? [];
  const liveOnly = Object.values(live).filter((d) => d.last.type !== "finished" && !rows.some((r) => r.file_name === d.name));
  return (
    <Card title="Downloads">
      {!rows.length && !liveOnly.length ? (
        <Empty>No downloads.</Empty>
      ) : (
        <ul className="grid gap-2" aria-label="Downloads">
          {liveOnly.map((d) => (
            <li key={d.name} tabIndex={0} className="rounded-xl border border-white/[0.06] p-3">
              <span className="block text-zinc-100">{d.name}</span>
              <progress aria-label={`${d.name} progress`} max={d.total ?? 1} value={d.total ? d.done : undefined} />
              <span className="text-sm text-zinc-500">
                {bytes(d.done)} / {bytes(d.total)}
                {d.last.type === "retrying" && ` · retrying (${d.last.reason})`}
              </span>
            </li>
          ))}
          {rows.map((r) => {
            const l = r.file_name ? live[r.file_name] : undefined;
            const done = l?.done ?? r.bytes_done;
            const total = l?.total ?? r.size;
            return (
              <li key={r.id} tabIndex={0} className="rounded-xl border border-white/[0.06] p-3">
                <span className="flex flex-wrap items-center gap-2">
                  <span className="min-w-0 flex-1 truncate text-zinc-100">
                    {r.file_name ?? (r.nexus_mod_id ? `mod ${r.nexus_mod_id}` : r.collection_slug ?? `#${r.id}`)}
                  </span>
                  <Badge tone={r.status === "failed" ? "rose" : r.status === "done" ? "accent" : "zinc"}>{r.status}</Badge>
                </span>
                {r.status === "downloading" && <progress aria-label="progress" max={total ?? 1} value={total ? done : undefined} />}
                <span className="text-sm text-zinc-500">
                  {bytes(done)} / {bytes(total)} · {r.source}
                  {r.error && ` · ${r.error}`}
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </Card>
  );
}
