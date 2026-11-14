// SPDX-License-Identifier: GPL-3.0-or-later
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useSelection } from "./useSelection";

const mount = (ids: number[]) =>
  renderHook(({ ids }) => useSelection(ids), { initialProps: { ids } });

describe("useSelection", () => {
  // Fails when toggle does not add then remove, or clear leaves ids behind.
  it("toggles ids and clears", () => {
    const { result } = mount([1, 2, 3]);
    act(() => result.current.toggle(2));
    act(() => result.current.toggle(3));
    expect(result.current.count).toBe(2);
    expect(result.current.has(2)).toBe(true);
    act(() => result.current.toggle(2));
    expect(result.current.has(2)).toBe(false);
    act(() => result.current.clear());
    expect(result.current.count).toBe(0);
  });

  // Fails when a bulk action would target a mod that is no longer listed (uninstalled elsewhere, filtered out).
  it("prunes ids that leave the list, on that same render", () => {
    const { result, rerender } = mount([1, 2, 3]);
    act(() => result.current.toggle(1));
    act(() => result.current.toggle(3));
    rerender({ ids: [2, 3] });
    expect(result.current.count).toBe(1);
    expect(result.current.list).toEqual([3]);
    expect(result.current.has(1)).toBe(false);
  });

  // Fails when a pruned id silently comes back selected after it reappears (e.g. Undo restores it).
  it("does not resurrect a pruned id when it reappears", () => {
    const { result, rerender } = mount([1, 2]);
    act(() => result.current.toggle(1));
    rerender({ ids: [2] });
    rerender({ ids: [1, 2] });
    expect(result.current.has(1)).toBe(false);
    expect(result.current.count).toBe(0);
  });

  // Fails when the bulk list follows click order instead of list order (load order matters to the backend).
  it("lists selected ids in list order", () => {
    const { result } = mount([5, 4, 9]);
    act(() => result.current.toggle(9));
    act(() => result.current.toggle(5));
    expect(result.current.list).toEqual([5, 9]);
  });

  // Fails when an id that is not listed can be selected (the bar would count a ghost).
  it("ignores toggles of ids that are not listed", () => {
    const { result } = mount([1]);
    act(() => result.current.toggle(7));
    expect(result.current.count).toBe(0);
  });
});
