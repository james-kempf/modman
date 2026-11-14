// SPDX-License-Identifier: GPL-3.0-or-later
// Mods view hierarchy, navigation and style (UX audit T05: #14 #15 #16 #17 #18 #24-#29).
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useUi } from "../app/store";
import type { Handler } from "../mock/fake";
import { deployReport, mod } from "../mock/fixtures";
import { performNav } from "../nav/actions";
import { setup } from "../test/harness";
import source from "./Mods.tsx?raw";

async function open(handlers: Record<string, Handler> = {}) {
  const env = await setup(handlers);
  env.renderApp();
  act(() => useUi.getState().setView("mods"));
  return { ...env, user: userEvent.setup() };
}

const listed = (mods: ReturnType<typeof mod>[], changed = false) => () => ({ mods, changed_since_deploy: changed });
const sections = () => screen.findByRole("tablist", { name: "Mods sections" });
const detailOf = (m: ReturnType<typeof mod>) => () => ({ mod: m, collections: [], fomod: [], wins: 0, loses: 0, url: null });

describe("#14 sub-tabs are focusable Tabs, not bound to LB/RB", () => {
  // Fails when the sub-tabs are a Segmented group (no tablist/roving tabindex) or arrows select instead of moving focus.
  it("is a roving tablist: the selected tab is the one stop, arrows move focus, Enter selects", async () => {
    const { user } = await open({ mods_list: listed([mod({ name: "SkyUI" })]) });
    const list = await sections();
    expect(screen.queryByRole("group", { name: "Show" })).toBeNull();
    const tabs = within(list).getAllByRole("tab");
    expect(tabs.map((t) => t.textContent)).toEqual(["Load order", "Conflicts", "Updates", "Profiles"]);
    expect(tabs.map((t) => t.getAttribute("tabindex"))).toEqual(["0", "-1", "-1", "-1"]);
    act(() => tabs[0].focus());
    await user.keyboard("{ArrowRight}");
    expect(document.activeElement).toBe(tabs[1]);
    expect(tabs[0]).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(tabs[1]).toHaveAttribute("aria-selected", "true"));
    expect(screen.getByRole("tabpanel", { name: "Conflicts" })).toBeInTheDocument();
  });

  // Fails when LB/RB (PageUp/PageDown, or the pad's tab action) switch the Mods sub-tabs instead of the app's views.
  it("LB/RB and PageUp/PageDown still switch app views from the Mods view", async () => {
    const { user } = await open({ mods_list: listed([mod({ name: "SkyUI" })]) });
    const conflicts = within(await sections()).getByRole("tab", { name: "Conflicts" });
    await user.click(conflicts);
    expect(conflicts).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{PageDown}");
    expect(useUi.getState().view).toBe("collections");
    await user.keyboard("{PageUp}");
    expect(useUi.getState().view).toBe("mods");
    act(() => within(screen.getByRole("tablist", { name: "Mods sections" })).getAllByRole("tab")[0].focus());
    act(() => performNav({ type: "tab", delta: -1 }));
    expect(useUi.getState().view).toBe("browse");
  });
});

describe("#15 detail dialog footer", () => {
  // Fails when Close is the primary (accent) button or Uninstall sits among the safe actions on the right.
  it("puts Uninstall (danger) apart on the left and Close (default tone, focused) at the right end", async () => {
    const m = mod({ name: "SkyUI" });
    const { user } = await open({ mods_list: listed([m]), mod_detail: detailOf(m) });
    await user.click(await screen.findByRole("button", { name: "Details" }));
    const dialog = await screen.findByRole("dialog");
    const uninstall = within(dialog).getByRole("button", { name: "Uninstall" });
    const reinstall = within(dialog).getByRole("button", { name: /Reinstall/ });
    const close = within(dialog).getByRole("button", { name: "Close" });
    expect(close.className).not.toMatch(/bg-accent-400/);
    expect(close.className).toMatch(/bg-fill-2/);
    expect(uninstall.className).toMatch(/bg-rose-500/);
    const footer = uninstall.parentElement!;
    expect(footer.className).toMatch(/justify-between/);
    expect(footer.contains(close) && !close.parentElement!.contains(uninstall)).toBe(true);
    expect(uninstall.compareDocumentPosition(reinstall) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(reinstall.compareDocumentPosition(close) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await waitFor(() => expect(document.activeElement).toBe(close));
  });
});

describe("#16 profile row tones", () => {
  // Fails when Load is primary (one accent button per profile row) or Delete is loud before it is armed.
  it("Load is the default tone; Delete is ghost until armed, then danger", async () => {
    const { user } = await open({ profiles_list: () => [{ name: "evening", mods: 2, enabled: 1, saved_at: 0 }] });
    await user.click(await screen.findByRole("tab", { name: "Profiles" }));
    await screen.findByText("evening");
    const load = screen.getByRole("button", { name: "Load" });
    expect(load.className).not.toMatch(/bg-accent-400/);
    expect(load.className).toMatch(/bg-fill-2/);
    const del = () => screen.getByRole("button", { name: /^(Delete|Press again to delete)$/ });
    expect(del().className).not.toMatch(/bg-rose-500/);
    expect(del().className).toMatch(/text-zinc-300/);
    await user.click(del());
    expect(del()).toHaveTextContent("Press again to delete");
    expect(del().className).toMatch(/bg-rose-500/);
  });
});

describe("#17 conflict, plugin and update rows open the mod's Details", () => {
  const a = mod({ name: "Alpha" });
  const b = mod({ name: "Bravo" });
  const ref = (m: ReturnType<typeof mod>) => ({ id: m.id, uid: m.uid, name: m.name, priority: m.position });
  const handlers = {
    mods_list: listed([a, b]),
    mod_detail: (args: { reference?: string }) => detailOf(args?.reference === a.uid ? a : b)(),
    conflicts: () => ({
      files: [{ rel_path: "textures/sky.dds", winner: ref(b), losers: [ref(a)], plugin: false }],
      plugins: [
        { name: "Alpha.esp", provider: ref(a), overridden: [], load_index: 1, active: true },
        { name: "Orphan.esp", provider: null, overridden: [], load_index: 2, active: true },
      ],
    }),
  } as unknown as Record<string, Handler>;

  // Fails when the rows are bare li tabIndex=0 (focusable but do nothing on Enter/A).
  it("file conflict row (Enter) opens the winner; plugin row (click) opens its provider", async () => {
    const { user } = await open(handlers);
    await user.click(await screen.findByRole("tab", { name: "Conflicts" }));
    const files = await screen.findByRole("list", { name: "File conflicts" });
    expect(within(files).queryAllByRole("listitem").some((li) => li.hasAttribute("tabindex"))).toBe(false);
    const row = await within(files).findByRole("button", { name: /textures\/sky\.dds/ });
    expect(row.className).toMatch(/\brounded-inner\b/);
    act(() => row.focus());
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("dialog", { name: /Bravo/ })).toBeInTheDocument();
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    const plugins = screen.getByRole("list", { name: "Plugins" });
    await user.click(within(plugins).getByRole("button", { name: /Alpha\.esp/ }));
    expect(await screen.findByRole("dialog", { name: /Alpha/ })).toBeInTheDocument();
    // A plugin no mod provides has nothing to open.
    expect(within(plugins).queryByRole("button", { name: /Orphan\.esp/ })).toBeNull();
  });

  // Fails when an update row is a dead focus stop.
  it("update row opens the mod being updated", async () => {
    const { user } = await open({
      ...handlers,
      updates_check: () => ({
        checked: 2, fetched: 2, stopped_early: false, installed: 0,
        updates: [{ mod: a, file: { file_id: 1, name: "f", version: "2" }, pinned: false }],
      }),
    });
    await user.click(await screen.findByRole("tab", { name: "Updates" }));
    await user.click(await screen.findByRole("button", { name: "Check for updates" }));
    await user.click(await screen.findByRole("button", { name: /^Alpha/ }));
    expect(await screen.findByRole("dialog", { name: /Alpha/ })).toBeInTheDocument();
  });
});

describe("#18 changed-since-deploy banner", () => {
  // Fails when the banner only states the problem (no way to deploy) or uses an off-formula tint.
  it("uses the status tint formula and Deploy now deploys this game", async () => {
    const { t, user } = await open({
      mods_list: listed([mod({ name: "SkyUI" })], true),
      deploy: () => deployReport({ placed: 7 }),
    });
    const text = await screen.findByText("The mod set changed since the last deploy.");
    const banner = text.parentElement!;
    expect(banner.className).toMatch(/\bbg-amber-400\/10\b/);
    expect(banner.className).toMatch(/\bborder-amber-400\/15\b/);
    expect(banner.className).toMatch(/\btext-amber-300\b/);
    await user.click(within(banner).getByRole("button", { name: "Deploy now" }));
    await waitFor(() => expect(t.callsOf("deploy")).toHaveLength(1));
    await waitFor(() => expect(useUi.getState().notices.map((n) => n.text).join()).toMatch(/Deployed 7 files/));
  });
});

describe("#28 Loaders card", () => {
  // Fails when the Loaders card says Requirements manages them but offers no way there.
  it("links to the Requirements view", async () => {
    const { user } = await open({
      mods_list: () => ({
        mods: [mod({ name: "SkyUI" })],
        loaders: [{ key: "smapi", name: "SMAPI", version: "4.0", nexus_mod_id: null, install_id: 1, status: "installed" }],
        changed_since_deploy: false,
      }),
    });
    const card = (await screen.findByRole("list", { name: "Loaders" })).closest("section")!;
    await user.click(within(card).getByRole("button", { name: "Manage in Requirements" }));
    expect(useUi.getState().view).toBe("requirements");
  });
});

describe("#24 #25 #27 load order rows", () => {
  // Fails on a raw opacity (not the stale token), 18px move glyphs, or proportional digits in counts.
  it("dims a disabled mod at opacity-stale, uses 20px move glyphs and tabular counts", async () => {
    const off = mod({ name: "Off", enabled: false, position: 1 });
    await open({ mods_list: listed([off, mod({ name: "On", position: 2 })]) });
    const list = await screen.findByRole("list", { name: "Load order" });
    const row = within(list).getAllByRole("listitem")[0];
    expect(row.querySelector(".opacity-stale")).not.toBeNull();
    expect(row.querySelector(".opacity-40")).toBeNull();
    const glyph = within(row).getByRole("button", { name: "Move Off down" }).querySelector("svg")!;
    expect(glyph.getAttribute("width")).toBe("20");
    expect(screen.getByText(/2 mods · 1 enabled/).parentElement!.className).toMatch(/\btabular-nums\b/);
  });
});

describe("#14 #29 source hygiene", () => {
  // Fails when Segmented, arbitrary text sizes or primitive zinc text colours come back into Mods.tsx.
  it("has no Segmented, text-[Npx] or text-zinc-*", () => {
    expect(source).not.toMatch(/\bSegmented\b/);
    expect(source).not.toMatch(/text-\[\d/);
    expect(source).not.toMatch(/text-zinc-/);
  });
});
