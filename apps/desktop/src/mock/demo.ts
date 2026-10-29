// SPDX-License-Identifier: GPL-3.0-or-later
// Demo backend for `vite dev` outside Tauri: canned data plus job flows
// that raise prompts (FOMOD wizard on install, confirm on purge). State
// changes persist for the session, and cancel semantics follow the real
// backend: a declined confirm finishes the job ("Cancelled."), a cancelled
// FOMOD wizard fails it with "FOMOD installer cancelled", and a withdrawn
// prompt counts as cancel. Mods, the "changed since deploy" flag, profiles
// and collections are kept per game.

import type { Answer, CollectionRecord, ConfigEntry, ModView, Msg, ProfileSummary, Prompt, ReqRow, UpdateReport } from "../ipc/types";
import { createFakeTransport, type Args, type FakeTransport } from "./fake";
import { afterChange, autoupdateStatus, deployReport, experimentalGame, game, mod } from "./fixtures";
import { demoPoisoned, repairHandlers } from "./repair";

export function createDemoBackend(): FakeTransport {
  const games = [
    game({ prefix: "/home/deck/.local/share/Steam/steamapps/compatdata/489830/pfx" }),
    experimentalGame({ active: false, mod_count: 0 }),
  ];
  type Profile = ProfileSummary & { set: { uid: string; enabled: boolean }[] };
  interface GameState {
    mods: ModView[];
    /** The mod set changed since the last deploy. */
    dirty: boolean;
    /** Ids of the mods the last deploy placed. */
    deployed?: Set<number>;
    profiles: Profile[];
    collections: CollectionRecord[];
  }
  const state = new Map<string, GameState>();
  state.set(games[0].domain, {
    mods: [
      mod({ name: "SkyUI", nexus_mod_id: 12604 }),
      mod({ name: "Unofficial Patch", nexus_mod_id: 266 }),
      mod({ name: "Immersive Armors", enabled: false }),
    ],
    dirty: false,
    profiles: [],
    collections: [],
  });
  /** The state of the named game (a command's `game` argument), else the active one. */
  const st = (a?: Args): GameState => {
    const d = typeof a?.game === "string" && a.game ? a.game : active().domain;
    let g = state.get(d);
    if (!g) state.set(d, (g = { mods: [], dirty: false, profiles: [], collections: [] }));
    return g;
  };
  let jobSeq = 0;
  let promptSeq = 0;
  let collSeq = 0;
  const waiting = new Map<number, (a: Answer) => void>();
  let loggedIn = false;
  let timer = false;
  let shortcut = false;
  const bools = ["true", "false"];
  // The core's 9 keys with their kinds and options (config.rs).
  const config: ConfigEntry[] = [
    { key: "data-dir", value: "~/.local/share/vessel", description: "where downloads and per-game data live", kind: "path", options: [] },
    {
      key: "deploy-method",
      value: "auto",
      description: "auto|hardlink|reflink|symlink|copy",
      kind: "enum",
      options: ["auto", "hardlink", "reflink", "symlink", "copy"],
    },
    { key: "jobs", value: "4", description: "reserved; downloads run one at a time", kind: "number", options: [] },
    { key: "cdn", value: "", description: "preferred download server", kind: "text", options: [] },
    { key: "adult", value: "false", description: "show adult-only collections", kind: "bool", options: bools },
    { key: "auto-deploy", value: "false", description: "deploy after every change", kind: "bool", options: bools },
    {
      key: "fomod",
      value: "auto",
      description: "auto|ask|default — installer behaviour for single mods",
      kind: "enum",
      options: ["auto", "ask", "default"],
    },
    {
      key: "autoupdate-collections",
      value: "true",
      description: "auto-update moves collections to their newest revision",
      kind: "bool",
      options: bools,
    },
    {
      key: "autoupdate-interval",
      value: "0",
      description: "hours between extra auto-update checks; 0 = only after system/game updates",
      kind: "number",
      options: [],
    },
  ];
  const deps: ReqRow[] = [
    { status: "missing", key: "skse", name: "SKSE64", hint: "install 30379", urls: [], by: ["SkyUI"], detail: "", acked: false },
    { status: "external", key: "skse-loader", name: "SKSE loader", hint: "", urls: ["https://skse.silverlock.org"], by: ["SkyUI"], detail: "", acked: false },
  ];

  const t = createFakeTransport();
  const say = (job: number, text: string, level: Msg["level"] = "info") =>
    t.emit("vessel:activity", { job, msg: { level, spans: [{ text, style: "plain" }] }, text });
  const ask = (job: number, prompt: Prompt) =>
    new Promise<Answer>((resolve) => {
      const id = ++promptSeq;
      waiting.set(id, resolve);
      t.emit("vessel:prompt", { id, job, prompt });
    });
  const job = async <T>(kind: string, body: (id: number) => Promise<T>): Promise<T> => {
    const id = ++jobSeq;
    t.emit("vessel:job", { id, kind, state: "started", error: null });
    try {
      const r = await body(id);
      t.emit("vessel:job", { id, kind, state: "finished", error: null });
      return r;
    } catch (e) {
      t.emit("vessel:job", { id, kind, state: "failed", error: String((e as { message?: string }).message ?? e) });
      throw e;
    }
  };
  const active = () => games.find((g) => g.active)!;
  const now = () => Math.floor(Date.now() / 1000);
  const renumber = (g: GameState) => g.mods.forEach((m, k) => (m.position = k + 1));
  state.forEach(renumber);
  // The first game starts deployed and in sync: its deployment holds its enabled mods.
  for (const g of games) {
    const s = state.get(g.domain);
    if (s && g.deploy.active) s.deployed = new Set(s.mods.filter((m) => m.enabled).map((m) => m.id));
  }
  /** A mod-set change: deploy is due, or done when the command asked for it. */
  const changed = (g: GameState, deployNow = false) => {
    g.dirty = !deployNow;
    return deployNow
      ? afterChange({ deployed: deployReport({ mods: g.mods.filter((m) => m.enabled).length }), needs_deploy: false })
      : afterChange();
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
  // A withdrawn prompt (vessel:prompt-closed without an answer) cancels the question, as in the real backend.
  void t.listen<{ id: number }>("vessel:prompt-closed", ({ id }) => {
    const r = waiting.get(id);
    waiting.delete(id);
    r?.({ type: "cancel" });
  });

  Object.assign(t.handlers, {
    app_info: () => ({
      version: "0.1.0-demo",
      config_dir: "~/.config/vessel",
      data_dir: "~/.local/share/vessel",
      log_path: "~/.local/state/vessel/vessel.log",
      db_path: "~/.local/share/vessel/vessel.db",
      legacy_state_dir: null,
    }),
    steam_shortcut_status: () => ({ accounts: 1, added: shortcut ? 1 : 0, steam_running: false, exe: "/home/deck/Applications/vessel.AppImage" }),
    steam_shortcut_set: (a: Args) => {
      shortcut = !!a!.enabled;
      return { accounts: 1, added: shortcut ? 1 : 0, steam_running: false, exe: "/home/deck/Applications/vessel.AppImage" };
    },
    quit_app: () => undefined,
    app_ready: () => ({ startup_check: { system_busy: null, pending: [] }, queued: 0 }),
    startup_check: () => null,
    prompt_answer: (a: Args) => {
      const r = waiting.get(a!.id as number);
      waiting.delete(a!.id as number);
      r?.(a!.answer as Answer);
      t.emit("vessel:prompt-closed", { id: a!.id as number });
      return !!r;
    },
    log_tail: () => ["demo backend: no log file"],
    config_list: () => config.map((c) => ({ ...c })),
    config_set: (a: Args) => {
      const c = config.find((x) => x.key === a!.key);
      if (!c) throw { message: `unknown config key '${String(a!.key)}'` };
      const v = String(a!.value);
      const bad =
        (c.options.length && !c.options.includes(v)) ||
        (c.kind === "number" && !/^\d+$/.test(v)) ||
        (c.kind === "path" && !/^[/~]/.test(v));
      if (bad) throw { message: `invalid value '${v}' for ${c.key}` };
      c.value = v;
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
    games_list: () =>
      games.map((g) => {
        const s = state.get(g.domain);
        const on = s ? s.mods.filter((m) => m.enabled) : [];
        const placed = s?.deployed ?? new Set<number>();
        return {
          ...g,
          deploy: { ...g.deploy, enabled_mods: on.length, deployed_mods: on.filter((m) => placed.has(m.id)).length },
        };
      }),
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
    // Generated portrait art for the first game; the others have none (monogram tiles), so both paths show.
    game_art: (a: Args) => {
      const g = games.find((x) => x.domain === a!.domain);
      if (!g) throw { message: `no game '${String(a!.domain)}'` };
      if (g.domain !== "skyrimspecialedition") return null;
      return a!.kind === "library_hero" ? placeholderHero(g.name) : placeholderArt(g.name);
    },
    open_folder: (a: Args) => {
      const g = games.find((x) => x.domain === a!.game);
      if (!g) throw { message: `no game '${String(a!.game)}'` };
      const p = a!.which === "prefix" ? g.prefix : a!.which === "game" ? g.folder : a!.which === "mods" ? `~/.local/share/vessel/${g.domain}/staging` : undefined;
      if (p === undefined) throw { message: `unknown folder '${String(a!.which)}'` };
      if (!p) throw { message: `${g.domain}: no ${a!.which === "prefix" ? "Proton prefix" : "game folder"} is set` };
      return p;
    },
    // Generated tile art for collections, except slugs containing "noart" (monogram), so both paths show.
    collection_art: (a: Args) => (String(a!.slug).includes("noart") ? null : placeholderTile(String(a!.slug))),
    game_launch: () =>
      job("launch", async (id) => {
        const g = active();
        say(id, `Launching ${g.name} (Steam app ${g.appid})...`);
        return { appid: g.appid, deployed: false };
      }),
    mods_list: (a: Args) => {
      const g = st(a);
      return { mods: g.mods.map((m) => ({ ...m })), changed_since_deploy: g.dirty };
    },
    mod_detail: (a: Args) => {
      const m = st(a).mods.find((x) => x.uid === a!.reference)!;
      return { mod: m, collections: [], fomod: [], wins: 0, loses: 0, url: null };
    },
    mods_set_enabled: (a: Args) =>
      job("enable", async () => {
        const g = st(a);
        g.mods.forEach((m) => (a!.ids as number[]).includes(m.id) && (m.enabled = a!.enabled as boolean));
        return changed(g);
      }),
    mod_move: (a: Args) =>
      job("move", async () => {
        const g = st(a);
        const mods = g.mods;
        const i = mods.findIndex((m) => m.uid === a!.reference);
        if (i < 0) throw { message: `no mod '${String(a!.reference)}'` };
        const j = a!.spec === "up" ? i - 1 : a!.spec === "down" ? i + 1 : i;
        if (j < 0 || j >= mods.length) throw { message: `${mods[i].name} is already ${j < 0 ? "first" : "last"}` };
        [mods[i], mods[j]] = [mods[j], mods[i]];
        renumber(g);
        return { uid: a!.reference, from: i + 1, to: j + 1, of: mods.length, after: changed(g) };
      }),
    thumbnail: () => null,
    install: (a: Args) =>
      job("install", async (id) => {
        const g = st(a);
        say(id, `Downloading ${a!.target}…`);
        const answer = await fomod(id, "Demo Mod");
        if (answer.type === "cancel") throw cancelled();
        if (!(await confirm(id, "Install the patch for SkyUI too?"))) {
          say(id, "Cancelled.");
          return { game: active().domain, installed: null, collection: null, after: afterChange({ needs_deploy: false }) };
        }
        const m = mod({ name: `Demo Mod ${a!.target}`, game: active().domain, installed_at: now(), position: g.mods.length + 1 });
        g.mods.push(m);
        renumber(g);
        say(id, `Installed ${m.name}`, "success");
        return {
          game: active().domain,
          installed: { mod_id: m.id, uid: m.uid, name: m.name, files: 3, size: 1000, fomod_found: true, replaced: null, warnings: [] },
          collection: null,
          after: changed(g, a!.deploy === true),
        };
      }),
    mod_reinstall: (a: Args) =>
      job("mod_reinstall", async (id) => {
        const g = st(a);
        const m = g.mods.find((x) => x.uid === a!.reference);
        if (!m) throw { message: `no mod '${String(a!.reference)}'` };
        if ((await fomod(id, m.name)).type === "cancel") throw cancelled();
        m.installed_at = now();
        say(id, `Reinstalled ${m.name}`, "success");
        return {
          installed: { mod_id: m.id, uid: m.uid, name: m.name, files: m.file_count, size: m.size, fomod_found: true, replaced: m.uid, warnings: [] },
          after: changed(g),
        };
      }),
    mods_uninstall: (a: Args) =>
      job("mods_uninstall", async (id) => {
        const refs = a!.refs as string[];
        const g = st(a);
        const gone = g.mods.filter((m) => refs.includes(m.uid));
        if (!gone.length) throw { message: `no mod matches ${refs.join(", ")}` };
        if (!(await confirm(id, `Uninstall ${gone.length} mod(s)?`))) {
          say(id, "Cancelled.");
          return { done: false, mods: 0, archives: [], after: afterChange({ needs_deploy: false }) };
        }
        gone.forEach((m) => g.mods.splice(g.mods.indexOf(m), 1));
        renumber(g);
        return { done: true, mods: gone.length, archives: [], after: changed(g) };
      }),
    nxm_submit: (a: Args) => ({ row: 1, link: String(a!.url).split("?")[0], kind: "mod" }),
    downloads_list: () => [],
    deploy: (a: Args) =>
      job("deploy", async (id) => {
        const g = st(a);
        for (let d = 0; d <= 20; d += 5) t.emit("vessel:deploy-progress", { job: id, done: d, total: 20 });
        g.dirty = false;
        g.deployed = new Set(g.mods.filter((m) => m.enabled).map((m) => m.id));
        return deployReport({ mods: g.mods.filter((m) => m.enabled).length });
      }),
    purge: (args: Args) =>
      job("purge", async (id) => {
        const a = await ask(id, { type: "confirm", question: "Remove every deployed file?", default_yes: false });
        if (a.type !== "confirm" || !a.yes) {
          say(id, "Cancelled.");
          return null;
        }
        st(args).deployed = new Set();
        return { nothing_deployed: false, removed: 20, restored: 0, missing: 0, changed: 0, kept: 0, recovered: null, plugins: null };
      }),
    verify: () => ({ intact: 20, issues: [], interrupted: false }),
    deploy_status: () => active().deploy,
    conflicts: (a: Args) => {
      const m = st(a).mods;
      return { files: m.length >= 2 ? [{ rel_path: "textures/sky.dds", winner: m[1], losers: [m[0]], plugin: false }] : [], plugins: [] };
    },
    deps: () => deps.map((d) => ({ ...d })),
    deps_ack: (a: Args) => setAck(String(a!.name), true),
    deps_unack: (a: Args) => setAck(String(a!.name), false),
    profiles_list: (a: Args) => st(a).profiles.map(({ set: _, ...p }) => ({ ...p })),
    profile_save: (a: Args) =>
      job("profile_save", async (id) => {
        const { mods, profiles } = st(a);
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
        const g = st(a);
        const mods = g.mods;
        const p = g.profiles.find((x) => x.name === a!.name);
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
        return { missing, added_disabled: extra.length, after: changed(g) };
      }),
    profile_delete: (a: Args) => {
      const profiles = st(a).profiles;
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
    collections_list: (a: Args) => {
      const { collections, mods } = st(a);
      return collections.map((c) => ({ record: { ...c }, enabled: mods.filter((m) => c.mods.includes(m.id) && m.enabled).length }));
    },
    collection_info: (a: Args) => {
      const slug = collectionSlug(String(a!.reference));
      const installed = st(a).collections.find((c) => c.slug === slug) ?? null;
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
        const g = st(a);
        // Mods the game already has (an earlier install, or another collection) are not added twice.
        const members: ModView[] = [];
        const added: ModView[] = [];
        for (const n of [1, 2]) {
          const name = `${slug} mod ${n}`;
          const have = g.mods.find((m) => m.name === name);
          if (have) members.push(have);
          else {
            const m = mod({ name, game: active().domain, installed_at: now(), position: g.mods.length + 1, collections: [slug] });
            g.mods.push(m);
            added.push(m);
            members.push(m);
          }
        }
        renumber(g);
        const rec: CollectionRecord = {
          id: ++collSeq,
          game: active().domain,
          slug,
          name: `Demo collection ${slug}`,
          author: "curator",
          revision: (a!.revision as number | null) ?? 3,
          dir: null,
          failed: [],
          installed_at: now(),
          mods: members.map((m) => m.id),
        };
        const old = g.collections.findIndex((c) => c.slug === slug);
        if (old >= 0) g.collections[old] = rec;
        else g.collections.push(rec);
        const deploy = a!.deploy === true;
        if (added.length || deploy) changed(g, deploy);
        return {
          ...base,
          installed: added.length,
          present: members.length - added.length,
          deployed: deploy ? deployReport({ mods: g.mods.filter((m) => m.enabled).length }) : null,
        };
      }),
    collection_update: (a: Args) =>
      job("collection_update", async () => {
        const slug = collectionSlug(String(a!.reference));
        const c = st(a).collections.find((x) => x.slug === slug);
        if (!c) throw { message: `collection ${slug} is not installed` };
        const up = c.revision >= 3;
        c.revision = 3;
        return { ...collectionResult(slug), up_to_date: up, present: c.mods.length };
      }),
    collection_remove: (a: Args) =>
      job("collection_remove", async (id) => {
        const slug = collectionSlug(String(a!.reference));
        const g = st(a);
        const { collections, mods } = g;
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
        renumber(g);
        return { removed: true, uninstalled, after: keep ? afterChange({ needs_deploy: false }) : changed(g) };
      }),
    updates_check: (a: Args) => ({ checked: st(a).mods.length, fetched: 0, stopped_early: false, updates: [], installed: 0 }),
    // Installs what updates_check reports (tests and the CT suites replace updates_check).
    updates_install: (a: Args) =>
      job("updates_install", async (id) => {
        const g = st(a);
        const rep = (await t.handlers.updates_check(a)) as UpdateReport;
        let installed = 0;
        for (const u of rep.updates) {
          if (u.pinned) continue;
          const m = g.mods.find((x) => x.id === u.mod.id);
          if (!m) continue;
          m.version = u.file.version ?? u.file.name;
          m.installed_at = now();
          installed++;
          say(id, `Updated ${m.name} to ${m.version}`, "success");
        }
        if (installed) changed(g);
        return { ...rep, updates: rep.updates.filter((u) => u.pinned), installed };
      }),
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
  // Poisoned backups, quarantines and restage (the first game starts with poisoned backups).
  Object.assign(
    t.handlers,
    repairHandlers(
      {
        job,
        say: (id, text) => say(id, text),
        domain: (a) => (typeof a?.game === "string" && a.game ? a.game : active().domain),
        mods: (a) => st(a).mods,
        changed: (a) => void (st(a).dirty = true),
      },
      { [games[0].domain]: demoPoisoned() },
    ),
  );
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

/** A flat 600x900 SVG poster with the game's name, as a data: URL (stands in for Steam's library art). */
export function placeholderArt(name: string): string {
  const esc = name.replace(/[<>&"']/g, (c) => `&#${c.charCodeAt(0)};`);
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="600" height="900" viewBox="0 0 600 900">` +
    `<rect width="600" height="900" fill="#1d2a33"/><rect x="40" y="40" width="520" height="820" fill="none" stroke="#3b5566" stroke-width="4"/>` +
    `<circle cx="300" cy="360" r="120" fill="#2c4250"/>` +
    `<text x="300" y="640" fill="#e4eef2" font-family="sans-serif" font-size="44" text-anchor="middle">${esc}</text></svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** A flat wide SVG standing in for Steam's hero art, as a data: URL. */
export function placeholderHero(name: string): string {
  const esc = name.replace(/[<>&"']/g, (c) => `&#${c.charCodeAt(0)};`);
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="1920" height="620" viewBox="0 0 1920 620">` +
    `<rect width="1920" height="620" fill="#22313b"/><circle cx="1500" cy="250" r="220" fill="#3b5566"/>` +
    `<rect x="0" y="470" width="1920" height="150" fill="#2c4250"/>` +
    `<text x="120" y="300" fill="#e4eef2" font-family="sans-serif" font-size="96">${esc}</text></svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** A flat square SVG standing in for a collection's Nexus tile image, as a data: URL. */
export function placeholderTile(slug: string): string {
  const esc = slug.slice(0, 8).replace(/[<>&"']/g, (c) => `&#${c.charCodeAt(0)};`);
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="320" height="320" viewBox="0 0 320 320">` +
    `<rect width="320" height="320" fill="#2a2f45"/><path d="M0 240 L110 130 L190 200 L250 150 L320 220 V320 H0Z" fill="#46507a"/>` +
    `<circle cx="235" cy="80" r="34" fill="#8a93c4"/>` +
    `<text x="24" y="60" fill="#e8eaf6" font-family="sans-serif" font-size="34">${esc}</text></svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** Slug from a collection URL, nxm link or bare slug. */
function collectionSlug(ref: string): string {
  const m = /collections\/([^/?#\s]+)/.exec(ref);
  return (m ? m[1] : ref.trim()) || "demo";
}
