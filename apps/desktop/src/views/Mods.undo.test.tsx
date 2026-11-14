// SPDX-License-Identifier: GPL-3.0-or-later
// Mods view (UX audit T11, #1): Undo of a single uninstall through one
// modRestore(tokens) call, offered only for restorable entries.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useUi } from "../app/store";
import type { RestoreErrorKind, RestoreReason, RestoredMod, UninstallRestore } from "../ipc/types";
import type { Args, Handler } from "../mock/fake";
import { afterChange, mod } from "../mock/fixtures";
import { setup } from "../test/harness";

const quiet = afterChange({ needs_deploy: false });
const notices = () => useUi.getState().notices;
const texts = () => notices().map((n) => n.text);

/**
 * Alpha, Bravo (disabled), Charlie. Uninstall removes the mod and returns `restore`
 * for it; mod_restore re-adds it at its old place unless `restoreWith` says otherwise.
 */
function backend(
  entry: Partial<UninstallRestore> = {},
  restoreWith?: (tokens: string[], m: ReturnType<typeof mod>) => RestoredMod[],
  archives: string[] = [],
) {
  const all = [mod({ name: "Alpha" }), mod({ name: "Bravo", enabled: false }), mod({ name: "Charlie" })];
  all.forEach((m, i) => (m.position = i + 1));
  let mods = [...all];
  const handlers: Record<string, Handler> = {
    mods_list: () => ({ mods: mods.map((m) => ({ ...m })), changed_since_deploy: false }),
    mod_detail: (a: Args) => ({ mod: mods.find((m) => m.uid === a!.reference), collections: [], fomod: [], wins: 0, loses: 0, url: null }),
    mods_uninstall: (a: Args) => {
      const gone = mods.find((m) => (a!.refs as string[]).includes(m.uid))!;
      mods = mods.filter((m) => m !== gone);
      const restore: UninstallRestore = { token: "tok-1", uid: gone.uid, name: gone.name, restorable: true, reason: null, ...entry };
      return { done: true, mods: 1, archives, after: quiet, restore: [restore] };
    },
    mod_restore: (a: Args) => {
      const tokens = a!.tokens as string[];
      const m = all[1];
      if (restoreWith) return { results: restoreWith(tokens, m), restored: 0, after: quiet };
      mods = all.filter((x) => x === m || mods.includes(x));
      return { results: [{ token: tokens[0], uid: m.uid, name: m.name, ok: true, error: null, warnings: [] }], restored: 1, after: quiet };
    },
  };
  return { all, handlers };
}

async function uninstallBravo(handlers: Record<string, Handler>) {
  const env = await setup(handlers);
  env.renderApp();
  act(() => useUi.getState().setView("mods"));
  const user = userEvent.setup();
  const row = (await screen.findByTitle("Bravo")).closest("li")!;
  await user.click(within(row).getByRole("button", { name: "Details" }));
  await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Uninstall" }));
  await waitFor(() => expect(texts().some((t) => t.startsWith("Uninstalled Bravo"))).toBe(true));
  await waitFor(() => expect(screen.queryByTitle("Bravo")).toBeNull());
  return { ...env, user };
}

const fail = (kind: RestoreErrorKind) => (tokens: string[], m: ReturnType<typeof mod>): RestoredMod[] => [
  kind === "expired"
    ? { token: tokens[0], uid: null, name: null, ok: false, error: { kind, message: "gone" }, warnings: [] }
    : { token: tokens[0], uid: m.uid, name: m.name, ok: false, error: { kind, message: "disk full" }, warnings: [] },
];

describe("#1 uninstall Undo availability", () => {
  // Fails when a restorable uninstall gets no Undo, or Undo sends more than the opaque tokens.
  it("offers Undo for a restorable entry; Undo makes one modRestore call with the tokens only", async () => {
    const b = backend();
    const { t, user } = await uninstallBravo(b.handlers);
    expect(notices().find((n) => n.text === "Uninstalled Bravo")?.action?.label).toBe("Undo");
    await user.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(texts()).toContain("Restored Bravo"));
    expect(t.callsOf("mod_restore")).toEqual([{ game: "skyrimspecialedition", tokens: ["tok-1"] }]);
  });

  // Fails when Undo is offered without the backend's restorable flag, or the reason is not given.
  it.each<[RestoreReason, RegExp]>([
    ["folder_install", /installed from a folder/],
    ["archive_deleted", /archive was deleted/],
    ["outside_downloads", /outside the downloads folder/],
  ])("restorable=false (%s): no Undo, and the notice says why", async (reason, why) => {
    const { handlers } = backend({ restorable: false, reason });
    await uninstallBravo(handlers);
    const n = notices().find((x) => x.text.startsWith("Uninstalled Bravo"))!;
    expect(n.action).toBeUndefined();
    expect(n.text).toMatch(/No Undo for Bravo: /);
    expect(n.text).toMatch(why);
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
  });

  // Fails when the view works out availability from archives, reason or paths instead of restorable.
  it.each<[string, Partial<UninstallRestore>, string[], string | undefined]>([
    ["archive reported deleted, yet restorable", { restorable: true, reason: null }, ["/d/bravo.7z"], "Undo"],
    ["archive kept and no reason, yet not restorable", { restorable: false, reason: null }, [], undefined],
  ])("follows restorable alone: %s", async (_, entry, archives, label) => {
    await uninstallBravo(backend(entry, undefined, archives).handlers);
    expect(notices().find((n) => n.text.startsWith("Uninstalled Bravo"))?.action?.label).toBe(label);
  });
});

describe("#1 uninstall Undo results", () => {
  // Fails when the restored row is not focused (focus lost to <body> with the notice).
  it("focuses the restored row's Details after Undo, never body", async () => {
    const b = backend();
    const { user } = await uninstallBravo(b.handlers);
    await user.click(await screen.findByRole("button", { name: "Undo" }));
    expect(document.activeElement).not.toBe(document.body);
    const details = async () => within((await screen.findByTitle("Bravo")).closest("li")!).getByRole("button", { name: "Details" });
    await waitFor(async () => expect(document.activeElement).toBe(await details()));
  });

  // Fails when a failed restore is silent, does not name the mod and reason, or skips the refetch.
  it.each<[RestoreErrorKind, string]>([
    ["archive_missing", "Couldn't restore Bravo: its archive is no longer in the downloads folder."],
    ["conflict", "Couldn't restore Bravo: a mod with the same ID is installed again."],
    ["install_failed", "Couldn't restore Bravo: reinstalling it failed (disk full)."],
    ["expired", "Couldn't restore Bravo: Undo is no longer available."],
  ])("%s gives a typed error notice and a refetch", async (kind, text) => {
    const { handlers } = backend({}, fail(kind));
    const { t, user } = await uninstallBravo(handlers);
    const before = t.callsOf("mods_list").length;
    await user.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(texts()).toContain(text));
    expect(notices().find((n) => n.text === text)?.tone).toBe("error");
    expect(texts()).not.toContain("Restored Bravo");
    await waitFor(() => expect(t.callsOf("mods_list").length).toBeGreaterThan(before));
  });

  // Fails when a rejected modRestore call gives a notice that does not name the mod, or no refetch.
  it("a rejected call gives an error notice naming the mod and the error, and a refetch", async () => {
    const { handlers } = backend();
    handlers.mod_restore = () => {
      throw { message: "backend went away" };
    };
    const { t, user } = await uninstallBravo(handlers);
    const before = t.callsOf("mods_list").length;
    await user.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(texts()).toContain("Couldn't restore Bravo: backend went away"));
    await waitFor(() => expect(t.callsOf("mods_list").length).toBeGreaterThan(before));
  });

  // Fails when Undo pressed twice does not report the used-up token as expired.
  it("Undo pressed twice: the second gives the expired notice", async () => {
    const { handlers } = backend();
    let used = false;
    const restore = handlers.mod_restore;
    handlers.mod_restore = (a: Args) => {
      if (!used) return (used = true), restore(a);
      return { results: fail("expired")(a!.tokens as string[], mod()), restored: 0, after: quiet };
    };
    const { t, user } = await uninstallBravo(handlers);
    const action = notices().find((n) => n.action)!.action!;
    await user.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(texts()).toContain("Restored Bravo"));
    await act(async () => action.onAction());
    await waitFor(() => expect(texts()).toContain("Couldn't restore Bravo: Undo is no longer available."));
    expect(t.callsOf("mod_restore")).toHaveLength(2);
  });
});
