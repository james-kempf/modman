// SPDX-License-Identifier: GPL-3.0-or-later
// The demo backend behind `vite dev` (and the CT/T16 browser suites): state
// persists, invalid commands are rejected like the real backend, and cancel
// follows the real backend's semantics.
import { describe, expect, it } from "vitest";
import type { JobEvent, PromptEvent } from "../ipc/events";
import type {
  Answer,
  ApplyOutcome,
  ConfigEntry,
  GameView,
  InstallResult,
  LoaderCard,
  ModRestoreResult,
  ModsView,
  Preview,
  ReqRow,
  RestoreErrorKind,
  RunReport,
  UninstallResult,
} from "../ipc/types";
import { createDemoBackend, type DemoOptions } from "./demo";

function demo(opts: DemoOptions = {}) {
  const t = createDemoBackend(opts);
  const prompts: PromptEvent[] = [];
  const jobs: JobEvent[] = [];
  void t.listen<PromptEvent>("vessel:prompt", (p) => prompts.push(p));
  void t.listen<JobEvent>("vessel:job", (j) => jobs.push(j));
  const call = <T>(cmd: string, args?: Record<string, unknown>) => t.invoke<T>(cmd, args);
  const tick = () => new Promise((r) => setTimeout(r, 0));
  /** Answer the newest prompt. */
  const answer = async (a: Answer) => {
    await tick();
    await call("prompt_answer", { id: prompts[prompts.length - 1].id, answer: a });
  };
  return { t, prompts, jobs, call, answer, tick };
}

describe("demo backend", () => {
  // Fails when acknowledging an external requirement is forgotten on the next deps read.
  it("persists deps_ack / deps_unack", async () => {
    const d = demo();
    await d.call("deps_ack", { name: "skse-loader" });
    expect((await d.call<ReqRow[]>("deps")).find((r) => r.key === "skse-loader")?.acked).toBe(true);
    await d.call("deps_unack", { name: "skse-loader" });
    expect((await d.call<ReqRow[]>("deps")).find((r) => r.key === "skse-loader")?.acked).toBe(false);
  });

  // Fails when the demo shows only one art path (both art tiles and monogram tiles must be reachable in the browser suites).
  it("game_art: generated art for one game, none for another, rejects unknown games", async () => {
    const d = demo();
    expect(await d.call<string | null>("game_art", { game: "skyrimspecialedition" })).toMatch(/^data:image\/svg\+xml;/);
    expect(await d.call("game_art", { game: "cyberpunk2077" })).toBeNull();
    await expect(d.call("game_art", { game: "nope" })).rejects.toEqual({ message: "no game 'nope'" });
  });

  // Fails when the browser suites lose a U02 path: hero art, folder links, collection art/monogram, config kinds, tracker counts.
  it("U02 commands: hero art, open_folder, collection_art, config kinds, deploy counts", async () => {
    const d = demo();
    expect(await d.call<string>("game_art", { game: "skyrimspecialedition", kind: "library_hero" })).toMatch(/width%3D%221920%22/);
    expect(await d.call("open_folder", { game: "skyrimspecialedition", which: "prefix" })).toMatch(/pfx$/);
    await expect(d.call("open_folder", { game: "cyberpunk2077", which: "prefix" })).rejects.toHaveProperty("message");
    await expect(d.call("open_folder", { game: "skyrimspecialedition", which: "/etc" })).rejects.toHaveProperty("message");
    expect(await d.call("collection_art", { game: "skyrimspecialedition", slug: "abc" })).toMatch(/^data:image\//);
    expect(await d.call("collection_art", { game: "skyrimspecialedition", slug: "noart1" })).toBeNull();
    const cfg = await d.call<ConfigEntry[]>("config_list");
    expect(cfg).toHaveLength(9);
    expect(cfg.find((c) => c.key === "fomod")).toMatchObject({ kind: "enum", options: ["auto", "ask", "default"] });
    await expect(d.call("config_set", { key: "fomod", value: "none" })).rejects.toHaveProperty("message");
    const sky = async () => (await d.call<GameView[]>("games_list")).find((g) => g.id === "skyrimspecialedition")!.deploy;
    expect(await sky()).toMatchObject({ enabled_mods: 2, deployed_mods: 2 });
    const mods = (await d.call<ModsView>("mods_list")).mods;
    await d.call("mods_set_enabled", { ids: [mods[2].id], enabled: true });
    expect(await sky()).toMatchObject({ enabled_mods: 3, deployed_mods: 2 });
    await d.call("deploy", {});
    expect(await sky()).toMatchObject({ enabled_mods: 3, deployed_mods: 3 });
  });

  it("persists config_set", async () => {
    const d = demo();
    await d.call("config_set", { key: "deploy-method", value: "copy" });
    expect(await d.call("config_list")).toContainEqual(expect.objectContaining({ key: "deploy-method", value: "copy" }));
  });

  // Fails when the same game can be added twice.
  it("rejects a duplicate game_add", async () => {
    const d = demo();
    await d.call("game_add", { domain: "fallout4", path: null });
    await expect(d.call("game_add", { domain: "fallout4", path: null })).rejects.toMatchObject({ message: /already added/ });
  });

  // Fails when deploy:true is ignored, or a new mod gets a fixed old timestamp / wrong position.
  it("install honours deploy:true and stamps the mod now", async () => {
    const d = demo();
    const r = d.call<InstallResult>("install", { target: "266", fomod: "ask", deploy: true });
    await d.answer({ type: "choices", indices: [1] });
    await d.answer({ type: "confirm", yes: true });
    const res = await r;
    expect(res.after.deployed).not.toBeNull();
    expect(res.after.needs_deploy).toBe(false);
    const mods = (await d.call<ModsView>("mods_list")).mods;
    const added = mods[mods.length - 1];
    expect(added.installed_at).toBeGreaterThan(Date.now() / 1000 - 60);
    expect(added.position).toBe(mods.length);
  });

  // Fails when a bad move corrupts the list (undefined entries) or after <next> does not move the row down.
  it("bounds-checks mod_move", async () => {
    const d = demo();
    const [first, second] = (await d.call<ModsView>("mods_list")).mods;
    await expect(d.call("mod_move", { reference: first.uid, spec: "before", arg: "nope" })).rejects.toMatchObject({ message: /bad move/ });
    let mods = (await d.call<ModsView>("mods_list")).mods;
    expect(mods.every(Boolean)).toBe(true);
    expect(mods.map((m) => m.position)).toEqual([1, 2, 3]);
    await expect(d.call("mod_move", { reference: first.uid, spec: "after", arg: second.uid })).resolves.toMatchObject({ from: 1, to: 2 });
    mods = (await d.call<ModsView>("mods_list")).mods;
    expect(mods.map((m) => m.uid).slice(0, 2)).toEqual([second.uid, first.uid]);
  });

  // Fails when the demo rejects the move-to-position specs the views send (first, last, #N), or drifts from core clamping.
  it("mod_move accepts first, last and #N like core move_mod", async () => {
    const d = demo();
    const uids = async () => (await d.call<ModsView>("mods_list")).mods.map((m) => m.uid);
    const [a, b, c] = await uids();
    await expect(d.call("mod_move", { reference: a, spec: "last", arg: null })).resolves.toMatchObject({ from: 1, to: 3, of: 3 });
    expect(await uids()).toEqual([b, c, a]);
    await expect(d.call("mod_move", { reference: a, spec: "first", arg: null })).resolves.toMatchObject({ from: 3, to: 1 });
    expect(await uids()).toEqual([a, b, c]);
    await expect(d.call("mod_move", { reference: a, spec: "#2", arg: null })).resolves.toMatchObject({ from: 1, to: 2 });
    expect(await uids()).toEqual([b, a, c]);
    // Out of range clamps to the end, as in core.
    await expect(d.call("mod_move", { reference: b, spec: "#99", arg: null })).resolves.toMatchObject({ from: 1, to: 3 });
    expect(await uids()).toEqual([a, c, b]);
    expect((await d.call<ModsView>("mods_list")).mods.map((m) => m.position)).toEqual([1, 2, 3]);
    await expect(d.call("mod_move", { reference: a, spec: "#x", arg: null })).rejects.toMatchObject({ message: /bad move/ });
  });

  // Fails when profile_load only flips enabled flags and leaves the load order as it is (core reorders).
  it("profile_load restores the saved order; mods not in the profile follow, disabled", async () => {
    const d = demo();
    const [a, b, c] = (await d.call<ModsView>("mods_list")).mods;
    await d.call("mod_move", { reference: c.uid, spec: "first", arg: null });
    await d.call("profile_save", { name: "evening" });
    await d.call("mod_move", { reference: c.uid, spec: "last", arg: null });
    await d.call("mods_set_enabled", { ids: [a.id], enabled: false });
    // A mod installed after the save is not in the profile.
    await d.call("install", { target: "/home/deck/Downloads/Extra.7z", name: "Extra" });
    expect(await d.call("profile_load", { name: "evening" })).toMatchObject({ missing: 0, added_disabled: 1 });
    const after = (await d.call<ModsView>("mods_list")).mods;
    expect(after.map((m) => m.uid).slice(0, 3)).toEqual([c.uid, a.uid, b.uid]);
    expect(after.map((m) => m.position)).toEqual([1, 2, 3, 4]);
    expect(after.find((m) => m.uid === a.uid)?.enabled).toBe(true);
    expect(after.slice(3).every((m) => !m.enabled)).toBe(true);
  });

  // Fails when overwriting a profile defaults to No (core's save_profile confirm defaults to yes).
  it("profile_save asks before overwriting, defaulting to yes", async () => {
    const d = demo();
    await d.call("profile_save", { name: "evening" });
    expect(d.prompts).toHaveLength(0);
    const r = d.call("profile_save", { name: "evening" });
    await d.tick();
    expect(d.prompts[d.prompts.length - 1].prompt).toMatchObject({ type: "confirm", default_yes: true });
    await d.answer({ type: "confirm", yes: true });
    expect(await r).toBe(3);
  });

  // Fails when installing a local archive always runs the FOMOD and patch prompts, or ignores the given name.
  it("install from an archive path needs no prompt, uses the name and returns installed", async () => {
    const d = demo();
    const r = await d.call<InstallResult>("install", { target: "/home/deck/Downloads/Better Lamps.7z", name: "Better Lamps" });
    expect(d.prompts).toHaveLength(0);
    expect(r.installed).toMatchObject({ name: "Better Lamps" });
    const added = (await d.call<ModsView>("mods_list")).mods.find((m) => m.uid === r.installed!.uid);
    expect(added).toMatchObject({ name: "Better Lamps", archive: "/home/deck/Downloads/Better Lamps.7z", nexus_mod_id: null });
  });

  // Fails when demo mods have no archive (Undo of an uninstall would have nothing to reinstall from), or no folder install remains.
  it("fixture mods expose archive paths, plus one folder install without one", async () => {
    const d = demo();
    const mods = (await d.call<ModsView>("mods_list")).mods;
    const folder = mods.filter((m) => m.archive === null);
    expect(folder).toHaveLength(1);
    expect(mods.filter((m) => m.archive !== null).every((m) => /\.(7z|zip|rar)$/.test(m.archive!))).toBe(true);
    expect(mods.length - folder.length).toBeGreaterThanOrEqual(2);
  });

  it("has handlers for every command the views call", () => {
    const d = demo();
    for (const cmd of [
      "mod_reinstall",
      "mods_uninstall",
      "mod_restore",
      "profile_save",
      "profile_load",
      "profile_delete",
      "collection_info",
      "collection_install",
      "collection_update",
      "collection_remove",
      "updates_install",
    ])
      expect(d.t.handlers[cmd], cmd).toBeTypeOf("function");
  });

  // Fails when the demo's uninstall confirm drifts from the backend's danger confirm, or ignores option_checked.
  it("mods_uninstall asks the danger confirm and deletes archives only when the option is ticked", async () => {
    const d = demo();
    const [a, b] = (await d.call<ModsView>("mods_list")).mods;
    const kept = d.call("mods_uninstall", { refs: [a.uid], delete_archive: false });
    await d.answer({ type: "confirm", yes: true });
    expect(await kept).toMatchObject({ done: true, mods: 1, archives: [] });
    expect(d.prompts[d.prompts.length - 1].prompt).toEqual({
      type: "confirm",
      question: "Uninstall 1 mod(s)?",
      default_yes: true,
      title: `Uninstall ${a.name}?`,
      yes_label: `Uninstall ${a.name}`,
      no_label: "Keep mod",
      danger: true,
      option: "Delete archive too",
    });
    const deleted = d.call("mods_uninstall", { refs: [b.uid], delete_archive: false });
    await d.answer({ type: "confirm", yes: true, option_checked: true });
    expect(await deleted).toMatchObject({ done: true, mods: 1, archives: [expect.stringContaining(b.uid)] });
  });

  it("profiles save, list, load and delete", async () => {
    const d = demo();
    await d.call("profile_save", { name: "evening" });
    expect(await d.call("profiles_list")).toEqual([expect.objectContaining({ name: "evening", mods: 3 })]);
    await d.call("profile_load", { name: "evening" });
    await d.call("profile_delete", { name: "evening" });
    expect(await d.call("profiles_list")).toEqual([]);
  });

  it("collections install, list and remove", async () => {
    const d = demo();
    const r = d.call("collection_install", { reference: "https://next.nexusmods.com/x/collections/abc", deploy: false });
    await d.answer({ type: "confirm", yes: true });
    expect(await r).toMatchObject({ slug: "abc", installed: 2, cancelled: false });
    expect(await d.call("collections_list")).toEqual([expect.objectContaining({ enabled: 2 })]);
    const rm = d.call("collection_remove", { reference: "abc", keep_mods: false });
    await d.answer({ type: "confirm", yes: true });
    expect(await rm).toMatchObject({ removed: true, uninstalled: 2 });
  });

  // D17: the real backend fails the job with "FOMOD installer cancelled" on a cancelled wizard.
  it("a cancelled FOMOD wizard fails the job with the real backend's message", async () => {
    const d = demo();
    const r = d.call("install", { target: "1", fomod: "ask" });
    await d.answer({ type: "cancel" });
    await expect(r).rejects.toMatchObject({ message: "FOMOD installer cancelled" });
    expect(d.jobs[d.jobs.length - 1]).toMatchObject({ state: "failed", error: "FOMOD installer cancelled" });
  });

  // Fails when a withdrawn prompt (prompt-closed without an answer) leaves its job running forever.
  it("a withdrawn prompt resolves its job as cancelled", async () => {
    const d = demo();
    const r = d.call("purge");
    await d.tick();
    d.t.emit("vessel:prompt-closed", { id: d.prompts[0].id });
    expect(await r).toBeNull();
    expect(d.jobs[d.jobs.length - 1]).toMatchObject({ kind: "purge", state: "finished" });
  });

  // CT2 D16: fails when a reinstalled collection adds its mods a second time.
  it("collection_install skips mods already present", async () => {
    const d = demo();
    for (let i = 0; i < 2; i++) {
      const r = d.call("collection_install", { reference: "abc", deploy: false });
      await d.answer({ type: "confirm", yes: true });
      await r;
    }
    const names = (await d.call<ModsView>("mods_list")).mods.map((m) => m.name);
    expect(names.filter((n) => n.startsWith("abc mod"))).toEqual(["abc mod 1", "abc mod 2"]);
  });

  // CT2 D16: fails when updates_install ignores the updates that updates_check reported.
  it("updates_install installs the reported updates", async () => {
    const d = demo();
    const m = (await d.call<ModsView>("mods_list")).mods[0];
    d.t.handlers.updates_check = () => ({
      checked: 3, fetched: 1, stopped_early: false, installed: 0,
      updates: [{ mod: m, file: { file_id: 9, name: "SkyUI 6.0", version: "6.0" }, pinned: false }],
    });
    expect(await d.call("updates_install", {})).toMatchObject({ installed: 1, updates: [] });
    const after = await d.call<ModsView>("mods_list");
    expect(after.mods[0].version).toBe("6.0");
    expect(after.changed_since_deploy).toBe(true);
  });

  // CT2 D16: fails when mods, the changed-since-deploy flag or collections are shared by every game.
  it("keeps mods, the dirty flag and collections per game", async () => {
    const d = demo();
    const sky = (await d.call<ModsView>("mods_list")).mods;
    await d.call("mods_set_enabled", { ids: [sky[0].id], enabled: false });
    expect((await d.call<ModsView>("mods_list", { game: "skyrimspecialedition" })).changed_since_deploy).toBe(true);
    const cp = await d.call<ModsView>("mods_list", { game: "cyberpunk2077" });
    expect(cp).toEqual({ mods: [], changed_since_deploy: false, loaders: [] });
    await d.call("deploy", { game: "cyberpunk2077" });
    expect((await d.call<ModsView>("mods_list")).changed_since_deploy).toBe(true);
    expect(await d.call("collections_list", { game: "cyberpunk2077" })).toEqual([]);
  });

  // Fails when the demo's loader flow drifts from the core contract: a loader download must give a pending
  // preview (no mod), a run must use the reviewed token once, and the card must reflect the result.
  it("loaders: install gives a pending preview; the reviewed token runs once and updates the card", async () => {
    const d = demo();
    const before = (await d.call<ModsView>("mods_list")).mods.length;
    const r = await d.call<InstallResult>("install", { target: "30379" });
    expect(r.installed).toBeNull();
    expect(r.loader).toMatchObject({ loader_key: "skse", op: "install" });
    expect((await d.call<ModsView>("mods_list")).mods).toHaveLength(before);
    const p = await d.call<Preview>("loader_preview_reopen", { preview_id: r.loader!.preview_id, unverified_ack: false });
    await expect(d.call("loader_run", { preview_id: r.loader!.preview_id, plan_hash: r.loader!.plan_hash })).rejects.toHaveProperty("code", "loader_consent");
    expect(await d.call<RunReport>("loader_run", { preview_id: p.preview_id, plan_hash: p.plan_hash })).toMatchObject({ outcome: { kind: "succeeded" } });
    await expect(d.call("loader_run", { preview_id: p.preview_id, plan_hash: p.plan_hash })).rejects.toHaveProperty("code", "loader_consent");
    const [card] = await d.call<LoaderCard[]>("loader_status");
    expect(card.status).toMatchObject({ state: "installed" });
    expect(card.launch.desired).toBeTruthy();
    expect(await d.call<ApplyOutcome>("launch_options_apply", { account: null })).toMatchObject({ kind: "choose_account" });
    expect(await d.call("loader_junk_remove", { mod_ids: [9001] })).toBe(1);
    expect(await d.call("loader_junk_scan")).toEqual([]);
  });

  describe("mod_restore (Undo of an uninstall)", () => {
    /** Uninstall a mod by uid, answering the confirm (option = "Delete archive too"). */
    const uninstall = async (d: ReturnType<typeof demo>, uid: string, deleteArchive = false) => {
      const r = d.call<UninstallResult>("mods_uninstall", { refs: [uid], delete_archive: false });
      await d.answer({ type: "confirm", yes: true, option_checked: deleteArchive });
      return r;
    };
    const list = async (d: ReturnType<typeof demo>) => (await d.call<ModsView>("mods_list")).mods;

    // Fails when Undo brings the mod back changed: another uid, enabled state or position.
    it("round-trips uninstall -> mod_restore(token): same uid, enabled state and position; the token is used up", async () => {
      const d = demo();
      const [, b] = await list(d);
      await d.call("mods_set_enabled", { ids: [b.id], enabled: false });
      const u = await uninstall(d, b.uid);
      expect(u.restore).toEqual([{ token: expect.any(String), uid: b.uid, name: b.name, restorable: true, reason: null }]);
      expect((await list(d)).map((m) => m.uid)).not.toContain(b.uid);
      const r = await d.call<ModRestoreResult>("mod_restore", { tokens: [u.restore![0].token] });
      expect(r).toMatchObject({ restored: 1, results: [{ uid: b.uid, name: b.name, ok: true, error: null, warnings: [] }] });
      expect((await list(d))[1]).toMatchObject({ uid: b.uid, name: b.name, enabled: false, position: 2, archive: b.archive });
      expect(d.jobs.some((j) => j.kind === "mod_restore" && j.state === "finished")).toBe(true);
      // Undo pressed twice: the used token is expired.
      const again = await d.call<ModRestoreResult>("mod_restore", { tokens: [u.restore![0].token] });
      expect(again).toMatchObject({ restored: 0, results: [{ uid: null, ok: false, error: { kind: "expired" } }] });
    });

    // Fails when the demo marks a mod restorable that the backend would not, or keeps a token for it.
    it("not restorable: folder install, deleted archive, archive outside downloads (restore gives expired)", async () => {
      const d = demo();
      const mods = await list(d);
      const folder = mods.find((m) => m.archive === null)!;
      const outside = d.call<InstallResult>("install", { target: "/home/deck/Downloads/Lamps.7z", deploy: false });
      const lamps = (await outside).installed!.uid;
      const cases = [
        [await uninstall(d, folder.uid), "folder_install"],
        [await uninstall(d, mods[0].uid, true), "archive_deleted"],
        [await uninstall(d, lamps), "outside_downloads"],
      ] as const;
      for (const [u, reason] of cases) {
        expect(u.restore).toEqual([expect.objectContaining({ restorable: false, reason })]);
        const r = await d.call<ModRestoreResult>("mod_restore", { tokens: [u.restore![0].token] });
        expect(r.results[0].error?.kind).toBe("expired");
      }
    });

    // Fails when an injected failure is not reported per kind, or uses up the token (no retry).
    it.each<RestoreErrorKind>(["archive_missing", "conflict", "install_failed"])(
      "injected %s fails that mod and keeps the token for a retry",
      async (kind) => {
        const restoreFailures = new Map<string, RestoreErrorKind>();
        const d = demo({ restoreFailures });
        const [a] = await list(d);
        const u = await uninstall(d, a.uid);
        restoreFailures.set(a.uid, kind);
        const token = u.restore![0].token;
        const r = await d.call<ModRestoreResult>("mod_restore", { tokens: [token] });
        expect(r).toMatchObject({ restored: 0, results: [{ uid: a.uid, name: a.name, ok: false, error: { kind } }] });
        expect((await list(d)).map((m) => m.uid)).not.toContain(a.uid);
        restoreFailures.clear();
        expect((await d.call<ModRestoreResult>("mod_restore", { tokens: [token] })).restored).toBe(1);
        expect((await list(d))[0].uid).toBe(a.uid);
      },
    );
  });
});
