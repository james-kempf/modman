// SPDX-License-Identifier: GPL-3.0-or-later
// Activity: jobs (running and finished), their streamed messages, waiting
// questions, forwarded nxm links, the download queue and the log file tail.
// Fluid like the other views: jobs and messages side by side from xl, stacked
// below; every column is minmax(0, ...) so long log lines scroll inside their
// box instead of widening the page.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { errMsg } from "../app/bridge";
import { jobLabel, jobOutcome } from "../app/jobs";
import { useUi } from "../app/store";
import { ArrowClockwiseIcon } from "@phosphor-icons/react";
import { Badge, Button, Card, CardSection, Empty, ErrorNote, MsgLine, StatusDot, ViewFrame, bytes } from "../components/ui";
import * as ipc from "../ipc/commands";

export function Activity() {
  return (
    <ViewFrame>
      <div data-testid="activity-columns" className="grid grid-cols-1 gap-6 xl:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
        <Jobs />
        <Messages />
      </div>
      <DownloadsCard />
      <LogTail />
    </ViewFrame>
  );
}

function Jobs() {
  const jobs = useUi((s) => s.jobs);
  const prompts = useUi((s) => s.prompts.length);
  const nxm = useUi((s) => s.nxm);
  const list = Object.values(jobs).sort((a, b) => b.id - a.id);
  return (
    <Card title="Jobs">
      <CardSection>
        {prompts > 0 && (
          <p className="mb-3 flex items-center gap-2 text-body text-amber-300">
            <StatusDot tone="amber" live />
            {prompts} question(s) waiting for an answer.
          </p>
        )}
        {!list.length ? (
          <Empty>No jobs yet in this session.</Empty>
        ) : (
          // One tab stop for the whole list (a scroll region), not one per row; at most MAX_JOBS rows.
          <ul
            className="grid max-h-[min(28rem,60vh)] content-start gap-1 overflow-y-auto overscroll-contain"
            aria-label="Jobs"
            tabIndex={0}
          >
            {list.map((j) => {
              const o = jobOutcome(j);
              return (
                <li key={j.id} className="flex flex-wrap items-center gap-3 rounded-inner px-3 py-2.5">
                  <StatusDot tone={o === "running" ? "accent" : o === "failed" ? "rose" : "zinc"} live={o === "running"} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-body text-zinc-200">{jobLabel(j.kind)}</span>
                    <span className="block font-mono text-micro text-dim">
                      #{j.id} · {o}
                    </span>
                  </span>
                  {/* A cancel is the user's choice: no error text. */}
                  {j.error && o === "failed" && <span className="w-full pl-5 text-label wrap-anywhere text-rose-300">{j.error}</span>}
                </li>
              );
            })}
          </ul>
        )}
        {nxm.length > 0 && (
          <>
            <h3 className="eyebrow mt-5 mb-2 px-3">nxm links</h3>
            <ul className="grid gap-1 px-3 font-mono text-caption">
              {nxm.slice(0, 10).map((n, i) => (
                <li key={i} className={n.status === "rejected" ? "text-rose-300" : "text-zinc-300"}>
                  {n.status}: {n.link ?? n.error}
                </li>
              ))}
            </ul>
          </>
        )}
      </CardSection>
    </Card>
  );
}

function Messages() {
  const activity = useUi((s) => s.activity);
  const recent = activity.slice(-200).reverse();
  return (
    <Card title="Messages">
      <CardSection>
        {!recent.length ? (
          <Empty>Messages from running jobs appear here.</Empty>
        ) : (
          <ol
            aria-label="Messages"
            tabIndex={0}
            className="grid max-h-[min(28rem,60vh)] content-start gap-0.5 overflow-y-auto font-mono text-[12.5px] leading-[1.7] wrap-anywhere whitespace-pre-wrap"
          >
            {recent.map((a) => (
              <li key={a.seq}>
                {a.job !== null && <span className="mr-2 text-dim">#{a.job}</span>}
                <MsgLine msg={a.msg} />
              </li>
            ))}
          </ol>
        )}
      </CardSection>
    </Card>
  );
}

function LogTail() {
  const [lines, setLines] = useState(200);
  const q = useQuery({ queryKey: ["log", lines], queryFn: () => ipc.logTail(lines) });
  return (
    <Card
      title="Vessel log"
      actions={
        <>
          <Button size="sm" tone="ghost" icon={<ArrowClockwiseIcon aria-hidden size={14} />} onClick={() => q.refetch()}>
            Refresh
          </Button>
          <Button size="sm" tone="ghost" onClick={() => setLines((n) => n + 500)}>
            Show more
          </Button>
        </>
      }
    >
      <CardSection>
        {q.isError && (
          <div className="mb-4">
            <ErrorNote>Could not read the log file: {errMsg(q.error)}</ErrorNote>
          </div>
        )}
        <pre
          tabIndex={0}
          aria-label="Log"
          className="max-h-[min(32rem,60vh)] overflow-auto rounded-card bg-ink p-4 font-mono text-caption leading-[1.7] text-zinc-400"
        >
          {q.data?.join("\n") || "(empty)"}
        </pre>
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
              <li key={d.name} tabIndex={0} className="grid gap-2 rounded-inner py-3">
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
                <li key={r.id} tabIndex={0} className="grid gap-2 rounded-inner py-3">
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
