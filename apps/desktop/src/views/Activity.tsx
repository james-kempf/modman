// SPDX-License-Identifier: GPL-3.0-or-later
// Activity: jobs (running and finished), their streamed messages, waiting
// questions, forwarded nxm links and the log file tail.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useUi } from "../app/store";
import { Badge, Button, Card, Empty, MsgLine, ViewHeader } from "../components/ui";
import * as ipc from "../ipc/commands";

export function Activity() {
  return (
    <div className="grid gap-6">
      <ViewHeader title="Activity" />
      <div className="grid gap-6 xl:grid-cols-[1fr_1.4fr]">
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
    <Card title="Jobs">
      {prompts > 0 && <p className="mb-3 text-amber-300">{prompts} question(s) waiting for an answer.</p>}
      {!list.length ? (
        <Empty>No jobs yet in this session.</Empty>
      ) : (
        <ul className="grid gap-2" aria-label="Jobs">
          {list.map((j) => (
            <li key={j.id} tabIndex={0} className="flex flex-wrap items-center gap-3 rounded-xl border border-white/[0.06] p-3">
              <span className="font-mono text-zinc-500">#{j.id}</span>
              <span className="min-w-0 flex-1 text-zinc-100">{j.kind}</span>
              <Badge tone={j.state === "started" ? "accent" : j.state === "failed" ? "rose" : "zinc"}>
                {j.state === "started" ? "running" : j.state}
              </Badge>
              {j.error && <span className="w-full text-sm text-rose-300">{j.error}</span>}
            </li>
          ))}
        </ul>
      )}
      {nxm.length > 0 && (
        <>
          <h3 className="eyebrow mt-5 mb-2">nxm links</h3>
          <ul className="grid gap-1 text-sm">
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
    <Card title="Messages">
      {!recent.length ? (
        <Empty>Messages from running jobs appear here.</Empty>
      ) : (
        <ol aria-label="Messages" tabIndex={0} className="grid max-h-[28rem] gap-1 overflow-y-auto font-mono text-sm">
          {recent.map((a) => (
            <li key={a.seq}>
              {a.job !== null && <span className="mr-2 text-zinc-600">#{a.job}</span>}
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
      title="Log file"
      actions={
        <>
          <Button onClick={() => q.refetch()}>Refresh</Button>
          <Button onClick={() => setLines((n) => n + 500)}>Show more</Button>
        </>
      }
    >
      <pre tabIndex={0} aria-label="Log" className="max-h-[28rem] overflow-auto rounded-xl bg-ink p-4 font-mono text-xs leading-relaxed text-zinc-400">
        {q.data?.join("\n") || "(empty)"}
      </pre>
    </Card>
  );
}
