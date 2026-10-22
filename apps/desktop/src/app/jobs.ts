// SPDX-License-Identifier: GPL-3.0-or-later
// Job presentation: readable names for backend job kinds, and telling a
// job the user cancelled apart from one that failed.

import type { JobEntry } from "./store";

const LABEL: Record<string, string> = {
  game_add: "Add game",
  game_remove: "Remove game",
  launch: "Launch game",
  mods_set_enabled: "Enable/disable mods",
  enable: "Enable/disable mods",
  mod_move: "Move mod",
  move: "Move mod",
  mods_uninstall: "Uninstall mods",
  mod_reinstall: "Reinstall mod",
  install: "Install",
  deploy: "Deploy",
  purge: "Purge",
  autoupdate_enable: "Enable auto-update",
  collection_install: "Install collection",
  collection_update: "Update collection",
  collection_remove: "Remove collection",
  nxm: "Download link",
  plugins_masterlist: "Update LOOT masterlist",
  plugins_sort: "Sort plugins",
  profile_save: "Save profile",
  profile_load: "Load profile",
  updates_check: "Check updates",
  updates_install: "Install updates",
};

/** "game_add" -> "Add game"; unknown kinds get their words capitalised. */
export function jobLabel(kind: string): string {
  if (LABEL[kind]) return LABEL[kind];
  const words = kind.replace(/[_-]+/g, " ").trim();
  return words ? words[0].toUpperCase() + words.slice(1) : "Job";
}

/**
 * An error that only says the user cancelled (e.g. core's "FOMOD installer
 * cancelled" when a prompt is answered with cancel or withdrawn).
 */
export function isCancelError(msg: string | null | undefined): boolean {
  return !!msg && /\bcancell?ed\b/i.test(msg);
}

export type JobOutcome = "running" | "finished" | "cancelled" | "failed";

export function jobOutcome(j: Pick<JobEntry, "state" | "error">): JobOutcome {
  if (j.state === "started") return "running";
  if (j.state === "finished") return "finished";
  return isCancelError(j.error) ? "cancelled" : "failed";
}
