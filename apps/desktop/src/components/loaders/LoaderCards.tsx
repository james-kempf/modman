// SPDX-License-Identifier: GPL-3.0-or-later
// Requirements: one card per mod loader of the game (SMAPI, ...). Every op
// goes through the consent dialog: install, upgrade and adopt download the
// loader from its Nexus page (the backend holds the archive as a pending
// preview), repair and uninstall preview with the cached archive. Also the
// runtime picker, Steam launch options (copy, "I've set it", apply with an
// account picker), the offered rollback after an interrupted run, run logs
// and the cleanup of loader archives that were installed as mods.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import {
  ArrowCounterClockwiseIcon,
  CopyIcon,
  DownloadSimpleIcon,
  FileTextIcon,
  TrashIcon,
  WrenchIcon,
} from "@phosphor-icons/react";
import { errMsg } from "../../app/bridge";
import { isCancelError } from "../../app/jobs";
import { refreshAll } from "../../app/queries";
import { openPendingLoaders } from "../../app/run";
import { useUi } from "../../app/store";
import * as ipc from "../../ipc/commands";
import type { LoaderCard, LoaderOp, SteamAccount } from "../../ipc/types";
import { Dialog } from "../dialogs/Dialog";
import { Badge, Button, Card, CardSection, Select, TILE, cx } from "../ui";
import { LoaderErrorNote } from "./LoaderError";

export function LoaderCards({ game }: { game: string }) {
  const q = useQuery({ queryKey: ["loaders", game], queryFn: () => ipc.loaderStatus(game) });
  const junk = useQuery({ queryKey: ["loader-junk", game], queryFn: () => ipc.loaderJunkScan(game) });
  if (q.error) return <LoaderErrorNote error={q.error} />;
  return (
    <>
      {q.data?.map((l) => <LoaderCardView key={l.key} l={l} game={game} />)}
      {!!junk.data?.length && <JunkPrompt game={game} junk={junk.data} />}
    </>
  );
}

/** Runs a card action: one at a time, the error kept for the typed note, everything refreshed after. */
function useAct() {
  const qc = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const last = useRef<(() => void) | null>(null);
  const act = async <T,>(key: string, fn: () => Promise<T>): Promise<T | undefined> => {
    if (busy) return undefined;
    last.current = () => void act(key, fn);
    setBusy(key);
    setError(null);
    try {
      return await fn();
    } catch (e) {
      if (isCancelError(errMsg(e))) useUi.getState().notify("info", `${key} cancelled.`);
      else setError(e);
      return undefined;
    } finally {
      setBusy(null);
      void refreshAll(qc);
    }
  };
  return { act, error, busy, retry: () => last.current?.() };
}

const STATE_BADGE: Record<LoaderCard["status"]["state"], { text: string; tone: "accent" | "amber" | "rose" | "zinc" }> = {
  not_installed: { text: "not installed", tone: "zinc" },
  installed: { text: "installed", tone: "accent" },
  busy: { text: "busy", tone: "amber" },
  manual_detected: { text: "installed outside Vessel", tone: "amber" },
  drifted: { text: "needs repair", tone: "amber" },
};

function LoaderCardView({ l, game }: { l: LoaderCard; game: string }) {
  const notify = useUi((s) => s.notify);
  const openConsent = useUi((s) => s.openLoaderConsent);
  const { act, error, busy, retry } = useAct();
  const [pickRuntime, setPickRuntime] = useState(false);
  const [accounts, setAccounts] = useState<{ list: SteamAccount[]; chosen: string } | null>(null);
  const [logText, setLogText] = useState<string | null>(null);
  const s = l.status;
  const version = "version" in s ? s.version : null;
  const interrupted = s.state === "busy" && s.status === "interrupted";

  /** Install, upgrade and adopt: Vessel downloads the loader; the backend returns a pending preview. */
  const download = (label: string) =>
    act(label, async () => {
      const r = await ipc.install({ target: String(l.nexus_mod_id), game });
      openPendingLoaders(r, game);
      if (!r.loader) notify("info", `${label}: nothing to review.`);
    });
  const previewOp = (op: LoaderOp, label: string) =>
    act(label, async () => openConsent({ game, preview: await ipc.loaderPreview({ op, loaderKey: l.key }, game) }));
  const openLog = () =>
    l.last_run !== null && act("Open log", async () => setLogText(await ipc.loaderLogGet(l.last_run!)));
  const apply = (account: number | null) =>
    act("Apply to Steam", async () => {
      const r = await ipc.launchOptionsApply(account, game);
      if (r.kind === "choose_account") {
        const d = r.default ?? r.accounts[0]?.account_id;
        setAccounts({ list: r.accounts, chosen: String(d) });
      } else {
        setAccounts(null);
        notify("success", `Launch options set in Steam: ${r.value || "(cleared)"}`);
      }
    });

  const runtimeOptions = [
    ...(l.runtime === "unknown" && !l.runtime_override ? [{ value: "unknown", label: "Not detected" }] : []),
    { value: "native", label: "Native Linux" },
    { value: "proton", label: "Proton (Windows version)" },
    ...(l.runtime_override ? [{ value: "auto", label: "Detect automatically" }] : []),
  ];
  const showRuntime = l.runtime === "unknown" || pickRuntime || !!l.runtime_override;
  const launch = l.launch;
  const launchSet = !!launch.desired && (launch.acked === launch.desired || launch.applied === launch.desired);
  const canDownload = l.nexus_mod_id !== null;

  return (
    <Card
      aria-label={l.name}
      title={
        <span className="flex flex-wrap items-center gap-2">
          {l.name}
          <Badge tone={interrupted ? "rose" : STATE_BADGE[s.state].tone}>
            {interrupted ? "interrupted" : STATE_BADGE[s.state].text}
          </Badge>
          {version && <Badge>{version}</Badge>}
          <Badge>{l.runtime}</Badge>
        </span>
      }
      intro="Mod loader. Vessel installs it into the game folder with the vendor's installer, tracks every file and can undo it."
    >
      <CardSection className="grid gap-4">
        {s.state === "not_installed" && <p className="text-body text-zinc-300">Not installed.</p>}
        {s.state === "installed" && <p className="text-body text-zinc-300">Installed and tracked by Vessel.</p>}
        {s.state === "drifted" && (
          <p role="status" className="text-body text-amber-200">
            The game folder no longer matches the install: {s.reason}. A Steam update or file check usually causes this;
            repair puts the loader back.
          </p>
        )}
        {s.state === "manual_detected" && (
          <p className="text-body text-amber-200">
            Installed outside Vessel ({s.evidence.join(", ")}). Adopt it so Vessel can track, repair and remove it.
          </p>
        )}
        {s.state === "busy" &&
          (interrupted ? (
            <p role="status" className="text-body text-rose-200">
              An installer run was interrupted
              {l.recovered?.action.kind === "rollback_offered" && l.recovered.action.killed
                ? " (Vessel stopped the leftover installer)"
                : ""}
              . Its changes are recorded: roll them back, or keep the files as they are.
            </p>
          ) : (
            <p className="text-body text-zinc-300">An operation is running ({s.status}).</p>
          ))}
        {!canDownload && (s.state === "not_installed" || s.state === "manual_detected") && (
          <p className="text-body text-zinc-400">Vessel has no download page for this loader; install it from a local archive with the CLI.</p>
        )}
        <div className="flex flex-wrap gap-2">
          {s.state === "not_installed" && canDownload && (
            <Button size="sm" tone="primary" busy={busy === "Install"} icon={<DownloadSimpleIcon aria-hidden size={14} />} onClick={() => void download("Install")}>
              Install
            </Button>
          )}
          {s.state === "manual_detected" && canDownload && (
            <Button size="sm" tone="primary" busy={busy === "Adopt"} onClick={() => void download("Adopt")}>
              Adopt
            </Button>
          )}
          {s.state === "installed" && canDownload && (
            <Button size="sm" busy={busy === "Upgrade"} icon={<DownloadSimpleIcon aria-hidden size={14} />} onClick={() => void download("Upgrade")}>
              Upgrade
            </Button>
          )}
          {(s.state === "installed" || s.state === "drifted") && (
            <>
              <Button
                size="sm"
                tone={s.state === "drifted" ? "primary" : "default"}
                busy={busy === "Repair"}
                icon={<WrenchIcon aria-hidden size={14} />}
                onClick={() => void previewOp("repair", "Repair")}
              >
                Repair
              </Button>
              <Button size="sm" tone="danger" busy={busy === "Uninstall"} icon={<TrashIcon aria-hidden size={14} />} onClick={() => void previewOp("uninstall", "Uninstall")}>
                Uninstall
              </Button>
            </>
          )}
          {interrupted && (
            <>
              <Button
                size="sm"
                tone="primary"
                busy={busy === "Roll back"}
                icon={<ArrowCounterClockwiseIcon aria-hidden size={14} />}
                onClick={() =>
                  void act("Roll back", async () => {
                    const lost = await ipc.loaderRollback(s.install_id);
                    notify(lost.length ? "warn" : "success", lost.length ? `Rolled back; could not restore: ${lost.join(", ")}` : "Rolled back.");
                  })
                }
              >
                Roll back
              </Button>
              <Button size="sm" busy={busy === "Keep files"} onClick={() => void act("Keep files", () => ipc.loaderDeclineRollback(s.install_id))}>
                Keep files
              </Button>
            </>
          )}
          {l.last_run !== null && (
            <Button size="sm" tone="ghost" busy={busy === "Open log"} icon={<FileTextIcon aria-hidden size={14} />} onClick={() => void openLog()}>
              View log
            </Button>
          )}
        </div>
        {error !== null && (
          <LoaderErrorNote
            error={error}
            handlers={{
              retry,
              pick_runtime: () => setPickRuntime(true),
              open_log: l.last_run !== null ? () => void openLog() : undefined,
              adopt: canDownload ? () => void download("Adopt") : undefined,
            }}
          />
        )}
      </CardSection>

      {showRuntime && (
        <CardSection className="grid gap-3">
          {l.runtime === "unknown" && (
            <p className="text-body text-zinc-300">Vessel cannot tell whether the game runs natively or through Proton. Choose which one it uses.</p>
          )}
          <Select
            label="Game runtime"
            value={l.runtime_override ?? l.runtime}
            options={runtimeOptions}
            busy={busy === "Set runtime"}
            onChange={(v) =>
              v !== "unknown" &&
              void act("Set runtime", () => ipc.gameRuntimeSet(v === "auto" ? null : (v as "native" | "proton"), game))
            }
            className="max-w-sm"
          />
        </CardSection>
      )}

      {(launch.desired || launch.applied) && (
        <CardSection className="grid gap-3">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-body font-medium text-zinc-100">Steam launch options</h3>
            <Badge tone={launchSet ? "accent" : "amber"}>{launchSet ? "set" : "not set yet"}</Badge>
          </div>
          {launch.desired ? (
            <>
              <p className="text-body text-zinc-400">
                Set these in Steam (game Properties, Launch options), or let Vessel write them while Steam is closed.
              </p>
              <p className={cx(TILE, "px-4 py-3 font-mono text-caption break-all text-zinc-100 select-all")}>{launch.desired}</p>
            </>
          ) : (
            <p className="text-body text-zinc-400">No launch options are needed any more; Apply clears the ones Vessel set.</p>
          )}
          <div className="flex flex-wrap gap-2">
            {launch.desired && (
              <>
                <Button
                  size="sm"
                  icon={<CopyIcon aria-hidden size={14} />}
                  onClick={() =>
                    void act("Copy", async () => {
                      await navigator.clipboard.writeText(launch.desired!);
                      notify("success", "Launch options copied.");
                    })
                  }
                >
                  Copy
                </Button>
                <Button size="sm" busy={busy === "Ack"} disabled={launch.acked === launch.desired} onClick={() => void act("Ack", () => ipc.launchOptionsAck(game))}>
                  I've set it
                </Button>
              </>
            )}
            <Button size="sm" tone="primary" busy={busy === "Apply to Steam"} disabled={l.steam_running} onClick={() => void apply(null)}>
              Apply to Steam
            </Button>
          </div>
          {l.steam_running && <p className="text-body text-amber-200">Close Steam to apply: it rewrites its settings when it exits.</p>}
          {accounts && (
            <div className="grid gap-3">
              <Select
                label="Steam account"
                value={accounts.chosen}
                options={accounts.list.map((a) => ({
                  value: String(a.account_id),
                  label: a.persona_name ? `${a.persona_name} (${a.account_name})` : a.account_name || String(a.account_id),
                }))}
                onChange={(v) => setAccounts({ ...accounts, chosen: v })}
                className="max-w-sm"
              />
              <div className="flex flex-wrap gap-2">
                <Button size="sm" tone="ghost" onClick={() => setAccounts(null)}>
                  Cancel
                </Button>
                <Button size="sm" tone="primary" disabled={l.steam_running} busy={busy === "Apply to Steam"} onClick={() => void apply(Number(accounts.chosen))}>
                  Apply for this account
                </Button>
              </div>
            </div>
          )}
        </CardSection>
      )}

      {logText !== null && (
        <Dialog wide title={`${l.name} installer log`} onBack={() => setLogText(null)}>
          <pre
            tabIndex={0}
            aria-label="Installer log"
            className={cx(TILE, "max-h-[60vh] overflow-auto p-4 font-mono text-caption whitespace-pre-wrap text-zinc-300")}
          >
            {logText || "No output."}
          </pre>
          <div className="mt-7 flex justify-end">
            <Button tone="primary" data-autofocus onClick={() => setLogText(null)}>
              Close
            </Button>
          </div>
        </Dialog>
      )}
    </Card>
  );
}

function JunkPrompt({ game, junk }: { game: string; junk: { mod_id: number; name: string }[] }) {
  const notify = useUi((s) => s.notify);
  const { act, error, busy } = useAct();
  return (
    <Card
      aria-label="Loader leftovers"
      title="Loader archives installed as mods"
      intro="These mods are really loader installers that ended up under the mods folder. They do nothing there."
    >
      <CardSection className="grid gap-3">
        <ul className="grid gap-1 text-body text-zinc-200">
          {junk.map((j) => (
            <li key={j.mod_id} className="wrap-anywhere">
              {j.name}
            </li>
          ))}
        </ul>
        <div className="flex flex-wrap gap-2">
          <Button
            size="sm"
            tone="danger"
            busy={busy === "Remove"}
            icon={<TrashIcon aria-hidden size={14} />}
            onClick={() =>
              void act("Remove", async () => {
                const n = await ipc.loaderJunkRemove(
                  junk.map((j) => j.mod_id),
                  game,
                );
                notify("success", `Removed ${n} leftover loader archive${n === 1 ? "" : "s"}.`);
              })
            }
          >
            Remove them
          </Button>
        </div>
        {error !== null && <LoaderErrorNote error={error} />}
      </CardSection>
    </Card>
  );
}
