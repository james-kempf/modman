// SPDX-License-Identifier: GPL-3.0-or-later
// Mods view (UX audit T08, #12 #13): move to top/bottom/#N from the detail dialog,
// row selection with the bulk bar (one IPC call per action), and focus kept through all of it.
import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "../app/App";
import { resetBridge, startBridge } from "../app/bridge";
import { makeQueryClient } from "../app/queries";
import { resetUi, useUi } from "../app/store";
import { setTransport } from "../ipc/transport";
import type { ModView } from "../ipc/types";
import { createDemoBackend } from "../mock/demo";

const GAME = "skyrimspecialedition";
const notices = () => [useUi.getState().toast].flatMap((x) => (x ? [x] : []));
const texts = () => notices().map((n) => (n.detail ? `${n.text} · ${n.detail}` : n.text));

/** The full App on the demo backend (SkyUI, Unofficial Patch, Immersive Armors = folder install). */
async function demoApp() {
  resetUi();
  resetBridge();
  const t = createDemoBackend();
  setTransport(t);
  const qc = makeQueryClient();
  await startBridge(qc);
  render(
    <QueryClientProvider client={qc}>
      <App />
    </QueryClientProvider>,
  );
  act(() => useUi.getState().setView("mods"));
  await screen.findByTitle("SkyUI");
  return { t, user: userEvent.setup() };
}

const order = () =>
  within(screen.getByRole("list", { name: "Load order" }))
    .getAllByRole("listitem")
    .map((li) => li.querySelector("[title]")!.getAttribute("title"));
const select = (name: string) => screen.getByRole("checkbox", { name: `Select ${name}` });
const bar = () => screen.getByRole("region", { name: "Selected mods" });
const announced = () => screen.getByTestId("mods-announce").textContent;
const uidOf = async (t: ReturnType<typeof createDemoBackend>, name: string) =>
  (await t.invoke<{ mods: ModView[] }>("mods_list", { game: GAME })).mods.find((m) => m.name === name)!.uid;

async function selectAll(user: ReturnType<typeof userEvent.setup>, names: string[]) {
  for (const n of names) await user.click(select(n));
}

/** Bar Uninstall, then the backend's danger confirm ("Uninstall N mods?"), optionally ticking the option. */
async function bulkUninstall(user: ReturnType<typeof userEvent.setup>, n: number, deleteArchive = false) {
  await user.click(within(bar()).getByRole("button", { name: "Uninstall" }));
  const dlg = await screen.findByRole("dialog", { name: `Uninstall ${n} mods?` });
  if (deleteArchive) await user.click(within(dlg).getByRole("checkbox", { name: /Delete archive too/ }));
  await user.click(within(dlg).getByRole("button", { name: `Uninstall ${n} mods` }));
  await waitFor(() => expect(texts().some((x) => x.startsWith(`Uninstalled · ${n} mods`))).toBe(true));
}

describe("#12 move to position (detail dialog)", () => {
  // Fails when a move sends the wrong spec, is not announced, or drops focus from the pressed control.
  it.each<[string, "first" | "last" | "#2", string, string[]]>([
    ["bottom", "last", "Immersive Armors moved to position 3 of 3", ["SkyUI", "Unofficial Patch", "Immersive Armors"]],
    ["top", "first", "Immersive Armors moved to position 1 of 3", ["Immersive Armors", "SkyUI", "Unofficial Patch"]],
  ])("Move to %s calls modMove(uid, %s) and announces it", async (where, spec, said, after) => {
    const { t, user } = await demoApp();
    // Start from the middle so both ends move.
    await user.click(within(screen.getByTitle("Immersive Armors").closest("li")!).getByRole("button", { name: "Move Immersive Armors up" }));
    await waitFor(() => expect(order()).toEqual(["SkyUI", "Immersive Armors", "Unofficial Patch"]));
    const uid = await uidOf(t, "Immersive Armors");
    await user.click(within(screen.getByTitle("Immersive Armors").closest("li")!).getByRole("button", { name: "Details" }));
    const dlg = await screen.findByRole("dialog");
    const btn = within(dlg).getByRole("button", { name: `Move Immersive Armors to ${where}` });
    await user.click(btn);
    await waitFor(() => expect(order()).toEqual(after));
    expect(t.callsOf("mod_move").at(-1)).toEqual({ game: GAME, reference: uid, spec, arg: null });
    await waitFor(() => expect(announced()).toContain(said));
    expect(document.activeElement).toBe(btn);
  });

  // Fails when a typed position is not sent as "#N" or the dialog loses focus.
  it("Position 2 + Move calls modMove(uid, '#2'); the subtitle follows", async () => {
    const { t, user } = await demoApp();
    const uid = await uidOf(t, "SkyUI");
    await user.click(within(screen.getByTitle("SkyUI").closest("li")!).getByRole("button", { name: "Details" }));
    const dlg = await screen.findByRole("dialog");
    await user.type(within(dlg).getByLabelText("Position"), "2");
    const go = within(dlg).getByRole("button", { name: "Move" });
    await user.click(go);
    await waitFor(() => expect(order()).toEqual(["Unofficial Patch", "SkyUI", "Immersive Armors"]));
    expect(t.callsOf("mod_move").at(-1)).toEqual({ game: GAME, reference: uid, spec: "#2", arg: null });
    await waitFor(() => expect(announced()).toContain("SkyUI moved to position 2 of 3"));
    await waitFor(() => expect(within(dlg).getByText(new RegExp(`^#2 · ${uid}`))).toBeTruthy());
    expect(document.activeElement).toBe(go);
  });

  // Fails when an up/down move is not announced or loses focus.
  it("Move down is announced and keeps focus on the button", async () => {
    const { user } = await demoApp();
    const down = screen.getByRole("button", { name: "Move SkyUI down" });
    await user.click(down);
    await waitFor(() => expect(order()).toEqual(["Unofficial Patch", "SkyUI", "Immersive Armors"]));
    await waitFor(() => expect(announced()).toContain("SkyUI moved to position 2 of 3"));
    expect(document.activeElement).toBe(down);
  });
});

describe("#13 selection and bulk actions", () => {
  // Fails when the bar is missing, miscounts, or the count is not announced.
  it("selecting rows shows the bar with the count and announces it", async () => {
    const { user } = await demoApp();
    expect(screen.queryByRole("region", { name: "Selected mods" })).toBeNull();
    await selectAll(user, ["SkyUI", "Unofficial Patch"]);
    expect(within(bar()).getByText("2 selected")).toBeTruthy();
    expect(select("SkyUI").getAttribute("aria-checked")).toBe("true");
    await waitFor(() => expect(announced()).toContain("2 selected"));
  });

  // Fails when bulk enable/disable makes one call per mod, or focus leaves the pressed button.
  it.each([
    ["Disable", false],
    ["Enable", true],
  ] as const)("%s sends one modsSetEnabled call with every selected id", async (label, on) => {
    const { t, user } = await demoApp();
    const ids = (await t.invoke<{ mods: ModView[] }>("mods_list", { game: GAME })).mods.map((m) => m.id);
    await selectAll(user, ["SkyUI", "Unofficial Patch", "Immersive Armors"]);
    const btn = within(bar()).getByRole("button", { name: label });
    await user.click(btn);
    await waitFor(() => expect(t.callsOf("mods_set_enabled")).toEqual([{ game: GAME, ids, enabled: on }]));
    await waitFor(() => expect(btn.getAttribute("aria-busy")).toBeNull());
    expect(document.activeElement).toBe(btn);
  });

  // Fails when Clear leaves focus on <body> as the bar unmounts.
  it("Clear empties the selection and focus returns to the last toggled row", async () => {
    const { user } = await demoApp();
    await selectAll(user, ["SkyUI", "Unofficial Patch"]);
    await user.click(within(bar()).getByRole("button", { name: "Clear" }));
    expect(screen.queryByRole("region", { name: "Selected mods" })).toBeNull();
    expect(document.activeElement).toBe(select("Unofficial Patch"));
    expect(announced()).toContain("Selection cleared");
  });

  // Fails when bulk uninstall makes one call per mod, asks the backend to delete archives, or drops focus.
  it("Uninstall sends one modsUninstall call with every ref and deleteArchive=false; focus goes to a remaining row", async () => {
    const { t, user } = await demoApp();
    const uids = [await uidOf(t, "SkyUI"), await uidOf(t, "Unofficial Patch")];
    await selectAll(user, ["SkyUI", "Unofficial Patch"]);
    await bulkUninstall(user, 2);
    expect(t.callsOf("mods_uninstall")).toEqual([{ game: GAME, refs: uids, delete_archive: false }]);
    await waitFor(() => expect(order()).toEqual(["Immersive Armors"]));
    await waitFor(() => expect(document.activeElement).toBe(select("Immersive Armors")));
  });
});

