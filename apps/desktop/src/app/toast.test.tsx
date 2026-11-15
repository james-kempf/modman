// SPDX-License-Identifier: GPL-3.0-or-later
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TOAST_MIN_DWELL_MS, resetUi, useUi } from "./store";
import { showToast, useToast } from "./toast";

const current = () => useUi.getState().toast;
const line = (job: number, text: string) =>
  useUi.getState().onActivity({ job, msg: { level: "info", spans: [{ text, style: "plain" }] }, text });

beforeEach(() => {
  vi.useFakeTimers();
  resetUi();
});
afterEach(() => vi.useRealTimers());

describe("showToast from any component (one state, last write wins)", () => {
  function Writer({ name, text }: { name: string; text: string }) {
    return (
      <button type="button" onClick={() => showToast("info", text)}>
        {name}
      </button>
    );
  }
  function Reader() {
    const t = useToast();
    return <output>{t?.text ?? "none"}</output>;
  }

  // Fails when a second writer waits for the first message to expire, or keeps the first one's timer.
  it("two components writing back to back: the content swaps at once and the timer restarts", () => {
    render(
      <>
        <Writer name="A" text="from A" />
        <Writer name="B" text="from B" />
        <Reader />
      </>,
    );
    fireEvent.click(screen.getByRole("button", { name: "A" }));
    expect(screen.getByRole("status")).toHaveTextContent("from A");
    act(() => vi.advanceTimersByTime(5_000));
    fireEvent.click(screen.getByRole("button", { name: "B" }));
    expect(screen.getByRole("status")).toHaveTextContent("from B");
    // A's 6 s mark passes; B has its own full 6 s from its write.
    act(() => vi.advanceTimersByTime(1_500));
    expect(screen.getByRole("status")).toHaveTextContent("from B");
    act(() => vi.advanceTimersByTime(4_499));
    expect(screen.getByRole("status")).toHaveTextContent("from B");
    act(() => vi.advanceTimersByTime(1));
    expect(screen.getByRole("status")).toHaveTextContent("none");
  });
});

describe("live (persistent) job toasts (T16)", () => {
  const start = (id: number, kind = "deploy") => useUi.getState().onJob({ id, kind, state: "started", error: null });
  const end = (id: number, kind = "deploy", state: "finished" | "failed" = "finished", error: string | null = null) =>
    useUi.getState().onJob({ id, kind, state, error });

  // Fails when progress goes through the FIFO/dwell (lagging) or a progress update re-creates the toast.
  it("updates in place at once: same id and key, no dwell, no delay", () => {
    start(1);
    const first = current()!;
    expect(first).toMatchObject({ text: "Deploying", detail: "Deploy", busy: true, persist: "job:1" });
    line(1, "a");
    expect(current()).toMatchObject({ id: first.id, persist: "job:1", detail: "a" });
    line(1, "b");
    useUi.getState().onDeployProgress({ job: 1, done: 2, total: 9 });
    expect(current()).toMatchObject({ id: first.id, detail: "2 / 9 files" });
  });

  // Fails when a running job's toast auto-closes (it must stay until the job ends).
  it("never auto-closes while the job runs", () => {
    start(1);
    vi.advanceTimersByTime(60_000);
    expect(current()).toMatchObject({ text: "Deploying" });
    useUi.getState().closeToast(current()!.id); // a stray close does not end it either
    expect(current()).toMatchObject({ text: "Deploying" });
  });

  // Fails when the outcome does not replace the live toast, or the outcome is not a normal (TTL) toast.
  it("on completion the outcome replaces it and closes on its TTL", () => {
    start(1, "purge");
    vi.advanceTimersByTime(TOAST_MIN_DWELL_MS);
    end(1, "purge");
    expect(current()).toMatchObject({ tone: "success", text: "Purged", detail: "Purge" });
    expect(current()!.persist).toBeUndefined();
    vi.advanceTimersByTime(6_000);
    expect(current()).toBeNull();
  });

  // Fails when failure or cancellation leaves the live toast running.
  it("failure and cancellation end it", () => {
    start(1, "install");
    vi.advanceTimersByTime(TOAST_MIN_DWELL_MS);
    end(1, "install", "failed", "disk full");
    expect(current()).toMatchObject({ tone: "error", text: "Install failed", detail: "disk full · see Activity" });
    start(2, "install");
    vi.advanceTimersByTime(6_000 + TOAST_MIN_DWELL_MS);
    end(2, "install", "failed", "FOMOD installer cancelled");
    vi.advanceTimersByTime(TOAST_MIN_DWELL_MS);
    expect(current()).toMatchObject({ tone: "info", text: "Install cancelled" });
    vi.advanceTimersByTime(6_000);
    expect(current()).toBeNull();
  });

  // Fails when a transient waits behind the live toast, the live toast is lost, or it returns stale.
  it("a transient takes the slot; the live toast returns with its latest content", () => {
    start(1);
    vi.advanceTimersByTime(TOAST_MIN_DWELL_MS);
    showToast("success", "Saved", { detail: "Profile x" });
    expect(current()).toMatchObject({ text: "Saved" });
    line(1, "hidden update 1");
    line(1, "hidden update 2"); // applied, not queued
    vi.advanceTimersByTime(6_000);
    expect(current()).toMatchObject({ text: "Deploying", detail: "hidden update 2", persist: "job:1" });
    line(1, "shown again");
    expect(current()).toMatchObject({ detail: "shown again" });
  });

  // Fails when concurrent jobs fight over the slot or one is lost: the newest shows with "+n running", and when
  // it ends the next one returns.
  it("two concurrent jobs: the newest shows with '+1 running'; the other returns when it ends", () => {
    start(1, "install");
    start(2, "purge");
    expect(current()).toMatchObject({ text: "Purging", detail: "Purge · +1 running" });
    line(1, "from one"); // hidden: applied silently
    expect(current()).toMatchObject({ text: "Purging" });
    vi.advanceTimersByTime(TOAST_MIN_DWELL_MS);
    end(2, "purge");
    expect(current()).toMatchObject({ text: "Purged" });
    vi.advanceTimersByTime(6_000);
    expect(current()).toMatchObject({ text: "Installing", detail: "from one", persist: "job:1" });
  });

  // Fails when writes stop showing in arrival order: the job's outcome arriving after the command's result
  // replaces it (after the dwell), last write wins.
  it("an outcome arriving after the command's result is shown (arrival order)", () => {
    start(1, "mods_uninstall");
    showToast("success", "Uninstalled", { detail: "SkyUI" });
    useUi.getState().onJob({ id: 1, kind: "mods_uninstall", state: "finished", error: null });
    vi.advanceTimersByTime(TOAST_MIN_DWELL_MS);
    expect(current()).toMatchObject({ text: "Uninstalled", detail: "SkyUI" });
    vi.advanceTimersByTime(TOAST_MIN_DWELL_MS);
    expect(current()).toMatchObject({ tone: "success", text: "Uninstalled", detail: "Uninstall mods" });
  });

  // Fails when a stale timer (of a replaced message) closes the newer toast.
  it("a stale timer never closes a newer toast", () => {
    showToast("info", "first");
    const firstId = current()!.id;
    vi.advanceTimersByTime(5_000);
    showToast("error", "boom");
    vi.advanceTimersByTime(1_500);
    expect(current()).toMatchObject({ text: "boom" });
    useUi.getState().closeToast(firstId);
    expect(current()).toMatchObject({ text: "boom" });
  });

  // Fails when a download's progress events do not reach its job's live toast.
  it("download progress updates its job's live toast", () => {
    start(1, "install");
    useUi.getState().onDownload(1, "SkyUI.7z", { type: "started", resumed_from: 0, total: 4096 } as never);
    useUi.getState().onDownload(1, "SkyUI.7z", { type: "progress", done: 2048, total: 4096 } as never);
    expect(current()).toMatchObject({ text: "Installing", persist: "job:1" });
    expect(current()!.detail).toMatch(/^SkyUI\.7z · .+ \/ .+$/);
  });
});

describe(`minimum dwell (${TOAST_MIN_DWELL_MS} ms) before a message can be replaced`, () => {
  const D = TOAST_MIN_DWELL_MS;
  const shown: string[] = [];
  let stop = () => {};
  beforeEach(() => {
    shown.length = 0;
    stop = useUi.subscribe((s, p) => s.toast !== p.toast && s.toast && shown.push(s.toast.text));
  });
  afterEach(() => stop());

  // Fails when a newer message replaces one that has been up for less than the dwell.
  it(`a replacement at 50 ms waits until ${TOAST_MIN_DWELL_MS} ms`, () => {
    showToast("info", "first");
    vi.advanceTimersByTime(50);
    showToast("info", "second");
    expect(current()!.text).toBe("first");
    vi.advanceTimersByTime(D - 50 - 1);
    expect(current()!.text).toBe("first");
    vi.advanceTimersByTime(1);
    expect(current()!.text).toBe("second");
  });

  // Fails when the dwell adds a delay to a message that has already been up long enough.
  it("a replacement after the dwell is immediate", () => {
    showToast("info", "first");
    vi.advanceTimersByTime(D + 100);
    showToast("info", "second");
    expect(current()!.text).toBe("second");
  });

  // Fails when rapid writes skip a message, reorder, or show one for less than the dwell.
  it(`three rapid writes each show for at least ${TOAST_MIN_DWELL_MS} ms, in order`, () => {
    const at: Record<string, number> = {};
    const t0 = Date.now();
    const stop2 = useUi.subscribe((s, p) => s.toast !== p.toast && s.toast && (at[s.toast.text] = Date.now() - t0));
    showToast("info", "a");
    showToast("info", "b");
    showToast("info", "c");
    vi.advanceTimersByTime(D * 3);
    stop2();
    expect(shown).toEqual(["a", "b", "c"]);
    expect(at.b - at.a).toBeGreaterThanOrEqual(D);
    expect(at.c - at.b).toBeGreaterThanOrEqual(D);
  });

  // Fails when the auto-close timer counts from the write instead of from when the message was shown.
  it("the TTL counts from show time", () => {
    showToast("info", "first");
    showToast("info", "second"); // shows after the dwell
    vi.advanceTimersByTime(D);
    expect(current()!.text).toBe("second");
    vi.advanceTimersByTime(6_000 - 1);
    expect(current()!.text).toBe("second");
    vi.advanceTimersByTime(1);
    expect(current()).toBeNull();
  });

  // Fails when a stale timer or a late close of an earlier id drops a waiting message.
  it("a waiting message is never dropped by a stale timer or closeToast of an earlier id", () => {
    const first = showToast("info", "first");
    const second = showToast("warn", "second");
    useUi.getState().closeToast(second); // still waiting: not the shown id, ignored
    vi.advanceTimersByTime(D);
    expect(current()!.text).toBe("second");
    useUi.getState().closeToast(first); // replaced: ignored
    expect(current()!.text).toBe("second");
  });

});

describe("deploy completion (T14)", () => {
  // Fails when a deploy ends with two completion toasts (the result plus the generic "Deploy · Finished"),
  // or a failed deploy loses its error toast.
  it("a deploy produces exactly one completion toast: the deploy result", () => {
    const shown: { text: string; detail?: string }[] = [];
    const stop = useUi.subscribe((s, p) => s.toast !== p.toast && s.toast && shown.push({ text: s.toast.text, detail: s.toast.detail }));
    const ui = useUi.getState();
    ui.onJob({ id: 1, kind: "deploy", state: "started", error: null });
    // The backend's job event and the command's own result arrive in either order.
    ui.onJob({ id: 1, kind: "deploy", state: "finished", error: null });
    showToast("success", "Deployed", { detail: "3 files · 1 mod" });
    vi.advanceTimersByTime(TOAST_MIN_DWELL_MS * 5);
    stop();
    // The job writer's generic outcome would also be "Deployed" (detail "Deploy"): only the result shows.
    const done = shown.filter((t) => t.text === "Deployed");
    expect(done).toEqual([{ text: "Deployed", detail: "3 files · 1 mod" }]);
  });

  it("a failed deploy still shows its error", () => {
    const ui = useUi.getState();
    ui.onJob({ id: 2, kind: "deploy", state: "started", error: null });
    vi.advanceTimersByTime(TOAST_MIN_DWELL_MS);
    ui.onJob({ id: 2, kind: "deploy", state: "failed", error: "disk full" });
    expect(current()).toMatchObject({ tone: "error", text: "Deploy failed" });
  });

  // Fails when the deploy exception leaks to other jobs (they keep the generic outcome, arrival order).
  it("other jobs still write their generic outcome", () => {
    const ui = useUi.getState();
    ui.onJob({ id: 3, kind: "purge", state: "started", error: null });
    vi.advanceTimersByTime(TOAST_MIN_DWELL_MS);
    ui.onJob({ id: 3, kind: "purge", state: "finished", error: null });
    expect(current()).toMatchObject({ text: "Purged", detail: "Purge" });
  });
});
