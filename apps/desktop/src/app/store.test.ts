// SPDX-License-Identifier: GPL-3.0-or-later
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MAX_JOBS, TOAST_MIN_DWELL_MS, resetUi, useUi } from "./store";

const texts = () => [useUi.getState().toast].flatMap((x) => (x ? [x] : [])).map((n) => n.text);

describe("notices (T16 F5)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    resetUi();
  });
  afterEach(() => vi.useRealTimers());
  const D = TOAST_MIN_DWELL_MS;

  it("collapses a repeated notice instead of stacking it", () => {
    useUi.getState().showToast("warn", "Not deployed", { detail: "Changes are not in the game yet." });
    vi.advanceTimersByTime(D);
    useUi.getState().showToast("warn", "Not deployed", { detail: "Changes are not in the game yet." });
    expect(texts()).toEqual(["Not deployed"]);
  });

  // Fails when the store keeps several shown notices again: there is one shown toast; waiting writes show
  // one after another, in arrival order.
  it("holds one shown notice; rapid writes show in order", () => {
    for (const t of ["a", "b", "c", "d"]) useUi.getState().showToast("error", t);
    const seen = [texts()[0]];
    for (let i = 0; i < 3; i++) {
      vi.advanceTimersByTime(D);
      seen.push(texts()[0]);
    }
    expect(seen).toEqual(["a", "b", "c", "d"]);
  });

  // Fails when a TTL changes, or the replacing notice keeps the old one's tone timing.
  it("auto-dismisses info/success after 6 s and warn/error after 10 s, by the shown notice's tone", () => {
    useUi.getState().showToast("success", "Deployed");
    vi.advanceTimersByTime(5_999);
    expect(texts()).toEqual(["Deployed"]);
    vi.advanceTimersByTime(1);
    expect(texts()).toEqual([]);
    useUi.getState().showToast("info", "fyi");
    vi.advanceTimersByTime(D);
    useUi.getState().showToast("error", "boom");
    vi.advanceTimersByTime(9_999);
    expect(texts()).toEqual(["boom"]);
    vi.advanceTimersByTime(1);
    expect(texts()).toEqual([]);
  });

  // Fails when the replaced notice's timer still runs and closes the newer one early.
  it("a stale timer never closes the newer notice", () => {
    useUi.getState().showToast("info", "first");
    const firstId = useUi.getState().toast!.id;
    vi.advanceTimersByTime(5_000);
    useUi.getState().showToast("warn", "second");
    // The first notice's 6 s mark passes: the second (10 s from its own start) stays.
    vi.advanceTimersByTime(1_500);
    expect(texts()).toEqual(["second"]);
    // A late dismiss for the replaced id is a no-op.
    useUi.getState().closeToast(firstId);
    expect(texts()).toEqual(["second"]);
    vi.advanceTimersByTime(8_499);
    expect(texts()).toEqual(["second"]);
    vi.advanceTimersByTime(1);
    expect(texts()).toEqual([]);
  });

  // Fails when the replaced duplicate's timer dismisses the new one early.
  it("a repeated notice gets a fresh timeout", () => {
    useUi.getState().showToast("info", "again");
    vi.advanceTimersByTime(5_000);
    useUi.getState().showToast("info", "again");
    vi.advanceTimersByTime(5_000);
    expect(texts()).toEqual(["again"]);
  });
});

describe("notices are informational only (T05)", () => {
  beforeEach(() => resetUi());

  // Fails when a notice carries a control again (an action, a Deploy now flag or a game scope for one).
  it("hold only tone and text", () => {
    useUi.getState().showToast("success", "Uninstalled SkyUI");
    const n = [useUi.getState().toast].flatMap((x) => (x ? [x] : []))[0]!;
    expect(n).toEqual({ id: n.id, tone: "success", text: "Uninstalled SkyUI" });
    const api = useUi.getState() as unknown as Record<string, unknown>;
    expect(api.runNoticeAction).toBeUndefined();
    expect(api.holdNotice).toBeUndefined();
  });
});

describe("CT2 per-game deploy-due state (D02/D04)", () => {
  beforeEach(() => resetUi());

  // D02: fails when deploy-due is one global flag (a change in game A shows as due in game B).
  it("is kept per game", () => {
    useUi.getState().markDeployDue("a");
    expect(useUi.getState().deployDue.a).toBeDefined();
    expect(useUi.getState().deployDue.b).toBeUndefined();
  });

  // D04: fails when a deploy of game B clears game A's pending state.
  it("a deploy clears only its own game", () => {
    const ui = useUi.getState();
    ui.markDeployDue("a");
    ui.markDeployDue("b");
    useUi.getState().clearDeployDue("b");
    expect(Object.keys(useUi.getState().deployDue)).toEqual(["a"]);
  });

  // D04: fails when a deploy that started before a change clears the change's pending state.
  it("a change made while the deploy ran stays pending", () => {
    const since = useUi.getState().stamp();
    useUi.getState().markDeployDue("a");
    useUi.getState().clearDeployDue("a", since);
    expect(useUi.getState().deployDue.a).toBeDefined();
    const later = useUi.getState().stamp();
    useUi.getState().clearDeployDue("a", later);
    expect(useUi.getState().deployDue.a).toBeUndefined();
  });
});

describe("CT2 jobs and prompts (D08/D15)", () => {
  beforeEach(() => resetUi());

  // D08: fails when the jobs map grows without bound over a long session.
  it("caps jobs, dropping the oldest finished ones and keeping running ones", () => {
    const ui = useUi.getState();
    ui.onJob({ id: 1, kind: "deploy", state: "started", error: null });
    for (let i = 2; i <= MAX_JOBS + 50; i++) {
      ui.onJob({ id: i, kind: "enable", state: "started", error: null });
      ui.onJob({ id: i, kind: "enable", state: "finished", error: null });
    }
    const ids = Object.keys(useUi.getState().jobs).map(Number);
    expect(ids).toHaveLength(MAX_JOBS);
    expect(ids).toContain(1);
    expect(ids).toContain(MAX_JOBS + 50);
    expect(ids).not.toContain(2);
  });

  // D15: fails when a prompt of a job that already failed stays open (its answer goes nowhere).
  it("withdraws the prompts of a job that ended, with a notice", async () => {
    const ui = useUi.getState();
    ui.onPrompt({ id: 7, job: 70, prompt: { type: "confirm", question: "Overwrite?", default_yes: true } });
    ui.onPrompt({ id: 8, job: 80, prompt: { type: "confirm", question: "Other?", default_yes: true } });
    ui.onJob({ id: 70, kind: "install", state: "failed", error: "network went away" });
    expect(useUi.getState().prompts.map((p) => p.id)).toEqual([8]);
    await Promise.resolve();
    // The job's own outcome shows first (arrival order); the closed-question notice follows after its dwell.
    expect(texts()).toEqual(["Install failed"]);
    await new Promise((r) => setTimeout(r, TOAST_MIN_DWELL_MS + 20));
    expect(texts()).toEqual(["Question closed"]);
    expect(useUi.getState().toast?.detail).toBe("Job #70 ended, so its question was closed");
  });
});
