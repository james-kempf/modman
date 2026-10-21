// SPDX-License-Identifier: GPL-3.0-or-later
// Requirements of the enabled mods: missing / disabled / external tools,
// with install, enable and acknowledge actions.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useActiveGame } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { Badge, Button, Card, Empty, ViewHeader } from "../components/ui";
import * as ipc from "../ipc/commands";
import type { ReqRow, ReqStatus } from "../ipc/types";

const ORDER: ReqStatus[] = ["missing", "disabled", "external", "gone", "ok"];
const LABEL: Record<ReqStatus, string> = {
  missing: "Missing",
  disabled: "Installed but disabled",
  external: "External tools",
  gone: "No longer on Nexus",
  ok: "Satisfied",
};
const TONE = { missing: "rose", disabled: "amber", external: "amber", gone: "zinc", ok: "accent" } as const;

/** The install target in a fix hint such as "install 123" (null = not an install). */
export function installTarget(hint: string): string | null {
  const m = /^install\s+(\S+)/.exec(hint.trim());
  return m ? m[1] : null;
}

export function Requirements() {
  const { game } = useActiveGame();
  const qc = useQueryClient();
  const run = useRun();
  const refreshing = useBusy("Refresh requirements");
  const q = useQuery({
    queryKey: ["deps", game?.domain],
    queryFn: () => ipc.deps({}, game?.domain),
    enabled: !!game,
  });
  const rows = q.data ?? [];
  return (
    <div className="grid gap-6">
      <ViewHeader title="Requirements">
        <Button
          busy={refreshing}
          disabled={!game}
          onClick={async () => {
            const fresh = await run("Refresh requirements", () => ipc.deps({ refresh: true }));
            if (fresh) qc.setQueryData(["deps", game?.domain], fresh);
          }}
        >
          Refresh from Nexus
        </Button>
      </ViewHeader>
      {!game ? (
        <Empty>No active game.</Empty>
      ) : q.isLoading ? (
        <p className="text-zinc-400">Checking requirements…</p>
      ) : q.error ? (
        <Empty>Could not check requirements: {String((q.error as { message?: string }).message ?? q.error)}</Empty>
      ) : !rows.length ? (
        <Empty>No requirements recorded for the enabled mods.</Empty>
      ) : (
        ORDER.filter((s) => rows.some((r) => r.status === s)).map((s) => (
          <Card key={s} title={`${LABEL[s]} (${rows.filter((r) => r.status === s).length})`}>
            <ul className="grid gap-2">
              {rows
                .filter((r) => r.status === s)
                .map((r) => (
                  <Row key={r.key} r={r} />
                ))}
            </ul>
          </Card>
        ))
      )}
    </div>
  );
}

function Row({ r }: { r: ReqRow }) {
  const run = useRun();
  const target = r.status === "missing" ? installTarget(r.hint) : null;
  return (
    <li className="flex flex-wrap items-center gap-3 rounded-xl border border-white/[0.06] p-3">
      <span className="min-w-0 flex-1">
        <span className="flex flex-wrap items-center gap-2 text-zinc-100">
          {r.name} <Badge tone={TONE[r.status]}>{r.status}</Badge>
          {r.acked && <Badge>acknowledged</Badge>}
        </span>
        <span className="block text-sm text-zinc-500">
          {r.by.length > 0 && `needed by ${r.by.join(", ")}`}
          {r.detail && ` · ${r.detail}`}
        </span>
        {r.hint && !target && r.status !== "ok" && (
          <span className="block font-mono text-xs text-zinc-400">fix: modman {r.hint}</span>
        )}
        {r.urls.map((u) => (
          <span key={u} className="block font-mono text-xs break-all text-zinc-500 select-all">
            {u}
          </span>
        ))}
      </span>
      {target && (
        <Button tone="primary" onClick={() => run("Install", () => ipc.install({ target, fomod: "ask" }))}>
          Install
        </Button>
      )}
      {r.status === "external" &&
        (r.acked ? (
          <Button onClick={() => run("Unacknowledge", () => ipc.depsUnack(r.key))}>Unacknowledge</Button>
        ) : (
          <Button onClick={() => run("Acknowledge", () => ipc.depsAck(r.key))}>I have it</Button>
        ))}
    </li>
  );
}
