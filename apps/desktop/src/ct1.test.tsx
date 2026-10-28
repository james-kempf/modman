// SPDX-License-Identifier: GPL-3.0-or-later
// Regression tests for the continuous-testing round 1 defects (CT1 D01-D19).
// Layout defects are guarded by the classes that fix them (jsdom has no
// layout); the Playwright re-run covers the rendered result.
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StrictMode, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { jobLabel } from "./app/jobs";
import { useUi, type ViewId } from "./app/store";
import { Dialog } from "./components/dialogs/Dialog";
import { RootBoundary } from "./components/shell/ErrorBoundary";
import type { GameView } from "./ipc/types";
import type { Handler } from "./mock/fake";
import { experimentalGame, game, mod } from "./mock/fixtures";
import { setup } from "./test/harness";

async function open(view: ViewId, handlers: Record<string, Handler> = {}, games: GameView[] = [game()]) {
  const env = await setup(handlers, games);
  env.renderApp();
  act(() => useUi.getState().setView(view));
  return { ...env, user: userEvent.setup() };
}

/** A command that stays pending until `release()` (a slow backend). */
function pending<T>(value: T) {
  let release!: () => void;
  const gate = new Promise<void>((r) => (release = r));
  return { handler: async () => (await gate, value), release: () => act(() => release()) };
}

const main = () => screen.getByRole("main");

afterEach(() => vi.restoreAllMocks());

describe("CT1 D01 Overview long names", () => {
  // Fails when an Overview grid track is `1fr`/`auto` (min-content): an unbroken mod name widens <main>.
  it("every Overview grid uses minmax(0, ...) tracks", async () => {
    await open("overview", {
      mods_list: () => ({ mods: [mod({ name: "x".repeat(140) })], changed_since_deploy: false }),
    });
    await within(main()).findByRole("region", { name: "Mods" });
    const root = main().querySelector<HTMLElement>(":scope > div > div")!;
    // The page grid, the hero row and the summary-card row.
    const grids = [root, ...root.querySelectorAll<HTMLElement>(":scope > .stagger")];
    expect(grids).toHaveLength(3);
    for (const g of grids) {
      expect(g.className).toMatch(/(?:^|\s)grid-cols-\[minmax\(0,1fr\)\]/);
      expect(g.className).not.toMatch(/(?:^|\s)(?:md:|lg:|xl:)?grid-cols-[23](?:\s|$)/);
      expect(g.className).not.toMatch(/grid-cols-\[1\.35fr/);
    }
    expect(screen.getByRole("region", { name: "Skyrim Special Edition" }).className).toMatch(/minmax\(0,1\.35fr\)/);
  });
});

describe("CT1 D02 render errors", () => {
  // Fails when a throwing view unmounts the whole window (no error boundary).
  it("a view that throws shows an error note with Try again; the sidebar stays", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const bad = { ...mod({ name: "Broken" }), collections: null as unknown as string[] };
    const mods = [bad];
    await open("mods", { mods_list: () => ({ mods, changed_since_deploy: false }) });
    expect(await screen.findByText(/This view hit an error/)).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Views" })).toBeInTheDocument();
    // The other views still work.
    act(() => useUi.getState().setView("overview"));
    expect(await within(main()).findByRole("heading", { level: 2, name: "Skyrim Special Edition" })).toBeInTheDocument();
  });

  it("Try again re-renders the view after the data is fixed", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const bad = { ...mod({ name: "Broken" }), collections: null as unknown as string[] };
    let mods = [bad];
    const { user, qc } = await open("mods", { mods_list: () => ({ mods, changed_since_deploy: false }) });
    await screen.findByText(/This view hit an error/);
    mods = [mod({ name: "Fixed" })];
    await act(() => qc.refetchQueries());
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Fixed")).toBeInTheDocument();
  });

  // Fails when an error outside the views leaves a blank window.
  it("the root boundary shows a Reload action instead of a blank window", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    function Boom(): never {
      throw new Error("kaboom");
    }
    render(
      <RootBoundary>
        <Boom />
      </RootBoundary>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("kaboom");
    expect(screen.getByRole("button", { name: "Reload" })).toBeInTheDocument();
  });
});

describe("CT1 D03/D04 double activation", () => {
  // Fails when a double click on a move arrow sends two mod_move commands (a mod moved twice / out of range).
  it("Mods: move and enable toggle send one command per double click", async () => {
    const mv = pending({ uid: "x", from: 2, to: 1, of: 2, after: { deployed: null, needs_deploy: false } });
    const en = pending({ deployed: null, needs_deploy: false });
    const { t, user } = await open("mods", {
      mods_list: () => ({ mods: [mod({ name: "A", position: 1 }), mod({ name: "B", position: 2 })], changed_since_deploy: false }),
      mod_move: mv.handler,
      mods_set_enabled: en.handler,
    });
    await user.dblClick(await screen.findByRole("button", { name: "Move B up" }));
    expect(t.callsOf("mod_move")).toHaveLength(1);
    // Another move while one is in flight is dropped too (one list, one move at a time).
    await user.click(screen.getByRole("button", { name: "Move A down" }));
    expect(t.callsOf("mod_move")).toHaveLength(1);
    await user.dblClick(screen.getByRole("switch", { name: "Disable B" }));
    expect(t.callsOf("mods_set_enabled")).toHaveLength(1);
    expect(screen.getByRole("switch", { name: "Disable B" })).toHaveAttribute("aria-busy", "true");
    mv.release();
    en.release();
    await waitFor(() => expect(screen.getByRole("switch", { name: "Disable B" })).not.toHaveAttribute("aria-busy"));
  });

  const deps = () => [
    { status: "missing", key: "skse", name: "SKSE64", hint: "install 30379", urls: [], by: [], detail: "", acked: false },
    { status: "external", key: "ext", name: "Loader", hint: "", urls: [], by: [], detail: "", acked: false },
  ];

  // Fails when Requirements ack and Install can be activated twice while the first call runs.
  it("Requirements: I have it and Install run once per double click", async () => {
    const ack = pending({ ...deps()[1], acked: true });
    const inst = pending({ game: null, installed: null, collection: null, after: { deployed: null, needs_deploy: false } });
    const { t, user } = await open("requirements", { deps, deps_ack: ack.handler, install: inst.handler });
    await user.dblClick(await screen.findByRole("button", { name: "I have it" }));
    expect(t.callsOf("deps_ack")).toHaveLength(1);
    const install = within(main()).getByRole("button", { name: "Install" });
    await user.dblClick(install);
    expect(t.callsOf("install")).toHaveLength(1);
    expect(install).toHaveAttribute("aria-busy", "true");
    ack.release();
    inst.release();
  });
});

describe("CT1 D05 focus across back-to-back dialogs", () => {
  // Fails when the second dialog of a chain records the first dialog's button as its opener (focus lost on close).
  it("returns focus to the first opener after a chained dialog", async () => {
    function Host() {
      const [step, setStep] = useState(0);
      return (
        <>
          <div inert={step > 0}>
            {/* Focus recovery would pick this one: the test needs the real opener back. */}
            <button>other</button>
            <button onClick={() => setStep(1)}>opener</button>
          </div>
          {step === 1 && (
            <Dialog key="a" title="first" onBack={() => setStep(0)}>
              <button data-autofocus onClick={() => setStep(2)}>
                next
              </button>
            </Dialog>
          )}
          {step === 2 && (
            <Dialog key="b" title="second" onBack={() => setStep(0)}>
              <button data-autofocus onClick={() => setStep(0)}>
                done
              </button>
            </Dialog>
          )}
        </>
      );
    }
    render(
      <StrictMode>
        <Host />
      </StrictMode>,
    );
    const opener = screen.getByText("opener");
    opener.focus();
    act(() => opener.click());
    act(() => screen.getByText("next").click());
    expect(document.activeElement).toBe(screen.getByText("done"));
    // Let the first dialog's restore frame run (as it would before a person answers the second).
    await act(() => new Promise((r) => requestAnimationFrame(r)));
    act(() => screen.getByText("done").click());
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });

  // Fails when the second dialog opens a commit later (the backend asks again), sees <body> focused and records no opener.
  it("returns focus to the first opener when the next dialog opens in a later commit", async () => {
    function Host() {
      const [step, setStep] = useState(0);
      return (
        <>
          <div inert={step > 0}>
            <button>other</button>
            <button onClick={() => setStep(1)}>opener</button>
          </div>
          {step === 1 && (
            <Dialog key="a" title="first" onBack={() => setStep(0)}>
              <button
                data-autofocus
                onClick={() => {
                  setStep(0);
                  // The next prompt arrives after the first one was dropped.
                  queueMicrotask(() => setStep(2));
                }}
              >
                next
              </button>
            </Dialog>
          )}
          {step === 2 && (
            <Dialog key="b" title="second" onBack={() => setStep(0)}>
              <button data-autofocus onClick={() => setStep(0)}>
                done
              </button>
            </Dialog>
          )}
        </>
      );
    }
    render(<Host />);
    const opener = screen.getByText("opener");
    opener.focus();
    act(() => opener.click());
    await act(async () => screen.getByText("next").click());
    expect(document.activeElement).toBe(screen.getByText("done"));
    await act(() => new Promise((r) => requestAnimationFrame(r)));
    act(() => screen.getByText("done").click());
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });

  // Fails when the Install button is natively disabled once the field is cleared: focus cannot return to it.
  it("Install stays focusable (aria-disabled) with an empty target", async () => {
    await open("install");
    const form = (await screen.findByLabelText(/Mod id, Nexus URL/)).closest("form")!;
    const b = within(form).getByRole("button", { name: "Install" });
    expect(b).not.toBeDisabled();
    expect(b).toHaveAttribute("aria-disabled", "true");
  });
});

describe("CT1 D06 per-game results", () => {
  const two = () => {
    const games = [game(), experimentalGame({ active: false })];
    return {
      games,
      list: () => games.map((g) => ({ ...g })),
      switchTo: (domain: string) => games.forEach((g) => (g.active = g.domain === domain)),
    };
  };

  // Fails when Verify, Collections lookup or Install search results survive a game switch.
  it("Verify, lookup and search results reset when the active game changes", async () => {
    const g = two();
    const { user, qc } = await open("overview", {
      games_list: g.list,
      verify: () => ({ intact: 20, issues: [], interrupted: false }),
      search: () => ({ total: 1, mods: [{ modId: 1, name: "Hit", author: "a", version: "1", summary: null, downloads: 1, endorsements: 1, category: null, installed: false, picture: null }] }),
      collection_info: () => ({
        game: "skyrimspecialedition",
        slug: "abc",
        revision: { name: "Coll ABC", author: "c", game_name: "Skyrim", number: 1, status: "", latest: 1, mod_count: 1, optional_count: 0, total_size: 1, summary: "", url: "" },
        installed: null,
        update_available: null,
      }),
    });
    // Switch while the view stays mounted (changing views would remount it and hide the bug).
    const flip = async (to: string) => {
      g.switchTo(to);
      await act(() => qc.invalidateQueries());
    };
    await user.click(await screen.findByRole("button", { name: "Verify" }));
    expect(await screen.findByText(/20 files intact/)).toBeInTheDocument();
    await flip("cyberpunk2077");
    await within(main()).findByRole("heading", { level: 2, name: "Cyberpunk 2077" });
    expect(screen.queryByText(/20 files intact/)).toBeNull();

    act(() => useUi.getState().setView("install"));
    await user.type(await screen.findByLabelText("Search"), "x");
    await user.click(screen.getByRole("button", { name: "Search" }));
    expect(await screen.findByText("Hit")).toBeInTheDocument();
    await flip("skyrimspecialedition");
    await within(main()).findByText("Skyrim Special Edition");
    expect(screen.queryByText("Hit")).toBeNull();

    act(() => useUi.getState().setView("collections"));
    await user.type(await screen.findByLabelText(/Collection URL/), "abc");
    await user.click(screen.getByRole("button", { name: "Look up" }));
    expect(await screen.findByText("Coll ABC")).toBeInTheDocument();
    await flip("cyberpunk2077");
    await within(main()).findByText("Cyberpunk 2077");
    expect(screen.queryByText("Coll ABC")).toBeNull();
  });
});

describe("CT1 D07 deploy-due notice", () => {
  // Fails when a "Deploy now" notice stays after a deploy made elsewhere.
  it("is dismissed when a deploy succeeds", async () => {
    const { user } = await open("overview", {
      deploy: () => ({ dry_run: false, method: "auto", mods: 1, skipped: [], conflicts: [], placed: 3, backed_up: 0, new_dirs: 0, by_method: {}, purged: null, recovered: null, plugins: null }),
    });
    act(() => {
      useUi.getState().markDeployDue("skyrimspecialedition");
      useUi.getState().notify("warn", "Changes are not in the game yet.", "skyrimspecialedition");
    });
    expect(useUi.getState().notices.some((n) => n.deploy)).toBe(true);
    await user.click(await within(main()).findByRole("button", { name: "Deploy" }));
    await waitFor(() => expect(useUi.getState().notices.some((n) => n.deploy)).toBe(false));
    expect(useUi.getState().notices.map((n) => n.text)).toContain("Skyrim Special Edition: Deployed 3 files from 1 mods");
  });
});

describe("CT1 D08/D09/D11/D18 layout guards", () => {
  // D08: fails when a long game name widens a library tile or the sidebar header instead of truncating.
  it("long game names truncate in the library tiles and the sidebar header", async () => {
    await open("games", {}, [game({ name: "x".repeat(120) }), experimentalGame({ active: false })]);
    const tile = await within(main()).findByRole("button", { name: "x".repeat(120) });
    expect(tile.className).toMatch(/\bmin-w-0\b/);
    expect(tile.closest("li")!.className).toMatch(/\bmin-w-0\b/);
    expect(within(tile).getByText("x".repeat(120)).className).toMatch(/\btruncate\b/);
    const sidebar = screen.getByRole("complementary", { name: "Sidebar" });
    expect(within(sidebar).getByText("x".repeat(120)).className).toMatch(/\btruncate\b/);
  });

  // D09: fails when the sidebar grows the page past the viewport on short windows.
  it("sidebar scrolls internally", async () => {
    await open("overview");
    const aside = screen.getByRole("complementary", { name: "Sidebar" });
    expect(aside.className).toMatch(/\bmin-h-0\b/);
    expect(aside.className).toMatch(/\boverflow-y-auto\b/);
  });

  // D11: fails when unbroken requirement names, config keys or dialog titles overflow their box.
  it("unbroken text wraps in requirement rows, config labels and dialog titles", async () => {
    const U = "y".repeat(120);
    await open("requirements", {
      deps: () => [{ status: "external", key: "k", name: U, hint: "", urls: [], by: [U], detail: U, acked: false }],
    });
    expect((await screen.findByRole("heading", { level: 3, name: U })).className).toMatch(/wrap-anywhere/);
    expect(screen.getByText(`Needed by ${U}`).className).toMatch(/wrap-anywhere/);
    render(
      <Dialog title={U} onBack={() => {}}>
        <button>x</button>
      </Dialog>,
    );
    expect(screen.getByRole("heading", { level: 2, name: U }).className).toMatch(/wrap-anywhere/);
  });

  it("config labels wrap", async () => {
    await open("settings", { config_list: () => [{ key: "k".repeat(100), value: "v", description: "d" }] });
    const label = (await screen.findByText("k".repeat(100))).closest("label")!;
    expect(label.className).toMatch(/wrap-anywhere/);
  });

  // D18: fails when the mod name block can shrink to a few characters next to the controls at 900px.
  it("Mods rows keep a name basis and wrap the controls as a group", async () => {
    await open("mods", { mods_list: () => ({ mods: [mod({ name: "Unofficial Patch" })], changed_since_deploy: false }) });
    const name = await screen.findByText("Unofficial Patch");
    const block = name.parentElement!.parentElement!;
    expect(block.className).toMatch(/flex-\[1_1_18rem\]/);
    const li = block.closest("li")!;
    expect(li.className).toMatch(/flex-wrap/);
    expect(within(li).getByRole("switch").parentElement!.className).toMatch(/ml-auto/);
  });
});

describe("CT1 D10 failing queries", () => {
  const fail = (what: string) => () => {
    throw { message: `${what} failed` };
  };

  // Fails when a failing read renders as empty/"not signed in" instead of an error note.
  it("Settings shows notes for account, config, auto-update and app info", async () => {
    await open("settings", {
      account: fail("account"),
      config_list: fail("config_list"),
      autoupdate_status: fail("autoupdate_status"),
      app_info: fail("app_info"),
    });
    for (const w of ["account", "config_list", "autoupdate_status", "app_info"])
      expect(await within(main()).findByText(new RegExp(`${w} failed`))).toBeInTheDocument();
  });

  it.each([
    ["collections", "collections_list"],
    ["install", "downloads_list"],
    ["activity", "log_tail"],
  ] as const)("%s shows a note when %s fails", async (view, cmd) => {
    await open(view, { [cmd]: fail(cmd) });
    expect(await within(main()).findByText(new RegExp(`${cmd} failed`))).toBeInTheDocument();
  });

  it("Mods profiles shows a note when profiles_list fails", async () => {
    const { user } = await open("mods", { profiles_list: fail("profiles_list") });
    await user.click(await screen.findByRole("button", { name: "Profiles" }));
    expect(await within(main()).findByText(/profiles_list failed/)).toBeInTheDocument();
  });
});

describe("CT1 D12 auto-update toggle while busy", () => {
  // Fails when the busy toggle is natively disabled: focus drops to <body> after a click.
  it("stays focusable and ignores clicks while the change runs", async () => {
    const en = pending(null);
    const { t, user } = await open("settings", { autoupdate_enable: en.handler });
    const sw = await screen.findByRole("switch", { name: /Auto-update/ });
    await user.click(sw);
    expect(sw).not.toBeDisabled();
    expect(sw).toHaveAttribute("aria-disabled", "true");
    expect(document.activeElement).toBe(sw);
    await user.click(sw);
    expect(t.callsOf("autoupdate_enable")).toHaveLength(1);
    expect(t.callsOf("autoupdate_disable")).toHaveLength(0);
    en.release();
  });
});

describe("CT1 D13 profile save failure", () => {
  // Fails when the name field is cleared even though profile_save failed.
  it("keeps the name when saving fails", async () => {
    const { user } = await open("mods", {
      profile_save: () => {
        throw { message: "disk full" };
      },
    });
    await user.click(await screen.findByRole("button", { name: "Profiles" }));
    const field = screen.getByLabelText("Save the current mod set as");
    await user.type(field, "evening");
    await user.click(screen.getByRole("button", { name: "Save profile" }));
    expect(await screen.findByText(/Save profile: disk full/)).toBeInTheDocument();
    expect(field).toHaveValue("evening");
  });
});

describe("CT1 D15 Install without games", () => {
  // Fails when Install offers its forms with no game to install into.
  it("shows the No active game empty state", async () => {
    await open("install", {}, []);
    expect(await within(main()).findByRole("heading", { name: "No active game" })).toBeInTheDocument();
    expect(within(main()).queryByLabelText(/Mod id, Nexus URL/)).toBeNull();
  });
});

describe("CT1 D16/D17 job labels and cancellation", () => {
  // D16: fails when raw kinds such as game_add are shown.
  it("maps job kinds to readable labels", () => {
    expect(jobLabel("game_add")).toBe("Add game");
    expect(jobLabel("mods_set_enabled")).toBe("Enable/disable mods");
    expect(jobLabel("collection_install")).toBe("Install collection");
    expect(jobLabel("some_new_kind")).toBe("Some new kind");
  });

  // D17: fails when a cancelled FOMOD wizard is shown as a failed job / error notice.
  it("a cancelled job shows a neutral Cancelled state, not an error", async () => {
    const { t, user } = await open("install", {
      install: () => {
        throw { message: "FOMOD installer cancelled" };
      },
    });
    act(() => {
      t.emit("modman:job", { id: 7, kind: "game_add", state: "started", error: null });
    });
    expect(await screen.findByRole("status", { name: "Current job" })).toHaveTextContent("Add game");
    act(() => t.emit("modman:job", { id: 7, kind: "game_add", state: "failed", error: "FOMOD installer cancelled" }));
    const island = screen.getByRole("status", { name: "Current job" });
    expect(island).toHaveTextContent("Cancelled");
    expect(island).not.toHaveTextContent("Failed");
    await user.type(screen.getByLabelText(/Mod id, Nexus URL/), "266");
    await user.click(within(screen.getByLabelText(/Mod id, Nexus URL/).closest("form")!).getByRole("button", { name: "Install" }));
    await waitFor(() => expect(useUi.getState().notices.map((n) => [n.tone, n.text])).toContainEqual(["info", "Install cancelled."]));
    expect(screen.queryByRole("alert")).toBeNull();
    act(() => useUi.getState().setView("activity"));
    const jobs = await screen.findByRole("list", { name: "Jobs" });
    expect(jobs).toHaveTextContent("cancelled");
    expect(jobs).not.toHaveTextContent("FOMOD installer cancelled");
  });
});

describe("CT1 D19 purge result", () => {
  // Fails when Purge finishes silently.
  it("shows a notice from the PurgeReport", async () => {
    const { user } = await open("overview", {
      purge: () => ({ nothing_deployed: false, removed: 20, restored: 2, missing: 0, changed: 0, kept: 0, recovered: null, plugins: null }),
    });
    await user.click(await screen.findByRole("button", { name: "Purge" }));
    await waitFor(() =>
      expect(useUi.getState().notices.map((n) => n.text)).toContain("Purged: removed 20 files, restored 2 originals"),
    );
  });

  it("says nothing for a declined purge (null)", async () => {
    const { user, t } = await open("overview", { purge: () => null });
    await user.click(await screen.findByRole("button", { name: "Purge" }));
    await waitFor(() => expect(t.callsOf("purge")).toHaveLength(1));
    expect(useUi.getState().notices).toHaveLength(0);
  });
});
