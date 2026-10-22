// SPDX-License-Identifier: GPL-3.0-or-later
// Requirements of the enabled mods: missing / disabled / external tools,
// with install, enable and acknowledge actions.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useActiveGame } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import {
  ArrowClockwiseIcon,
  CheckCircleIcon,
  DownloadSimpleIcon,
  LinkIcon,
  PuzzlePieceIcon,
  SealCheckIcon,
  WarningIcon,
  XCircleIcon,
} from "@phosphor-icons/react";
import { Badge, Button, Empty, ErrorNote, ViewHeader, cx } from "../components/ui";
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
const BLURB: Partial<Record<ReqStatus, string>> = {
  missing: "The mods that need these will not work until they are installed.",
  disabled: "Enable them and deploy again.",
  external: "Hosted outside Nexus, so modman cannot tell whether they are installed. Mark them once you have.",
  gone: "Listed as required, but the page was removed. Usually safe to ignore.",
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
      <ViewHeader
        title="Requirements"
        eyebrow={game?.name}
        description="What your enabled mods need, from their Nexus pages. Loaders like UE4SS or SKSE are detected by their files in the game folder."
      >
        <Button
          size="sm"
          busy={refreshing}
          disabled={!game}
          icon={<ArrowClockwiseIcon aria-hidden size={15} />}
          onClick={async () => {
            const fresh = await run("Refresh requirements", () => ipc.deps({ refresh: true }));
            if (fresh) qc.setQueryData(["deps", game?.domain], fresh);
          }}
        >
          Refresh from Nexus
        </Button>
      </ViewHeader>
      {!game ? (
        <Empty icon={<PuzzlePieceIcon size={28} weight="duotone" />} title="No active game">
          Add one on the Overview.
        </Empty>
      ) : q.isLoading ? (
        <p className="px-1 py-6 text-[14px] text-zinc-400">Checking requirements…</p>
      ) : q.error ? (
        <ErrorNote>Could not check requirements: {String((q.error as { message?: string }).message ?? q.error)}</ErrorNote>
      ) : !rows.length ? (
        <Empty icon={<PuzzlePieceIcon size={28} weight="duotone" />} title="No requirements listed">
          None of the enabled mods list requirements on Nexus.
        </Empty>
      ) : (
        <div className="grid gap-10">
          {ORDER.filter((s) => rows.some((r) => r.status === s)).map((s) => {
            const items = rows.filter((r) => r.status === s);
            return (
              <section key={s}>
                <div className="mb-4 flex items-baseline gap-3 border-b border-white/[0.05] pb-3">
                  <h2 className="text-[17px] font-medium tracking-tight text-zinc-100">
                    {LABEL[s]} <span className="ml-1 font-mono text-[12px] font-normal text-dim">{items.length}</span>
                  </h2>
                  {BLURB[s] && <p className="ml-auto hidden max-w-[52ch] text-right text-[13px] text-dim md:block">{BLURB[s]}</p>}
                </div>
                <ul className={cx("stagger grid gap-3", items.length > 1 && "lg:grid-cols-2")}>
                  {items.map((r) => (
                    <Row key={r.key} r={r} />
                  ))}
                </ul>
              </section>
            );
          })}
        </div>
      )}
    </div>
  );
}

function StatusIcon({ r }: { r: ReqRow }) {
  if (r.status === "missing") return <XCircleIcon aria-hidden size={20} weight="duotone" className="text-rose-300" />;
  if (r.status === "disabled" || (r.status === "external" && !r.acked))
    return <WarningIcon aria-hidden size={20} weight="duotone" className="text-amber-300" />;
  if (r.acked) return <SealCheckIcon aria-hidden size={20} weight="duotone" className="text-accent-300" />;
  return (
    <CheckCircleIcon aria-hidden size={20} weight="duotone" className={r.status === "gone" ? "text-zinc-500" : "text-accent-300"} />
  );
}

function Row({ r }: { r: ReqRow }) {
  const run = useRun();
  const target = r.status === "missing" ? installTarget(r.hint) : null;
  const bad = r.status === "missing";
  return (
    <li className={cx("h-full rounded-2xl border p-5", bad ? "border-rose-400/15 bg-rose-500/[0.035]" : "border-white/[0.05] bg-ink-2")}>
      <div className="flex items-start gap-3">
        <span className="mt-0.5">
          <StatusIcon r={r} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-[15.5px] font-medium text-zinc-100">{r.name}</h3>
            <Badge tone={TONE[r.status]}>{r.status}</Badge>
            {r.acked && <Badge tone="accent">acknowledged</Badge>}
          </div>
          {r.detail && <p className="mt-1 text-[13px] text-dim">{r.detail}</p>}
          {r.by.length > 0 && <p className="mt-2 text-[13px] leading-relaxed text-zinc-400">Needed by {r.by.join(", ")}</p>}
          {r.hint && !target && r.status !== "ok" && (
            <p className="mt-2 font-mono text-[12px] text-zinc-400">fix: modman {r.hint}</p>
          )}
          {r.urls.map((u) => (
            <p key={u} className="mt-2 flex items-center gap-1.5 font-mono text-[12px] break-all text-zinc-400 select-all">
              <LinkIcon aria-hidden size={13} className="shrink-0" />
              {u}
            </p>
          ))}
          {(target || r.status === "external") && (
            <div className="mt-4 flex flex-wrap gap-2">
              {target && (
                <Button
                  size="sm"
                  tone="primary"
                  icon={<DownloadSimpleIcon aria-hidden size={14} />}
                  onClick={() => run("Install", () => ipc.install({ target, fomod: "ask" }))}
                >
                  Install
                </Button>
              )}
              {r.status === "external" &&
                (r.acked ? (
                  <Button size="sm" tone="ghost" onClick={() => run("Unacknowledge", () => ipc.depsUnack(r.key))}>
                    Unacknowledge
                  </Button>
                ) : (
                  <Button size="sm" onClick={() => run("Acknowledge", () => ipc.depsAck(r.key))}>
                    I have it
                  </Button>
                ))}
            </div>
          )}
        </div>
      </div>
    </li>
  );
}
