// SPDX-License-Identifier: GPL-3.0-or-later
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ACTION_NOTICE_TTL, MAX_JOBS, resetUi, useUi } from "./store";

const texts = () => useUi.getState().notices.map((n) => n.text);

describe("notices (T16 F5)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useUi.setState({ notices: [] });
  });
  afterEach(() => vi.useRealTimers());

  it("collapses a repeated notice instead of stacking it", () => {
    useUi.getState().notify("warn", "Changes are not in the game yet.", "g");
    useUi.getState().notify("warn", "Changes are not in the game yet.", "g");
    expect(texts()).toEqual(["Changes are not in the game yet."]);
  });

  it("keeps at most 3 notices", () => {
    for (const t of ["a", "b", "c", "d"]) useUi.getState().notify("error", t);
    expect(texts()).toEqual(["b", "c", "d"]);
  });

  it("auto-dismisses info/success after 6 s and warn after 10 s", () => {
    useUi.getState().notify("success", "Deployed");
    useUi.getState().notify("info", "fyi");
    useUi.getState().notify("warn", "careful");
    vi.advanceTimersByTime(6_001);
    expect(texts()).toEqual(["careful"]);
    vi.advanceTimersByTime(4_000);
    expect(texts()).toEqual([]);
  });

  it("closes errors and deploy notices after their timeout too", () => {
    useUi.getState().notify("error", "boom");
    useUi.getState().notify("warn", "deploy?", "g");
    vi.advanceTimersByTime(9_999);
    expect(texts()).toEqual(["boom", "deploy?"]);
    vi.advanceTimersByTime(1);
    expect(texts()).toEqual([]);
  });

  // Fails when the replaced duplicate's timer dismisses the new one early.
  it("a repeated notice gets a fresh timeout", () => {
    useUi.getState().notify("info", "again");
    vi.advanceTimersByTime(5_000);
    useUi.getState().notify("info", "again");
    vi.advanceTimersByTime(5_000);
    expect(texts()).toEqual(["again"]);
  });
});

describe("notice actions (T03)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useUi.setState({ notices: [] });
  });
  afterEach(() => vi.useRealTimers());
  const only = () => useUi.getState().notices[0]!;

  // Fails when an action notice with a game is turned into a "Deploy now" one, or loses its game scope.
  it("keeps the action and game, and offers no Deploy now", () => {
    const onAction = vi.fn();
    useUi.getState().notify("success", "Uninstalled SkyUI", "g", { label: "Undo", onAction });
    expect(only()).toMatchObject({ game: "g", action: { label: "Undo" } });
    expect(only().deploy).toBeUndefined();
  });

  // Fails when the existing deploy notice shape changes (no action, deploy flag set).
  it("leaves deploy notices as they were", () => {
    useUi.getState().notify("warn", "Changes are not in the game yet.", "g");
    expect(only()).toEqual({ id: only().id, tone: "warn", text: "Changes are not in the game yet.", deploy: true, game: "g" });
  });

  // Fails when using the action does not dismiss first, runs twice, or does not call the handler.
  it("runNoticeAction dismisses the notice, then calls the action once", () => {
    const seen: number[] = [];
    const onAction = vi.fn(() => seen.push(useUi.getState().notices.length));
    useUi.getState().notify("info", "Removed", undefined, { label: "Undo", onAction });
    const id = only().id;
    useUi.getState().runNoticeAction(id);
    useUi.getState().runNoticeAction(id);
    expect(onAction).toHaveBeenCalledTimes(1);
    expect(seen).toEqual([0]);
    expect(texts()).toEqual([]);
  });

  // Fails when an Undo toast closes at the 6 s info TTL, before a pad user can reach it.
  it("stays up for ACTION_NOTICE_TTL, then closes without calling the action", () => {
    const onAction = vi.fn();
    useUi.getState().notify("info", "Removed", undefined, { label: "Undo", onAction });
    vi.advanceTimersByTime(ACTION_NOTICE_TTL - 1);
    expect(texts()).toEqual(["Removed"]);
    vi.advanceTimersByTime(1);
    expect(texts()).toEqual([]);
    expect(onAction).not.toHaveBeenCalled();
  });

  // Fails when the timer dismisses a notice that has focus (focus would drop out from under the user).
  it("waits while held and restarts the full TTL on release", () => {
    useUi.getState().notify("info", "Removed", undefined, { label: "Undo", onAction: () => {} });
    const id = only().id;
    vi.advanceTimersByTime(ACTION_NOTICE_TTL - 1000);
    useUi.getState().holdNotice(id, true);
    vi.advanceTimersByTime(ACTION_NOTICE_TTL * 3);
    expect(texts()).toEqual(["Removed"]);
    useUi.getState().holdNotice(id, false);
    vi.advanceTimersByTime(ACTION_NOTICE_TTL - 1);
    expect(texts()).toEqual(["Removed"]);
    vi.advanceTimersByTime(1);
    expect(texts()).toEqual([]);
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

  // D04: fails when a deploy of game B clears game A's pending state or its Deploy-now notice.
  it("a deploy clears only its own game and notices", () => {
    const ui = useUi.getState();
    ui.markDeployDue("a");
    ui.markDeployDue("b");
    ui.notify("warn", "Changes are not in the game yet.", "a");
    ui.notify("warn", "Changes are not in the game yet.", "b");
    useUi.getState().clearDeployDue("b");
    expect(Object.keys(useUi.getState().deployDue)).toEqual(["a"]);
    expect(useUi.getState().notices.map((n) => n.game)).toEqual(["a"]);
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
    expect(texts()).toEqual(["A question from job #70 closed: the job ended."]);
  });
});
