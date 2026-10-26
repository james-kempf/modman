// SPDX-License-Identifier: GPL-3.0-or-later
// Demo backend for poisoned-backup cleanup, quarantine restore and restage
// (backups_poisoned, backups_clean, quarantines_list, quarantine_restore,
// restage). Follows the core's contract: cleanup and restore refuse unless
// `confirmed` is true, restage refuses while confirmed poisoned backups
// exist (unless `ignore_poisoned`) and never deploys.

import type { ModView, PoisonedBackup, QuarantineManifest, QuarantineReport, RestageReport, RestoreReport } from "../ipc/types";
import type { Args, Handler } from "./fake";

export interface RepairHost {
  job: <T>(kind: string, body: (id: number) => Promise<T>) => Promise<T>;
  say: (job: number, text: string) => void;
  /** The game a command names, else the active one. */
  domain: (a: Args) => string;
  mods: (a: Args) => ModView[];
  /** The game's staged files changed: its deploy is due. */
  changed: (a: Args) => void;
}

/** Poisoned backups the demo starts with: two confirmed, one ambiguous. */
export function demoPoisoned(): PoisonedBackup[] {
  const entry = (backup_id: number, path: string, cls: PoisonedBackup["class"], extra: Partial<PoisonedBackup> = {}): PoisonedBackup => ({
    backup_id,
    target: "game_dir",
    path,
    sha256: backup_id.toString(16).padStart(64, "0"),
    class: cls,
    reason:
      cls === "confirmed"
        ? "modman placed this path before the backup was taken"
        : "no deploy record before the backup; the bytes match a mod file",
    mods: ["mod-1"],
    deployed: false,
    deployed_by_enabled_mod: false,
    ...extra,
  });
  return [
    entry(101, "Data/SKSE/Plugins/SkyUI.dll", "confirmed", { deployed: true, deployed_by_enabled_mod: true }),
    entry(102, "Data/Interface/skyui_cfg.txt", "confirmed"),
    entry(103, "Data/meshes/actors/character/skeleton.nif", "ambiguous", { mods: ["mod-2"] }),
  ];
}

const notConfirmed = (what: string) => ({ message: `${what} needs confirmation: nothing was changed.` });

export function repairHandlers(h: RepairHost, seed: Record<string, PoisonedBackup[]>): Record<string, Handler> {
  const poisoned = new Map(Object.entries(seed).map(([d, l]) => [d, l.map((e) => ({ ...e }))]));
  const quarantines = new Map<string, QuarantineManifest[]>();
  /** Entries of each quarantine, as they were before it (restore puts them back). */
  const held = new Map<string, PoisonedBackup[]>();
  /** Mods restaged in the new layout, per game: the default restage skips them. */
  const restaged = new Map<string, Set<number>>();
  let seq = 0;
  const list = (a: Args) => {
    const d = h.domain(a);
    if (!poisoned.has(d)) poisoned.set(d, []);
    return poisoned.get(d)!;
  };
  const qs = (a: Args) => {
    const d = h.domain(a);
    if (!quarantines.has(d)) quarantines.set(d, []);
    return quarantines.get(d)!;
  };
  const now = () => Math.floor(Date.now() / 1000);

  return {
    backups_poisoned: (a: Args) => list(a).map((e) => ({ ...e, mods: [...e.mods] })),
    backups_clean: (a: Args) =>
      h.job("backups_clean", async (id): Promise<QuarantineReport> => {
        if (a?.confirmed !== true) throw notConfirmed("Quarantine");
        const all = list(a);
        const take = all.filter((e) => e.class === "confirmed" || a.include_ambiguous === true);
        const skipped = all.length - take.length;
        if (!take.length) {
          h.say(id, "Nothing to quarantine.");
          return { id: null, dir: null, backups: 0, game_files: 0, skipped_ambiguous: skipped };
        }
        const qid = `${now()}-${++seq}`;
        all.splice(0, all.length, ...all.filter((e) => !take.includes(e)));
        held.set(qid, take);
        qs(a).push({
          id: qid,
          game: h.domain(a),
          created_at: now(),
          restored_at: null,
          items: take.map((e) => ({
            path: e.path,
            target: e.target,
            sha256: e.sha256,
            origin: "backup",
            class: e.class,
            reason: e.reason,
            backup: { id: e.backup_id, size: 1024, mode: 0o644, mtime_ns: null, created_at: now() },
            deployment_ids: [],
          })),
        });
        h.say(id, `Quarantined ${take.length} backup(s) as ${qid}.`);
        return { id: qid, dir: `~/.local/share/modman/${h.domain(a)}/quarantine/${qid}`, backups: take.length, game_files: 0, skipped_ambiguous: skipped };
      }),
    quarantines_list: (a: Args) => qs(a).map((q) => ({ ...q, items: q.items.map((i) => ({ ...i })) })),
    quarantine_restore: (a: Args) =>
      h.job("quarantine_restore", async (id): Promise<RestoreReport> => {
        if (a?.confirmed !== true) throw notConfirmed("Restore");
        const qid = String(a.id);
        const q = qs(a).find((x) => x.id === qid);
        if (!q) throw { message: `no quarantine '${qid}'` };
        if (q.restored_at !== null) throw { message: `quarantine '${qid}' was already restored` };
        const back = held.get(qid) ?? [];
        list(a).push(...back);
        q.restored_at = now();
        h.say(id, `Restored quarantine ${qid}: ${back.length} backup(s).`);
        return { id: qid, backups: back.length, game_files: 0 };
      }),
    restage: (a: Args) =>
      h.job("restage", async (id): Promise<RestageReport> => {
        const all = list(a);
        const confirmed = all.filter((e) => e.class === "confirmed").length;
        if (confirmed > 0 && a?.ignore_poisoned !== true)
          throw {
            message:
              `Restage blocked: ${confirmed} held backup(s) are mod files modman placed, not game originals, ` +
              "so a purge would put mod files back. Review and quarantine them first: " +
              "modman deploy clean-backups (then --apply), or pass --ignore-poisoned.",
          };
        const d = h.domain(a);
        const done = restaged.get(d) ?? new Set<number>();
        restaged.set(d, done);
        const ids = Array.isArray(a?.ids) ? (a.ids as number[]) : [];
        const pick = h.mods(a).filter((m) => (ids.length ? ids.includes(m.id) : a?.all === true || !done.has(m.id)));
        pick.forEach((m) => done.add(m.id));
        if (pick.length) h.changed(a);
        h.say(id, pick.length ? `Restaged ${pick.length} mod(s).` : "Nothing to restage.");
        return {
          restaged: pick.map((m) => ({ id: m.id, uid: m.uid, files: 4, carried: [], backed_up: [] })),
          no_archive: [],
          failed: [],
          backup_dir: pick.length ? `~/.local/share/modman/${d}/restage-backup` : null,
          poisoned_confirmed: confirmed,
          poisoned_ambiguous: all.length - confirmed,
        };
      }),
  };
}
