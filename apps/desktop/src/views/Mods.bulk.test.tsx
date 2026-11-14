// SPDX-License-Identifier: GPL-3.0-or-later
// Mods view (UX audit T08, #12 #13): move to top/bottom/#N from the detail dialog,
// row selection with the bulk bar (one IPC call per action), bulk Undo through one
// modRestore call, and focus kept through all of it.
import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "../app/App";
import { resetBridge, startBridge } from "../app/bridge";
import { makeQueryClient } from "../app/queries";
import { resetUi, useUi } from "../app/store";
import { setTransport } from "../ipc/transport";
import type { ModView, RestoreErrorKind, RestoredMod, UninstallRestore } from "../ipc/types";
import { createDemoBackend } from "../mock/demo";
import type { Args } from "../mock/fake";
import { afterChange, mod } from "../mock/fixtures";
import { setup } from "../test/harness";

const GAME = "skyrimspecialedition";
const notices = () => useUi.getState().notices;
const texts = () => notices().map((n) => n.text);
const quiet = afterChange({ needs_deploy: false });

/** The full App on the demo backend (SkyUI, Unofficial Patch, Immersive Armors = folder install). */
async function demoApp(restoreFailures?: Map<string, RestoreErrorKind>) {
  resetUi();
  resetBridge();
  const t = createDemoBackend({ restoreFailures });
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
  await waitFor(() => expect(texts().some((x) => x.startsWith(`Uninstalled ${n} mods`))).toBe(true));
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

describe("#13 bulk Undo (demo backend)", () => {
  // Fails when bulk Undo sends more than one call, or the mods do not come back in place.
  it("archives kept: one modRestore call restores every mod at its old position; focus on a restored row", async () => {
    const { t, user } = await demoApp();
    await selectAll(user, ["SkyUI", "Unofficial Patch"]);
    await bulkUninstall(user, 2);
    const n = notices().find((x) => x.text === "Uninstalled 2 mods")!;
    expect(n.action?.label).toBe("Undo");
    await user.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(texts()).toContain("Restored 2 mods"));
    expect(t.callsOf("mod_restore")).toHaveLength(1);
    expect((t.callsOf("mod_restore")[0] as { tokens: string[] }).tokens).toHaveLength(2);
    await waitFor(() => expect(order()).toEqual(["SkyUI", "Unofficial Patch", "Immersive Armors"]));
    const details = within(screen.getByTitle("SkyUI").closest("li")!).getByRole("button", { name: "Details" });
    await waitFor(() => expect(document.activeElement).toBe(details));
    // An Undo does not reselect the restored rows.
    expect(screen.queryByRole("region", { name: "Selected mods" })).toBeNull();
  });

  // Fails when Undo is offered after "Delete archive too" (nothing restorable).
  it("'Delete archive too' ticked: no Undo", async () => {
    const { user } = await demoApp();
    await selectAll(user, ["SkyUI", "Unofficial Patch"]);
    await bulkUninstall(user, 2, true);
    const n = notices().find((x) => x.text.startsWith("Uninstalled 2 mods and deleted 2 archives"))!;
    expect(n.action).toBeUndefined();
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
  });

  // Fails when a non-restorable entry is sent, or the notice does not name it and say why.
  it("mixed folder and archive mods: only restorable tokens are sent and the folder install is named with its reason", async () => {
    const { t, user } = await demoApp();
    await selectAll(user, ["SkyUI", "Unofficial Patch", "Immersive Armors"]);
    await bulkUninstall(user, 3);
    const n = notices().find((x) => x.text.startsWith("Uninstalled 3 mods"))!;
    expect(n.text).toMatch(/No Undo for Immersive Armors: it was installed from a folder/);
    const [u] = t.callsOf("mods_uninstall");
    expect(u).toMatchObject({ refs: expect.any(Array) });
    await user.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(texts()).toContain("Restored 2 mods"));
    expect((t.callsOf("mod_restore")[0] as { tokens: string[] }).tokens).toHaveLength(2);
    await waitFor(() => expect(order()).toEqual(["SkyUI", "Unofficial Patch"]));
  });

  // Fails when one failed mod stops the others, is not reported, or the list does not refetch.
  it.each<RestoreErrorKind>(["archive_missing", "conflict"])("one %s: the rest restore, an error notice names it, the list refetches", async (kind) => {
    const failures = new Map<string, RestoreErrorKind>();
    const { t, user } = await demoApp(failures);
    failures.set(await uidOf(t, "SkyUI"), kind);
    await selectAll(user, ["SkyUI", "Unofficial Patch"]);
    await bulkUninstall(user, 2);
    const before = t.callsOf("mods_list").length;
    await user.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(texts()).toContain("Restored Unofficial Patch"));
    const err = notices().find((x) => x.tone === "error")!;
    expect(err.text).toMatch(/^Couldn't restore SkyUI: /);
    await waitFor(() => expect(t.callsOf("mods_list").length).toBeGreaterThan(before));
    // Unofficial Patch goes back to position 2; SkyUI stays out.
    await waitFor(() => expect(order()).toEqual(["Immersive Armors", "Unofficial Patch"]));
    expect(document.activeElement).not.toBe(document.body);
  });
});

/** A fake backend (no prompt): uninstall removes the refs; mod_restore answers with `restoreWith`. */
async function fakeApp(names: string[], restoreWith: (tokens: string[], gone: ModView[]) => RestoredMod[]) {
  let mods = names.map((name, i) => mod({ name, position: i + 1 }));
  let gone: ModView[] = [];
  const env = await setup({
    mods_list: () => ({ mods, changed_since_deploy: false }),
    mods_uninstall: (a: Args) => {
      gone = mods.filter((m) => (a!.refs as string[]).includes(m.uid));
      mods = mods.filter((m) => !gone.includes(m));
      const restore = gone.map((m, i): UninstallRestore => ({ token: `tok-${i}`, uid: m.uid, name: m.name, restorable: true, reason: null }));
      return { done: true, mods: gone.length, archives: [], after: quiet, restore };
    },
    mod_restore: (a: Args) => ({ results: restoreWith(a!.tokens as string[], gone), restored: 0, after: quiet }),
  });
  env.renderApp();
  act(() => useUi.getState().setView("mods"));
  await screen.findByTitle(names[0]);
  const user = userEvent.setup();
  await selectAll(user, names.slice(0, 3));
  await user.click(within(bar()).getByRole("button", { name: "Uninstall" }));
  await waitFor(() => expect(texts().some((x) => x.startsWith("Uninstalled 3 mods"))).toBe(true));
  return { ...env, user };
}

describe("#13 bulk Undo outcome fits the notice stack", () => {
  // Fails when several failures get one notice each (MAX_NOTICES = 3 pushes the outcome out).
  it("several failures make ONE error notice listing each mod and reason", async () => {
    const kinds: RestoreErrorKind[] = ["archive_missing", "conflict", "expired"];
    const { user } = await fakeApp(["Alpha", "Bravo", "Charlie", "Delta"], (tokens, gone) =>
      tokens.map((token, i) =>
        kinds[i] === "expired"
          ? { token, uid: null, name: null, ok: false, error: { kind: "expired", message: "gone" }, warnings: [] }
          : { token, uid: gone[i].uid, name: gone[i].name, ok: false, error: { kind: kinds[i], message: "x" }, warnings: [] },
      ),
    );
    await user.click(await screen.findByRole("button", { name: "Undo" }));
    const text = "Couldn't restore 3 mods: Alpha (archive missing), Bravo (conflict), Charlie (expired).";
    await waitFor(() => expect(texts()).toContain(text));
    expect(notices().filter((n) => n.tone === "error")).toHaveLength(1);
    expect(document.activeElement).not.toBe(document.body);
  });

  // Fails when expired tokens in a bulk Undo give no typed notice.
  it("all tokens expired: a typed expired notice", async () => {
    const { user } = await fakeApp(["Alpha", "Bravo", "Charlie"], (tokens) =>
      tokens.map((token) => ({ token, uid: null, name: null, ok: false, error: { kind: "expired", message: "gone" }, warnings: [] })),
    );
    await user.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() =>
      expect(texts()).toContain("Couldn't restore 3 mods: Alpha (expired), Bravo (expired), Charlie (expired)."),
    );
  });
});
