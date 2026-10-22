// SPDX-License-Identifier: GPL-3.0-or-later
// Sidebar game picker and the add-game dialog, over the full App (keyboard
// nav and the gamepad action layer installed).
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { experimentalGame, game } from "../mock/fixtures";
import { performNav } from "../nav/actions";
import { setup } from "../test/harness";

const twoGames = () => [game(), experimentalGame({ active: false, mod_count: 7 })];
const trigger = () => screen.findByRole("button", { name: "Game: Skyrim Special Edition" });

describe("game picker", () => {
  // Fails when the picker is not keyboard operable or picking does not run game_use + refresh.
  it("opens with Enter on the active game, moves with arrows, picks with Enter and refreshes", async () => {
    const { t, renderApp } = await setup({ game_use: () => null }, twoGames());
    renderApp();
    const user = userEvent.setup();
    const btn = await trigger();
    btn.focus();
    await user.keyboard("{Enter}");
    const menu = screen.getByRole("menu", { name: "Games" });
    expect(btn).toHaveAttribute("aria-expanded", "true");
    expect(document.activeElement).toBe(within(menu).getByRole("menuitemradio", { name: /Skyrim/ }));
    const listed = t.callsOf("games_list").length;
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(within(menu).getByRole("menuitemradio", { name: /Cyberpunk/ }));
    await user.keyboard("{Enter}");
    await waitFor(() => expect(t.callsOf("game_use")).toEqual([{ domain: "cyberpunk2077" }]));
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(btn);
    await waitFor(() => expect(t.callsOf("games_list").length).toBeGreaterThan(listed));
  });

  // Fails when Escape / gamepad B leaves the menu open or drops focus to <body>.
  it("Escape and gamepad B close without switching; focus returns to the button", async () => {
    const { t, renderApp } = await setup({ game_use: () => null }, twoGames());
    renderApp();
    const user = userEvent.setup();
    const btn = await trigger();
    btn.focus();
    await user.keyboard("{Enter}");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(btn);
    // Gamepad: A opens, D-pad moves, B closes.
    act(() => performNav({ type: "select" }));
    expect(screen.getByRole("menu")).toBeInTheDocument();
    act(() => performNav({ type: "move", dir: "down" }));
    expect(document.activeElement).toHaveAccessibleName(/Cyberpunk/);
    // Tabs cannot change under the open menu.
    act(() => performNav({ type: "tab", delta: 1 }));
    expect(screen.getByRole("heading", { level: 1, name: "Overview" })).toBeInTheDocument();
    act(() => performNav({ type: "back" }));
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(btn);
    expect(t.callsOf("game_use")).toEqual([]);
  });

  // Fails when the menu omits the experimental label.
  it("marks experimental games in the menu", async () => {
    const { renderApp } = await setup({}, twoGames());
    renderApp();
    await userEvent.setup().click(await trigger());
    const item = within(screen.getByRole("menu")).getByRole("menuitemradio", { name: /Cyberpunk/ });
    expect(within(item).getByTestId("experimental-badge")).toHaveTextContent("Experimental");
    expect(item).toHaveAttribute("aria-checked", "false");
  });

  // Fails when an empty game list still renders a picker (nothing to pick).
  it("with zero games shows only Add game", async () => {
    const { renderApp } = await setup({}, []);
    renderApp();
    const header = screen.getByRole("banner");
    await waitFor(() => expect(within(header).queryByText("Loading…")).toBeNull());
    expect(within(header).getByRole("button", { name: "Add game" })).toBeInTheDocument();
    expect(within(header).queryByRole("button", { name: /^Game:|Choose a game/ })).toBeNull();
  });
});

describe("add-game dialog", () => {
  const detected = [
    { domain: "fallout4", name: "Fallout 4", install_dir: "/s/Fallout 4", support: "experimental", configured: false },
    { domain: "skyrimspecialedition", name: "Skyrim Special Edition", install_dir: "/s/SSE", support: "tested", configured: true },
  ];

  // Fails when detected games are not offered, or Add does not pass the Steam folder / close the dialog.
  it("adds a game found in Steam and returns focus to Add game", async () => {
    const { t, renderApp } = await setup({ games_detect: () => detected, game_add: () => game({ domain: "fallout4" }) });
    renderApp();
    const user = userEvent.setup();
    const open = await within(screen.getByRole("banner")).findByRole("button", { name: "Add game" });
    open.focus();
    await user.keyboard("{Enter}");
    const dlg = await screen.findByRole("dialog", { name: "Add a game" });
    // Configured games are not offered again.
    const list = await within(dlg).findByRole("list", { name: "Detected games" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    await user.click(within(list).getByRole("button", { name: "Add Fallout 4" }));
    await waitFor(() =>
      expect(t.callsOf("game_add")).toEqual([{ domain: "fallout4", path: "/s/Fallout 4", subdir: null, profile: null }]),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(open));
  });

  // Fails when the manual form drops the domain or folder, or Escape does not close.
  it("adds any game by Nexus domain; Escape closes", async () => {
    const { t, renderApp } = await setup({ game_add: () => undefined });
    renderApp();
    const user = userEvent.setup();
    await user.click(await within(screen.getByRole("banner")).findByRole("button", { name: "Add game" }));
    const dlg = await screen.findByRole("dialog", { name: "Add a game" });
    await within(dlg).findByText(/No other supported games/);
    await user.type(within(dlg).getByLabelText("Nexus game domain"), "stardewvalley");
    await user.type(within(dlg).getByLabelText("Game folder (optional)"), "/games/sv");
    await user.click(within(dlg).getByRole("button", { name: "Add game" }));
    await waitFor(() =>
      expect(t.callsOf("game_add")).toEqual([{ domain: "stardewvalley", path: "/games/sv", subdir: null, profile: null }]),
    );
    // game_add returned nothing (declined in a prompt): the dialog stays.
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    within(dlg).getByLabelText("Nexus game domain").focus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
