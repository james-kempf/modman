// SPDX-License-Identifier: GPL-3.0-or-later
import { beforeEach, describe, expect, it } from "vitest";
import { setTransport } from "../ipc/transport";
import { EVENTS } from "../ipc/events";
import { createFakeTransport } from "../mock/fake";
import { resetBridge, startBridge } from "./bridge";
import { makeQueryClient } from "./queries";
import { resetUi, useUi } from "./store";
import { afterChange } from "../mock/fixtures";
import { afterChangeOf } from "./run";

beforeEach(() => {
  resetUi();
  resetBridge();
});

describe("bridge", () => {
  it("registers every event listener before calling app_ready, and calls it once", async () => {
    const order: string[] = [];
    const t = createFakeTransport({
      app_ready: () => {
        order.push(`ready after ${t.listened.length} listeners`);
        return { startup_check: { system_busy: null, pending: [] }, queued: 0 };
      },
    });
    setTransport(t);
    const qc = makeQueryClient();
    await Promise.all([startBridge(qc), startBridge(qc)]);
    await startBridge(qc);
    expect(t.callsOf("app_ready")).toHaveLength(1);
    expect(new Set(t.listened)).toEqual(new Set(Object.values(EVENTS)));
    expect(order).toEqual([`ready after ${Object.values(EVENTS).length} listeners`]);
    expect(useUi.getState().startupCheck).toEqual({ system_busy: null, pending: [] });
  });

  it("dedupes prompts re-emitted on app_ready by id; prompt-closed drops them", async () => {
    const t = createFakeTransport({ app_ready: () => ({ startup_check: null, queued: 0 }) });
    setTransport(t);
    await startBridge(makeQueryClient());
    const p = { id: 7, job: 1, prompt: { type: "confirm" as const, question: "?", default_yes: true } };
    t.emit("vessel:prompt", p);
    t.emit("vessel:prompt", p);
    expect(useUi.getState().prompts).toHaveLength(1);
    t.emit("vessel:prompt-closed", { id: 7 });
    expect(useUi.getState().prompts).toHaveLength(0);
  });

  it("tracks jobs and deploy progress from events", async () => {
    const t = createFakeTransport({ app_ready: () => ({ startup_check: null, queued: 0 }) });
    setTransport(t);
    await startBridge(makeQueryClient());
    t.emit("vessel:job", { id: 1, kind: "deploy", state: "started", error: null });
    t.emit("vessel:deploy-progress", { job: 1, done: 5, total: 10 });
    expect(useUi.getState().deployProgress?.done).toBe(5);
    t.emit("vessel:job", { id: 1, kind: "deploy", state: "failed", error: "boom" });
    expect(useUi.getState().jobs[1]).toMatchObject({ state: "failed", error: "boom" });
    expect(useUi.getState().deployProgress).toBeNull();
  });
});

describe("afterChangeOf", () => {
  it("finds AfterChange directly or nested under `after`", () => {
    const a = afterChange();
    expect(afterChangeOf(a)).toBe(a);
    expect(afterChangeOf({ uid: "x", after: a })).toBe(a);
    expect(afterChangeOf({ placed: 1 })).toBeNull();
    expect(afterChangeOf(null)).toBeNull();
  });
});
