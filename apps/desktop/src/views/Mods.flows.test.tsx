// SPDX-License-Identifier: GPL-3.0-or-later
// Mods view flows (UX audit T06): uninstall wiring (#1, no Undo yet), profile load with
// snapshot and Undo (#2), optimistic enable switch (#21), tab and filters kept in the store (#23).
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useUi } from "../app/store";
import type { Args, Handler } from "../mock/fake";
import { afterChange, mod } from "../mock/fixtures";
import { setup } from "../test/harness";
import { snapshotName } from "./Mods";

async function open(handlers: Record<string, Handler> = {}) {
  const env = await setup(handlers);
  env.renderApp();
  act(() => useUi.getState().setView("mods"));
  return { ...env, user: userEvent.setup() };
}

const notices = () => useUi.getState().notices.map((n) => n.text);
const tab = (user: ReturnType<typeof userEvent.setup>, name: string) =>
  screen.findByRole("tab", { name }).then((b) => user.click(b));
const quiet = afterChange({ needs_deploy: false });

/** A mod list whose uninstall removes the mod (the refetch then drops its row). */
function modList(names: string[], archives: string[] = []) {
  let mods = names.map((name, i) => mod({ name, position: i + 1 }));
  return {
    mods: () => mods,
    handlers: {
      mods_list: () => ({ mods, changed_since_deploy: false }),
      mod_detail: (a: Args) => ({ mod: mods.find((m) => m.uid === a!.reference), collections: [], fomod: [], wins: 0, loses: 0, url: null }),
      mods_uninstall: (a: Args) => {
        mods = mods.filter((m) => !(a!.refs as string[]).includes(m.uid));
        return { done: true, mods: 1, archives, after: quiet };
      },
    },
  };
}

async function uninstall(user: ReturnType<typeof userEvent.setup>, name: string) {
  const row = (await screen.findByTitle(name)).closest("li")!;
  await user.click(within(row).getByRole("button", { name: "Details" }));
  await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Uninstall" }));
}

describe("#1 uninstall wiring", () => {
  // Fails when the view asks the backend to delete the archive itself (the confirm's option decides), or names the wrong result.
  it("calls modsUninstall(refs, false) and says Uninstalled <name>; focus moves to the next row's Details", async () => {
    const l = modList(["Alpha", "Bravo", "Charlie"]);
    const { t, user } = await open(l.handlers);
    const b = l.mods()[1];
    await uninstall(user, "Bravo");
    await waitFor(() => expect(notices()).toContain("Uninstalled Bravo"));
    expect(t.callsOf("mods_uninstall")).toEqual([{ game: null, refs: [b.uid], delete_archive: false }]);
    // Fails when focus falls to <body> with the removed row.
    await waitFor(() => expect(screen.queryByTitle("Bravo")).toBeNull());
    const c = within(screen.getByTitle("Charlie").closest("li")!).getByRole("button", { name: "Details" });
    await waitFor(() => expect(document.activeElement).toBe(c));
  });

  // Fails when a ticked "Delete archive too" is not reported (the user cannot tell the archive is gone).
  it("says the archive was deleted; focus moves to the previous row when the last one goes", async () => {
    const l = modList(["Alpha", "Bravo"], ["/downloads/b.7z"]);
    const { user } = await open(l.handlers);
    await uninstall(user, "Bravo");
    await waitFor(() => expect(notices()).toContain("Uninstalled Bravo and deleted its archive"));
    const a = within(screen.getByTitle("Alpha").closest("li")!).getByRole("button", { name: "Details" });
    await waitFor(() => expect(document.activeElement).toBe(a));
  });

  // Fails when uninstalling the only mod leaves focus on <body>.
  it("focuses the list heading when the list becomes empty", async () => {
    const l = modList(["Alpha"]);
    const { user } = await open(l.handlers);
    await uninstall(user, "Alpha");
    const heading = await screen.findByRole("heading", { name: "No mods installed yet" });
    await waitFor(() => expect(document.activeElement).toBe(heading));
  });
});

/** Profiles kept by name; profile_save on an existing name returns null like a declined overwrite. */
function profiles(initial: string[], save?: (name: string) => number | null) {
  const names = [...initial];
  const order: string[] = [];
  return {
    names,
    order,
    handlers: {
      profiles_list: () => names.map((name) => ({ name, mods: 2, enabled: 1, saved_at: 0 })),
      profile_save: (a: Args) => {
        const n = String(a!.name);
        order.push(`save ${n}`);
        const r = save ? save(n) : names.includes(n) ? null : 2;
        if (r !== null && !names.includes(n)) names.push(n);
        return r;
      },
      profile_delete: (a: Args) => {
        order.push(`delete ${String(a!.name)}`);
        names.splice(names.indexOf(String(a!.name)), 1);
        return null;
      },
      profile_load: (a: Args) => {
        order.push(`load ${String(a!.name)}`);
        return { missing: 0, added_disabled: 0, after: quiet };
      },
    },
  };
}

const loadButton = async (name: string) =>
  within((await screen.findByText(name, { selector: "span" })).closest("li")!).getByRole("button", { name: "Load" });

describe("#2 profile load snapshot and Undo", () => {
  // Fails when the load runs before the current set is saved, or Undo goes through the auto-saving load.
  it("saves before-<name> first, then loads; Undo loads the snapshot without saving again", async () => {
    const p = profiles(["evening"]);
    const { user } = await open(p.handlers);
    await tab(user, "Profiles");
    const load = await loadButton("evening");
    await user.click(load);
    await waitFor(() => expect(notices()).toContain("Loaded profile evening"));
    expect(p.order).toEqual(["save before-evening", "load evening"]);
    // Fails when focus leaves the Load button (e.g. the list re-mounts on refetch).
    await screen.findByText("before-evening", { selector: "span" });
    expect(document.activeElement).toBe(load);
    await user.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(notices()).toContain("Restored the mod set from before loading evening"));
    expect(p.order).toEqual(["save before-evening", "load evening", "load before-evening"]);
    // Focus stays in the profiles list (the Undo notice is gone), never <body>.
    expect(document.activeElement).toBe(await loadButton("before-evening"));
  });

  // Fails when a user's own before-<name> profile is overwritten.
  it("never overwrites a user profile: picks before-<name>.2", async () => {
    const p = profiles(["evening", "before-evening"]);
    const { user } = await open(p.handlers);
    await tab(user, "Profiles");
    await user.click(await loadButton("evening"));
    await waitFor(() => expect(notices()).toContain("Loaded profile evening"));
    expect(p.order).toEqual(["save before-evening.2", "load evening"]);
  });

  // Fails when a second load piles up before-<name>.2, .3 ... or hits the overwrite prompt.
  it("replaces its own snapshot with delete + save", async () => {
    const p = profiles(["evening"]);
    const { user } = await open(p.handlers);
    await tab(user, "Profiles");
    await user.click(await loadButton("evening"));
    await waitFor(() => expect(notices()).toContain("Loaded profile evening"));
    await user.click(await loadButton("evening"));
    await waitFor(() => expect(p.order).toHaveLength(5));
    expect(p.order.slice(2)).toEqual(["delete before-evening", "save before-evening", "load evening"]);
    expect(p.names).toEqual(["evening", "before-evening"]);
  });

  // Fails when the load goes ahead without a snapshot to undo to.
  it("aborts the load with an error notice when the snapshot is not saved", async () => {
    const p = profiles(["evening"], () => null);
    const { t, user } = await open(p.handlers);
    await tab(user, "Profiles");
    await user.click(await loadButton("evening"));
    await waitFor(() => expect(notices().some((n) => /^Load profile: couldn't save .*before-evening/.test(n))).toBe(true));
    expect(t.callsOf("profile_load")).toHaveLength(0);
    expect(useUi.getState().notices.find((n) => n.tone === "error")).toBeDefined();
  });

  it("snapshotName: free name, own snapshot replaced, user profile skipped", () => {
    expect(snapshotName("x", [], [])).toEqual({ name: "before-x", replace: false });
    expect(snapshotName("x", ["before-x"], ["before-x"])).toEqual({ name: "before-x", replace: true });
    expect(snapshotName("x", ["before-x", "before-x.2"], [])).toEqual({ name: "before-x.3", replace: false });
    expect(snapshotName("x", ["before-x", "before-x.2"], ["before-x.2"])).toEqual({ name: "before-x.2", replace: true });
  });
});

describe("#21 optimistic enable switch", () => {
  // Fails when the switch waits for the job to flip, or stays flipped after the job failed.
  it("flips at once, spins while the job runs, and flips back with an error notice on failure", async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const b = mod({ name: "B", enabled: true });
    const { user } = await open({
      mods_list: () => ({ mods: [b], changed_since_deploy: false }),
      mods_set_enabled: async () => {
        await gate;
        throw { message: "database is locked" };
      },
    });
    const sw = await screen.findByRole("switch", { name: "Disable B" });
    await user.click(sw);
    expect(sw).toHaveAttribute("aria-checked", "false");
    expect(sw).toHaveAttribute("aria-busy", "true");
    act(() => release());
    await waitFor(() => expect(sw).not.toHaveAttribute("aria-busy"));
    expect(sw).toHaveAttribute("aria-checked", "true");
    expect(notices()).toContain("Enable/disable: database is locked");
  });

  // Fails when a successful change flickers back to the old state before the list refetches.
  it("keeps the new state after success", async () => {
    let on = true;
    const { user } = await open({
      mods_list: () => ({ mods: [mod({ id: 7, uid: "b", name: "B", enabled: on })], changed_since_deploy: false }),
      mods_set_enabled: (a: Args) => ((on = a!.enabled as boolean), quiet),
    });
    await user.click(await screen.findByRole("switch", { name: "Disable B" }));
    const sw = await screen.findByRole("switch", { name: "Enable B" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    expect(sw).not.toHaveAttribute("aria-busy");
  });
});

describe("#23 tab and filters survive a view switch", () => {
  // Fails when leaving the Mods view resets the sub-tab or the search (local component state).
  it("keeps the selected tab and the search", async () => {
    const mods = [mod({ name: "SkyUI" }), mod({ name: "Other" })];
    const { user } = await open({ mods_list: () => ({ mods, changed_since_deploy: false }) });
    await user.type(await screen.findByRole("searchbox", { name: "Search mods" }), "sky");
    act(() => useUi.getState().setView("games"));
    act(() => useUi.getState().setView("mods"));
    expect(await screen.findByRole("searchbox", { name: "Search mods" })).toHaveValue("sky");
    expect(screen.queryByText("Other")).toBeNull();
    await tab(user, "Profiles");
    act(() => useUi.getState().setView("games"));
    act(() => useUi.getState().setView("mods"));
    expect(await screen.findByRole("tab", { name: "Profiles" })).toHaveAttribute("aria-selected", "true");
  });
});
