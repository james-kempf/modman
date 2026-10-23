// SPDX-License-Identifier: GPL-3.0-or-later
// Game library (landing view) and the add-game dialog, over the full App
// (keyboard nav and the gamepad action layer installed).
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { refreshAll } from "../app/queries";
import { useUi } from "../app/store";
import { experimentalGame, game } from "../mock/fixtures";
import type { Handler } from "../mock/fake";
import { performNav } from "../nav/actions";
import { setup } from "../test/harness";

const JPEG = "data:image/jpeg;base64,/9j/4AAQ";
const twoGames = () => [game(), experimentalGame({ active: false, mod_count: 7 })];

/** The library over a backend whose game_use switches the active game. */
async function library(handlers: Record<string, Handler> = {}, games = twoGames()) {
  let current = games;
  const env = await setup(
    {
      games_list: () => current,
      game_use: (a) => {
        current = current.map((g) => ({ ...g, active: g.domain === (a as { domain: string }).domain }));
        return null;
      },
      game_art: () => null,
      ...handlers,
    },
    games,
  );
  env.renderApp();
  const list = await screen.findByRole("list", { name: "Game library" });
  return { ...env, list, user: userEvent.setup() };
}
const tile = (name: string) => within(screen.getByRole("list", { name: "Game library" })).getByRole("button", { name });
const overview = () => screen.findByRole("heading", { level: 1, name: "Overview" });

describe("game library", () => {
  // Fails when the app does not open on the library, or a game / the Add game tile is missing or misplaced.
  it("is the landing view: one tile per game, Add game last", async () => {
    const { list } = await library();
    expect(useUi.getState().view).toBe("games");
    expect(document.activeElement).toHaveAttribute("data-tab", "games");
    const tiles = within(list).getAllByRole("button");
    expect(tiles.map((b) => b.getAttribute("aria-label") ?? b.textContent)).toEqual([
      "Skyrim Special Edition",
      "Cyberpunk 2077",
      "Add game",
    ]);
    expect(tiles[0]).toHaveAttribute("aria-current", "true");
    expect(tiles[0]).toHaveAccessibleDescription(/Active/);
    expect(tiles[1]).not.toHaveAttribute("aria-current");
  });

  // Fails when picking another game does not run game_use or does not open its Overview.
  it("clicking a game makes it active and opens its Overview", async () => {
    const { t, user } = await library();
    await user.click(tile("Cyberpunk 2077"));
    await waitFor(() => expect(t.callsOf("game_use")).toEqual([{ domain: "cyberpunk2077" }]));
    await overview();
    expect(await within(screen.getByRole("main")).findByRole("heading", { level: 2, name: "Cyberpunk 2077" })).toBeInTheDocument();
    expect(document.activeElement).toHaveAttribute("data-tab", "overview");
  });

  // Fails when the active game's tile switches again instead of only opening its Overview.
  it("Enter on the active game opens its Overview without game_use", async () => {
    const { t, user } = await library();
    tile("Skyrim Special Edition").focus();
    await user.keyboard("{Enter}");
    await overview();
    expect(t.callsOf("game_use")).toEqual([]);
  });

  // Fails when a failed switch still navigates (the Overview would show the old game).
  it("stays on the library when game_use fails", async () => {
    const { user } = await library({
      game_use: () => {
        throw { message: "database is locked" };
      },
    });
    await user.click(tile("Cyberpunk 2077"));
    expect(await screen.findByRole("alert")).toHaveTextContent("Switch game: database is locked");
    expect(useUi.getState().view).toBe("games");
  });

  // Fails when the tiles are not a 2D grid for the D-pad (Down must reach the tile below, not the next one).
  it("gamepad D-pad moves between tiles in 2D and A selects", async () => {
    const { t, list } = await library({}, [
      game(),
      experimentalGame({ active: false }),
      experimentalGame({ domain: "fallout4", name: "Fallout 4", active: false }),
      experimentalGame({ domain: "starfield", name: "Starfield", active: false }),
    ]);
    // Three columns of 200x340 tiles (jsdom has no layout).
    within(list)
      .getAllByRole("button")
      .forEach((b, i) => {
        const x = 400 + (i % 3) * 220;
        const y = 100 + Math.floor(i / 3) * 360;
        b.getBoundingClientRect = () => new DOMRect(x, y, 200, 340);
      });
    tile("Skyrim Special Edition").focus();
    act(() => performNav({ type: "move", dir: "right" }));
    expect(document.activeElement).toBe(tile("Cyberpunk 2077"));
    act(() => performNav({ type: "move", dir: "down" }));
    expect(document.activeElement).toBe(within(list).getByRole("button", { name: "Add game" }));
    act(() => performNav({ type: "move", dir: "left" }));
    expect(document.activeElement).toBe(tile("Starfield"));
    act(() => performNav({ type: "move", dir: "up" }));
    expect(document.activeElement).toBe(tile("Skyrim Special Edition"));
    act(() => performNav({ type: "move", dir: "down" }));
    act(() => performNav({ type: "select" }));
    await waitFor(() => expect(t.callsOf("game_use")).toEqual([{ domain: "starfield" }]));
    await overview();
  });

  // Fails when an experimental game is not labelled, or a tested one is labelled experimental.
  it("labels support: experimental vs tested", async () => {
    await library();
    expect(within(tile("Cyberpunk 2077")).getByTestId("experimental-badge")).toHaveTextContent("Experimental");
    expect(within(tile("Skyrim Special Edition")).queryByTestId("experimental-badge")).toBeNull();
    expect(tile("Skyrim Special Edition")).toHaveAccessibleDescription(/Tested/);
  });

  // Fails when a game whose deploy is due (backend flag or a change this session) has no marker.
  it("marks games whose deploy is due", async () => {
    await library({
      mods_list: (a) => ({ mods: [], changed_since_deploy: (a as { game: string }).game === "cyberpunk2077" }),
    });
    await waitFor(() => expect(within(tile("Cyberpunk 2077")).getByTestId("deploy-due")).toHaveTextContent("Deploy due"));
    expect(within(tile("Skyrim Special Edition")).queryByTestId("deploy-due")).toBeNull();
    act(() => useUi.getState().markDeployDue("skyrimspecialedition"));
    expect(within(tile("Skyrim Special Edition")).getByTestId("deploy-due")).toBeInTheDocument();
  });

  // Fails when art is not shown as a data: image, or a game without art / app id has no monogram.
  it("shows the library art, else a monogram", async () => {
    const { t } = await library(
      { game_art: (a) => ((a as { domain: string }).domain === "skyrimspecialedition" ? JPEG : null) },
      [...twoGames(), game({ domain: "manual", name: "Manual Game", appid: null, active: false })],
    );
    const art = await within(tile("Skyrim Special Edition")).findByTestId("game-art");
    expect(art).toHaveAttribute("src", JPEG);
    expect(art).toHaveAttribute("alt", "");
    await waitFor(() => expect(within(tile("Cyberpunk 2077")).getByTestId("game-monogram")).toHaveTextContent("C2"));
    expect(within(tile("Manual Game")).getByTestId("game-monogram")).toHaveTextContent("MG");
    // No app id: nothing to look up.
    expect(t.callsOf("game_art")).toEqual([{ domain: "skyrimspecialedition" }, { domain: "cyberpunk2077" }]);
  });

  // Fails when a non-image string from the backend ends up in <img src>.
  it("ignores art that is not a data: image", async () => {
    await library({ game_art: () => "https://evil.example/x.jpg" });
    await waitFor(() => expect(within(tile("Skyrim Special Edition")).getByTestId("game-monogram")).toBeInTheDocument());
    expect(screen.queryByTestId("game-art")).toBeNull();
  });

  // Fails when every command or job refresh looks the art up again (it may hit the network).
  it("art is not refetched by the post-command refresh", async () => {
    const { t, qc } = await library();
    await waitFor(() => expect(t.callsOf("game_art")).toHaveLength(2));
    const listed = t.callsOf("games_list").length;
    await act(() => refreshAll(qc));
    await waitFor(() => expect(t.callsOf("games_list").length).toBeGreaterThan(listed));
    expect(t.callsOf("game_art")).toHaveLength(2);
  });

  // Fails when an empty library offers nothing to do.
  it("with zero games shows only the Add game tile and a short note", async () => {
    const { list } = await library({}, []);
    expect(within(list).getAllByRole("button").map((b) => b.textContent)).toEqual(["Add game"]);
    expect(screen.getByText(/No games yet/)).toBeInTheDocument();
    expect(within(screen.getByRole("complementary", { name: "Sidebar" })).getByText("No game selected")).toBeInTheDocument();
  });

  // Fails when the old dropdown comes back in the sidebar.
  it("the sidebar names the active game without a picker", async () => {
    await library();
    const sidebar = screen.getByRole("complementary", { name: "Sidebar" });
    expect(within(sidebar).getByText("Skyrim Special Edition")).toBeInTheDocument();
    expect(within(sidebar).queryByRole("button", { name: /^Game:|Choose a game|Add game/ })).toBeNull();
    expect(screen.queryByRole("menu")).toBeNull();
  });
});

describe("add-game dialog", () => {
  const detected = [
    { domain: "fallout4", name: "Fallout 4", install_dir: "/s/Fallout 4", support: "experimental", configured: false },
    { domain: "skyrimspecialedition", name: "Skyrim Special Edition", install_dir: "/s/SSE", support: "tested", configured: true },
  ];

  // Fails when detected games are not offered, or Add does not pass the Steam folder / close the dialog.
  it("adds a game found in Steam from the Add game tile and returns focus to it", async () => {
    const { t, user, list } = await library({ games_detect: () => detected, game_add: () => game({ domain: "fallout4" }) });
    const open = within(list).getByRole("button", { name: "Add game" });
    open.focus();
    await user.keyboard("{Enter}");
    const dlg = await screen.findByRole("dialog", { name: "Add a game" });
    // Configured games are not offered again.
    const found = await within(dlg).findByRole("list", { name: "Detected games" });
    expect(within(found).getAllByRole("listitem")).toHaveLength(1);
    await user.click(within(found).getByRole("button", { name: "Add Fallout 4" }));
    await waitFor(() =>
      expect(t.callsOf("game_add")).toEqual([{ domain: "fallout4", path: "/s/Fallout 4", subdir: null, profile: null }]),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(open));
  });

  // Fails when the manual form drops the domain or folder, or Escape does not close.
  it("adds any game by Nexus domain; Escape closes", async () => {
    const { t, user, list } = await library({ game_add: () => undefined });
    await user.click(within(list).getByRole("button", { name: "Add game" }));
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

describe("no active game", () => {
  // Fails when a per-game view does not point back to the library.
  it.each(["mods", "install", "collections", "requirements"] as const)("%s points back to Games", async (view) => {
    const { user } = await library({}, [experimentalGame({ active: false })]);
    act(() => useUi.getState().setView(view));
    await user.click(await within(screen.getByRole("main")).findByRole("button", { name: "Go to Games" }));
    expect(await screen.findByRole("list", { name: "Game library" })).toBeInTheDocument();
  });
});
