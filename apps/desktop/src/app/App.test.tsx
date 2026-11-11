// SPDX-License-Identifier: GPL-3.0-or-later
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { afterChange, deployReport, experimentalGame, game, mod } from "../mock/fixtures";
import { setup } from "../test/harness";
import { VIEWS, useUi } from "./store";

const heading = (name: string) => screen.findByRole("heading", { level: 1, name });

const viewTabs = () => screen.getByRole("tablist", { name: "Views" });

describe("keyboard-only navigation", () => {
  // Fails when a view is unreachable from the top tabs with arrows + Enter, or Enter does not select it.
  it("reaches all 8 views from the top tabs with arrows + Enter", async () => {
    const { renderApp } = await setup();
    renderApp();
    const user = userEvent.setup();
    // The app opens on the Games library; focus starts on its tab.
    expect(document.activeElement).toHaveAttribute("data-tab", "games");
    await heading("Games");
    for (const v of VIEWS.slice(1)) {
      await user.keyboard("{ArrowRight}");
      expect(document.activeElement).toHaveAttribute("data-tab", v.id);
      await user.keyboard("{Enter}");
      await heading(v.label);
      expect(document.activeElement).toHaveAttribute("aria-selected", "true");
    }
  });

  // Fails when PageDown/PageUp (LB/RB under Steam Input) stop cycling views, do not wrap at either end,
  // leave focus behind, or Escape from the view no longer returns to the selected tab.
  it("PageDown/PageUp switch tabs with wrap-around; arrows enter the view; Escape returns to the tab", async () => {
    const { renderApp } = await setup();
    renderApp();
    const user = userEvent.setup();
    await user.keyboard("{PageDown}{PageDown}");
    await heading(VIEWS[2].label);
    expect(document.activeElement).toHaveAttribute("data-tab", VIEWS[2].id);
    // First -> last.
    await user.keyboard("{PageUp}{PageUp}{PageUp}");
    await heading("Settings");
    expect(document.activeElement).toHaveAttribute("data-tab", "settings");
    // Last -> first, and back.
    await user.keyboard("{PageDown}");
    await heading("Games");
    expect(document.activeElement).toHaveAttribute("data-tab", "games");
    await user.keyboard("{PageUp}");
    await heading("Settings");
    // Down leaves the tab bar for the Settings content.
    const main = screen.getByRole("main");
    await user.keyboard("{ArrowDown}");
    expect(main).toContainElement(document.activeElement as HTMLElement);
    await user.keyboard("{ArrowDown}{Escape}");
    expect(document.activeElement).toHaveAttribute("data-tab", "settings");
    expect(document.activeElement).toHaveAttribute("aria-selected", "true");
  });

  // Fails when the top tabs lose tab semantics or the roving tabindex (exactly one stop: the selected tab).
  it("is a tablist of 8 tabs with one roving stop on the selected tab", async () => {
    const { renderApp } = await setup();
    renderApp();
    const user = userEvent.setup();
    const tabs = within(viewTabs()).getAllByRole("tab");
    expect(tabs).toHaveLength(8);
    tabs.forEach((t) => expect(t.className).toMatch(/\bmin-h-control-xs\b/));
    const stops = () => within(viewTabs()).getAllByRole("tab").filter((t) => t.tabIndex === 0);
    expect(stops()).toEqual([viewTabs().querySelector("[data-tab=games]")]);
    await user.keyboard("{PageDown}");
    await heading("Overview");
    expect(stops()).toEqual([viewTabs().querySelector("[data-tab=overview]")]);
    expect(viewTabs().querySelector("[data-tab=overview]")).toHaveAttribute("aria-controls", screen.getByRole("main").id);
  });

  // Fails when the LB/RB glyphs end up inside the tablist, become focusable or announced, or show
  // without controller mode (html[data-pad], the pad: variant).
  it("LB/RB glyphs flank the tablist, hidden from AT and focus, shown only in controller mode", async () => {
    const { renderApp } = await setup();
    renderApp();
    const list = viewTabs();
    const glyphs = Array.from(screen.getByRole("banner", { name: "App" }).querySelectorAll("kbd"));
    expect(glyphs.map((g) => g.textContent)).toEqual(["LB", "RB"]);
    const [lb, rb] = glyphs;
    expect(lb.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(rb.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_PRECEDING).toBeTruthy();
    for (const g of glyphs) {
      expect(list.contains(g)).toBe(false);
      expect(g.parentElement!.parentElement).toBe(list.parentElement);
      expect(g.closest("[aria-hidden='true']")).not.toBeNull();
      expect(g).not.toHaveAttribute("tabindex");
      expect(g.closest("button")).toBeNull();
      expect(g.className).toMatch(/(^|\s)hidden(\s|$)/);
      expect(g.className).toMatch(/(^|\s)pad:inline-flex(\s|$)/);
    }
  });

  // Fails when the Activity tab loses its running-jobs + prompts badge.
  it("badges the Activity tab with running jobs and prompts", async () => {
    const { renderApp } = await setup();
    renderApp();
    const activity = () => viewTabs().querySelector("[data-tab=activity]")!;
    expect(activity().textContent).toBe("Activity");
    act(() => useUi.getState().onPrompt({ id: 1, job: null, prompt: { type: "confirm", question: "Sure?", default_yes: true } }));
    await waitFor(() => expect(activity().textContent).toBe("Activity1"));
  });
});

describe("header game title", () => {
  // Fails when the active game's name goes back into a card with a status dot and mod count, loses its
  // full-name tooltip, or scrolls away with the view (the header must sit outside the scrolling <main>).
  it("is plain pinned text: no card, dot or count", async () => {
    const { renderApp } = await setup();
    renderApp();
    const name = await screen.findByText("Skyrim Special Edition", { selector: "header span" });
    expect(name).toHaveAttribute("title", "Skyrim Special Edition");
    expect(name.className).toMatch(/\btruncate\b/);
    expect(name.className).toMatch(/\btext-zinc-100\b/);
    const header = name.closest("header")!;
    expect(header).toBe(screen.getByRole("banner", { name: "App" }));
    expect(screen.getByRole("main").contains(header)).toBe(false);
    expect(header.className).toMatch(/\bshrink-0\b/);
    // The name's own group (the header also holds the bordered LB/RB keycaps and the Activity badge).
    const group = name.parentElement!;
    expect(group.querySelector(".rounded-pill, [title$=' mods']")).toBeNull();
    expect(group.querySelector(".border, [class*='bg-ink-2']")).toBeNull();
    expect(group.className).not.toMatch(/\bborder\b|bg-ink-2/);
  });

  // Fails when the experimental badge comes back next to the header title (the user removed it there).
  it("shows no experimental badge, even for an experimental game", async () => {
    const { renderApp } = await setup({}, [experimentalGame()]);
    renderApp();
    const name = await screen.findByText("Cyberpunk 2077", { selector: "header span" });
    expect(within(name.closest("header")!).queryByTestId("experimental-badge")).toBeNull();
  });
});

describe("experimental badge", () => {
  // Fails when the badge drops off Overview, or comes back in the app header or the Mods header (removed there).
  it("is shown on Overview for a game with support=experimental, not in the app header or Mods", async () => {
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
  // header "Deploy changes" button; the deploy stays on the Overview's Deployment card.
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
      deps: () => [{ status: "missing", key: "skse", name: "SKSE64", hint: "install 30379", urls: [], by: [], detail: "", acked: false }],
      install: () => {
        throw { message: "mod not found" };
      },
    });
    renderApp();
    act(() => useUi.getState().setView("requirements"));
    const user = userEvent.setup();
    await user.click(await within(screen.getByRole("main")).findByRole("button", { name: "Install" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("mod not found");
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
