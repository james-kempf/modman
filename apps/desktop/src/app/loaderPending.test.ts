// SPDX-License-Identifier: GPL-3.0-or-later
// Pending loader previews from command results and nxm links reach the
// consent queue (never run on their own).
import { act } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LoaderPending } from "../ipc/types";
import { setup } from "../test/harness";
import { loaderPendingOf } from "./run";
import { useUi } from "./store";

const p = (id: string): LoaderPending => ({ loader_key: "smapi", preview_id: id, plan_hash: `h${id}`, op: "install", version: null });

describe("pending loaders", () => {
  // Fails when one of the three result shapes (install, collection, install-of-a-collection) loses its loader.
  it("are found in install, collection and nested collection results", () => {
    expect(loaderPendingOf({ loader: p("a") })).toEqual([p("a")]);
    expect(loaderPendingOf({ loaders_pending: [p("b")] })).toEqual([p("b")]);
    expect(loaderPendingOf({ loader: null, collection: { loaders_pending: [p("c")] } })).toEqual([p("c")]);
    expect(loaderPendingOf({ installed: {} })).toEqual([]);
  });

  // Fails when an nxm link for a loader (vessel:loader-pending) never opens the consent dialog, or opens it twice.
  it("from an nxm link open the consent queue once, and nothing runs", async () => {
    const { t } = await setup();
    act(() => t.emit("vessel:loader-pending", { game: "stardewvalley", pending: [p("n1")] }));
    act(() => t.emit("vessel:loader-pending", { game: "stardewvalley", pending: [p("n1")] }));
    expect(useUi.getState().loaderConsents.map((c) => c.pending?.preview_id)).toEqual(["n1"]);
    expect(useUi.getState().loaderConsents[0].game).toBe("stardewvalley");
    expect(t.callsOf("loader_run")).toEqual([]);
  });

  // Fails when an interrupted run found at startup is not pointed out to the user.
  it("a startup rollback offer raises one notice", async () => {
    await setup({
      app_ready: () => ({
        startup_check: null,
        queued: 0,
        loader_recovered: [{ run_id: 1, install_id: 2, action: { kind: "rollback_offered", killed: false } }],
      }),
    });
    expect(useUi.getState().notices.filter((n) => /loader install was interrupted/.test(n.text))).toHaveLength(1);
  });
});
