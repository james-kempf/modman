// SPDX-License-Identifier: GPL-3.0-or-later
// U02 UI batch: Overview title card (app id tag, folder links, button
// heights), deploy tracker, Collections card, hero backdrop, Settings
// controls per value kind, bottom padding, collection art, Mods search and
// filters. jsdom has no layout: placement is checked by DOM order/classes.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useUi, type ViewId } from "./app/store";
import type { CollectionSummary, ConfigEntry, GameView, ModView } from "./ipc/types";
import type { Handler } from "./mock/fake";
import { deployStatus, game, mod } from "./mock/fixtures";
import { performNav } from "./nav/actions";
import { setup } from "./test/harness";
import { deployTracker } from "./views/Overview";
import { configError } from "./views/Settings";
import { filterMods, NO_FILTER } from "./views/Mods";

const JPEG = "data:image/jpeg;base64,/9j/4AAQ";

async function open(view: ViewId, handlers: Record<string, Handler> = {}, games: GameView[] = [game()]) {
  const env = await setup(handlers, games);
  env.renderApp();
  act(() => useUi.getState().setView(view));
  return { ...env, user: userEvent.setup() };
}
const main = () => screen.getByRole("main");

describe("Overview title card", () => {
  // Fails when the app id tag is missing, not first, or shown for a game without one.
  it("first tag is the Steam app id, omitted without one", async () => {
    await open("overview");
    const tags = await screen.findByTestId("game-tags");
    expect(tags.firstElementChild).toHaveTextContent("App 489830");
    expect(within(main()).queryByText(/Starts the game through Steam/)).toBeNull();
  });

  it("no app id: no app tag", async () => {
    await open("overview", {}, [game({ appid: null })]);
    const tags = await screen.findByTestId("game-tags");
    expect(tags).not.toHaveTextContent(/App \d/);
    expect(tags.firstElementChild).toHaveTextContent("bethesda");
  });

  // Fails when a folder link sends a path (the backend must resolve it) or the Proton link shows without a prefix.
  it("Game folder / Proton folder open through open_folder by game and kind", async () => {
    const { user, t } = await open("overview", { open_folder: (a) => `/resolved/${(a as { which: string }).which}` }, [
      game({ prefix: "/pfx" }),
    ]);
    await user.click(await within(main()).findByRole("button", { name: "Game folder" }));
    await user.click(within(main()).getByRole("button", { name: "Proton folder" }));
    await waitFor(() =>
      expect(t.callsOf("open_folder")).toEqual([
        { game: "skyrimspecialedition", which: "game" },
        { game: "skyrimspecialedition", which: "prefix" },
      ]),
    );
    expect(within(main()).queryByText(game().folder!)).toBeNull();
  });

  it("no prefix: no Proton folder link; a failed open is a notice", async () => {
    const { user } = await open("overview", {
      open_folder: () => {
        throw { message: "folder missing" };
      },
    });
    const btn = await within(main()).findByRole("button", { name: "Game folder" });
    expect(within(main()).queryByRole("button", { name: "Proton folder" })).toBeNull();
    await user.click(btn);
    expect(await screen.findByRole("alert")).toHaveTextContent("folder missing");
  });

  // Fails when Remove game is smaller than Play again.
  it("Remove game has the height of Play", async () => {
    await open("overview");
    const play = await within(main()).findByRole("button", { name: "Play" });
    const remove = within(main()).getByRole("button", { name: "Remove game" });
    const h = (el: HTMLElement) => el.className.match(/\bmin-h-\d+\b/)?.[0];
    expect(h(remove)).toBe(h(play));
  });
});

describe("Deployment tracker", () => {
  // Fails when the tracker math drifts: 0% before a deploy, 100% in sync, a fraction (rounded down) out of sync.
  it("tracker math", () => {
    const d = (o = {}) => deployStatus({ enabled_mods: 4, deployed_mods: 3, ...o });
    expect(deployTracker(d({ active: false, deployed_mods: 0 }), false)).toEqual({ deployed: 0, enabled: 4, pct: 0 });
    expect(deployTracker(d({ active: false }), true).pct).toBe(0);
    expect(deployTracker(d(), false)).toEqual({ deployed: 4, enabled: 4, pct: 100 });
    expect(deployTracker(d(), true)).toEqual({ deployed: 3, enabled: 4, pct: 75 });
    expect(deployTracker(d({ enabled_mods: 3, deployed_mods: 2 }), true).pct).toBe(66);
    expect(deployTracker(d({ deployed_mods: 9 }), true).deployed).toBe(4);
    expect(deployTracker(d({ enabled_mods: 0, deployed_mods: 0 }), true)).toEqual({ deployed: 0, enabled: 0, pct: 0 });
    expect(deployTracker(d({ enabled_mods: 0, deployed_mods: 0 }), false).pct).toBe(100);
  });

  // Fails when the old "linked into the game" text comes back or the tracker is not shown.
  it("shows progress and the count, without the old text", async () => {
    await open("overview", { mods_list: () => ({ mods: [], changed_since_deploy: true }) }, [
      game({ deploy: deployStatus({ enabled_mods: 5, deployed_mods: 2 }) }),
    ]);
    const tracker = await screen.findByTestId("deploy-tracker");
    await waitFor(() => expect(tracker).toHaveTextContent("2 of 5 enabled mods deployed"));
    expect(within(tracker).getByRole("progressbar", { name: "Enabled mods deployed" })).toHaveAttribute("value", "40");
    expect(tracker).toHaveTextContent("40%");
    expect(screen.queryByText(/Your enabled mods are linked into the game/)).toBeNull();
  });
});

const coll = (slug: string, mods: number[], enabled: number): CollectionSummary => ({
  record: { id: mods.length + slug.length, game: "skyrimspecialedition", slug, name: `Coll ${slug}`, author: "a", revision: 1, dir: null, failed: [], installed_at: 1, mods },
  enabled,
});

describe("Overview Collections card", () => {
  // Fails when the card is missing or not between Mods and Requirements.
  it("sits between Mods and Requirements with counts and a link", async () => {
    const { user } = await open("overview", { collections_list: () => [coll("abc", [1, 2, 3], 2), coll("def", [4], 1)] });
    const card = await screen.findByRole("region", { name: "Collections" });
    const order = Array.from(card.parentElement!.children).map((c) => c.getAttribute("aria-label"));
    expect(order.indexOf("Collections")).toBe(order.indexOf("Mods") + 1);
    expect(order.indexOf("Requirements")).toBe(order.indexOf("Collections") + 1);
    await waitFor(() => expect(card).toHaveTextContent("2installed3/4mods enabled"));
    expect(within(card).getByRole("progressbar")).toHaveAttribute("value", "3");
    await user.click(within(card).getByRole("button", { name: "Open Collections" }));
    expect(useUi.getState().view).toBe("collections");
  });

  it("empty state", async () => {
    await open("overview");
    const card = await screen.findByRole("region", { name: "Collections" });
    await waitFor(() => expect(card).toHaveTextContent("No collections installed"));
    expect(card).toHaveTextContent("0installed0/0mods enabled");
    expect(within(card).queryByTestId("collections-tracker")).toBeNull();
  });
});

describe("Overview hero backdrop", () => {
  // Fails when the hero art is not requested by kind, shown elsewhere, or a non-image is used.
  it("shows the hero art behind the Overview only", async () => {
    const { t } = await open("overview", {
      game_art: (a) => ((a as { kind: string }).kind === "library_hero" ? JPEG : null),
    });
    const img = await screen.findByTestId("hero-art");
    expect(img).toHaveAttribute("src", JPEG);
    expect(img).toHaveAttribute("alt", "");
    expect(img).toHaveAttribute("aria-hidden", "true");
    // Behind the scrolling view, not inside it.
    expect(main().contains(img)).toBe(false);
    expect(img.className).toMatch(/opacity-\[0\.\d+\]/);
    expect(t.callsOf("game_art")).toContainEqual({ domain: "skyrimspecialedition", kind: "library_hero" });
    act(() => useUi.getState().setView("mods"));
    await waitFor(() => expect(screen.queryByTestId("hero-art")).toBeNull());
  });

  it("no art, a non-image or no app id: no backdrop", async () => {
    const { t } = await open("overview", { game_art: () => "https://evil.example/x.jpg" });
    await screen.findByTestId("deploy-tracker");
    await waitFor(() => expect(t.callsOf("game_art").length).toBeGreaterThan(0));
    expect(screen.queryByTestId("hero-art")).toBeNull();
  });

  it("no app id: no lookup, no backdrop", async () => {
    const { t } = await open("overview", { game_art: () => JPEG }, [game({ appid: null })]);
    await screen.findByTestId("deploy-tracker");
    expect(t.callsOf("game_art")).toEqual([]);
    expect(screen.queryByTestId("hero-art")).toBeNull();
  });
});

const cfg = (key: string, kind: ConfigEntry["kind"], value: string, options: string[] = []): ConfigEntry => ({
  key,
  value,
  description: `${key} setting`,
  kind,
  options,
});
const CONFIG = [
  cfg("data-dir", "path", "/data"),
  cfg("deploy-method", "enum", "auto", ["auto", "hardlink", "reflink", "symlink", "copy"]),
  cfg("jobs", "number", "4"),
  cfg("adult", "bool", "false", ["true", "false"]),
];

async function settings() {
  const values = new Map(CONFIG.map((c) => [c.key, c.value]));
  const env = await open("settings", {
    config_list: () => CONFIG.map((c) => ({ ...c, value: values.get(c.key)! })),
    config_set: (a) => {
      const { key, value } = a as { key: string; value: string };
      values.set(key, value);
      return { ...CONFIG.find((c) => c.key === key)!, value, old_data_left: null };
    },
  });
  return env;
}

describe("Settings controls per kind", () => {
  // Fails when an enum key is a free text field or the dropdown is not keyboard operable (Enter, arrows, Enter; Esc).
  it("enum: dropdown, keyboard", async () => {
    const { user, t } = await settings();
    const btn = await screen.findByRole("button", { name: /deploy-method/ });
    expect(btn).toHaveAttribute("aria-haspopup", "listbox");
    expect(btn).toHaveTextContent("auto");
    btn.focus();
    await user.keyboard("{Enter}");
    const list = await screen.findByRole("listbox");
    expect(within(list).getAllByRole("option").map((o) => o.textContent)).toEqual(["auto", "hardlink", "reflink", "symlink", "copy"]);
    expect(document.activeElement).toHaveTextContent("auto");
    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(document.activeElement).toHaveTextContent("reflink");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(document.activeElement).toBe(btn);
    expect(t.callsOf("config_set")).toEqual([]);
    await user.keyboard("{Enter}");
    await screen.findByRole("listbox");
    await user.keyboard("{ArrowDown}{Enter}");
    await waitFor(() => expect(t.callsOf("config_set")).toEqual([{ key: "deploy-method", value: "hardlink" }]));
    await waitFor(() => expect(screen.getByRole("button", { name: /deploy-method/ })).toHaveTextContent("hardlink"));
    // Focus stays on the dropdown after the save.
    expect(document.activeElement).toBe(screen.getByRole("button", { name: /deploy-method/ }));
  });

  // Fails when the dropdown cannot be driven by the gamepad actions (A, D-pad, B).
  it("enum: dropdown, gamepad actions", async () => {
    const { t } = await settings();
    const btn = await screen.findByRole("button", { name: /deploy-method/ });
    btn.focus();
    act(() => performNav({ type: "select" }));
    await screen.findByRole("listbox");
    act(() => performNav({ type: "back" }));
    await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
    expect(document.activeElement).toBe(btn);
    act(() => performNav({ type: "select" }));
    await screen.findByRole("listbox");
    act(() => performNav({ type: "move", dir: "down" }));
    expect(document.activeElement).toHaveTextContent("hardlink");
    act(() => performNav({ type: "select" }));
    await waitFor(() => expect(t.callsOf("config_set")).toEqual([{ key: "deploy-method", value: "hardlink" }]));
  });

  // Fails when a bool key is a text field instead of a switch.
  it("bool: switch saves true/false at once", async () => {
    const { user, t } = await settings();
    const sw = await screen.findByRole("switch", { name: "adult" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    await user.click(sw);
    await waitFor(() => expect(t.callsOf("config_set")).toEqual([{ key: "adult", value: "true" }]));
    await waitFor(() => expect(screen.getByRole("switch", { name: "adult" })).toHaveAttribute("aria-checked", "true"));
    expect(screen.queryByRole("textbox", { name: /adult/ })).toBeNull();
  });

  // Fails when numbers/paths lose their validation (a bad value reaches the backend).
  it("number and path: text fields with validation", async () => {
    const { user, t } = await settings();
    const jobs = await screen.findByRole("textbox", { name: /jobs/ });
    await user.clear(jobs);
    await user.type(jobs, "abc");
    expect(jobs).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText("Enter a whole number.")).toBeInTheDocument();
    const save = within(jobs.closest("form")!).getByRole("button", { name: "Save" });
    expect(save).toBeDisabled();
    await user.clear(jobs);
    await user.type(jobs, "8{Enter}");
    await waitFor(() => expect(t.callsOf("config_set")).toEqual([{ key: "jobs", value: "8" }]));
    expect(configError(cfg("data-dir", "path", ""), "relative/x")).toMatch(/absolute/);
    expect(configError(cfg("data-dir", "path", ""), "~/mods")).toBeNull();
    expect(configError(cfg("cdn", "text", ""), "anything")).toBeNull();
  });
});

describe("bottom padding", () => {
  // Fails when the view's bottom padding differs from its side padding, or notices may cover controls again.
  it("equals the side padding, and grows with the notice stack", async () => {
    await open("settings");
    const m = main();
    const inner = m.firstElementChild as HTMLElement;
    expect(inner.className).toMatch(/\bpx-10\b/);
    expect(m.className).toMatch(/\bpb-10\b/);
    act(() => useUi.getState().notify("info", "hello"));
    await waitFor(() => expect(m.className).toMatch(/\bpb-28\b/));
    expect(m.className).toMatch(/\bscroll-pb-60\b/);
  });
});

describe("Collections art", () => {
  // Fails when the Nexus tile is not shown, a non-image is used, or the monogram fallback is lost.
  it("tile art when the backend has it, else the monogram", async () => {
    const { t } = await open("collections", {
      collections_list: () => [coll("abc", [1], 1), coll("noart", [2], 0)],
      collection_art: (a) => ((a as { slug: string }).slug === "abc" ? JPEG : "https://evil.example/x.png"),
    });
    const art = await screen.findByTestId("collection-art");
    expect(art).toHaveAttribute("src", JPEG);
    expect(art.closest("li")).toHaveTextContent("Coll abc");
    await waitFor(() => expect(screen.getByTestId("collection-monogram").closest("li")).toHaveTextContent("Coll noart"));
    expect(t.callsOf("collection_art")).toEqual(
      expect.arrayContaining([
        { game: "skyrimspecialedition", slug: "abc" },
        { game: "skyrimspecialedition", slug: "noart" },
      ]),
    );
  });
});

describe("Mods search and filters", () => {
  const MODS: ModView[] = [
    mod({ name: "SkyUI", author: "schlangster", nexus_mod_id: 12604, position: 1 }),
    mod({ name: "Unofficial Patch", author: "Arthmoor", enabled: false, position: 2, collections: ["abc"] }),
    mod({ name: "Immersive Armors", author: "Hothtrooper", position: 3, collections: ["abc", "def"] }),
  ];

  // Fails when search misses name/author/uid/Nexus id, or a filter keeps the wrong mods.
  it("filterMods", () => {
    const names = (f: Partial<typeof NO_FILTER>, data = {}) => filterMods(MODS, { ...NO_FILTER, ...f }, data).map((m) => m.name);
    expect(names({ query: "sky" })).toEqual(["SkyUI"]);
    expect(names({ query: "ARTHMOOR" })).toEqual(["Unofficial Patch"]);
    expect(names({ query: MODS[2].uid })).toEqual(["Immersive Armors"]);
    expect(names({ query: "12604" })).toEqual(["SkyUI"]);
    expect(names({ state: "enabled" })).toEqual(["SkyUI", "Immersive Armors"]);
    expect(names({ state: "disabled" })).toEqual(["Unofficial Patch"]);
    expect(names({ collection: "def" })).toEqual(["Immersive Armors"]);
    expect(names({ collection: "*" })).toEqual(["Unofficial Patch", "Immersive Armors"]);
    expect(names({ collection: "-" })).toEqual(["SkyUI"]);
    expect(names({ state: "conflicts" }, { conflicted: new Set([MODS[0].id]) })).toEqual(["SkyUI"]);
    expect(names({ state: "updates" }, { updated: new Set([MODS[2].id]) })).toEqual(["Immersive Armors"]);
    expect(names({ query: "zzz" })).toEqual([]);
  });

  // Fails when the search does not filter instantly, the filters are not operable, or there is no empty state.
  it("search box, filters and the empty state in the view", async () => {
    const { user } = await open("mods", {
      mods_list: () => ({ mods: MODS, changed_since_deploy: false }),
      conflicts: () => ({ files: [{ rel_path: "a.dds", winner: MODS[2], losers: [MODS[0]], plugin: false }], plugins: [] }),
    });
    const list = await screen.findByRole("list", { name: "Load order" });
    const search = screen.getByRole("searchbox", { name: "Search mods" });
    await user.type(search, "armor");
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(screen.getByText(/1 of 3 mods/)).toBeInTheDocument();
    await user.clear(search);
    // Show: Has conflicts.
    await user.click(screen.getByRole("button", { name: /^Filter/ }));
    await user.click(await screen.findByRole("option", { name: "Has conflicts" }));
    await waitFor(() => expect(within(list).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      expect.stringContaining("SkyUI"),
      expect.stringContaining("Immersive Armors"),
    ]));
    // Collection: def.
    await user.click(screen.getByRole("button", { name: "Collection Any" }));
    await user.click(await screen.findByRole("option", { name: "def" }));
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    // No match: empty state, then clear.
    await user.type(search, "nothing like this");
    expect(screen.getByText("No mods match")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Clear search and filters" }));
    expect(within(list).getAllByRole("listitem")).toHaveLength(3);
    expect(search).toHaveValue("");
  });

  // Fails when "Has updates" is offered without update data, or does not use the Updates tab's report.
  it("Has updates is offered once the Updates tab has checked", async () => {
    const { user, qc } = await open("mods", { mods_list: () => ({ mods: MODS, changed_since_deploy: false }) });
    await screen.findByRole("list", { name: "Load order" });
    await user.click(screen.getByRole("button", { name: /^Filter/ }));
    expect(screen.queryByRole("option", { name: "Has updates" })).toBeNull();
    await user.keyboard("{Escape}");
    act(() =>
      qc.setQueryData(["updates", "skyrimspecialedition"], {
        checked: 3,
        fetched: 3,
        stopped_early: false,
        installed: 0,
        updates: [{ mod: MODS[1], file: { file_id: 1, name: "f", version: "2", category_name: null, file_name: null, size_in_bytes: null, uploaded_timestamp: null, is_primary: true }, pinned: false }],
      }),
    );
    await user.click(screen.getByRole("button", { name: /^Filter/ }));
    await user.click(await screen.findByRole("option", { name: "Has updates" }));
    const list = screen.getByRole("list", { name: "Load order" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(list).toHaveTextContent("Unofficial Patch");
  });
});
