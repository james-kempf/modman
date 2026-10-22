// SPDX-License-Identifier: GPL-3.0-or-later
// Demo backend for `vite dev` outside Tauri: canned data plus job flows
// that raise prompts (FOMOD wizard on install, confirm on purge).

import type { Answer, Msg, Prompt } from "../ipc/types";
import { createFakeTransport, type Args, type FakeTransport } from "./fake";
import { afterChange, autoupdateStatus, deployReport, experimentalGame, game, mod } from "./fixtures";

export function createDemoBackend(): FakeTransport {
  const games = [game(), experimentalGame({ active: false, mod_count: 0 })];
  const mods = [
    mod({ name: "SkyUI", nexus_mod_id: 12604 }),
    mod({ name: "Unofficial Patch", nexus_mod_id: 266 }),
    mod({ name: "Immersive Armors", enabled: false }),
  ];
  let jobSeq = 0;
  let promptSeq = 0;
  const waiting = new Map<number, (a: Answer) => void>();
  let loggedIn = false;
  let timer = false;

  const t = createFakeTransport();
  const say = (job: number, text: string, level: Msg["level"] = "info") =>
    t.emit("modman:activity", { job, msg: { level, spans: [{ text, style: "plain" }] }, text });
  const ask = (job: number, prompt: Prompt) =>
    new Promise<Answer>((resolve) => {
      const id = ++promptSeq;
      waiting.set(id, resolve);
      t.emit("modman:prompt", { id, job, prompt });
    });
  const job = async <T>(kind: string, body: (id: number) => Promise<T>): Promise<T> => {
    const id = ++jobSeq;
    t.emit("modman:job", { id, kind, state: "started", error: null });
    try {
      const r = await body(id);
      t.emit("modman:job", { id, kind, state: "finished", error: null });
      return r;
    } catch (e) {
      t.emit("modman:job", { id, kind, state: "failed", error: String((e as { message?: string }).message ?? e) });
      throw e;
    }
  };
  const active = () => games.find((g) => g.active)!;

  Object.assign(t.handlers, {
    app_info: () => ({
      version: "0.1.0-demo",
      config_dir: "~/.config/modman",
      data_dir: "~/.local/share/modman",
      log_path: "~/.local/state/modman/modman.log",
      db_path: "~/.local/share/modman/modman.db",
      legacy_state_dir: null,
    }),
    app_ready: () => ({ startup_check: { system_busy: null, pending: [] }, queued: 0 }),
    startup_check: () => null,
    prompt_answer: (a: Args) => {
      const r = waiting.get(a!.id as number);
      waiting.delete(a!.id as number);
      r?.(a!.answer as Answer);
      t.emit("modman:prompt-closed", { id: a!.id as number });
      return !!r;
    },
    log_tail: () => ["demo backend: no log file"],
    config_list: () => [
      { key: "deploy-method", value: "auto", description: "hardlink, reflink, symlink, copy or auto" },
      { key: "autoupdate-interval", value: "0", description: "hours between auto-update runs (0 = hourly)" },
    ],
    config_set: (a: Args) => ({ key: a!.key, value: a!.value, description: "", old_data_left: null }),
    account: () =>
      loggedIn
        ? { key_source: "keyring", user: "demo", premium: true, quota: "20000/day" }
        : { key_source: null, user: null, premium: null, quota: null },
    login: () => {
      loggedIn = true;
      return { user: "demo", premium: true, backend: "keyring", file: null, keyring_unavailable: null };
    },
    logout: () => {
      loggedIn = false;
      return false;
    },
    games_list: () => games.map((g) => ({ ...g })),
    games_detect: () => [
      {
        domain: "fallout4",
        name: "Fallout 4",
        install_dir: "/steam/common/Fallout 4",
        support: "experimental",
        configured: games.some((g) => g.domain === "fallout4"),
      },
    ],
    game_use: (a: Args) => {
      games.forEach((g) => (g.active = g.domain === a!.domain));
      return null;
    },
    game_add: (a: Args) =>
      job("game_add", async (id) => {
        const domain = String(a!.domain);
        const name = domain === "fallout4" ? "Fallout 4" : domain;
        games.forEach((g) => (g.active = false));
        const g = experimentalGame({ domain, name, profile: "bethesda", folder: (a!.path as string) ?? null, mod_count: 0, active: true });
        games.push(g);
        say(id, `Added ${name}.`, "success");
        return g;
      }),
    game_remove: (a: Args) =>
      job("game_remove", async (id) => {
        const g = games.find((x) => x.domain === a!.domain)!;
        const r = await ask(id, { type: "confirm", question: `Forget ${g.name}?`, default_yes: false });
        if (r.type !== "confirm" || !r.yes) return false;
        games.splice(games.indexOf(g), 1);
        if (g.active && games[0]) games[0].active = true;
        return true;
      }),
    game_launch: () =>
      job("launch", async (id) => {
        const g = active();
        say(id, `Launching ${g.name} (Steam app ${g.appid})...`);
        return { appid: g.appid, deployed: false };
      }),
    mods_list: () => ({ mods: mods.map((m) => ({ ...m })), changed_since_deploy: false }),
    mod_detail: (a: Args) => {
      const m = mods.find((x) => x.uid === a!.reference)!;
      return { mod: m, collections: [], fomod: [], wins: 0, loses: 0, url: null };
    },
    mods_set_enabled: (a: Args) =>
      job("enable", async () => {
        mods.forEach((m) => (a!.ids as number[]).includes(m.id) && (m.enabled = a!.enabled as boolean));
        return afterChange();
      }),
    mod_move: (a: Args) =>
      job("move", async () => {
        const i = mods.findIndex((m) => m.uid === a!.reference);
        const j = a!.spec === "up" ? i - 1 : i + 1;
        [mods[i], mods[j]] = [mods[j], mods[i]];
        mods.forEach((m, k) => (m.position = k + 1));
        return { uid: a!.reference, from: i + 1, to: j + 1, of: mods.length, after: afterChange() };
      }),
    thumbnail: () => null,
    install: (a: Args) =>
      job("install", async (id) => {
        say(id, `Downloading ${a!.target}…`);
        let answer = await ask(id, {
          type: "fomod_group",
          module: "Demo Mod",
          step: "Main",
          group: "Textures",
          kind: "SelectExactlyOne",
          kind_label: "Select one",
          options: [
            { name: "1K", description: "Small textures.", kind: "Optional", kind_label: null },
            { name: "2K", description: "Balanced.", kind: "Recommended", kind_label: "Recommended" },
          ],
          defaults: [1],
          error: null,
        });
        if (answer.type === "cancel") throw { message: "cancelled" };
        answer = await ask(id, { type: "confirm", question: "Install the patch for SkyUI too?", default_yes: true });
        if (answer.type === "cancel") throw { message: "cancelled" };
        const m = mod({ name: `Demo Mod ${a!.target}` });
        mods.push(m);
        say(id, `Installed ${m.name}`, "success");
        return {
          game: active().domain,
          installed: { mod_id: m.id, uid: m.uid, name: m.name, files: 3, size: 1000, fomod_found: true, replaced: null, warnings: [] },
          collection: null,
          after: afterChange(),
        };
      }),
    nxm_submit: (a: Args) => ({ row: 1, link: String(a!.url).split("?")[0], kind: "mod" }),
    downloads_list: () => [],
    deploy: () =>
      job("deploy", async (id) => {
        for (let d = 0; d <= 20; d += 5) t.emit("modman:deploy-progress", { job: id, done: d, total: 20 });
        return deployReport();
      }),
    purge: () =>
      job("purge", async (id) => {
        const a = await ask(id, { type: "confirm", question: "Remove every deployed file?", default_yes: false });
        if (a.type !== "confirm" || !a.yes) return null;
        return { nothing_deployed: false, removed: 20, restored: 0, missing: 0, changed: 0, kept: 0, recovered: null, plugins: null };
      }),
    verify: () => ({ intact: 20, issues: [], interrupted: false }),
    deploy_status: () => active().deploy,
    conflicts: () => ({
      files: [{ rel_path: "textures/sky.dds", winner: mods[1], losers: [mods[0]], plugin: false }],
      plugins: [],
    }),
    deps: () => [
      { status: "missing", key: "skse", name: "SKSE64", hint: "install 30379", urls: [], by: ["SkyUI"], detail: "", acked: false },
      { status: "external", key: "skse-loader", name: "SKSE loader", hint: "", urls: ["https://skse.silverlock.org"], by: ["SkyUI"], detail: "", acked: false },
    ],
    deps_ack: (a: Args) => ({ status: "external", key: a!.name, name: a!.name, hint: "", urls: [], by: [], detail: "", acked: true }),
    deps_unack: (a: Args) => ({ status: "external", key: a!.name, name: a!.name, hint: "", urls: [], by: [], detail: "", acked: false }),
    profiles_list: () => [],
    plugins_list: () => [
      { name: "Skyrim.esm", active: true, managed: false },
      { name: "Update.esm", active: true, managed: false },
      { name: "Unofficial Skyrim Special Edition Patch.esp", active: true, managed: true },
      { name: "SkyUI_SE.esp", active: false, managed: true },
    ],
    collections_list: () => [],
    updates_check: () => ({ checked: mods.length, fetched: 0, stopped_early: false, updates: [], installed: 0 }),
    search: () => ({ total: 0, mods: [] }),
    autoupdate_status: () =>
      autoupdateStatus({ timer: { installed: timer, active: timer ? "active" : null, next_elapse: null, every_hours: 1 } }),
    autoupdate_enable: () => {
      timer = true;
      return autoupdateStatus();
    },
    autoupdate_disable: () => {
      const was = timer;
      timer = false;
      return was;
    },
    autoupdate_runs: () => [],
  });
  return t;
}
