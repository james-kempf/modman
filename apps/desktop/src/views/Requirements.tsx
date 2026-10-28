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
import {
  Badge,
  Button,
  Card,
  Empty,
  ErrorNote,
  SECTION_PAD,
  ViewHeader,
  cx,
} from "../components/ui";
import * as ipc from "../ipc/commands";
import type { ReqRow, ReqStatus } from "../ipc/types";
import { NoGame } from "../components/shell/NoGame";

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
  external:
    "Hosted outside Nexus, so modman cannot tell whether they are installed. Mark them once you have.",
  gone: "Listed as required, but the page was removed. Usually safe to ignore.",
};
const TONE = {
  missing: "rose",
  disabled: "amber",
  external: "amber",
  gone: "zinc",
  ok: "accent",
} as const;

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
    <div className="grid grid-cols-[minmax(0,1fr)] gap-6">
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
            const fresh = await run(
              "Refresh requirements",
              () => ipc.deps({ refresh: true }),
              { key: "Refresh requirements" },
            );
            if (fresh) qc.setQueryData(["deps", game?.domain], fresh);
          }}
        >
          Refresh from Nexus
        </Button>
      </ViewHeader>
      {!game ? (
        <NoGame icon={<PuzzlePieceIcon size={28} weight="duotone" />} />
      ) : q.isLoading ? (
        <p className="px-1 py-6 text-[14px] text-zinc-400">
          Checking requirements…
        </p>
      ) : q.error ? (
        <ErrorNote>
          Could not check requirements:{" "}
          {String((q.error as { message?: string }).message ?? q.error)}
        </ErrorNote>
      ) : !rows.length ? (
        <Empty
          icon={<PuzzlePieceIcon size={28} weight="duotone" />}
          title="No requirements listed"
        >
          None of the enabled mods list requirements on Nexus.
        </Empty>
      ) : (
        <div className="grid grid-cols-[minmax(0,1fr)] gap-6">
          {ORDER.filter((s) => rows.some((r) => r.status === s)).map((s) => {
            const items = rows.filter((r) => r.status === s);
            return (
              <Card
                key={s}
                title={
                  <>
                    {LABEL[s]}{" "}
                    <span className="ml-1 font-mono text-[12px] font-normal text-dim">
                      {items.length}
                    </span>
                  </>
                }
                intro={BLURB[s]}
              >
                <ul className="rows">
                  {items.map((r) => (
                    <Row key={r.key} r={r} />
                  ))}
                </ul>
              </Card>
            );
          })}
        </div>
      )}
    </div>
  );
}

function StatusIcon({ r }: { r: ReqRow }) {
  if (r.status === "missing")
    return (
      <XCircleIcon
        aria-hidden
        size={20}
        weight="duotone"
        className="text-rose-300"
      />
    );
  if (r.status === "disabled" || (r.status === "external" && !r.acked))
    return (
      <WarningIcon
        aria-hidden
        size={20}
        weight="duotone"
        className="text-amber-300"
      />
    );
  if (r.acked)
    return (
      <SealCheckIcon
        aria-hidden
        size={20}
        weight="duotone"
        className="text-accent-300"
      />
    );
  return (
    <CheckCircleIcon
      aria-hidden
      size={20}
      weight="duotone"
      className={r.status === "gone" ? "text-zinc-500" : "text-accent-300"}
    />
  );
}

/** In-flight keys: a double click installs or (un)acknowledges once. */
export const installKey = (target: string) => `Install ${target}`;
const ackKey = (key: string) => `Acknowledge ${key}`;

function Row({ r }: { r: ReqRow }) {
  const run = useRun();
  const target = r.status === "missing" ? installTarget(r.hint) : null;
  const installing = useBusy(installKey(target ?? ""));
  const acking = useBusy(ackKey(r.key));
  const bad = r.status === "missing";
  return (
    <li className={cx("min-w-0", SECTION_PAD, bad && "bg-rose-500/[0.035]")}>
      <div className="flex items-start gap-3">
        <span className="mt-0.5">
          <StatusIcon r={r} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="min-w-0 text-[15.5px] font-medium wrap-anywhere text-zinc-100">
              {r.name}
            </h3>
            <Badge tone={TONE[r.status]}>{r.status}</Badge>
            {r.acked && <Badge tone="accent">acknowledged</Badge>}
          </div>
          {r.detail && (
            <p className="mt-1 text-[13px] wrap-anywhere text-dim">
              {r.detail}
            </p>
          )}
          {r.by.length > 0 && (
            <p className="mt-2 text-[13px] leading-relaxed wrap-anywhere text-zinc-400">
              Needed by {r.by.join(", ")}
            </p>
          )}
          {r.hint && !target && r.status !== "ok" && (
            <p className="mt-2 font-mono text-[12px] wrap-anywhere text-zinc-400">
              fix: modman {r.hint}
            </p>
          )}
          {r.urls.map((u) => (
            <p
              key={u}
              className="mt-2 flex items-center gap-1.5 font-mono text-[12px] break-all text-zinc-400 select-all"
            >
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
                  busy={installing}
                  icon={<DownloadSimpleIcon aria-hidden size={14} />}
                  onClick={() =>
                    run(
                      "Install",
                      () => ipc.install({ target, fomod: "ask" }),
                      { key: installKey(target) },
                    )
                  }
                >
                  Install
                </Button>
              )}
              {r.status === "external" &&
                (r.acked ? (
                  <Button
                    size="sm"
                    tone="ghost"
                    busy={acking}
                    onClick={() =>
                      run("Unacknowledge", () => ipc.depsUnack(r.key), {
                        key: ackKey(r.key),
                      })
                    }
                  >
                    Unacknowledge
                  </Button>
                ) : (
                  <Button
                    size="sm"
                    busy={acking}
                    onClick={() =>
                      run("Acknowledge", () => ipc.depsAck(r.key), {
                        key: ackKey(r.key),
                      })
                    }
                  >
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
