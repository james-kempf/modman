// SPDX-License-Identifier: GPL-3.0-or-later
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useUi } from "./store";

const texts = () => useUi.getState().notices.map((n) => n.text);

describe("notices (T16 F5)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useUi.setState({ notices: [] });
  });
  afterEach(() => vi.useRealTimers());

  it("collapses a repeated notice instead of stacking it", () => {
    useUi.getState().notify("warn", "Changes are not in the game yet.", true);
    useUi.getState().notify("warn", "Changes are not in the game yet.", true);
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

  it("keeps errors and deploy notices until dismissed", () => {
    useUi.getState().notify("error", "boom");
    useUi.getState().notify("warn", "deploy?", true);
    vi.advanceTimersByTime(60_000);
    expect(texts()).toEqual(["boom", "deploy?"]);
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
