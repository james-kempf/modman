// SPDX-License-Identifier: GPL-3.0-or-later
// Regression tests for the continuous-testing round 2 defects (CT2 D01-D16).
// Layout defects are guarded by the classes / DOM placement that fix them
// (jsdom has no layout); the Playwright re-run covers the rendered result.
import { QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { StrictMode } from "react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useUi, type ViewId } from "./app/store";
import { makeQueryClient } from "./app/queries";
import { domainProblem } from "./components/AddGameDialog";
import { ViewBoundary } from "./components/ErrorBoundary";
import { monogram } from "./components/ui";
import type { CollectionResult, GameView } from "./ipc/types";
import type { Handler } from "./mock/fake";
import { afterChange, deployReport, experimentalGame, game, mod } from "./mock/fixtures";
import { setup } from "./test/harness";

async function open(view: ViewId, handlers: Record<string, Handler> = {}, games: GameView[] = [game()]) {
  const env = await setup(handlers, games);
  env.renderApp();
  act(() => useUi.getState().setView(view));
  return { ...env, user: userEvent.setup() };
}

const main = () => screen.getByRole("main");
const noticeTexts = () => useUi.getState().notices.map((n) => n.text);

const collResult = (o: Partial<CollectionResult> = {}): CollectionResult => ({
  game: "skyrimspecialedition",
  slug: "abc",
  revision: 3,
  cancelled: false,
  download_only: false,
  installed: 2,
  present: 0,
  failed: [],
  removed: 0,
  up_to_date: false,
  deployed: null,
  ...o,
});

const collInfo = (installed: { revision: number } | null) => ({
  game: "skyrimspecialedition",
  slug: "abc",
  revision: {
    name: "Coll ABC", author: "c", game_name: "Skyrim", number: 3, status: "published", latest: 3,
    mod_count: 2, optional_count: 0, total_size: 1, summary: null, url: "u",
  },
  installed,
  update_available: null,
});

afterEach(() => vi.restoreAllMocks());

describe("CT2 D01 dialogs", () => {
  // Fails when the dialog renders inside the view (a transformed ancestor traps its fixed backdrop) and the sidebar stays live.
  it("render in a portal on <body>, fixed, and make the app inert", async () => {
    const m = mod({ name: "SkyUI" });
    const { user } = await open("mods", { mods_list: () => ({ mods: [m], changed_since_deploy: false }), mod_detail: () => ({ mod: m, collections: [], fomod: [], wins: 0, loses: 0, url: null }) });
    await user.click(await screen.findByRole("button", { name: "Details" }));
    const dialog = await screen.findByRole("dialog");
    expect(main().contains(dialog)).toBe(false);
    const backdrop = dialog.parentElement!;
    expect(backdrop.parentElement).toBe(document.body);
    expect(backdrop.className).toMatch(/\bfixed inset-0\b/);
    // Fits short windows: scrolls inside, height bound to the viewport.
    expect(dialog.className).toMatch(/max-h-\[calc\(100vh-2rem\)\]/);
    expect(dialog.className).toMatch(/overflow-y-auto/);
    const tab = document.querySelector("[data-tab=settings]")!;
    expect(tab.closest("[inert]")).not.toBeNull();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(tab.closest("[inert]")).toBeNull();
  });

  // Fails when a dialog opened over another leaves the lower one interactive.
  it("a prompt over a dialog makes the lower dialog inert, and restores it", async () => {
    const { user } = await open("overview");
    await user.click(within(screen.getByRole("complementary", { name: "Sidebar" })).getByRole("button", { name: "Add game" }));
    const add = await screen.findByRole("dialog", { name: "Add a game" });
    act(() => useUi.getState().onPrompt({ id: 1, job: null, prompt: { type: "confirm", question: "Sure?", default_yes: true } }));
    await screen.findByRole("dialog", { name: "Confirm" });
    expect(add.closest("[inert]")).not.toBeNull();
    act(() => useUi.getState().dropPrompt(1));
    await waitFor(() => expect(add.closest("[inert]")).toBeNull());
    expect(document.querySelector("[data-tab=overview]")!.closest("[inert]")).not.toBeNull();
  });
});

describe("CT2 D02/D04 deploy-due per game", () => {
  const two = [game(), experimentalGame({ active: false })];

  // D02: fails when the deploy-due state of one game shows after switching to another.
  it("is not shown for another game", async () => {
    let games = two;
    const { qc } = await open("overview", { games_list: () => games }, two);
    act(() => {
      useUi.getState().markDeployDue("skyrimspecialedition");
      useUi.getState().notify("warn", "Changes are not in the game yet.", "skyrimspecialedition");
    });
    expect(await screen.findByRole("button", { name: "Deploy changes" })).toBeInTheDocument();
    games = [game({ active: false }), experimentalGame({ active: true })];
    await act(() => qc.refetchQueries({ queryKey: ["games"] }));
    await within(main()).findByText("Cyberpunk 2077");
    expect(screen.queryByRole("button", { name: "Deploy changes" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Deploy now" })).toBeNull();
    expect(within(main()).getByText("Deployed and in sync")).toBeInTheDocument();
  });

  // D02: fails when the backend's per-game changed_since_deploy is not used for the sidebar.
  it("follows the game's changed_since_deploy", async () => {
    await open("overview", { mods_list: () => ({ mods: [], changed_since_deploy: true }) });
    expect(await screen.findByRole("button", { name: "Deploy changes" })).toBeInTheDocument();
  });

  // D04: fails when a deploy clears the pending state although mods changed while it ran, or the notice omits the game.
  it("a change during the deploy stays pending; the notice names the game", async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const { user, t } = await open("overview", { deploy: async () => (await gate, deployReport({ placed: 5, mods: 1 })) });
    act(() => useUi.getState().markDeployDue("skyrimspecialedition"));
    await user.click(await within(main()).findByRole("button", { name: "Deploy" }));
    await waitFor(() => expect(t.callsOf("deploy")).toEqual([{ game: "skyrimspecialedition", dry_run: false, method: null }]));
    act(() => useUi.getState().markDeployDue("skyrimspecialedition"));
    act(() => release());
    await waitFor(() => expect(noticeTexts()).toContain("Skyrim Special Edition: Deployed 5 files from 1 mods"));
    expect(useUi.getState().deployDue.skyrimspecialedition).toBeDefined();
  });
});

describe("CT2 D04 deploy progress", () => {
  // Fails when the progress of another game's deploy shows under the active game's header.
  it("is shown only for the game being deployed", async () => {
    await open("overview");
    act(() => {
      useUi.getState().setDeployGame("cyberpunk2077");
      useUi.getState().onDeployProgress({ job: 1, done: 2, total: 20 });
    });
    expect(screen.queryByText(/deploying 2\/20/)).toBeNull();
    act(() => useUi.getState().setDeployGame("skyrimspecialedition"));
    expect(await screen.findByText(/deploying 2\/20/)).toBeInTheDocument();
  });
});

describe("CT2 D03/D07 collection install", () => {
  // D03: fails when a collection install with deploy off does not offer the deploy (CollectionResult has no needs_deploy).
  it("deploy off: warns and shows Deploy changes when the game is out of sync", async () => {
    let dirty = false;
    const { user } = await open("collections", {
      collection_info: () => collInfo(null),
      collection_install: () => ((dirty = true), collResult()),
      mods_list: () => ({ mods: [], changed_since_deploy: dirty }),
    });
    await user.type(await screen.findByLabelText(/Collection URL/), "abc");
    await user.click(screen.getByRole("button", { name: "Look up" }));
    await user.click(await screen.findByRole("switch", { name: "Deploy when done" }));
    await user.click(screen.getByRole("button", { name: "Install collection" }));
    await waitFor(() => expect(noticeTexts()).toContain("Changes are not in the game yet."));
    expect(useUi.getState().deployDue.skyrimspecialedition).toBeDefined();
    expect(await screen.findByRole("button", { name: "Deploy changes" })).toBeInTheDocument();
  });

  // D03: fails when a collection install that deployed says nothing about the deploy. D07: the panel stays at "Install collection".
  it("deploy on: 'Deployed N files' notice, and the look-up panel refreshes", async () => {
    let installed: { revision: number } | null = null;
    const { user } = await open("collections", {
      collection_info: () => collInfo(installed),
      collection_install: () => ((installed = { revision: 3 }), collResult({ deployed: deployReport({ placed: 7, mods: 2 }) })),
    });
    act(() => useUi.getState().markDeployDue("skyrimspecialedition"));
    await user.type(await screen.findByLabelText(/Collection URL/), "abc");
    await user.click(screen.getByRole("button", { name: "Look up" }));
    await user.click(await screen.findByRole("button", { name: "Install collection" }));
    await waitFor(() => expect(noticeTexts()).toContain("Deployed 7 files from 2 mods"));
    expect(useUi.getState().deployDue).toEqual({});
    expect(await screen.findByRole("button", { name: "Install this revision" })).toBeInTheDocument();
    expect(screen.getByText(/Installed at revision 3/)).toBeInTheDocument();
  });

  // D07: fails when a cancelled install shows a success tone.
  it("a cancelled install is a neutral notice", async () => {
    const { user } = await open("collections", {
      collection_info: () => collInfo(null),
      collection_install: () => collResult({ cancelled: true, installed: 0 }),
    });
    await user.type(await screen.findByLabelText(/Collection URL/), "abc");
    await user.click(screen.getByRole("button", { name: "Look up" }));
    await user.click(await screen.findByRole("button", { name: "Install collection" }));
    await waitFor(() => expect(useUi.getState().notices).toEqual([expect.objectContaining({ tone: "info", text: "abc: cancelled" })]));
  });
});

describe("CT2 D05 error boundary recovery", () => {
  const bad = () => ({ ...mod({ name: "Broken" }), collections: null as unknown as string[] });

  // Fails when Try again re-renders with the cached bad data (the query is not refetched).
  it("Try again refetches the view's data", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    let mods = [bad()];
    const { user } = await open("mods", { mods_list: () => ({ mods, changed_since_deploy: false }) });
    await screen.findByText(/This view hit an error/);
    mods = [mod({ name: "Fixed" })];
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Fixed")).toBeInTheDocument();
  });

  // Fails when coming back to a crashed view serves the cached bad data again.
  it("recovers after navigating away and back once the data is good", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    let mods = [bad()];
    await open("mods", { mods_list: () => ({ mods, changed_since_deploy: false }) });
    await screen.findByText(/This view hit an error/);
    mods = [mod({ name: "Fixed" })];
    act(() => useUi.getState().setView("install"));
    act(() => useUi.getState().setView("mods"));
    expect(await screen.findByText("Fixed")).toBeInTheDocument();
  });
});

describe("CT2 D05 Activity log", () => {
  // Fails when one malformed streamed message (kept in the session log, not a query) breaks Activity permanently.
  it("renders a malformed message span instead of crashing", async () => {
    await open("activity");
    act(() =>
      useUi.getState().onActivity({ job: 1, msg: { level: "info", spans: [{ text: { bad: 1 } as unknown as string, style: "plain" }] }, text: "x" }),
    );
    expect(await screen.findByText('{"bad":1}')).toBeInTheDocument();
    expect(screen.queryByText(/This view hit an error/)).toBeNull();
  });
});

describe("CT2 D05 boundary under StrictMode", () => {
  // Fails when StrictMode's simulated unmount leaves the boundary stuck on "Loading…" after its automatic retry.
  it("shows the fallback after the automatic retry fails", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    function Boom(): never {
      throw new Error("bad data");
    }
    render(
      <StrictMode>
        <QueryClientProvider client={makeQueryClient()}>
          <ViewBoundary name="Mods">
            <Boom />
          </ViewBoundary>
        </QueryClientProvider>
      </StrictMode>,
    );
    expect(await screen.findByText(/This view hit an error/)).toBeInTheDocument();
  });
});

describe("CT2 D06/D08 layout guards", () => {
  // D06: fails when the header actions squeeze the title instead of wrapping below it.
  it("ViewHeader wraps the actions below a readable title", async () => {
    await open("mods");
    const h1 = await within(main()).findByRole("heading", { level: 1, name: "Mods" });
    const header = h1.closest("header")!;
    expect(header.className).toMatch(/\bflex-wrap\b/);
    expect(header.className).not.toMatch(/md:flex-row/);
    expect(h1.parentElement!.className).toMatch(/flex-\[1_1_24rem\]/);
  });

  // D08: fails when every job row is a tab stop and the list grows the page.
  it("Activity jobs are one scroll region, not one tab stop per row", async () => {
    await open("activity");
    act(() => {
      for (let i = 1; i <= 5; i++) {
        useUi.getState().onJob({ id: i, kind: "enable", state: "started", error: null });
        useUi.getState().onJob({ id: i, kind: "enable", state: "finished", error: null });
      }
    });
    const list = await screen.findByRole("list", { name: "Jobs" });
    expect(list.getAttribute("tabindex")).toBe("0");
    expect(list.className).toMatch(/overflow-y-auto/);
    expect(within(list).getAllByRole("listitem").every((li) => !li.hasAttribute("tabindex"))).toBe(true);
  });
});

describe("CT2 D09 multi-line paste", () => {
  // Fails when a multi-line nxm paste is sent as one glued string.
  it("nxm field: each pasted link is shown and sent separately", async () => {
    const { user, t } = await open("install", { nxm_submit: (a) => ({ row: 1, link: String(a!.url).split("?")[0], kind: "mod" }) });
    const field = await screen.findByLabelText("nxm link");
    fireEvent.paste(field, { clipboardData: { getData: () => "nxm://a/mods/1/files/1?key=k\nnxm://a/mods/2/files/2?key=k\n" } });
    expect(field).toHaveValue("nxm://a/mods/1/files/1?key=k nxm://a/mods/2/files/2?key=k");
    await user.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(t.callsOf("nxm_submit").map((a) => a!.url)).toEqual(["nxm://a/mods/1/files/1?key=k", "nxm://a/mods/2/files/2?key=k"]));
    await waitFor(() => expect(noticeTexts()).toContain("Queued 2 links"));
    expect(field).toHaveValue("");
  });

  // Fails when lines are glued together ("266267") in the install field.
  it("install field keeps the first pasted line and says so", async () => {
    await open("install");
    const field = await screen.findByLabelText(/Mod id, Nexus URL/);
    fireEvent.paste(field, { clipboardData: { getData: () => "266\n267\n" } });
    expect(field).toHaveValue("266");
    expect(noticeTexts()).toContain("Pasted the first of 2 lines; install one mod at a time.");
  });
});

describe("CT2 D10/D11 result notices", () => {
  // D11: fails when a declined overwrite (null result) clears the name.
  it("profile save: declined overwrite keeps the name; a save says so", async () => {
    let result: number | null = null;
    const { user } = await open("mods", { profile_save: () => result });
    await user.click(await screen.findByRole("button", { name: "Profiles" }));
    const field = await screen.findByLabelText("Save the current mod set as");
    await user.type(field, "evening");
    await user.click(screen.getByRole("button", { name: "Save profile" }));
    await waitFor(() => expect(noticeTexts()).toContain("Profile evening was not overwritten."));
    expect(field).toHaveValue("evening");
    result = 3;
    await user.click(screen.getByRole("button", { name: "Save profile" }));
    await waitFor(() => expect(noticeTexts()).toContain("Saved profile evening (3 mods)"));
    expect(field).toHaveValue("");
  });

  // D10: fails when uninstall / remove collection / install updates finish silently.
  it("uninstall, remove collection and install updates report their result", async () => {
    const m = mod({ name: "SkyUI" });
    const { user } = await open("mods", {
      mods_list: () => ({ mods: [m], changed_since_deploy: false }),
      mod_detail: () => ({ mod: m, collections: [], fomod: [], wins: 0, loses: 0, url: null }),
      mods_uninstall: () => ({ done: true, mods: 1, archives: [], after: afterChange({ needs_deploy: false }) }),
      updates_check: () => ({ checked: 1, fetched: 1, stopped_early: false, installed: 0, updates: [{ mod: m, file: { file_id: 1, name: "f", version: "2" }, pinned: false }] }),
      updates_install: () => ({ checked: 1, fetched: 1, stopped_early: false, installed: 1, updates: [] }),
      collections_list: () => [{ record: { id: 1, game: "skyrimspecialedition", slug: "abc", name: "ABC", author: null, revision: 1, dir: null, failed: [], installed_at: 0, mods: [] }, enabled: 0 }],
      collection_remove: () => ({ removed: true, uninstalled: 2, after: afterChange({ needs_deploy: false }) }),
    });
    await user.click(await screen.findByRole("button", { name: "Details" }));
    await user.click(await screen.findByRole("button", { name: "Uninstall" }));
    await waitFor(() => expect(noticeTexts()).toContain("Uninstalled SkyUI"));
    await user.click(screen.getByRole("button", { name: "Updates" }));
    await user.click(await screen.findByRole("button", { name: "Check for updates" }));
    await user.click(await screen.findByRole("button", { name: "Install updates" }));
    await waitFor(() => expect(noticeTexts()).toContain("Installed 1 update"));
    act(() => useUi.getState().setView("collections"));
    await user.click(await screen.findByRole("button", { name: "Remove" }));
    await waitFor(() => expect(noticeTexts()).toContain("Removed collection abc and 2 mods"));
  });
});

describe("CT2 D12/D13 text handling", () => {
  // D12: fails when an invalid domain is sent to game_add.
  it("Add game validates the domain inline", async () => {
    const { user, t } = await open("overview", { game_add: () => game() });
    await user.click(within(screen.getByRole("complementary", { name: "Sidebar" })).getByRole("button", { name: "Add game" }));
    const field = await screen.findByLabelText("Nexus game domain");
    await user.type(field, "Skyrim SE");
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Add game" }));
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(field).toHaveAccessibleDescription(/lowercase letters and digits/);
    expect(t.callsOf("game_add")).toEqual([]);
    expect(domainProblem("  fallout4 ")).toBeNull();
    expect(domainProblem("   ")).not.toBeNull();
  });

  // D13: fails when initials drop non-Latin letters or split emoji / combining sequences.
  it("monogram initials are Unicode-aware", () => {
    expect(monogram("Ünofficial Ëxtended Patch").initials).toBe("ÜË");
    expect(monogram("スカイリム 日本語化").initials).toBe("ス日");
    expect(monogram("🧙‍♂️ Wizard Robes").initials).toBe("WR");
    expect(monogram("🔥🔥🔥").initials).toBe("🔥");
    expect(monogram("Z̷̢̛a̸l̴g̵o̶").initials).toBe("Z");
    expect(monogram("").initials).toBe("?");
  });
});

describe("CT2 D14/D15 focus and orphaned prompts", () => {
  // D14: fails when focus leaves the view after the focused collection row is removed.
  it("after Remove, focus goes to the look-up field", async () => {
    let list = [{ record: { id: 1, game: "skyrimspecialedition", slug: "abc", name: "ABC", author: null, revision: 1, dir: null, failed: [], installed_at: 0, mods: [] }, enabled: 0 }];
    const { user } = await open("collections", {
      collections_list: () => list,
      collection_remove: () => ((list = []), { removed: true, uninstalled: 0, after: afterChange({ needs_deploy: false }) }),
    });
    await user.click(await screen.findByRole("button", { name: "Remove" }));
    await waitFor(() => expect(screen.getByLabelText(/Collection URL/)).toHaveFocus());
  });

  // D15: fails when a prompt of a job that ended stays on screen.
  it("a prompt of a job that ended is withdrawn", async () => {
    await open("overview");
    act(() => {
      useUi.getState().onJob({ id: 70, kind: "install", state: "started", error: null });
      useUi.getState().onPrompt({ id: 9, job: 70, prompt: { type: "confirm", question: "Overwrite?", default_yes: true } });
    });
    await screen.findByRole("dialog", { name: "Confirm" });
    act(() => useUi.getState().onJob({ id: 70, kind: "install", state: "failed", error: "network went away" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(noticeTexts()).toContain("A question from job #70 closed: the job ended."));
  });
});
