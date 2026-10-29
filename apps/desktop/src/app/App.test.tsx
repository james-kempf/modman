// SPDX-License-Identifier: GPL-3.0-or-later
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { afterChange, deployReport, experimentalGame, game, mod } from "../mock/fixtures";
import { setup } from "../test/harness";
import { VIEWS, useUi } from "./store";

const heading = (name: string) => screen.findByRole("heading", { level: 1, name });

describe("keyboard-only navigation", () => {
  it("reaches all 8 views from the tab rail with arrows + Enter", async () => {
    const { renderApp } = await setup();
    renderApp();
    const user = userEvent.setup();
    // The app opens on the Games library; focus starts on its tab.
    expect(document.activeElement).toHaveAttribute("data-tab", "games");
    await heading("Games");
    for (const v of VIEWS.slice(1)) {
      await user.keyboard("{ArrowDown}");
      expect(document.activeElement).toHaveAttribute("data-tab", v.id);
      await user.keyboard("{Enter}");
      await heading(v.label);
      expect(document.activeElement).toHaveAttribute("aria-current", "page");
    }
  });

  it("PageDown/PageUp switch tabs; arrows enter the view; Escape returns to the tab", async () => {
    const { renderApp } = await setup();
    renderApp();
    const user = userEvent.setup();
    await user.keyboard("{PageDown}{PageDown}");
    await heading("Mods");
    expect(document.activeElement).toHaveAttribute("data-tab", "mods");
    await user.keyboard("{PageUp}{PageUp}{PageUp}");
    await heading("Settings");
    // Walk past the remaining tabs into the Settings content.
    const main = screen.getByRole("main");
    for (let i = 0; i < 20 && !main.contains(document.activeElement); i++) await user.keyboard("{ArrowDown}");
    expect(main).toContainElement(document.activeElement as HTMLElement);
    await user.keyboard("{Escape}");
    expect(document.activeElement).toHaveAttribute("data-tab", "settings");
  });

  it("has a visible focus target on every tab (buttons, no hover-only controls)", async () => {
    const { renderApp } = await setup();
    renderApp();
    const tabs = within(screen.getByRole("navigation", { name: "Views" })).getAllByRole("button");
    expect(tabs).toHaveLength(8);
    tabs.forEach((t) => expect(t.className).toMatch(/min-h-12/));
  });
});

describe("sidebar game title", () => {
  // Fails when the active game's name goes back into a card with a status dot and mod count, loses its
  // full-name tooltip, or scrolls away with the tabs (the header must stay pinned in the scrolling sidebar).
  it("is plain pinned text: no card, dot or count", async () => {
    const { renderApp } = await setup();
    renderApp();
    const name = await screen.findByText("Skyrim Special Edition", { selector: "header span" });
    expect(name).toHaveAttribute("title", "Skyrim Special Edition");
    expect(name.className).toMatch(/\btruncate\b/);
    expect(name.className).toMatch(/\btext-zinc-100\b/);
    const header = name.closest("header")!;
    expect(header.className).toMatch(/\bsticky\b/);
    expect(header.className).toMatch(/\btop-0\b/);
    expect(header.querySelector(".rounded-pill, [title$=' mods']")).toBeNull();
    expect(header.querySelector(".border, [class*='bg-ink-2']")).toBeNull();
  });

  // Fails when the experimental badge comes back next to the sidebar title (the user removed it there).
  it("shows no experimental badge, even for an experimental game", async () => {
    const { renderApp } = await setup({}, [experimentalGame()]);
    renderApp();
    const name = await screen.findByText("Cyberpunk 2077", { selector: "header span" });
    expect(within(name.closest("header")!).queryByTestId("experimental-badge")).toBeNull();
  });
});

describe("experimental badge", () => {
  // Fails when the badge drops off Overview, or comes back in the sidebar header or the Mods header (removed there).
  it("is shown on Overview for a game with support=experimental, not in the sidebar or Mods", async () => {
    const { renderApp } = await setup({}, [experimentalGame()]);
    renderApp();
    const name = await screen.findByText("Cyberpunk 2077", { selector: "header span" });
    expect(within(name.closest("header")!).queryByTestId("experimental-badge")).toBeNull();
    act(() => useUi.getState().setView("mods"));
    await heading("Mods");
    expect(within(screen.getByRole("main")).queryByTestId("experimental-badge")).toBeNull();
    act(() => useUi.getState().setView("overview"));
    await heading("Overview");
    expect(within(screen.getByRole("main")).getByTestId("experimental-badge")).toHaveTextContent("Experimental");
  });

  it("is not shown for a tested game", async () => {
    const { renderApp } = await setup({}, [game()]);
    renderApp();
    await screen.findByText("Skyrim Special Edition", { selector: "header span" });
    expect(screen.queryByTestId("experimental-badge")).toBeNull();
  });
});

describe("structured results", () => {
  // U02: fails when a mod change (enable, move) brings back the "not in the game yet" toast or the
  // sidebar "Deploy changes" button; the deploy stays on the Overview's Deployment card.
  it("AfterChange.needs_deploy marks the game out of sync without a notice; Overview deploys", async () => {
    const [a, b] = [mod({ name: "SkyUI", enabled: true }), mod({ name: "USSEP", enabled: true, position: 2 })];
    const { renderApp, t } = await setup({
      mods_list: () => ({ mods: [a, b], changed_since_deploy: false }),
      mods_set_enabled: () => afterChange({ needs_deploy: true }),
      mod_move: () => ({ uid: b.uid, from: 2, to: 1, of: 2, after: afterChange({ needs_deploy: true }) }),
      deploy: () => deployReport({ placed: 42 }),
    });
    renderApp();
    act(() => useUi.getState().setView("mods"));
    const user = userEvent.setup();
    // The enable control is a switch (legacy toggle).
    await user.click(await screen.findByRole("switch", { name: "Disable SkyUI" }));
    expect(t.callsOf("mods_set_enabled")).toEqual([{ game: null, ids: [a.id], enabled: false }]);
    await waitFor(() => expect(useUi.getState().deployDue.skyrimspecialedition).toBeDefined());
    await user.click(screen.getByRole("button", { name: "Move USSEP up" }));
    await waitFor(() => expect(t.callsOf("mod_move")).toHaveLength(1));
    expect(useUi.getState().notices).toEqual([]);
    expect(screen.queryByRole("button", { name: "Deploy changes" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Deploy now" })).toBeNull();
    act(() => useUi.getState().setView("overview"));
    expect(await screen.findByText("Changes waiting to be deployed")).toBeInTheDocument();
    await user.click(within(screen.getByRole("main")).getByRole("button", { name: "Deploy" }));
    await waitFor(() => expect(t.callsOf("deploy")).toHaveLength(1));
    await screen.findByText(/Deployed 42 files/);
    expect(useUi.getState().deployDue).toEqual({});
  });

  it("command errors become a notice with the CmdError message", async () => {
    const { renderApp } = await setup({
      nxm_submit: () => {
        throw { message: "not an nxm link" };
      },
    });
    renderApp();
    act(() => useUi.getState().setView("install"));
    const user = userEvent.setup();
    await user.type(await screen.findByLabelText("nxm link"), "nxm://bad");
    await user.click(screen.getByRole("button", { name: "Download" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("not an nxm link");
  });
});

describe("job island", () => {
  // Fails when the island does not follow job events (stale "running" pill or none at all).
  it("shows the running job with its latest line, then its outcome", async () => {
    const { renderApp, t } = await setup();
    renderApp();
    act(() => {
      t.emit("vessel:job", { id: 4, kind: "deploy", state: "started", error: null });
      t.emit("vessel:activity", { job: 4, msg: { level: "info", spans: [{ text: "Linking files", style: "plain" }] }, text: "Linking files" });
    });
    const island = await screen.findByRole("status", { name: "Current job" });
    expect(island).toHaveTextContent("Deploy");
    expect(island).toHaveTextContent("Linking files");
    act(() => t.emit("vessel:job", { id: 4, kind: "deploy", state: "failed", error: "disk full" }));
    expect(screen.getByRole("status", { name: "Current job" })).toHaveTextContent("Failed: disk full");
  });
});
