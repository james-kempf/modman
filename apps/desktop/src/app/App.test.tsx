// SPDX-License-Identifier: GPL-3.0-or-later
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { afterChange, deployReport, experimentalGame, game, mod } from "../mock/fixtures";
import { setup } from "../test/harness";
import { VIEWS, useUi } from "./store";

const heading = (name: string) => screen.findByRole("heading", { level: 1, name });

describe("keyboard-only navigation", () => {
  it("reaches all 7 views from the tab rail with arrows + Enter", async () => {
    const { renderApp } = await setup();
    renderApp();
    const user = userEvent.setup();
    // Focus starts on the current tab.
    expect(document.activeElement).toHaveAttribute("data-tab", "overview");
    await heading("Overview");
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
    await user.keyboard("{PageDown}");
    await heading("Mods");
    expect(document.activeElement).toHaveAttribute("data-tab", "mods");
    await user.keyboard("{PageUp}{PageUp}");
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
    expect(tabs).toHaveLength(7);
    tabs.forEach((t) => expect(t.className).toMatch(/min-h-12/));
  });
});

describe("experimental badge", () => {
  it("is shown for a game with support=experimental (header, Mods, Overview)", async () => {
    const { renderApp } = await setup({}, [experimentalGame()]);
    renderApp();
    await screen.findByText("Cyberpunk 2077", { selector: "header span" });
    expect(screen.getAllByTestId("experimental-badge").length).toBeGreaterThanOrEqual(2);
    act(() => useUi.getState().setView("mods"));
    await heading("Mods");
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
  it("AfterChange.needs_deploy offers Deploy now, which deploys", async () => {
    const m = mod({ name: "SkyUI", enabled: true });
    const { renderApp, t } = await setup({
      mods_list: () => ({ mods: [m], changed_since_deploy: false }),
      mods_set_enabled: () => afterChange({ needs_deploy: true }),
      deploy: () => deployReport({ placed: 42 }),
    });
    renderApp();
    act(() => useUi.getState().setView("mods"));
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Disable SkyUI" }));
    expect(t.callsOf("mods_set_enabled")).toEqual([{ game: null, ids: [m.id], enabled: false }]);
    await user.click(await screen.findByRole("button", { name: "Deploy now" }));
    await waitFor(() => expect(t.callsOf("deploy")).toHaveLength(1));
    await screen.findByText(/Deployed 42 files/);
    expect(useUi.getState().needsDeploy).toBe(false);
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
