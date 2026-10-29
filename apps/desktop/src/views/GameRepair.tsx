// SPDX-License-Identifier: GPL-3.0-or-later
// Settings card for the active game: poisoned held backups (backups that
// hold mod files instead of game originals) and their quarantine, restoring
// a quarantine, and restaging mods into the current layout. The backup and
// quarantine lists start collapsed behind a count. Cleanup and
// restore change files, so each asks in a dialog first and only then calls
// the backend with confirmed=true. Restage refuses while confirmed poisoned
// backups exist; its message is shown here.

import { ArrowCounterClockwiseIcon, ArrowsClockwiseIcon, ShieldWarningIcon } from "@phosphor-icons/react";
import { useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { errMsg } from "../app/bridge";
import { useActiveGame } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { useUi } from "../app/store";
import { Dialog } from "../components/dialogs/Dialog";
import { Badge, Button, Card, CardSection, ErrorNote, ListToggle, Row, Toggle, when } from "../components/ui";
import * as ipc from "../ipc/commands";
import type { PoisonedBackup, QuarantineManifest } from "../ipc/types";

const CLEAN = "Quarantine backups";
const RESTORE = "Restore quarantine";
const RESTAGE = "Restage mods";

/** Nothing without an active game. Keyed by game: switching games resets the opt-in and messages. */
export function GameRepairCard() {
  const { game } = useActiveGame();
  if (!game) return null;
  return <GameRepair key={game.domain} domain={game.domain} name={game.name} />;
}

function GameRepair({ domain, name }: { domain: string; name: string }) {
  const poisoned = useQuery({ queryKey: ["poisoned", domain], queryFn: () => ipc.backupsPoisoned(domain) });
  const quarantines = useQuery({ queryKey: ["quarantines", domain], queryFn: () => ipc.quarantinesList(domain) });
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const markDeployDue = useUi((s) => s.markDeployDue);
  const cleaning = useBusy(CLEAN);
  const restaging = useBusy(RESTAGE);
  const [includeAmbiguous, setIncludeAmbiguous] = useState(false);
  const [asking, setAsking] = useState<{ kind: "clean" } | { kind: "restore"; q: QuarantineManifest } | null>(null);
  const [blocked, setBlocked] = useState<string | null>(null);
  // Long lists start collapsed behind a count.
  const [showPoisoned, setShowPoisoned] = useState(false);
  const [showQuarantines, setShowQuarantines] = useState(false);

  const list = poisoned.data ?? [];
  const confirmed = list.filter((e) => e.class === "confirmed");
  const ambiguous = list.filter((e) => e.class === "ambiguous");
  const selected = includeAmbiguous ? list : confirmed;

  const clean = async () => {
    setAsking(null);
    const r = await run(CLEAN, () => ipc.backupsClean({ includeAmbiguous, confirmed: true }, domain), { key: CLEAN });
    if (!r) return;
    if (r.id) {
      setBlocked(null);
      notify("success", `Quarantined ${r.backups} backup(s) and ${r.game_files} game file(s) as ${r.id}.`);
    } else notify("info", "Nothing to quarantine.");
  };

  const restore = async (q: QuarantineManifest) => {
    setAsking(null);
    const r = await run(RESTORE, () => ipc.quarantineRestore(q.id, true, domain), { key: RESTORE });
    if (r) notify("success", `Restored quarantine ${r.id}: ${r.backups} backup(s), ${r.game_files} game file(s).`);
  };

  const restage = async () => {
    setBlocked(null);
    const r = await run(
      RESTAGE,
      async () => {
        try {
          return await ipc.restage({}, domain);
        } catch (e) {
          setBlocked(errMsg(e));
          throw e;
        }
      },
      { key: RESTAGE },
    );
    if (!r) return;
    // Restage only rewrites the staged copies; the game folder changes on the next deploy.
    if (r.restaged.length) {
      markDeployDue(domain);
      notify("success", `Restaged ${r.restaged.length} mod(s) of ${name}. Deploy to apply.`);
    } else notify("info", "Nothing to restage: no mod is staged in an outdated layout.");
    if (r.no_archive.length) notify("warn", `No archive kept for ${r.no_archive.join(", ")}; reinstall these mods.`);
    if (r.failed.length) notify("error", `Restage failed for ${r.failed.map((f) => `${f.uid} (${f.error})`).join(", ")}`);
  };

  const history = [...(quarantines.data ?? [])].reverse();

  return (
    <Card
      navSections
      title={`Backups and staging: ${name}`}
      intro="Backups keep the game's originals. A poisoned one holds a mod file and would put it back on purge; quarantine moves it aside (nothing is deleted, it can be restored)."
    >
      <div className="rows">
        <Row
          title="Poisoned backups"
          desc={
            poisoned.isPending
              ? "Checking held backups…"
              : list.length
                ? `${confirmed.length} confirmed (placed by Vessel), ${ambiguous.length} ambiguous (no earlier deploy record).`
                : "None: every held backup differs from the mod files at its path."
          }
        >
          {list.length > 0 && (
            <ListToggle
              name="Poisoned backups"
              count={list.length}
              open={showPoisoned}
              onToggle={() => setShowPoisoned((o) => !o)}
              controls={`poisoned-${domain}`}
            />
          )}
          <Toggle
            label="Include ambiguous"
            checked={includeAmbiguous}
            disabled={!ambiguous.length}
            onChange={setIncludeAmbiguous}
          />
          <Button
            size="sm"
            tone="danger"
            icon={<ShieldWarningIcon aria-hidden size={15} />}
            busy={cleaning}
            inactive={!selected.length}
            onClick={() => selected.length && setAsking({ kind: "clean" })}
          >
            Quarantine…
          </Button>
        </Row>
        {poisoned.isError && (
          <CardSection>
            <ErrorNote>Could not check the backups: {errMsg(poisoned.error)}</ErrorNote>
          </CardSection>
        )}
        {showPoisoned && list.length > 0 && (
          <CardSection id={`poisoned-${domain}`} className="grid gap-3">
            <EntryGroup label="Confirmed" entries={confirmed} />
            <EntryGroup label="Ambiguous" entries={ambiguous} note={includeAmbiguous ? null : "Left alone unless you include them."} />
          </CardSection>
        )}
      </div>

      <div className="rows">
        <Row
          title="Quarantines"
          desc={
            quarantines.isError
              ? undefined
              : history.length
                ? `${history.length} quarantine(s), ${history.filter((q) => q.restored_at === null).length} not restored.`
                : `No quarantines for ${name}.`
          }
        >
          {history.length > 0 && (
            <ListToggle
              name="Quarantines"
              count={history.length}
              open={showQuarantines}
              onToggle={() => setShowQuarantines((o) => !o)}
              controls={`quarantines-${domain}`}
            />
          )}
        </Row>
        {quarantines.isError && (
          <CardSection>
            <ErrorNote>Could not read the quarantines: {errMsg(quarantines.error)}</ErrorNote>
          </CardSection>
        )}
        {showQuarantines && history.length > 0 && (
          <CardSection id={`quarantines-${domain}`}>
            <ul aria-label="Quarantines" className="grid gap-1.5">
              {history.map((q) => (
                <li
                  key={q.id}
                  className="flex flex-wrap items-center gap-3 rounded-xl border border-white/[0.05] bg-ink px-3 py-1.5 text-[12.5px]"
                >
                  <span className="min-w-0 flex-1">
                    <span className="block font-mono text-[12px] break-all text-zinc-200">{q.id}</span>
                    <span className="block text-dim">
                      {when(q.created_at)} · {q.items.length} item(s)
                    </span>
                  </span>
                  {q.restored_at !== null ? (
                    <Badge>restored {when(q.restored_at)}</Badge>
                  ) : (
                    <Button
                      size="sm"
                      aria-label={`Restore quarantine ${q.id}`}
                      icon={<ArrowCounterClockwiseIcon aria-hidden size={15} />}
                      onClick={() => setAsking({ kind: "restore", q })}
                    >
                      Restore…
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          </CardSection>
        )}

        <Row
          title="Restage mods"
          desc={
            confirmed.length
              ? `Blocked while ${confirmed.length} confirmed poisoned backup(s) exist: quarantine them first.`
              : "Rebuild mods staged in an outdated layout from their archives (your edits are kept), then deploy."
          }
        >
          <Button size="sm" icon={<ArrowsClockwiseIcon aria-hidden size={15} />} busy={restaging} onClick={() => void restage()}>
            Restage
          </Button>
        </Row>
        {blocked && (
          <CardSection>
            <ErrorNote>{blocked}</ErrorNote>
          </CardSection>
        )}
      </div>

      {asking?.kind === "clean" && (
        <ConfirmDialog
          title="Quarantine poisoned backups?"
          confirmLabel="Quarantine"
          onCancel={() => setAsking(null)}
          onConfirm={() => void clean()}
        >
          Move {selected.length} backup(s) of {name} to a quarantine folder
          {includeAmbiguous && ambiguous.length ? `, including ${ambiguous.length} ambiguous one(s)` : ""}. Nothing is deleted,
          and you can restore the quarantine later.
        </ConfirmDialog>
      )}
      {asking?.kind === "restore" && (
        <ConfirmDialog
          title="Restore this quarantine?"
          confirmLabel="Restore"
          onCancel={() => setAsking(null)}
          onConfirm={() => void restore(asking.q)}
        >
          Put the {asking.q.items.length} item(s) of quarantine {asking.q.id} back as held backups of {name}.
        </ConfirmDialog>
      )}
    </Card>
  );
}

function EntryGroup({ label, entries, note }: { label: string; entries: PoisonedBackup[]; note?: string | null }) {
  if (!entries.length) return null;
  return (
    <div className="grid gap-1.5">
      <h3 className="eyebrow">
        {label} ({entries.length}){note && <span className="ml-2 normal-case text-dim">{note}</span>}
      </h3>
      <ul aria-label={`${label} poisoned backups`} className="grid gap-1.5">
        {entries.map((e) => (
          // Focusable so the list can be read with the keyboard or gamepad.
          <li key={e.backup_id} tabIndex={0} className="grid gap-0.5 rounded-xl border border-white/[0.05] bg-ink px-3 py-2 text-[12.5px]">
            <span className="font-mono text-[12px] break-all text-zinc-200">{e.path}</span>
            <span className="text-dim">
              {e.reason}
              {e.deployed_by_enabled_mod && " · deployed by an enabled mod: the game file stays, only the backup moves"}
            </span>
            {e.mods.length > 0 && <span className="text-dim">mods: {e.mods.join(", ")}</span>}
          </li>
        ))}
      </ul>
    </div>
  );
}

function ConfirmDialog({
  title,
  confirmLabel,
  onCancel,
  onConfirm,
  children,
}: {
  title: string;
  confirmLabel: string;
  onCancel: () => void;
  onConfirm: () => void;
  children: ReactNode;
}) {
  return (
    <Dialog title={title} onBack={onCancel}>
      <p className="text-[16px] leading-relaxed text-zinc-200">{children}</p>
      <div className="mt-7 flex flex-wrap justify-end gap-2">
        <Button data-autofocus onClick={onCancel}>
          Cancel
        </Button>
        <Button tone="primary" onClick={onConfirm}>
          {confirmLabel}
        </Button>
      </div>
    </Dialog>
  );
}
