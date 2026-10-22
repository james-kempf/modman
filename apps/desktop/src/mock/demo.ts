// SPDX-License-Identifier: GPL-3.0-or-later
// Demo backend for `vite dev` outside Tauri: canned data plus job flows
// that raise prompts (FOMOD wizard on install, confirm on purge). State
// changes persist for the session, and cancel semantics follow the real
// backend: a declined confirm finishes the job ("Cancelled."), a cancelled
// FOMOD wizard fails it with "FOMOD installer cancelled", and a withdrawn
// prompt counts as cancel.

import type { Answer, CollectionRecord, ConfigEntry, Msg, ProfileSummary, Prompt, ReqRow } from "../ipc/types";
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
  /** The mod set changed since the last deploy. */
  let dirty = false;
  const config: ConfigEntry[] = [
    { key: "deploy-method", value: "auto", description: "hardlink, reflink, symlink, copy or auto" },
    { key: "autoupdate-interval", value: "0", description: "hours between auto-update runs (0 = hourly)" },
  ];
  const deps: ReqRow[] = [
    { status: "missing", key: "skse", name: "SKSE64", hint: "install 30379", urls: [], by: ["SkyUI"], detail: "", acked: false },
    { status: "external", key: "skse-loader", name: "SKSE loader", hint: "", urls: ["https://skse.silverlock.org"], by: ["SkyUI"], detail: "", acked: false },
  ];
  const profiles: (ProfileSummary & { set: { uid: string; enabled: boolean }[] })[] = [];
  const collections: CollectionRecord[] = [];

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
  const now = () => Math.floor(Date.now() / 1000);
  const renumber = () => mods.forEach((m, k) => (m.position = k + 1));
  renumber();
  /** A mod-set change: deploy is due, or done when the command asked for it. */
  const changed = (deployNow = false) => {
    dirty = !deployNow;
    return deployNow ? afterChange({ deployed: deployReport({ mods: mods.filter((m) => m.enabled).length }), needs_deploy: false }) : afterChange();
  };
  const cancelled = () => ({ message: "FOMOD installer cancelled" });
  const fomod = (id: number, module: string) =>
    ask(id, {
      type: "fomod_group",
      module,
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
  const confirm = async (id: number, question: string, default_yes = true) => {
    const a = await ask(id, { type: "confirm", question, default_yes });
    return a.type === "confirm" && a.yes;
  };
  // A withdrawn prompt (modman:prompt-closed without an answer) cancels the question, as in the real backend.
  void t.listen<{ id: number }>("modman:prompt-closed", ({ id }) => {
    const r = waiting.get(id);
    waiting.delete(id);
    r?.({ type: "cancel" });
  });

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
    config_list: () => config.map((c) => ({ ...c })),
    config_set: (a: Args) => {
      const c = config.find((x) => x.key === a!.key);
      if (!c) throw { message: `unknown config key '${String(a!.key)}'` };
      c.value = String(a!.value);
      return { ...c, old_data_left: null };
    },
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
        if (games.some((g) => g.domain === domain)) throw { message: `${domain} is already added` };
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
    mods_list: () => ({ mods: mods.map((m) => ({ ...m })), changed_since_deploy: dirty }),
    mod_detail: (a: Args) => {
      const m = mods.find((x) => x.uid === a!.reference)!;
      return { mod: m, collections: [], fomod: [], wins: 0, loses: 0, url: null };
    },
    mods_set_enabled: (a: Args) =>
      job("enable", async () => {
        mods.forEach((m) => (a!.ids as number[]).includes(m.id) && (m.enabled = a!.enabled as boolean));
        return changed();
      }),
    mod_move: (a: Args) =>
      job("move", async () => {
        const i = mods.findIndex((m) => m.uid === a!.reference);
        if (i < 0) throw { message: `no mod '${String(a!.reference)}'` };
        const j = a!.spec === "up" ? i - 1 : a!.spec === "down" ? i + 1 : i;
        if (j < 0 || j >= mods.length) throw { message: `${mods[i].name} is already ${j < 0 ? "first" : "last"}` };
        [mods[i], mods[j]] = [mods[j], mods[i]];
        renumber();
        return { uid: a!.reference, from: i + 1, to: j + 1, of: mods.length, after: changed() };
      }),
    thumbnail: () => null,
    install: (a: Args) =>
      job("install", async (id) => {
        say(id, `Downloading ${a!.target}…`);
        const answer = await fomod(id, "Demo Mod");
        if (answer.type === "cancel") throw cancelled();
        if (!(await confirm(id, "Install the patch for SkyUI too?"))) {
          say(id, "Cancelled.");
          return { game: active().domain, installed: null, collection: null, after: afterChange({ needs_deploy: false }) };
        }
        const m = mod({ name: `Demo Mod ${a!.target}`, installed_at: now(), position: mods.length + 1 });
        mods.push(m);
        renumber();
        say(id, `Installed ${m.name}`, "success");
        return {
          game: active().domain,
          installed: { mod_id: m.id, uid: m.uid, name: m.name, files: 3, size: 1000, fomod_found: true, replaced: null, warnings: [] },
          collection: null,
          after: changed(a!.deploy === true),
        };
      }),
    mod_reinstall: (a: Args) =>
      job("mod_reinstall", async (id) => {
        const m = mods.find((x) => x.uid === a!.reference);
        if (!m) throw { message: `no mod '${String(a!.reference)}'` };
        if ((await fomod(id, m.name)).type === "cancel") throw cancelled();
        m.installed_at = now();
        say(id, `Reinstalled ${m.name}`, "success");
        return {
          installed: { mod_id: m.id, uid: m.uid, name: m.name, files: m.file_count, size: m.size, fomod_found: true, replaced: m.uid, warnings: [] },
          after: changed(),
        };
      }),
    mods_uninstall: (a: Args) =>
      job("mods_uninstall", async (id) => {
        const refs = a!.refs as string[];
        const gone = mods.filter((m) => refs.includes(m.uid));
        if (!gone.length) throw { message: `no mod matches ${refs.join(", ")}` };
        if (!(await confirm(id, `Uninstall ${gone.length} mod(s)?`))) {
          say(id, "Cancelled.");
          return { done: false, mods: 0, archives: [], after: afterChange({ needs_deploy: false }) };
        }
        gone.forEach((m) => mods.splice(mods.indexOf(m), 1));
        renumber();
        return { done: true, mods: gone.length, archives: [], after: changed() };
      }),
    nxm_submit: (a: Args) => ({ row: 1, link: String(a!.url).split("?")[0], kind: "mod" }),
    downloads_list: () => [],
    deploy: () =>
      job("deploy", async (id) => {
        for (let d = 0; d <= 20; d += 5) t.emit("modman:deploy-progress", { job: id, done: d, total: 20 });
        dirty = false;
        return deployReport();
      }),
    purge: () =>
      job("purge", async (id) => {
        const a = await ask(id, { type: "confirm", question: "Remove every deployed file?", default_yes: false });
        if (a.type !== "confirm" || !a.yes) {
          say(id, "Cancelled.");
          return null;
        }
        return { nothing_deployed: false, removed: 20, restored: 0, missing: 0, changed: 0, kept: 0, recovered: null, plugins: null };
      }),
    verify: () => ({ intact: 20, issues: [], interrupted: false }),
    deploy_status: () => active().deploy,
    conflicts: () => ({
      files: [{ rel_path: "textures/sky.dds", winner: mods[1], losers: [mods[0]], plugin: false }],
      plugins: [],
    }),
    deps: () => deps.map((d) => ({ ...d })),
    deps_ack: (a: Args) => setAck(String(a!.name), true),
    deps_unack: (a: Args) => setAck(String(a!.name), false),
    profiles_list: () => profiles.map(({ set: _, ...p }) => ({ ...p })),
    profile_save: (a: Args) =>
      job("profile_save", async (id) => {
        const name = String(a!.name).trim();
        if (!name) throw { message: "profile name is empty" };
        const old = profiles.findIndex((p) => p.name === name);
        if (old >= 0 && !(await confirm(id, `Overwrite profile ${name}?`, false))) return null;
        const p = {
          name,
          mods: mods.length,
          enabled: mods.filter((m) => m.enabled).length,
          saved_at: now(),
          set: mods.map((m) => ({ uid: m.uid, enabled: m.enabled })),
        };
        if (old >= 0) profiles[old] = p;
        else profiles.push(p);
        return mods.length;
      }),
    profile_load: (a: Args) =>
      job("profile_load", async () => {
        const p = profiles.find((x) => x.name === a!.name);
        if (!p) throw { message: `no profile '${String(a!.name)}'` };
        let missing = 0;
        for (const e of p.set) {
          const m = mods.find((x) => x.uid === e.uid);
          if (m) m.enabled = e.enabled;
          else missing++;
        }
        const listed = new Set(p.set.map((e) => e.uid));
        const extra = mods.filter((m) => !listed.has(m.uid));
        extra.forEach((m) => (m.enabled = false));
        return { missing, added_disabled: extra.length, after: changed() };
      }),
    profile_delete: (a: Args) => {
      const i = profiles.findIndex((x) => x.name === a!.name);
      if (i < 0) throw { message: `no profile '${String(a!.name)}'` };
      profiles.splice(i, 1);
      return null;
    },
    plugins_list: () => [
      { name: "Skyrim.esm", active: true, managed: false },
      { name: "Update.esm", active: true, managed: false },
      { name: "Unofficial Skyrim Special Edition Patch.esp", active: true, managed: true },
      { name: "SkyUI_SE.esp", active: false, managed: true },
    ],
    collections_list: () =>
      collections.map((c) => ({ record: { ...c }, enabled: mods.filter((m) => c.mods.includes(m.id) && m.enabled).length })),
    collection_info: (a: Args) => {
      const slug = collectionSlug(String(a!.reference));
      const installed = collections.find((c) => c.slug === slug) ?? null;
      return {
        game: active().domain,
        slug,
        revision: {
          name: `Demo collection ${slug}`,
          author: "curator",
          game_name: active().name,
          number: 3,
          status: "published",
          latest: 3,
          mod_count: 2,
          optional_count: 1,
          total_size: 50 * 1024 * 1024,
          summary: "Two demo mods with the curator's choices.",
          url: `https://next.nexusmods.com/${active().domain}/collections/${slug}`,
        },
        installed,
        update_available: installed && installed.revision < 3 ? 3 : null,
      };
    },
    collection_install: (a: Args) =>
      job("collection_install", async (id) => {
        const slug = collectionSlug(String(a!.reference));
        const base = collectionResult(slug);
        if (!(await confirm(id, `Install collection ${slug} (2 mods)?`))) {
          say(id, "Cancelled.");
          return { ...base, cancelled: true };
        }
        const added = [1, 2].map((n) =>
          mod({ name: `${slug} mod ${n}`, installed_at: now(), position: mods.length + n, collections: [slug] }),
        );
        mods.push(...added);
        renumber();
        const rec: CollectionRecord = {
          id: collections.length + 1,
          game: active().domain,
          slug,
          name: `Demo collection ${slug}`,
          author: "curator",
          revision: (a!.revision as number | null) ?? 3,
          dir: null,
          failed: [],
          installed_at: now(),
          mods: added.map((m) => m.id),
        };
        const old = collections.findIndex((c) => c.slug === slug);
        if (old >= 0) collections[old] = rec;
        else collections.push(rec);
        const after = changed(a!.deploy === true);
        return { ...base, installed: added.length, deployed: after.deployed };
      }),
    collection_update: (a: Args) =>
      job("collection_update", async () => {
        const slug = collectionSlug(String(a!.reference));
        const c = collections.find((x) => x.slug === slug);
        if (!c) throw { message: `collection ${slug} is not installed` };
        const up = c.revision >= 3;
        c.revision = 3;
        return { ...collectionResult(slug), up_to_date: up, present: c.mods.length };
      }),
    collection_remove: (a: Args) =>
      job("collection_remove", async (id) => {
        const slug = collectionSlug(String(a!.reference));
        const c = collections.find((x) => x.slug === slug);
        if (!c) throw { message: `collection ${slug} is not installed` };
        const keep = a!.keep_mods === true;
        if (!(await confirm(id, `Remove collection ${slug}${keep ? "" : ` and its ${c.mods.length} mods`}?`, false))) {
          say(id, "Cancelled.");
          return { removed: false, uninstalled: 0, after: afterChange({ needs_deploy: false }) };
        }
        collections.splice(collections.indexOf(c), 1);
        let uninstalled = 0;
        if (!keep)
          for (const mid of c.mods) {
            const i = mods.findIndex((m) => m.id === mid);
            if (i >= 0) {
              mods.splice(i, 1);
              uninstalled++;
            }
          }
        renumber();
        return { removed: true, uninstalled, after: keep ? afterChange({ needs_deploy: false }) : changed() };
      }),
    updates_check: () => ({ checked: mods.length, fetched: 0, stopped_early: false, updates: [], installed: 0 }),
    updates_install: () =>
      job("updates_install", async () => ({ checked: mods.length, fetched: 0, stopped_early: false, updates: [], installed: 0 })),
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
  function setAck(key: string, acked: boolean): ReqRow {
    const d = deps.find((x) => x.key === key);
    if (!d || d.status !== "external") throw { message: `'${key}' is not an external requirement` };
    d.acked = acked;
    return { ...d };
  }
  function collectionResult(slug: string) {
    return {
      game: active().domain,
      slug,
      revision: 3,
      cancelled: false,
      download_only: false,
      installed: 0,
      present: 0,
      failed: [],
      removed: 0,
      up_to_date: false,
      deployed: null,
    };
  }
  return t;
}

/** Slug from a collection URL, nxm link or bare slug. */
function collectionSlug(ref: string): string {
  const m = /collections\/([^/?#\s]+)/.exec(ref);
  return (m ? m[1] : ref.trim()) || "demo";
}
