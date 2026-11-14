// SPDX-License-Identifier: GPL-3.0-or-later
// Undo of an uninstall, for one mod or many: the uninstall notice offers Undo
// only for the backend's restorable entries, and Undo restores them with one
// modRestore(tokens) call. One failed mod gets its own error notice, several
// share one that lists each with its reason; the list refetches either way (useRun refreshes every query when it ends).

import { errMsg } from "../../app/bridge";
import { useRun } from "../../app/run";
import { useUi } from "../../app/store";
import * as ipc from "../../ipc/commands";
import type {
  RestoreErrorKind,
  RestoreReason,
  RestoredMod,
  UninstallRestore,
  UninstallResult,
} from "../../ipc/types";

/** Why an uninstalled mod has no Undo, in plain words. */
export const NO_UNDO_REASON: Record<RestoreReason, string> = {
  folder_install: "it was installed from a folder, so there is no archive to reinstall it from",
  archive_deleted: "its archive was deleted",
  outside_downloads: "its archive is outside the downloads folder",
};

/** Why a restore failed, in plain words. */
export const RESTORE_ERROR: Record<RestoreErrorKind, string> = {
  archive_missing: "its archive is no longer in the downloads folder",
  conflict: "a mod with the same ID is installed again",
  expired: "Undo is no longer available",
  install_failed: "reinstalling it failed",
};

/** The entries Undo may restore: the backend's restorable flag alone decides. */
export const undoable = (r: UninstallResult): UninstallRestore[] =>
  (r.restore ?? []).filter((e) => e.restorable === true);

/** One sentence per entry without Undo, naming the mod and the reason. */
export function noUndoText(r: UninstallResult): string[] {
  return (r.restore ?? [])
    .filter((e) => e.restorable !== true)
    .map((e) =>
      e.reason ? `No Undo for ${e.name}: ${NO_UNDO_REASON[e.reason]}.` : `No Undo for ${e.name}.`,
    );
}

export function restoreFailedText(name: string, error: RestoredMod["error"]): string {
  const kind = error?.kind ?? "install_failed";
  const detail = kind === "install_failed" && error?.message ? ` (${error.message})` : "";
  return `Couldn't restore ${name}: ${RESTORE_ERROR[kind]}${detail}.`;
}

/** A failure's reason in a few words, for the combined notice. */
export const RESTORE_ERROR_SHORT: Record<RestoreErrorKind, string> = {
  archive_missing: "archive missing",
  conflict: "conflict",
  expired: "expired",
  install_failed: "install failed",
};

/**
 * Several failed mods in ONE notice ("Couldn't restore 3 mods: A (archive missing),
 * B (conflict), C (expired)."): the stack keeps only MAX_NOTICES, so one notice per
 * mod would push out the rest of the outcome.
 */
export function restoreFailuresText(failed: { name: string; error: RestoredMod["error"] }[]): string {
  const parts = failed.map(({ name, error }) => `${name} (${RESTORE_ERROR_SHORT[error?.kind ?? "install_failed"]})`);
  return `Couldn't restore ${failed.length} mods: ${parts.join(", ")}.`;
}

export const restoredText = (names: string[]): string =>
  names.length === 1 ? `Restored ${names[0]}` : `Restored ${names.length} mods`;

/** Where focus goes during and after Undo (the restore helper itself knows no rows). */
export interface UndoFocus {
  /** Undo was pressed: its notice is going away, so hold focus somewhere stable. */
  onUndo?: () => void;
  /** These mods are back (uids, lowest position first). */
  onRestored?: (uids: string[]) => void;
}

/**
 * Returns `notice(result, text)`: the uninstall's notice, with Undo when any
 * entry of `result.restore` is restorable, and the reason for each one that
 * is not. A declined uninstall gets an info notice with `text` only.
 */
export function useUninstallNotice(game: string, focus: UndoFocus = {}) {
  const run = useRun();
  const undo = async (entries: UninstallRestore[]) => {
    const ui = useUi.getState();
    focus.onUndo?.();
    const nameOf = (token: string, name: string | null) =>
      name ?? entries.find((e) => e.token === token)?.name ?? "a mod";
    const r = await run(
      "Undo uninstall",
      async () => {
        try {
          // Tokens only: the backend kept everything else it needs.
          return await ipc.modRestore(
            entries.map((e) => e.token),
            game,
          );
        } catch (e) {
          // Rejected as a whole: report it here so the notice names the mods.
          return { rejected: errMsg(e) };
        }
      },
      { key: "Undo uninstall" },
    );
    if (!r) return;
    if ("rejected" in r) {
      ui.notify("error", `Couldn't restore ${entries.map((e) => e.name).join(", ")}: ${r.rejected}`);
      return;
    }
    const failed = r.results
      .filter((x) => !x.ok)
      .map((f) => ({ name: nameOf(f.token, f.name), error: f.error }));
    if (failed.length === 1) ui.notify("error", restoreFailedText(failed[0].name, failed[0].error));
    else if (failed.length > 1) ui.notify("error", restoreFailuresText(failed));
    const ok = r.results.filter((x) => x.ok);
    if (ok.length) ui.notify("success", restoredText(ok.map((x) => nameOf(x.token, x.name))));
    for (const w of r.results.flatMap((x) => x.warnings)) ui.notify("info", w);
    const uids = ok.flatMap((x) => (x.uid ? [x.uid] : []));
    if (uids.length) focus.onRestored?.(uids);
  };
  return (r: UninstallResult, text: string) => {
    const ui = useUi.getState();
    if (!r.done) return ui.notify("info", text);
    const full = [text + (noUndoText(r).length ? "." : ""), ...noUndoText(r)].join(" ");
    const entries = undoable(r);
    if (!entries.length) return ui.notify("success", full);
    ui.notify("success", full, game, { label: "Undo", onAction: () => void undo(entries) });
  };
}
