// SPDX-License-Identifier: GPL-3.0-or-later
// Mods view states, errors and focus (UX audit T04: #3 #4 #5 #6 #7 #9 #10 #11 #19 #20 #22).
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useUi } from "../app/store";
import { SKELETON_DELAY_MS } from "../components/ui";
import type { Handler } from "../mock/fake";
import { mod } from "../mock/fixtures";
import { setup } from "../test/harness";
import { DELETE_ARM_MS, NO_FILTER, noMatchText, profileNameError } from "./Mods";

async function open(handlers: Record<string, Handler> = {}) {
  const env = await setup(handlers);
  env.renderApp();
  act(() => useUi.getState().setView("mods"));
  return { ...env, user: userEvent.setup() };
}

const fail = (message: string) => () => {
  throw { message };
};
const notices = () => useUi.getState().notices.map((n) => n.text);
const listed = (mods: ReturnType<typeof mod>[]) => () => ({ mods, changed_since_deploy: false });
const tab = (user: ReturnType<typeof userEvent.setup>, name: string) =>
  screen.findByRole("tab", { name }).then((b) => user.click(b));

function gate<T>(value: T) {
  let release!: () => void;
  const g = new Promise<void>((r) => (release = r));
  return { handler: async () => (await g, value), release: () => act(() => release()) };
}

afterEach(() => vi.useRealTimers());

describe("#3 move arrows at the ends", () => {
  // Fails when the end arrows are natively disabled: focus drops to <body> once a mod reaches the top.
  it("are inactive (focusable), ignore clicks, and keep focus on Move up at the top row", async () => {
    const a = mod({ name: "A", position: 1 });
    const b = mod({ name: "B", position: 2 });
    let order = [a, b];
    const { t, user } = await open({
      mods_list: () => ({ mods: order.map((m, i) => ({ ...m, position: i + 1 })), changed_since_deploy: false }),
      mod_move: () => {
        order = [b, a];
        return { uid: b.uid, from: 2, to: 1, of: 2, after: { deployed: null, needs_deploy: false } };
      },
    });
    const up = await screen.findByRole("button", { name: "Move B up" });
    expect(screen.getByRole("button", { name: "Move A up" })).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("button", { name: "Move A up" })).not.toBeDisabled();
    await user.click(up);
    await waitFor(() => expect(up).toHaveAttribute("aria-disabled", "true"));
    expect(up).toBeInTheDocument();
    expect(up).not.toBeDisabled();
    expect(document.activeElement).toBe(up);
    await user.click(up);
    expect(t.callsOf("mod_move")).toHaveLength(1);
  });
});

describe("#4 mod list read error", () => {
  // Fails when a failed mods_list renders as a bare Empty line with no way to retry.
  it("shows Couldn't read the mod list, the raw error, and Try again refetches", async () => {
    let broken = true;
    const m = mod({ name: "SkyUI" });
    const { user } = await open({
      mods_list: () => {
        if (broken) throw { message: "database is locked" };
        return { mods: [m], changed_since_deploy: false };
      },
    });
    const note = (await screen.findByText(/Couldn't read the mod list\./)).closest("[role=status]") as HTMLElement;
    expect(note).toHaveTextContent("database is locked");
    broken = false;
    await user.click(within(note).getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("list", { name: "Load order" })).toHaveTextContent("SkyUI");
  });
});

describe("#5 conflicts tab read error", () => {
  // Fails when a failed conflicts read shows "Could not load conflicts." with no error or retry.
  it("shows the error with Try again", async () => {
    let broken = true;
    const { t, user } = await open({
      conflicts: () => {
        if (broken) throw { message: "staging missing" };
        return { files: [], plugins: [] };
      },
    });
    await tab(user, "Conflicts");
    expect(await screen.findByText(/Couldn't read conflicts\./)).toBeInTheDocument();
    expect(screen.getByText("staging missing")).toBeInTheDocument();
    broken = false;
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("No two enabled mods provide the same file.")).toBeInTheDocument();
    expect(t.callsOf("conflicts").length).toBeGreaterThanOrEqual(2);
  });
});

describe("#19 Has conflicts filter with a failed conflicts read", () => {
  // Fails when the failure reads as "No mods match" (nothing in conflict) instead of an error.
  it("shows the read error with Try again, not the no-match state", async () => {
    const { user } = await open({ mods_list: listed([mod({ name: "SkyUI" })]), conflicts: fail("staging missing") });
    await screen.findByRole("list", { name: "Load order" });
    await user.click(screen.getByRole("button", { name: /^Filter/ }));
    await user.click(await screen.findByRole("option", { name: "Has conflicts" }));
    expect(await screen.findByText(/Couldn't read conflicts\./)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
    expect(screen.queryByText("No mods match")).toBeNull();
  });
});

describe("#6 profile name rule", () => {
  // Fails when the rule is hidden until the backend rejects the name.
  it("shows the allowed characters, validates on blur, and does not save an invalid name", async () => {
    const { t, user } = await open({ profile_save: () => 3 });
    await tab(user, "Profiles");
    const field = await screen.findByLabelText("Save the current mod set as");
    expect(field).toHaveAccessibleDescription(/Letters, digits, dot, dash and underscore only/);
    await user.type(field, "my evening!");
    // Not while typing the first time.
    expect(field).not.toHaveAttribute("aria-invalid");
    await user.tab();
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText("Not allowed in a profile name: space, “!”.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save profile" }));
    expect(t.callsOf("profile_save")).toHaveLength(0);
  });

  it("mirrors valid_profile_name", () => {
    for (const ok of ["evening", "a.b_c-1", "", "Z9"]) expect(profileNameError(ok), ok).toBeNull();
    for (const bad of ["a b", "x/y", "é", "a!"]) expect(profileNameError(bad), bad).not.toBeNull();
  });
});

describe("#7 Install updates with nothing to install", () => {
  // Fails when the button is natively disabled with no visible reason.
  it("is inactive (focusable) and described by the visible reason", async () => {
    const { t, user } = await open({ updates_install: fail("should not run") });
    await tab(user, "Updates");
    const install = await screen.findByRole("button", { name: "Install updates" });
    expect(install).not.toBeDisabled();
    expect(install).toHaveAttribute("aria-disabled", "true");
    expect(install).toHaveAccessibleDescription(/Install updates becomes available once a check finds some/);
    await user.click(install);
    expect(t.callsOf("updates_install")).toHaveLength(0);
  });
});

describe("#9 profile delete arm", () => {
  const profiles = () => [{ name: "evening", mods: 2, enabled: 1, saved_at: 0 }];
  const del = () => screen.getByRole("button", { name: /^(Delete|Press again to delete)$/ });

  // Fails when the armed state never lapses (a later stray press deletes).
  it("disarms after DELETE_ARM_MS", async () => {
    const { user } = await open({ profiles_list: profiles });
    await tab(user, "Profiles");
    await screen.findByText("evening");
    vi.useFakeTimers();
    fireEvent.click(del());
    expect(del()).toHaveTextContent("Press again to delete");
    act(() => vi.advanceTimersByTime(DELETE_ARM_MS));
    expect(del()).toHaveTextContent(/^Delete$/);
  });

  // Fails when moving focus away leaves Delete armed.
  it("disarms on blur and keeps one fixed width", async () => {
    const { user } = await open({ profiles_list: profiles });
    await tab(user, "Profiles");
    await screen.findByText("evening");
    await user.click(del());
    expect(del()).toHaveTextContent("Press again to delete");
    // The tone changes on arm (#16); the width and border box do not.
    const box = (c: string) => [...new Set(c.split(" ").filter((k) => /^(w-|border)/.test(k)))].sort();
    const armedBox = box(del().className);
    await user.tab();
    expect(del()).toHaveTextContent(/^Delete$/);
    expect(box(del().className)).toEqual(armedBox);
    expect(armedBox).toContain("w-44");
  });

  // Fails when a delete finishes silently.
  it("notifies Deleted profile <name>", async () => {
    const { t, user } = await open({ profiles_list: profiles, profile_delete: () => null });
    await tab(user, "Profiles");
    await screen.findByText("evening");
    await user.click(del());
    await user.click(del());
    await waitFor(() => expect(notices()).toContain("Deleted profile evening"));
    expect(t.callsOf("profile_delete")).toHaveLength(1);
  });
});

describe("#10 profiles loading", () => {
  // Fails when "No saved profiles." flashes while the list is still loading.
  it("shows no empty state until the list arrives", async () => {
    const g = gate([]);
    const { user } = await open({ profiles_list: g.handler });
    await tab(user, "Profiles");
    // The header shows its own "Loading…" while the game loads: look in the view only.
    expect(await within(screen.getByRole("main")).findByText("Loading…")).toBeInTheDocument();
    expect(screen.queryByText("No saved profiles.")).toBeNull();
    g.release();
    expect(await screen.findByText("No saved profiles.")).toBeInTheDocument();
  });
});

describe("#11 mod details read error", () => {
  // Fails when a failed mod_detail leaves "…" forever with no error or retry.
  it("shows the error and Try again in the dialog", async () => {
    let broken = true;
    const m = mod({ name: "SkyUI" });
    const { user } = await open({
      mods_list: listed([m]),
      mod_detail: () => {
        if (broken) throw { message: "no such mod" };
        return { mod: m, collections: [], fomod: [], wins: 1, loses: 0, url: null };
      },
    });
    await user.click(await screen.findByRole("button", { name: "Details" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/Couldn't read this mod's details\./)).toBeInTheDocument();
    expect(within(dialog).getByText("no such mod")).toBeInTheDocument();
    expect(within(dialog).queryByText("…")).toBeNull();
    broken = false;
    await user.click(within(dialog).getByRole("button", { name: "Try again" }));
    expect(await within(dialog).findByText("wins 1 · loses 0")).toBeInTheDocument();
  });
});

describe("#20 no-match state", () => {
  // Fails when an empty Load order card is shown under the no-match state, or the copy is generic.
  it("hides the Load order card and names the active filters", async () => {
    const { user } = await open({ mods_list: listed([mod({ name: "SkyUI" })]) });
    await screen.findByRole("list", { name: "Load order" });
    await user.type(screen.getByRole("searchbox", { name: "Search mods" }), "zzz");
    expect(screen.getByText("No mods match")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Load order" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Load order" })).toBeNull();
    expect(screen.getByText("Nothing in the load order passes: search “zzz”.")).toBeInTheDocument();
  });

  it("names the state and collection filters", () => {
    expect(noMatchText({ query: " sky ", state: "enabled", collection: "def" })).toBe(
      "Nothing in the load order passes: search “sky”, Enabled, collection “def”.",
    );
    expect(noMatchText({ ...NO_FILTER, state: "conflicts", collection: "-" })).toBe(
      "Nothing in the load order passes: Has conflicts, Not in a collection.",
    );
    expect(noMatchText({ ...NO_FILTER, state: "updates" }, true)).toBe("Nothing in the load order passes: Has updates.");
  });
});

describe("#22 delayed Loading…", () => {
  // Fails when Loading… shows at once (a flash on every fast load).
  it("shows only after SKELETON_DELAY_MS", async () => {
    let calledAt = 0;
    const g = gate({ mods: [], changed_since_deploy: false });
    await open({
      mods_list: () => {
        calledAt ||= performance.now();
        return g.handler();
      },
    });
    // The header shows its own "Loading…" while the game loads: look in the view only.
    await within(screen.getByRole("main")).findByText("Loading…", undefined, { timeout: 2000 });
    expect(calledAt).toBeGreaterThan(0);
    expect(performance.now() - calledAt).toBeGreaterThanOrEqual(SKELETON_DELAY_MS - 20);
    g.release();
  });
});
