// SPDX-License-Identifier: GPL-3.0-or-later
// T15 copy rule: every toast title is 1-2 words, no digits, no colon, no game name; specifics go in the detail.
// The suite-wide hook in test/setup.ts checks every toast any test shows; this file covers what the suite may
// not reach: every message builder, every job kind, every run() label and every literal title in the source.
import { describe, expect, it } from "vitest";
import { deployReport } from "../mock/fixtures";
import { installedMsg } from "../components/browse/ModActions";
import { collectionMsg, removedMsg } from "../views/Collections";
import { profileLoadedMsg, uninstalledMsg, updatesInstalledMsg } from "../views/Mods";
import { purgedMsg } from "../views/Overview";
import { deployedMsg } from "./run";
import { cancelledMsg, failedMsg, isToastTitle, jobDoneTitle, jobRunningTitle, type ToastMsg } from "./toastText";

const GAME = /\b(Skyrim|Cyberpunk|Fallout|Stardew|Palworld|Oblivion)\b/i;
const ok = (title: string) => isToastTitle(title) && !GAME.test(title);

// Every app source file as text (Vite glob; tests, mocks and test helpers excluded).
const files = import.meta.glob(["../**/*.{ts,tsx}", "!../**/*.test.{ts,tsx}", "!../mock/**", "!../test/**"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;
const code = Object.entries(files);

describe("toast titles", () => {
  // Fails when a builder puts counts, names, a colon or the game into the title.
  it("every message builder: short title, specifics in the detail", () => {
    const coll = { slug: "abc", installed: 2, present: 1, removed: 1, failed: [], cancelled: false, up_to_date: false } as never;
    const msgs: ToastMsg[] = [
      deployedMsg(deployReport({ placed: 120, mods: 8 })),
      purgedMsg({ nothing_deployed: true } as never),
      purgedMsg({ nothing_deployed: false, removed: 3, restored: 2, missing: 1, changed: 1, kept: 1, plugins: null } as never),
      collectionMsg(coll),
      collectionMsg({ ...(coll as object), cancelled: true } as never),
      collectionMsg({ ...(coll as object), up_to_date: true } as never),
      removedMsg("abc", { removed: true, uninstalled: 2 } as never),
      removedMsg("abc", { removed: false, uninstalled: 0 } as never),
      uninstalledMsg({ done: true, mods: 1, archives: [] } as never, "SkyUI"),
      uninstalledMsg({ done: true, mods: 3, archives: ["a"] } as never, "3 mods"),
      uninstalledMsg({ done: false, mods: 0, archives: [] } as never, "SkyUI"),
      profileLoadedMsg("evening", { missing: 2, added_disabled: 1 } as never),
      updatesInstalledMsg({ installed: 0 } as never),
      updatesInstalledMsg({ installed: 4 } as never),
      installedMsg({ installed: { name: "SkyUI", files: 12, replaced: null } } as never),
      installedMsg({ collection: { slug: "abc", installed: 1, present: 0, failed: [], cancelled: false } } as never),
      installedMsg({} as never),
    ];
    for (const m of msgs) expect(ok(m.title), m.title).toBe(true);
    expect(deployedMsg(deployReport({ placed: 120, mods: 8 }))).toEqual({ title: "Deployed", detail: "120 files · 8 mods" });
    expect(uninstalledMsg({ done: true, mods: 1, archives: [] } as never, "SkyUI")).toEqual({ title: "Uninstalled", detail: "SkyUI" });
  });

  // Fails when a job kind's running or done title breaks the rule.
  it("every job kind, running and done", () => {
    const kinds = [...files["./jobs.ts"].matchAll(/^\s+(\w+): "/gm)].map((m) => m[1]);
    expect(kinds.length).toBeGreaterThan(20);
    for (const k of [...kinds, "unknown_kind"]) {
      expect(ok(jobRunningTitle(k)), `${k} running`).toBe(true);
      expect(ok(jobDoneTitle(k)), `${k} done`).toBe(true);
    }
  });

  // Fails when a command's failure or cancellation title grows past two words (e.g. "Sign in failed").
  it("every run() and act() label, failed and cancelled", () => {
    const labels = new Set(code.flatMap(([, s]) => [...s.matchAll(/\b(?:run|act)\(\s*"([^"]+)"/g)].map((m) => m[1])));
    expect(labels.size).toBeGreaterThan(20);
    for (const l of labels) {
      expect(ok(failedMsg(l, "x").title), `${l} failed`).toBe(true);
      expect(ok(cancelledMsg(l).title), `${l} cancelled`).toBe(true);
    }
  });

  // Fails when a showToast call site writes a literal title that breaks the rule.
  it("every literal title passed to showToast in the source", () => {
    const titles: string[] = [];
    for (const [, s] of code) {
      for (const m of s.matchAll(/showToast\(\s*[^,]+,\s*([^,{]+?)\s*(?:,\s*\{|\))/g)) {
        for (const lit of m[1].matchAll(/"([^"]*)"/g)) titles.push(lit[1]);
      }
    }
    expect(titles.length).toBeGreaterThan(20);
    for (const t of titles) expect(ok(t), t).toBe(true);
  });
});
