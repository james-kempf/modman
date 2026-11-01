// SPDX-License-Identifier: GPL-3.0-or-later
// Mods installed as a loader (SMAPI): listed as installed in the Mods view
// (its own Loaders group) and removed from Browse through the loader
// uninstall consent flow, never mods_uninstall.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useUi, type ViewId } from "../app/store";
import type { RunReport } from "../ipc/types";
import type { Handler } from "../mock/fake";
import { game } from "../mock/fixtures";
import { demoPreview } from "../mock/loaders";
import { setup } from "../test/harness";

async function open(view: ViewId, handlers: Record<string, Handler> = {}) {
  const env = await setup(handlers, [game()]);
  env.renderApp();
  act(() => useUi.getState().setView(view));
  return { ...env, user: userEvent.setup() };
}

const smapi = { key: "smapi", name: "SMAPI", version: "4.5.2", nexus_mod_id: 2400, install_id: 3, status: "installed" };

const report: RunReport = {
  install_id: 3,
  run_id: 9,
  outcome: { kind: "succeeded" },
  status: "uninstalled",
  version: null,
  log_dir: "/d",
  warnings: [],
  launch_desired: null,
};

describe("loader-installed mods in the Mods view", () => {
  // Fails when a mod installed as a loader (no mods row) is missing from the Mods view, or the view
  // claims nothing is installed while a loader is.
  it("lists installed loaders in a separate Loaders group", async () => {
    await open("mods", { mods_list: () => ({ mods: [], changed_since_deploy: false, loaders: [smapi] }) });
    const group = await screen.findByRole("list", { name: "Loaders" });
    const row = within(group).getByRole("listitem");
    expect(row).toHaveTextContent("SMAPI");
    expect(row).toHaveTextContent("v4.5.2");
    expect(row).toHaveTextContent("installed");
    expect(screen.queryByRole("list", { name: "Load order" })).toBeNull();
  });
});

describe("Browse Remove on a loader-installed mod", () => {
  // Fails when Remove on a loader-installed hit calls mods_uninstall (which cannot find it) instead of
  // previewing the loader uninstall, opening the consent and running only on confirm.
  it("routes through loader_preview -> consent -> loader_run", async () => {
    const { t, user } = await open("browse", {
      browse: () => ({
        kind: "mods",
        total: 1,
        mods: [
          {
            modId: 2400,
            name: "SMAPI",
            author: "Pathoschild",
            version: "4.5.2",
            summary: null,
            downloads: 1,
            endorsements: 1,
            thumbnailUrl: null,
            category: null,
            updatedAt: null,
            installed: true,
            loader: "smapi",
          },
        ],
      }),
      loader_preview: (a) => demoPreview({ op: a!.op as never, loader_key: "smapi", preview_id: "p1", plan_hash: "h1" }),
      loader_run: () => report,
      mods_uninstall: () => {
        throw new Error("mods_uninstall must not be called for a loader");
      },
    });
    await user.click(await screen.findByRole("button", { name: "SMAPI" }));
    const details = await screen.findByRole("dialog", { name: "SMAPI" });
    await user.click(within(details).getByRole("button", { name: "Remove SMAPI" }));
    await waitFor(() =>
      expect(t.callsOf("loader_preview")).toEqual([{ game: "skyrimspecialedition", op: "uninstall", loader_key: "smapi", archive: null, unverified_ack: false }]),
    );
    // The consent opens over the details dialog.
    await waitFor(() => expect(screen.getAllByRole("dialog")).toHaveLength(2));
    const dialog = screen.getAllByRole("dialog").find((d) => d !== details)!;
    expect(t.callsOf("loader_run")).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: /uninstall/i }));
    await waitFor(() => expect(t.callsOf("loader_run")).toEqual([{ preview_id: "p1", plan_hash: "h1" }]));
    expect(t.callsOf("mods_uninstall")).toEqual([]);
  });
});
