// SPDX-License-Identifier: GPL-3.0-or-later
// Activity: jobs (running and finished), their streamed messages, waiting
// questions, forwarded nxm links and the log file tail. Fluid like the other
// views: jobs and messages side by side from xl, stacked below; every column
// is minmax(0, ...) so long log lines scroll inside their box instead of
// widening the page.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { errMsg } from "../app/bridge";
import { jobLabel, jobOutcome } from "../app/jobs";
import { useUi } from "../app/store";
import { ArrowClockwiseIcon } from "@phosphor-icons/react";
import { Button, Card, Empty, ErrorNote, MsgLine, StatusDot, ViewHeader } from "../components/ui";
import * as ipc from "../ipc/commands";

export function Activity() {
  return (
    <div className="grid grid-cols-1 gap-6">
      <ViewHeader
        title="Activity"
        description="Everything the app ran in this session, with its output. The log file also covers the command line and the auto-update timer."
      />
      <div data-testid="activity-columns" className="grid grid-cols-1 gap-6 xl:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
        <Jobs />
        <Messages />
      </div>
      <LogTail />
    </div>
  );
}

function Jobs() {
  const jobs = useUi((s) => s.jobs);
  const prompts = useUi((s) => s.prompts.length);
  const nxm = useUi((s) => s.nxm);
  const list = Object.values(jobs).sort((a, b) => b.id - a.id);
  return (
    <Card title="Jobs" className="min-w-0 !p-5">
      {prompts > 0 && (
        <p className="mb-3 flex items-center gap-2 text-[14px] text-amber-300">
          <StatusDot tone="amber" live />
          {prompts} question(s) waiting for an answer.
        </p>
      )}
      {!list.length ? (
        <Empty>No jobs yet in this session.</Empty>
      ) : (
        <ul className="grid content-start gap-1" aria-label="Jobs">
          {list.map((j) => {
            const o = jobOutcome(j);
            return (
              <li key={j.id} tabIndex={0} className="flex flex-wrap items-center gap-3 rounded-xl px-3 py-2.5 hover:bg-white/[0.03]">
                <StatusDot tone={o === "running" ? "accent" : o === "failed" ? "rose" : "zinc"} live={o === "running"} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[14px] text-zinc-200">{jobLabel(j.kind)}</span>
                  <span className="block font-mono text-[11px] text-dim">
                    #{j.id} · {o}
                  </span>
                </span>
                {/* A cancel is the user's choice: no error text. */}
                {j.error && o === "failed" && <span className="w-full pl-5 text-[13px] wrap-anywhere text-rose-300">{j.error}</span>}
              </li>
            );
          })}
        </ul>
      )}
      {nxm.length > 0 && (
        <>
          <h3 className="eyebrow mt-5 mb-2 px-3">nxm links</h3>
          <ul className="grid gap-1 px-3 font-mono text-[12px]">
            {nxm.slice(0, 10).map((n, i) => (
              <li key={i} className={n.status === "rejected" ? "text-rose-300" : "text-zinc-300"}>
                {n.status}: {n.link ?? n.error}
              </li>
            ))}
          </ul>
        </>
      )}
    </Card>
  );
}

function Messages() {
  const activity = useUi((s) => s.activity);
  const recent = activity.slice(-200).reverse();
  return (
    <Card title="Messages" className="min-w-0 !p-5">
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
    </Card>
  );
}

function LogTail() {
  const [lines, setLines] = useState(200);
  const q = useQuery({ queryKey: ["log", lines], queryFn: () => ipc.logTail(lines) });
  return (
    <Card
      title="modman log"
      className="min-w-0 !p-5"
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
      {q.isError && (
        <div className="mb-4">
          <ErrorNote>Could not read the log file: {errMsg(q.error)}</ErrorNote>
        </div>
      )}
      <pre
        tabIndex={0}
        aria-label="Log"
        className="max-h-[min(32rem,60vh)] overflow-auto rounded-2xl bg-ink p-4 font-mono text-[12px] leading-[1.7] text-zinc-400"
      >
        {q.data?.join("\n") || "(empty)"}
      </pre>
    </Card>
  );
}
