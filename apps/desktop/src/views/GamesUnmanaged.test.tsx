// SPDX-License-Identifier: GPL-3.0-or-later
// Games view: the second grid of games found in Steam but not managed yet.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { Detected, GameView } from "../ipc/types";
import { createDemoBackend } from "../mock/demo";
import { experimentalGame, game } from "../mock/fixtures";
import { performNav } from "../nav/actions";
import { setup } from "../test/harness";

const found = (o: Partial<Detected> = {}): Detected => ({
  domain: "fallout4",
  name: "Fallout 4",
  appid: 377160,
  install_dir: "/s/Fallout 4",
  support: "experimental",
  configured: false,
  ...o,
});

const unsupported = found({ domain: "steam-632360", name: "Risk of Rain 2", appid: 632360, install_dir: "/s/RoR2", support: null });
const JPEG = "data:image/jpeg;base64,/9j/";

async function setupArt() {
  const env = await setup({
    games_detect: () => [found(), unsupported],
    steam_art: (a) => ((a as { appid: number }).appid === 377160 ? JPEG : null),
  });
  env.renderApp();
  await screen.findByRole("list", { name: "Game library" });
  return env;
}

/** A backend whose game_add manages the detected game, as the real one does. */
async function view(detected: Detected[], games: GameView[] = [game()]) {
  let current = games;
  const env = await setup(
    {
      games_list: () => current,
      games_detect: () => detected.map((d) => ({ ...d, configured: d.configured || current.some((g) => g.id === d.domain) })),
      game_add: (a) => {
        const { domain } = a as { domain: string };
        const d = detected.find((x) => x.domain === domain)!;
        const g = experimentalGame({ id: domain, name: d.name, active: false });
        current = [...current, g];
        return g;
      },
    },
    games,
  );
  env.renderApp();
  await screen.findByRole("list", { name: "Game library" });
  return { ...env, user: userEvent.setup() };
}
const unmanaged = () => screen.findByRole("list", { name: "Unmanaged games" });
const libraryNames = () =>
  within(screen.getByRole("list", { name: "Game library" }))
    .getAllByRole("button")
    .map((b) => b.getAttribute("aria-label"));

describe("unmanaged games grid", () => {
  // Fails when configured / already-managed games are offered again, or a fresh one is missing.
  it("lists only detected games that are not managed", async () => {
    await view([
      found(),
      found({ domain: "skyrimspecialedition", name: "Skyrim Special Edition", configured: true }),
      // configured=false but already in the games list (stale detect).
      found({ domain: "skyrimspecialedition", name: "Skyrim dupe" }),
      found({ domain: "starfield", name: "Starfield", support: "tested" }),
    ]);
    const list = await unmanaged();
    expect(within(list).getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual(["Add Fallout 4", "Add Starfield"]);
    expect(within(list).getByRole("button", { name: "Add Fallout 4" })).toHaveAccessibleDescription(/Not managed.*Experimental/);
  });

  // Fails when there is no empty state, or an empty grid is rendered.
  it("shows an empty state when nothing new was found", async () => {
    await view([found({ configured: true })]);
    expect(await screen.findByText("No other games found in Steam.")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Unmanaged games" })).toBeNull();
  });

  // Fails when clicking a card does not add with the Steam folder, or the game does not move to the library.
  it("clicking a card adds the game and moves it to the library", async () => {
    const { t, user } = await view([found()]);
    await user.click(within(await unmanaged()).getByRole("button", { name: "Add Fallout 4" }));
    await waitFor(() =>
      expect(t.callsOf("game_add")).toEqual([{ domain: "fallout4", path: "/s/Fallout 4", subdir: null, profile: null }]),
    );
    await waitFor(() => expect(libraryNames()).toEqual(["Skyrim Special Edition", "Fallout 4"]));
    expect(await screen.findByText("No other games found in Steam.")).toBeInTheDocument();
  });

  // Fails when a double activation adds twice.
  it("a double click adds once", async () => {
    const { t, user } = await view([found()]);
    await user.dblClick(within(await unmanaged()).getByRole("button", { name: "Add Fallout 4" }));
    await waitFor(() => expect(libraryNames()).toContain("Fallout 4"));
    expect(t.callsOf("game_add")).toHaveLength(1);
  });

  // Fails when the cards are not reachable by the D-pad from the library or A does not add.
  it("D-pad reaches the cards from the library and A adds", async () => {
    const { t } = await view([found()]);
    const card = within(await unmanaged()).getByRole("button", { name: "Add Fallout 4" });
    const top = within(screen.getByRole("list", { name: "Game library" })).getByRole("button", { name: "Skyrim Special Edition" });
    top.getBoundingClientRect = () => new DOMRect(400, 100, 200, 340);
    card.getBoundingClientRect = () => new DOMRect(400, 520, 200, 340);
    top.focus();
    act(() => performNav({ type: "move", dir: "down" }));
    expect(document.activeElement).toBe(card);
    act(() => performNav({ type: "select" }));
    await waitFor(() => expect(t.callsOf("game_add")).toHaveLength(1));
  });

  // Fails when the search filter ignores the found games.
  it("the search filters found games too", async () => {
    const { user } = await view([found(), found({ domain: "starfield", name: "Starfield" })]);
    await user.type(screen.getByRole("searchbox", { name: "Search games" }), "star");
    await waitFor(async () =>
      expect(within(await unmanaged()).getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual(["Add Starfield"]),
    );
  });

  // Fails when unmanaged tiles skip Steam art (G1: appid null), ask game_art for an unmanaged game, or drop the monogram fallback.
  it("shows Steam art by app id, else a monogram", async () => {
    const { t } = await setupArt();
    const list = await unmanaged();
    const fo4 = within(list).getByRole("button", { name: "Add Fallout 4" });
    await waitFor(() => expect(within(fo4).getByTestId("game-art")).toHaveAttribute("src", JPEG));
    const ror = within(list).getByRole("button", { name: "Add Risk of Rain 2" });
    await waitFor(() => expect(t.callsOf("steam_art")).toHaveLength(2));
    expect(within(ror).getByTestId("game-monogram")).toBeInTheDocument();
    expect(t.callsOf("steam_art")).toEqual(
      expect.arrayContaining([
        { appid: 377160, kind: "library_600x900" },
        { appid: 632360, kind: "library_600x900" },
      ]),
    );
    expect(t.callsOf("game_art").filter((a) => (a as { game: string }).game !== "skyrimspecialedition")).toEqual([]);
  });

  // Fails when an unsupported game looks supported, is added directly with its key domain, or the dialog lacks the folder.
  it("an unsupported game is marked and opens the add dialog with its folder", async () => {
    const { t, user } = await view([found(), unsupported]);
    const list = await unmanaged();
    const ror = within(list).getByRole("button", { name: "Add Risk of Rain 2" });
    expect(ror).toHaveAccessibleDescription(/Unsupported/);
    expect(within(list).getByRole("button", { name: "Add Fallout 4" })).not.toHaveAccessibleDescription(/Unsupported/);
    await user.click(ror);
    const dlg = await screen.findByRole("dialog", { name: "Add a game" });
    expect(within(dlg).getByLabelText("Game folder (optional)")).toHaveValue("/s/RoR2");
    expect(within(dlg).getByText(/no preset for Risk of Rain 2/)).toBeInTheDocument();
    expect(within(dlg).queryByRole("button", { name: "Add Risk of Rain 2" })).toBeNull();
    expect(t.callsOf("game_add")).toEqual([]);
  });

  // Fails when demo mode offers no detected game to try the flow with.
  it("demo mode supplies a detected game", async () => {
    const list = await createDemoBackend().invoke<Detected[]>("games_detect");
    expect(list.some((d) => !d.configured)).toBe(true);
  });
});
