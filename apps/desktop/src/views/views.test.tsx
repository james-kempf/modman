// SPDX-License-Identifier: GPL-3.0-or-later
// View behaviour over the mocked IPC: each view calls the right command
// with the right (snake_case) arguments and renders structured results.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useUi, type ViewId } from "../app/store";
import { game, experimentalGame } from "../mock/fixtures";
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
  it("switches the active game and adds a detected one", async () => {
    const { t, user } = await open(
      "overview",
      {
        games_detect: () => [
          { domain: "fallout4", name: "Fallout 4", install_dir: "/s/Fallout 4", support: "experimental", configured: false },
        ],
        game_use: () => null,
        game_add: () => game({ domain: "fallout4" }),
      },
      [game(), experimentalGame({ active: false })],
    );
    await user.click(await screen.findByRole("button", { name: "Use" }));
    expect(t.callsOf("game_use")).toEqual([{ domain: "cyberpunk2077" }]);
    await user.click(screen.getByRole("button", { name: "Detect Steam games" }));
    await user.click(await screen.findByRole("button", { name: "Add" }));
    expect(t.callsOf("game_add")).toEqual([{ domain: "fallout4", path: "/s/Fallout 4", subdir: null, profile: null }]);
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

describe("Install", () => {
  it("installs a target with the FOMOD wizard mode and shows the result", async () => {
    const { t, user } = await open("install", {
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

  it("nxm paste box submits the link", async () => {
    const { t, user } = await open("install", {
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
  it("shows jobs and streamed messages from events, and the log tail", async () => {
    const { t } = await open("activity", { log_tail: () => ["line one"] });
    act(() => {
      t.emit("modman:job", { id: 9, kind: "install", state: "started", error: null });
      t.emit("modman:activity", { job: 9, msg: { level: "info", spans: [{ text: "Extracting", style: "plain" }] }, text: "Extracting" });
    });
    expect(await screen.findByRole("list", { name: "Jobs" })).toHaveTextContent("install");
    expect(screen.getByRole("list", { name: "Messages" })).toHaveTextContent("Extracting");
    expect(await screen.findByText("line one")).toBeInTheDocument();
  });
});
