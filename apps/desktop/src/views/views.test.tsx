// SPDX-License-Identifier: GPL-3.0-or-later
// View behaviour over the mocked IPC: each view calls the right command
// with the right (snake_case) arguments and renders structured results.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { VIEWS, useUi, type ViewId } from "../app/store";
import { game, experimentalGame, mod } from "../mock/fixtures";
import type { Handler } from "../mock/fake";
import { setup } from "../test/harness";
import { installTarget } from "./Requirements";

async function open(view: ViewId, handlers: Record<string, Handler> = {}, games = [game()]) {
  const env = await setup(handlers, games);
  env.renderApp();
  act(() => useUi.getState().setView(view));
  return { ...env, user: userEvent.setup() };
}

describe("Overview", () => {
  // Fails when the old games list comes back on Overview (games live in the sidebar picker now).
  it("has no games list; cards are about the active game", async () => {
    await open("overview", {}, [game(), experimentalGame({ active: false })]);
    const main = screen.getByRole("main");
    expect(await within(main).findByRole("heading", { level: 2, name: "Skyrim Special Edition" })).toBeInTheDocument();
    expect(within(main).queryByText("Cyberpunk 2077")).toBeNull();
    expect(within(main).queryByRole("button", { name: "Use" })).toBeNull();
    for (const card of ["Mods", "Requirements", "Plugins"]) expect(within(main).getByRole("region", { name: card })).toBeInTheDocument();
  });

  // Fails when Play does not call game_launch, or is offered without a Steam app id.
  it("Play launches the game through game_launch; disabled without an app id", async () => {
    const { t, user } = await open("overview", { game_launch: () => ({ appid: 489830, deployed: false }) });
    await user.click(await screen.findByRole("button", { name: "Play" }));
    await waitFor(() => expect(t.callsOf("game_launch")).toEqual([{ game: null }]));
    expect(await screen.findByText(/Starting Skyrim Special Edition/)).toBeInTheDocument();
  });

  it("Play is disabled without a Steam app id", async () => {
    await open("overview", {}, [game({ appid: null })]);
    expect(await screen.findByRole("button", { name: "Play" })).toBeDisabled();
    expect(screen.getByText(/No Steam app id/)).toBeInTheDocument();
  });

  // Fails when a deploy done by Play (pending changes) leaves the "deploy due" state set.
  it("Play that deployed first clears the deploy-due state", async () => {
    const env = await open("overview", { game_launch: () => ({ appid: 489830, deployed: true }) });
    act(() => useUi.getState().markDeployDue("skyrimspecialedition"));
    expect(await screen.findByText(/Play asks to deploy them first/)).toBeInTheDocument();
    await env.user.click(screen.getByRole("button", { name: "Play" }));
    await waitFor(() => expect(useUi.getState().deployDue).toEqual({}));
  });

  // Fails when summary counts are computed from the wrong fields (enabled, deps status/acked, plugins).
  it("summarises mods, requirements and plugins", async () => {
    await open("overview", {
      mods_list: () => ({
        mods: [mod({ name: "A" }), mod({ name: "B", enabled: false }), mod({ name: "C" })],
        changed_since_deploy: true,
      }),
      deps: () => [
        { status: "missing", key: "a", name: "A", hint: "", urls: [], by: [], detail: "", acked: false },
        { status: "disabled", key: "b", name: "B", hint: "", urls: [], by: [], detail: "", acked: false },
        { status: "external", key: "c", name: "C", hint: "", urls: [], by: [], detail: "", acked: true },
        { status: "external", key: "d", name: "D", hint: "", urls: [], by: [], detail: "", acked: false },
        { status: "ok", key: "e", name: "E", hint: "", urls: [], by: [], detail: "", acked: false },
      ],
      plugins_list: () => [
        { name: "Skyrim.esm", active: true, managed: false },
        { name: "a.esp", active: true, managed: true },
        { name: "b.esp", active: false, managed: true },
      ],
    });
    const mods = await screen.findByRole("region", { name: "Mods" });
    await waitFor(() => expect(mods).toHaveTextContent("3installed2enabled"));
    expect(mods).toHaveTextContent("Changes since the last deploy");
    const reqs = screen.getByRole("region", { name: "Requirements" });
    await waitFor(() => expect(reqs).toHaveTextContent("3missing2ok"));
    expect(reqs).toHaveTextContent("Includes 1 external tool to check by hand");
    const plugins = screen.getByRole("region", { name: "Plugins" });
    await waitFor(() => expect(plugins).toHaveTextContent("2active3in plugins.txt"));
    await userEvent.setup().click(within(reqs).getByRole("button", { name: "Open Requirements" }));
    expect(useUi.getState().view).toBe("requirements");
  });

  // Fails when the plugins card queries plugins_list for a game without plugins.txt (it rejects).
  it("hides the plugins card for non-Bethesda games", async () => {
    const { t } = await open("overview", {}, [experimentalGame()]);
    await screen.findByRole("region", { name: "Mods" });
    expect(screen.queryByRole("region", { name: "Plugins" })).toBeNull();
    expect(t.callsOf("plugins_list")).toEqual([]);
  });

  // Fails when the no-game state does not lead back to the Games view (where games are added).
  it("empty state points back to Games", async () => {
    const { user } = await open("overview", {}, []);
    await screen.findByRole("heading", { name: "No game yet" });
    await user.click(within(screen.getByRole("main")).getByRole("button", { name: "Go to Games" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Games" })).toBeInTheDocument();
    expect(document.activeElement).toHaveAttribute("data-tab", "games");
    expect(within(screen.getByRole("main")).getByRole("button", { name: "Add game" })).toBeInTheDocument();
  });

  it("Purge is a single click (the backend confirms through a prompt)", async () => {
    const { t, user } = await open("overview", { purge: () => null });
    await user.click(await screen.findByRole("button", { name: "Purge" }));
    await waitFor(() => expect(t.callsOf("purge")).toHaveLength(1));
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("Mods", () => {
  it("conflict view lists file conflicts with the winner", async () => {
    const m = (id: number, name: string) => ({ id, uid: `u${id}`, name, priority: id });
    const { user } = await open("mods", {
      conflicts: () => ({ files: [{ rel_path: "textures/sky.dds", winner: m(2, "B"), losers: [m(1, "A")], plugin: false }], plugins: [] }),
    });
    await user.click(await screen.findByRole("button", { name: "Conflicts" }));
    const list = await screen.findByRole("list", { name: "File conflicts" });
    expect(list).toHaveTextContent("textures/sky.dds");
    expect(list).toHaveTextContent("B wins over A");
  });
});

const hit = (modId: number, o: Record<string, unknown> = {}) => ({
  modId,
  name: `Mod ${modId}`,
  author: "a",
  version: "1",
  summary: null,
  downloads: 1,
  endorsements: 1,
  thumbnailUrl: null,
  category: null,
  updatedAt: null,
  installed: false,
  ...o,
});
const coll = (slug: string, o: Record<string, unknown> = {}) => ({
  slug,
  name: `Coll ${slug}`,
  summary: null,
  author: "c",
  category: null,
  endorsements: 1,
  downloads: 1,
  revision: 3,
  mod_count: 10,
  total_size: 1,
  image_url: null,
  installed_revision: null,
  ...o,
});

describe("Browse", () => {
  // Fails when Browse does not open on the top mods of all time, or a control (kind, period, sort, words)
  // does not reach the request.
  it("lists top mods of all time, and each control changes the request", async () => {
    const { t, user } = await open("browse", {
      browse: (a) => {
        const r = (a as { request: { kind: string } }).request;
        return r.kind === "mods" ? { kind: "mods", total: 1, mods: [hit(7)] } : { kind: "collections", total: 1, collections: [coll("abc")] };
      },
    });
    expect(await screen.findByText("Mod 7")).toBeInTheDocument();
    const req = () => (t.callsOf("browse").at(-1) as { request: Record<string, unknown> }).request;
    expect(req()).toEqual({ kind: "mods", period: "all", sort: "endorsements", text: "", offset: 0 });
    await user.click(screen.getByRole("button", { name: "Month" }));
    await waitFor(() => expect(req()).toMatchObject({ period: "month" }));
    await user.click(screen.getByRole("button", { name: /Sort/ }));
    await user.click(await screen.findByRole("option", { name: "Newest" }));
    await waitFor(() => expect(req()).toMatchObject({ sort: "newest" }));
    await user.type(screen.getByLabelText("Search"), "sky ui");
    await user.click(screen.getByRole("button", { name: "Search" }));
    await waitFor(() => expect(req()).toMatchObject({ text: "sky ui" }));
    await user.click(within(screen.getByRole("group", { name: "Show" })).getByRole("button", { name: "Collections" }));
    expect(await screen.findByText("Coll abc")).toBeInTheDocument();
    expect(req()).toEqual({ kind: "collections", period: "month", sort: "newest", text: "sky ui", offset: 0 });
  });

  // Fails when Load more asks for the wrong page, or stays offered when everything is shown.
  it("Load more fetches the next page by offset until every match is shown", async () => {
    const { t, user } = await open("browse", {
      browse: (a) => {
        const offset = (a as { request: { offset: number } }).request.offset;
        const ids = offset === 0 ? Array.from({ length: 24 }, (_, i) => i + 1) : [25];
        return { kind: "mods", total: 25, mods: ids.map((i) => hit(i)) };
      },
    });
    await user.click(await screen.findByRole("button", { name: "Load more" }));
    expect(await screen.findByText("Mod 25")).toBeInTheDocument();
    expect((t.callsOf("browse").at(-1) as { request: { offset: number } }).request.offset).toBe(24);
    expect(screen.getByText("25 of 25")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Load more" })).toBeNull();
  });

  // Fails when a tile installs the wrong thing: a mod by its Nexus id with the wizard, a new collection by
  // slug (optional mods asked), an installed one behind the latest revision by update; current ones say Installed.
  it("tiles install mods and collections, update stale collections, and mark installed ones", async () => {
    const { t, user } = await open("browse", {
      browse: (a) =>
        (a as { request: { kind: string } }).request.kind === "mods"
          ? { kind: "mods", total: 2, mods: [hit(7), hit(8, { installed: true })] }
          : {
              kind: "collections",
              total: 3,
              collections: [coll("new"), coll("old", { installed_revision: 2 }), coll("cur", { installed_revision: 3 })],
            },
      install: () => ({ game: "skyrimspecialedition", installed: { mod_id: 1, uid: "u", name: "Mod 7", files: 2, size: 1, fomod_found: false, replaced: null, warnings: [] }, collection: null, after: { deployed: null, needs_deploy: true } }),
      collection_install: () => ({ game: "skyrimspecialedition", slug: "new", revision: 3, cancelled: false, up_to_date: false, download_only: false, installed: 10, present: 0, failed: [], removed: 0, deployed: null }),
      collection_update: () => ({ game: "skyrimspecialedition", slug: "old", revision: 3, cancelled: false, up_to_date: false, download_only: false, installed: 1, present: 9, failed: [], removed: 0, deployed: null }),
    });
    await user.click(await screen.findByRole("button", { name: "Install Mod 7" }));
    await waitFor(() => expect(t.callsOf("install")).toEqual([expect.objectContaining({ target: "7", fomod: "ask" })]));
    const mods = screen.getByRole("list", { name: "Mods" });
    expect(within(mods).getAllByText("Installed")).toHaveLength(1);

    await user.click(within(screen.getByRole("group", { name: "Show" })).getByRole("button", { name: "Collections" }));
    await user.click(await screen.findByRole("button", { name: "Install Coll new" }));
    await waitFor(() => expect(t.callsOf("collection_install")).toEqual([expect.objectContaining({ reference: "new", optional: "ask" })]));
    await user.click(screen.getByRole("button", { name: "Update Coll old" }));
    await waitFor(() => expect(t.callsOf("collection_update")).toEqual([expect.objectContaining({ reference: "old" })]));
    const cols = screen.getByRole("list", { name: "Collections" });
    expect(within(cols).getAllByText("Installed")).toHaveLength(1);
  });

  // Fails when a game without a Nexus domain asks Nexus anyway (the backend would refuse).
  it("does not query Nexus for a game that is not linked to it", async () => {
    const { t } = await open("browse", {}, [game({ nexus_domain: null })]);
    expect(await screen.findByText(/not linked to a Nexus Mods game/)).toBeInTheDocument();
    expect(t.callsOf("browse")).toEqual([]);
  });
});

describe("Install", () => {
  it("installs a target with the FOMOD wizard mode and shows the result", async () => {
    const { t, user } = await open("browse", {
      install: () => ({
        game: "skyrimspecialedition",
        installed: { mod_id: 1, uid: "u", name: "SkyUI", files: 5, size: 1, fomod_found: true, replaced: null, warnings: [] },
        collection: null,
        after: { deployed: null, needs_deploy: false },
      }),
    });
    const target = await screen.findByLabelText(/Mod id, Nexus URL/);
    await user.type(target, "12604");
    await user.click(within(target.closest("form")!).getByRole("button", { name: "Install" }));
    await waitFor(() => expect(t.callsOf("install")).toHaveLength(1));
    expect(t.callsOf("install")[0]).toMatchObject({ target: "12604", fomod: "ask", deploy: false });
    expect(await screen.findByText(/Installed SkyUI \(5 files\)/)).toBeInTheDocument();
  });

  // T16 F2b: fails when the on-screen keyboard can only fill the field and the gamepad user must find Install.
  it("on-screen keyboard Install button fills the target and submits it", async () => {
    const { t } = await open("browse", {
      install: () => ({ game: null, installed: null, collection: null, after: { deployed: null, needs_deploy: false } }),
    });
    const target = (await screen.findByLabelText(/Mod id, Nexus URL/)) as HTMLInputElement;
    act(() => useUi.getState().openOsk({ input: target, label: "target" }));
    const dlg = await screen.findByRole("dialog");
    act(() => within(dlg).getByRole("button", { name: "7" }).click());
    act(() => within(dlg).getByRole("button", { name: "Install" }).click());
    await waitFor(() => expect(t.callsOf("install")).toHaveLength(1));
    expect(t.callsOf("install")[0]).toMatchObject({ target: "7" });
  });

  it("nxm paste box submits the link", async () => {
    const { t, user } = await open("browse", {
      nxm_submit: () => ({ row: 3, link: "nxm://skyrimspecialedition/mods/1/files/2", kind: "mod" }),
    });
    await user.type(await screen.findByLabelText("nxm link"), "nxm://skyrimspecialedition/mods/1/files/2?key=k&expires=1");
    await user.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(t.callsOf("nxm_submit")).toEqual([{ url: "nxm://skyrimspecialedition/mods/1/files/2?key=k&expires=1" }]));
    expect(await screen.findByText(/Queued mod link/)).toBeInTheDocument();
  });
});

describe("Collections", () => {
  it("looks up a collection and installs that revision", async () => {
    const { t, user } = await open("collections", {
      collection_info: () => ({
        game: "skyrimspecialedition",
        slug: "abc123",
        revision: {
          name: "Lorerim",
          author: "x",
          game_name: "Skyrim SE",
          number: 4,
          status: "published",
          latest: 4,
          mod_count: 10,
          optional_count: 2,
          total_size: 100,
          summary: "",
          url: "",
        },
        installed: null,
        update_available: null,
      }),
      collection_install: () => ({
        game: "skyrimspecialedition",
        slug: "abc123",
        revision: 4,
        cancelled: false,
        download_only: false,
        installed: 10,
        present: 0,
        failed: [],
        removed: 0,
        up_to_date: false,
        deployed: null,
      }),
    });
    await user.type(await screen.findByLabelText(/Collection URL/), "abc123");
    await user.click(screen.getByRole("button", { name: "Look up" }));
    await user.click(await screen.findByRole("button", { name: "Install collection" }));
    await waitFor(() => expect(t.callsOf("collection_install")).toHaveLength(1));
    expect(t.callsOf("collection_install")[0]).toMatchObject({ reference: "abc123", revision: 4, optional: "ask", deploy: true });
  });
});

describe("Requirements", () => {
  it("groups by status, installs missing ones and acknowledges external tools", async () => {
    const { t, user } = await open("requirements", {
      deps: () => [
        { status: "missing", key: "skse", name: "SKSE64", hint: "install 30379", urls: [], by: ["SkyUI"], detail: "", acked: false },
        { status: "external", key: "loader", name: "Loader", hint: "", urls: [], by: [], detail: "", acked: false },
      ],
      deps_ack: () => ({}),
      install: () => ({ game: null, installed: null, collection: null, after: { deployed: null, needs_deploy: false } }),
    });
    const missing = (await screen.findByRole("heading", { name: /Missing/ })).closest("section")!;
    await user.click(within(missing).getByRole("button", { name: "Install" }));
    await waitFor(() => expect(t.callsOf("install")[0]).toMatchObject({ target: "30379" }));
    await user.click(screen.getByRole("button", { name: "I have it" }));
    await waitFor(() => expect(t.callsOf("deps_ack")).toEqual([{ game: null, name: "loader" }]));
  });

  it("installTarget parses fix hints", () => {
    expect(installTarget("install 123")).toBe("123");
    expect(installTarget("enable foo")).toBeNull();
  });
});

describe("Activity", () => {
  // Fails when a fixed-width column comes back (overflow at 1024 px, dead space at 1920 px):
  // jobs and messages stack by default and share the width 1:2 from xl, in shrinkable tracks.
  it("lays out jobs and messages in fluid columns", async () => {
    await open("activity");
    const cols = await screen.findByTestId("activity-columns");
    expect(cols.className).toMatch(/\bgrid-cols-1\b/);
    expect(cols.className).toMatch(/xl:grid-cols-\[minmax\(0,1fr\)_minmax\(0,2fr\)\]/);
    expect(cols.className).not.toMatch(/\d+px/);
    for (const section of [...cols.children, screen.getByRole("heading", { name: "Vessel log" }).closest("section")!])
      expect(section.className).toMatch(/\bmin-w-0\b/);
  });

  it("shows jobs and streamed messages from events, and the log tail", async () => {
    const { t } = await open("activity", { log_tail: () => ["line one"] });
    act(() => {
      t.emit("vessel:job", { id: 9, kind: "install", state: "started", error: null });
      t.emit("vessel:activity", { job: 9, msg: { level: "info", spans: [{ text: "Extracting", style: "plain" }] }, text: "Extracting" });
    });
    expect(await screen.findByRole("list", { name: "Jobs" })).toHaveTextContent("Install");
    expect(screen.getByRole("list", { name: "Messages" })).toHaveTextContent("Extracting");
    expect(await screen.findByText("line one")).toBeInTheDocument();
  });
});

describe("view frame", () => {
  // Fails when a view goes back to its own hand-written outer wrapper instead of ViewFrame.
  it.each(VIEWS.map((v) => v.id))("%s renders inside ViewFrame", async (view) => {
    await open(view);
    const content = screen.getByRole("main").firstElementChild!;
    expect(content.firstElementChild).toHaveAttribute("data-view-frame");
  });
});
