// SPDX-License-Identifier: GPL-3.0-or-later
// Consent for a loader op (install, upgrade, repair, uninstall, adopt):
// what will run, exactly, with which environment and what it may change,
// then a live log. Confirm sends the reviewed preview_id + plan_hash to
// loader_run; Cancel sends nothing (the preview expires on its own). An
// unverified archive needs the acknowledgement box, which re-requests the
// preview with unverified_ack (it is part of the hashed plan).

import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { CheckIcon, ShieldWarningIcon, WarningIcon } from "@phosphor-icons/react";
import { refreshAll } from "../../app/queries";
import { useUi, type LoaderConsent } from "../../app/store";
import { loaderPreviewReopen, loaderRun } from "../../ipc/commands";
import type { LoaderCard, Preview, RunReport } from "../../ipc/types";
import { Dialog } from "../dialogs/Dialog";
import { Badge, Button, TILE, cx } from "../ui";
import { LoaderErrorNote } from "./LoaderError";

export const OP_VERB: Record<Preview["op"], string> = {
  install: "Install",
  upgrade: "Upgrade",
  repair: "Repair",
  uninstall: "Uninstall",
  adopt: "Adopt",
};

export function LoaderConsentHost() {
  const head = useUi((s) => s.loaderConsents[0]);
  if (!head) return null;
  // key: fresh state and focus per consent.
  return <LoaderConsentDialog key={head.id} consent={head} />;
}

type Phase = "review" | "running" | "done";

export function LoaderConsentDialog({ consent }: { consent: LoaderConsent }) {
  const qc = useQueryClient();
  const drop = useUi((s) => s.dropLoaderConsent);
  const [preview, setPreview] = useState<Preview | undefined>(consent.preview);
  const [error, setError] = useState<unknown>(null);
  const [acked, setAcked] = useState(false);
  const [busy, setBusy] = useState(!consent.preview);
  const [phase, setPhase] = useState<Phase>("review");
  const [report, setReport] = useState<RunReport | null>(null);
  const log = useUi((s) => (preview ? s.loaderLog[preview.preview_id] : undefined));
  const fetched = useRef(false);

  // A pending preview (install, collection, nxm) carries only its token: show it in full.
  useEffect(() => {
    if (consent.preview || !consent.pending || fetched.current) return;
    fetched.current = true;
    loaderPreviewReopen(consent.pending.preview_id, false)
      .then(setPreview, setError)
      .finally(() => setBusy(false));
  }, [consent]);

  const close = () => {
    if (phase !== "running") drop(consent.id);
  };
  const unverified = preview?.provenance?.kind === "unverified";
  const blocked = !preview || busy || (unverified && !acked);

  const toggleAck = async () => {
    if (!preview || busy) return;
    if (acked) return setAcked(false);
    setBusy(true);
    setError(null);
    try {
      setPreview(await loaderPreviewReopen(preview.preview_id, true));
      setAcked(true);
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };

  const retry = async () => {
    if (!preview) return;
    setBusy(true);
    setError(null);
    try {
      setPreview(await loaderPreviewReopen(preview.preview_id, acked));
      setPhase("review");
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };

  const confirm = async () => {
    if (blocked || !preview) return;
    setPhase("running");
    setError(null);
    try {
      setReport(await loaderRun(preview.preview_id, preview.plan_hash));
    } catch (e) {
      setError(e);
    } finally {
      setPhase("done");
      void refreshAll(qc);
      void qc.invalidateQueries({ queryKey: ["loaders"] });
    }
  };

  const name =
    qc
      .getQueryData<LoaderCard[]>(["loaders", consent.game ?? undefined])
      ?.find((l) => l.key === (preview?.loader_key ?? consent.pending?.loader_key))?.name ??
    (preview?.loader_key ?? consent.pending?.loader_key ?? "loader").toUpperCase();
  const op = preview?.op ?? consent.pending?.op ?? "install";
  const version = preview?.version ?? consent.pending?.version;

  return (
    <Dialog
      wide
      title={`${OP_VERB[op]} ${name}${version ? ` ${version}` : ""}`}
      subtitle={phase === "review" ? "Review what will run before you confirm. Nothing has run yet." : undefined}
      onBack={close}
    >
      {error !== null && (
        <div className="mb-5">
          <LoaderErrorNote error={error} handlers={preview ? { retry: () => void retry() } : {}} />
        </div>
      )}
      {!preview ? (
        busy && <p className="text-body text-zinc-400">Preparing the preview…</p>
      ) : phase === "review" ? (
        <Review preview={preview} acked={acked} busy={busy} onAck={() => void toggleAck()} />
      ) : (
        <Progress phase={phase} report={report} log={log} failed={error !== null} />
      )}
      <div className="mt-7 flex flex-wrap justify-end gap-2">
        {phase === "review" ? (
          <>
            <Button tone="ghost" data-autofocus onClick={close}>
              Cancel
            </Button>
            <Button tone="primary" disabled={blocked} onClick={() => void confirm()}>
              {OP_VERB[op]}
            </Button>
          </>
        ) : (
          <Button tone="primary" data-autofocus busy={phase === "running"} onClick={close}>
            {phase === "running" ? "Running…" : "Close"}
          </Button>
        )}
      </div>
    </Dialog>
  );
}

function Heading({ children }: { children: ReactNode }) {
  return <h3 className="eyebrow mb-2">{children}</h3>;
}

/** A scrollable detail block the D-pad can reach (and so scroll into view). */
function Block({ label, children, className }: { label: string; children: ReactNode; className?: string }) {
  return (
    <section aria-label={label} tabIndex={0} className={cx(TILE, "max-h-64 overflow-y-auto p-4", className)}>
      {children}
    </section>
  );
}

function originText(p: Preview["provenance"]): string {
  const o = p?.origin;
  if (!o) return "";
  if (o.kind === "nexus") return `Nexus Mods (${o.domain} mod ${o.mod_id}, file ${o.file_id})`;
  if (o.kind === "github") return `GitHub ${o.repo} ${o.tag} (${o.asset})`;
  return o.path ? `local file ${o.path}` : "a local file";
}

function runtimeText(p: Preview): string {
  if (p.runtime.kind === "proton") return `Proton (${p.runtime.proton})`;
  return p.runtime.kind === "native" ? "Native Linux" : "Unknown";
}

function Review({ preview: p, acked, busy, onAck }: { preview: Preview; acked: boolean; busy: boolean; onAck: () => void }) {
  const unverified = p.provenance?.kind === "unverified";
  const mins = Math.max(1, Math.round(p.timeout_secs / 60));
  return (
    <div className="grid gap-5">
      <div
        role="note"
        className="flex items-start gap-3 rounded-card border border-amber-400/15 bg-amber-500/5 px-4 py-3 text-body text-amber-100"
      >
        <ShieldWarningIcon aria-hidden size={20} weight="duotone" className="mt-0.5 shrink-0 text-amber-300" />
        <div>
          <p className="font-medium">Runs with your user permissions (no sandbox)</p>
          <p className="mt-1 text-zinc-300">{p.permissions}</p>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-body text-zinc-300">
        {p.provenance &&
          (unverified ? (
            <Badge tone="rose">unverified source</Badge>
          ) : (
            <Badge tone="accent">verified source</Badge>
          ))}
        {p.provenance && <span className="wrap-anywhere">From {originText(p.provenance)}</span>}
        <span>Runtime: {runtimeText(p)}</span>
        <span>Stopped after {mins} min</span>
      </div>

      {unverified && p.provenance?.kind === "unverified" && (
        <div className="grid gap-2">
          <p className="text-body text-rose-200">
            Vessel could not verify where this archive came from: {p.provenance.reason}.
          </p>
          <button
            type="button"
            role="checkbox"
            aria-checked={acked}
            aria-busy={busy || undefined}
            onClick={onAck}
            className={cx(
              "flex min-h-control-lg w-full items-start gap-3 rounded-card border px-4 py-3 text-left text-body transition-colors duration-base",
              acked ? "border-accent-400/40 bg-accent-400/10" : "border-hairline bg-fill-1",
            )}
          >
            <span
              aria-hidden
              className={cx(
                "mt-0.5 grid size-icon shrink-0 place-items-center rounded-inset border-2",
                acked
                  ? "border-accent-400 bg-accent-400 text-accent-950 in-highlight:border-accent-700 in-highlight:bg-accent-700 in-highlight:text-white"
                  : "border-zinc-500",
              )}
            >
              {acked && <CheckIcon size={12} weight="bold" />}
            </span>
            <span className="min-w-0 flex-1 text-zinc-100">I understand the source is unverified and want to run it anyway</span>
          </button>
        </div>
      )}

      {p.commands.map((c, i) => (
        <div key={i}>
          <Heading>{p.commands.length > 1 ? `Command ${i + 1}` : "Command"}</Heading>
          <Block label={`Command ${i + 1}`} className="grid gap-3 font-mono text-caption text-zinc-200">
            <div>
              <div className="text-dim">Program</div>
              <div className="break-all">{c.program}</div>
            </div>
            <div>
              <div className="text-dim">Arguments</div>
              <ol aria-label="Arguments" className="grid gap-0.5">
                {c.args.map((a, j) => (
                  <li key={j} className="break-all">
                    {a}
                  </li>
                ))}
              </ol>
            </div>
            {c.cmd_line && (
              <div>
                <div className="text-dim">cmd.exe line</div>
                <div className="break-all">{c.cmd_line}</div>
              </div>
            )}
            <div>
              <div className="text-dim">Working folder</div>
              <div className="break-all">{c.cwd}</div>
            </div>
            <div>
              <div className="text-dim">Environment (nothing else is passed)</div>
              <ul aria-label="Environment" className="grid gap-0.5">
                {c.env.map(([k, v]) => (
                  <li key={k} className="break-all">
                    {k}={v}
                  </li>
                ))}
              </ul>
            </div>
          </Block>
        </div>
      ))}

      {p.touch_scope.length > 0 && (
        <div>
          <Heading>May change in the game folder (backed up, undone on failure)</Heading>
          <Block label="Game folder changes">
            <ul aria-label="Touch scope" className="grid gap-0.5 font-mono text-caption text-zinc-300">
              {p.touch_scope.map((g) => (
                <li key={g} className="break-all">
                  {g}
                </li>
              ))}
            </ul>
          </Block>
        </div>
      )}

      {p.external_side_effects.length > 0 && (
        <div>
          <Heading>Outside the game folder</Heading>
          <ul aria-label="Side effects outside the game folder" className="grid gap-2">
            {p.external_side_effects.map((s) => (
              <li key={s} className="flex flex-wrap items-center gap-2 text-body text-zinc-200">
                <Badge tone="rose">unrecoverable</Badge>
                <span className="wrap-anywhere">{s}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {p.warnings.length > 0 && (
        <ul aria-label="Warnings" className="grid gap-1.5">
          {p.warnings.map((w) => (
            <li key={w} className="flex items-start gap-2 text-body text-amber-200">
              <WarningIcon aria-hidden size={16} className="mt-0.5 shrink-0" />
              <span className="wrap-anywhere">{w}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Progress({ phase, report, log, failed }: { phase: Phase; report: RunReport | null; log?: string; failed: boolean }) {
  const outcome = report?.outcome;
  return (
    <div className="grid gap-4">
      {phase === "running" && <p className="text-body text-zinc-300">Running the installer. Do not start the game until it finishes.</p>}
      {outcome?.kind === "succeeded" && (
        <p role="status" className="text-body text-accent-200">
          Done{report?.version ? `: version ${report.version}` : ""}.
        </p>
      )}
      {outcome?.kind === "failed" && (
        <p role="status" className="text-body text-rose-200">
          It failed and its changes were rolled back: {outcome.reason}
        </p>
      )}
      {report?.launch_desired && (
        <p className="text-body text-zinc-300">
          The game now needs these Steam launch options: <span className="font-mono text-zinc-100 select-all">{report.launch_desired}</span>.
          Set them from the loader card in Requirements.
        </p>
      )}
      {report?.warnings.map((w) => (
        <p key={w} className="flex items-start gap-2 text-body text-amber-200">
          <WarningIcon aria-hidden size={16} className="mt-0.5 shrink-0" />
          {w}
        </p>
      ))}
      {(phase === "running" || log || !failed) && (
        <div>
          <Heading>Installer output</Heading>
          <pre
            role="log"
            aria-label="Installer output"
            aria-live="polite"
            tabIndex={0}
            className={cx(TILE, "max-h-72 overflow-auto p-4 font-mono text-caption whitespace-pre-wrap text-zinc-300")}
          >
            {log || (phase === "running" ? "Waiting for output…" : "No output.")}
          </pre>
        </div>
      )}
    </div>
  );
}
