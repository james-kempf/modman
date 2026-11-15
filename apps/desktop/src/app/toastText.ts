// SPDX-License-Identifier: GPL-3.0-or-later
// Toast copy: a short generic title and a specific detail line (DESIGN.md toast copy rule, T15).
// Title: 1-2 words, no game name, no counts or item names, no colon. Vocabulary: a past-tense verb when
// something is done ("Deployed", "Uninstalled"), an -ing verb while it runs ("Deploying"), "<Noun> failed" /
// "Failed" for errors, "<Noun> cancelled" / "Cancelled", and a short state for the rest ("Already current",
// "Paste blocked"). Detail: the names, counts, reasons and next steps. No store import (the store uses it).

export interface ToastMsg {
  title: string;
  detail?: string;
}

const oneWord = (s: string) => !/\s/.test(s.trim());

/** "<Label> failed" when the label is one word, else "Failed" with the label leading the detail. */
export function failedMsg(label: string, why?: string | null): ToastMsg {
  if (oneWord(label)) return why ? { title: `${label} failed`, detail: why } : { title: `${label} failed` };
  return { title: "Failed", detail: why ? `${label} · ${why}` : label };
}

/** "<Label> cancelled" when the label is one word, else "Cancelled" with the label as the detail. */
export function cancelledMsg(label: string): ToastMsg {
  return oneWord(label) ? { title: `${label} cancelled` } : { title: "Cancelled", detail: label };
}

/** Job kinds: the -ing title while running and the past-tense title when finished. */
const JOB_TITLES: Record<string, [running: string, done: string]> = {
  deploy: ["Deploying", "Deployed"],
  purge: ["Purging", "Purged"],
  install: ["Installing", "Installed"],
  mods_uninstall: ["Uninstalling", "Uninstalled"],
  mod_reinstall: ["Reinstalling", "Reinstalled"],
  mod_restore: ["Restoring", "Restored"],
  mods_set_enabled: ["Updating", "Updated"],
  enable: ["Updating", "Updated"],
  mod_move: ["Moving", "Moved"],
  move: ["Moving", "Moved"],
  game_add: ["Adding game", "Game added"],
  game_remove: ["Removing game", "Game removed"],
  launch: ["Launching", "Launched"],
  backups_clean: ["Quarantining", "Quarantined"],
  quarantine_restore: ["Restoring", "Restored"],
  restage: ["Restaging", "Restaged"],
  collection_install: ["Installing", "Installed"],
  collection_update: ["Updating", "Updated"],
  collection_remove: ["Removing", "Removed"],
  nxm: ["Downloading", "Downloaded"],
  plugins_masterlist: ["Updating", "Updated"],
  plugins_sort: ["Sorting", "Sorted"],
  profile_save: ["Saving", "Saved"],
  profile_load: ["Loading", "Loaded"],
  updates_check: ["Checking", "Checked"],
  updates_install: ["Updating", "Updated"],
  loader_run: ["Installing", "Installed"],
  loader_rollback: ["Rolling back", "Rolled back"],
  loader_junk_remove: ["Cleaning up", "Cleaned up"],
  autoupdate_enable: ["Enabling", "Enabled"],
};

export const jobRunningTitle = (kind: string) => JOB_TITLES[kind]?.[0] ?? "Working";
export const jobDoneTitle = (kind: string) => JOB_TITLES[kind]?.[1] ?? "Finished";

/** True when `title` follows the rule: 1-2 words, no digits, no colon (the game name check is the caller's). */
export function isToastTitle(title: string): boolean {
  const words = title.trim().split(/\s+/);
  return words.length >= 1 && words.length <= 2 && words[0] !== "" && !/[\d:]/.test(title);
}
