// SPDX-License-Identifier: GPL-3.0-or-later
// The uninstall Undo helper over N entries (T11; T08 uses it for bulk).
import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { useUi } from "../../app/store";
import type { UninstallRestore, UninstallResult } from "../../ipc/types";
import { afterChange } from "../../mock/fixtures";
import { setup } from "../../test/harness";
import { useUninstallNotice } from "./restore";

const quiet = afterChange({ needs_deploy: false });
const entry = (n: number, o: Partial<UninstallRestore> = {}): UninstallRestore => ({
  token: `t${n}`,
  uid: `u${n}`,
  name: `Mod ${n}`,
  restorable: true,
  reason: null,
  ...o,
});
const result = (restore: UninstallRestore[]): UninstallResult => ({ done: true, mods: restore.length, archives: [], after: quiet, restore });

async function hook(handlers = {}) {
  const env = await setup(handlers);
  const onUndo = vi.fn();
  const onRestored = vi.fn();
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={env.qc}>{children}</QueryClientProvider>;
  const { result: h } = renderHook(() => useUninstallNotice("skyrimspecialedition", { onUndo, onRestored }), { wrapper });
  return { ...env, notice: h.current, onUndo, onRestored };
}

describe("useUninstallNotice with several entries", () => {
  // Fails when a non-restorable token is sent to modRestore, the rest go unnamed, or a partial failure hides the successes.
  it("sends only restorable tokens in one call, names the rest, and reports each result", async () => {
    const env = await hook({
      mod_restore: () => ({
        results: [
          { token: "t1", uid: "u1", name: "Mod 1", ok: true, error: null, warnings: ["Skipped deleted collection abc"] },
          { token: "t3", uid: "u3", name: "Mod 3", ok: false, error: { kind: "conflict", message: "x" }, warnings: [] },
        ],
        restored: 1,
        after: quiet,
      }),
    });
    act(() =>
      env.notice(result([entry(1), entry(2, { restorable: false, reason: "folder_install" }), entry(3)]), "Uninstalled 3 mods"),
    );
    const n = useUi.getState().notices[0];
    expect(n.text).toBe(
      "Uninstalled 3 mods. No Undo for Mod 2: it was installed from a folder, so there is no archive to reinstall it from.",
    );
    await act(async () => n.action!.onAction());
    expect(env.t.callsOf("mod_restore")).toEqual([{ game: "skyrimspecialedition", tokens: ["t1", "t3"] }]);
    expect(env.onUndo).toHaveBeenCalledOnce();
    await waitFor(() => expect(env.onRestored).toHaveBeenCalledWith(["u1"]));
    expect(useUi.getState().notices.map((x) => [x.tone, x.text])).toEqual([
      ["error", "Couldn't restore Mod 3: a mod with the same ID is installed again."],
      ["success", "Restored Mod 1"],
      ["info", "Skipped deleted collection abc"],
    ]);
  });

  // Fails when a declined uninstall, or one with no restore entries (legacy/CLI shape), still offers Undo.
  it("offers no Undo for a declined uninstall or a result without restore entries", async () => {
    const env = await hook();
    act(() => env.notice({ done: false, mods: 0, archives: [], after: quiet }, "Mod 1 was not uninstalled."));
    act(() => env.notice({ done: true, mods: 1, archives: [], after: quiet }, "Uninstalled Mod 1"));
    expect(useUi.getState().notices.map((x) => [x.tone, x.text, x.action])).toEqual([
      ["info", "Mod 1 was not uninstalled.", undefined],
      ["success", "Uninstalled Mod 1", undefined],
    ]);
  });
});
