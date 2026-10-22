// SPDX-License-Identifier: GPL-3.0-or-later
// The demo backend behind `vite dev` (and the CT/T16 browser suites): state
// persists, invalid commands are rejected like the real backend, and cancel
// follows the real backend's semantics.
import { describe, expect, it } from "vitest";
import type { JobEvent, PromptEvent } from "../ipc/events";
import type { Answer, InstallResult, ModsView, ReqRow } from "../ipc/types";
import { createDemoBackend } from "./demo";

function demo() {
  const t = createDemoBackend();
  const prompts: PromptEvent[] = [];
  const jobs: JobEvent[] = [];
  void t.listen<PromptEvent>("modman:prompt", (p) => prompts.push(p));
  void t.listen<JobEvent>("modman:job", (j) => jobs.push(j));
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

  // Fails when moving the first mod up corrupts the list (undefined entries).
  it("bounds-checks mod_move", async () => {
    const d = demo();
    const first = (await d.call<ModsView>("mods_list")).mods[0];
    await expect(d.call("mod_move", { reference: first.uid, spec: "up" })).rejects.toMatchObject({ message: /already first/ });
    const mods = (await d.call<ModsView>("mods_list")).mods;
    expect(mods.every(Boolean)).toBe(true);
    expect(mods.map((m) => m.position)).toEqual([1, 2, 3]);
  });

  it("has handlers for every command the views call", () => {
    const d = demo();
    for (const cmd of [
      "mod_reinstall",
      "mods_uninstall",
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
    d.t.emit("modman:prompt-closed", { id: d.prompts[0].id });
    expect(await r).toBeNull();
    expect(d.jobs[d.jobs.length - 1]).toMatchObject({ kind: "purge", state: "finished" });
  });
});
