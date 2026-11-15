// SPDX-License-Identifier: GPL-3.0-or-later
// Mods view flows (UX audit T06): uninstall wiring (#1), profile load (#2), optimistic enable switch (#21), tab and filters kept in the store (#23).
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useUi } from "../app/store";
import type { Args, Handler } from "../mock/fake";
import { afterChange, mod } from "../mock/fixtures";
import { setup } from "../test/harness";

async function open(handlers: Record<string, Handler> = {}) {
  const env = await setup(handlers);
  env.renderApp();
  act(() => useUi.getState().setView("mods"));
  return { ...env, user: userEvent.setup() };
}

const notices = () => [useUi.getState().toast].flatMap((x) => (x ? [x] : [])).map((n) => (n.detail ? `${n.text} · ${n.detail}` : n.text));
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
  await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: `Remove ${name}` }));
}

describe("#1 uninstall wiring", () => {
  // Fails when the view asks the backend to delete the archive itself (the confirm's option decides), or names the wrong result.
  it("calls modsUninstall(refs, false) and says Uninstalled <name>; focus moves to the next row's Details", async () => {
    const l = modList(["Alpha", "Bravo", "Charlie"]);
    const { t, user } = await open(l.handlers);
    const b = l.mods()[1];
    await uninstall(user, "Bravo");
    await waitFor(() => expect(notices()).toContain("Uninstalled · Bravo"));
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
    await waitFor(() => expect(notices()).toContain("Uninstalled · Bravo · archive deleted"));
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

describe("#2 profile load", () => {
  // Fails when loading saves a snapshot profile first again (the Undo snapshot is gone), or the toast is lost.
  it("loads the profile directly and says so; focus stays on Load", async () => {
    const p = profiles(["evening"]);
    const { user } = await open(p.handlers);
    await tab(user, "Profiles");
    const load = await loadButton("evening");
    await user.click(load);
    await waitFor(() => expect(notices()).toContain("Loaded · Profile evening"));
    expect(p.order).toEqual(["load evening"]);
    expect(document.activeElement).toBe(load);
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
    expect(notices()).toContain("Enable/disable failed · database is locked");
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
