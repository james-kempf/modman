// SPDX-License-Identifier: GPL-3.0-or-later
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TOAST_CLOSE_MS } from "../components/ui";
import { afterChange, deployReport, experimentalGame, game, mod } from "../mock/fixtures";
import { performNav } from "../nav/actions";
import { setPad } from "../nav/modality";
import { focusables } from "../nav/spatial";
import { setup } from "../test/harness";
import { TOAST_MIN_DWELL_MS, VIEWS, useUi } from "./store";

/** Let the shown toast reach its minimum dwell, so the next write replaces it at once (T12). */
const dwell = () => act(() => new Promise<void>((r) => setTimeout(r, TOAST_MIN_DWELL_MS)));

// The view is named by <main>'s aria-label (no visible page title).
const heading = (name: string) => screen.findByRole("main", { name });

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

  // Fails when a running-work counter comes back next to the Activity tab (the toast is the only live indicator).
  it("shows only the Activity label while a job runs or a prompt waits", async () => {
    const { renderApp, t } = await setup();
    renderApp();
    const activity = () => viewTabs().querySelector("[data-tab=activity]")!;
    expect(activity().textContent).toBe("Activity");
    act(() => {
      t.emit("vessel:job", { id: 1, kind: "deploy", state: "started", error: null });
      useUi.getState().onPrompt({ id: 1, job: 1, prompt: { type: "confirm", question: "Sure?", default_yes: true } });
    });
    expect(useUi.getState().jobs[1]?.state).toBe("started");
    expect(activity().textContent).toBe("Activity");
    expect(activity().querySelector(".bg-accent-400")).toBeNull();
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
    // The name's own group (the header also holds the bordered LB/RB keycaps).
    const group = name.parentElement!;
    expect(group.querySelector(".rounded-pill, [title$=' mods']")).toBeNull();
    expect(group.querySelector(".border, [class*='bg-ink-2']")).toBeNull();
    expect(group.className).not.toMatch(/\bborder\b|bg-ink-2/);
  });

  // Fails when the view tabs get their own side inset again (a padded wrapper or padding on row 1 only),
  // which puts the tab bar out of line with the brand and Quit above it.
  it("shares one side inset with the view tabs", async () => {
    const { renderApp } = await setup();
    renderApp();
    const header = screen.getByRole("banner", { name: "App" });
    const list = await screen.findByRole("tablist", { name: "Views" });
    expect(header.contains(list)).toBe(true);
    expect(header.className).toMatch(/(^|\s)px-6(\s|$)/);
    expect(header.className).toMatch(/(^|\s)lg:px-10(\s|$)/);
    const sidePad = /(^|\s)([a-z]+:)?(p|px|pl|pr)-/;
    for (let el = list.parentElement; el && el !== header; el = el.parentElement) expect(el.className).not.toMatch(sidePad);
    for (const row of header.children) expect(row.className).not.toMatch(sidePad);
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
    expect([useUi.getState().toast].flatMap((x) => (x ? [x] : []))).toEqual([]);
    expect(screen.queryByRole("button", { name: "Deploy changes" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Deploy now" })).toBeNull();
    act(() => useUi.getState().setView("overview"));
    expect(await screen.findByText("Changes waiting to be deployed")).toBeInTheDocument();
    await user.click(within(screen.getByRole("main")).getByRole("button", { name: "Deploy" }));
    await waitFor(() => expect(t.callsOf("deploy")).toHaveLength(1));
    await screen.findByText(/42 files/);
    expect(useUi.getState().deployDue).toEqual({});
  });

  it("command errors become a notice with the CmdError message", async () => {
    const { renderApp } = await setup({
      deps: () => [{ status: "missing", key: "skse", name: "SKSE64", hint: "install 30379", urls: [], by: [], detail: "", acked: false, action: null }],
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

describe("job progress in the toast", () => {
  // Fails when job events stop writing the one toast (a separate job pill, a stale running state, or no outcome).
  it("shows the running job with its latest line, then its outcome, in the same pill", async () => {
    const { renderApp, t } = await setup();
    renderApp();
    act(() => {
      t.emit("vessel:job", { id: 4, kind: "deploy", state: "started", error: null });
      t.emit("vessel:activity", { job: 4, msg: { level: "info", spans: [{ text: "Linking files", style: "plain" }] }, text: "Linking files" });
    });
    const pill = await screen.findByRole("status", { name: /^Deploy/ });
    expect(pill).toHaveAttribute("aria-busy", "true");
    await waitFor(() => expect(pill).toHaveTextContent("Linking files"));
    await dwell();
    act(() => t.emit("vessel:job", { id: 4, kind: "deploy", state: "failed", error: "disk full" }));
    expect(screen.getByRole("alert")).toBe(pill);
    expect(pill).toHaveTextContent("Deploy failed");
    expect(pill).toHaveTextContent("disk full · see Activity");
    expect(pill).not.toHaveAttribute("aria-busy");
  });
});

/** The pill's visible (announced) text: everything but the aria-hidden outgoing copy of the text cycle. */
const liveText = (el: Element) => {
  const copy = el.cloneNode(true) as Element;
  copy.querySelectorAll("[aria-hidden=true]").forEach((n) => n.remove());
  return copy.textContent ?? "";
};

describe("toast slot in the app bar", () => {
  const bar = () => screen.getByRole("banner", { name: "App" });
  const slot = () => within(bar()).getByRole("complementary", { name: "Notices" });

  // Fails when notices go back to the bottom stack, or the slot leaves its place between the game and the account.
  it("shows notices in the app bar, between the game name and the account", async () => {
    const { renderApp } = await setup();
    renderApp();
    const name = await within(bar()).findByText("Skyrim Special Edition");
    act(() => useUi.getState().showToast("success", "Deployed", { detail: "3 mods" }));
    const toast = within(slot()).getByRole("status");
    expect(toast).toHaveTextContent("Deployed3 mods");
    expect(slot()).toHaveAttribute("aria-live", "polite");
    const quit = within(bar()).getByRole("button", { name: "Quit" });
    // Document order = grid order at md and up: game, slot, account, Quit.
    expect(name.compareDocumentPosition(slot()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(slot().compareDocumentPosition(quit) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(slot().parentElement!.className).toMatch(/\bjustify-center\b/);
  });

  // Fails when a toast, notice or the job island renders outside the app bar again (the old bottom stack).
  it("renders nothing toast-like outside the app bar", async () => {
    const { renderApp, t } = await setup();
    renderApp();
    act(() => t.emit("vessel:job", { id: 9, kind: "deploy", state: "started", error: null }));
    await dwell();
    act(() => useUi.getState().showToast("error", "boom"));
    const outside = (el: HTMLElement) => !bar().contains(el);
    expect(screen.queryAllByRole("status").filter(outside)).toEqual([]);
    expect(screen.queryAllByRole("alert").filter(outside)).toEqual([]);
    expect(document.querySelector(".fixed.bottom-5")).toBeNull();
    expect(bar()).toContainElement(screen.getByRole("alert"));
    expect(screen.queryByRole("complementary", { name: "Job status" })).toBeNull();
  });

  // Fails when a message waits behind a running job's live toast, or job progress during a message steals the
  // slot back (T16: the message keeps it; the live toast returns with its latest state when the message closes).
  it("a message during a job takes the pill; job progress meanwhile is applied and shows when it returns", async () => {
    const { renderApp, t } = await setup();
    renderApp();
    act(() => t.emit("vessel:job", { id: 5, kind: "deploy", state: "started", error: null }));
    const pill = within(slot()).getByRole("status");
    expect(pill).toHaveTextContent("Deploying");
    await dwell();
    act(() => useUi.getState().showToast("info", "hello"));
    expect(within(slot()).getAllByRole("status")).toEqual([pill]);
    expect(pill).toHaveTextContent("hello");
    act(() => useUi.getState().onDeployProgress({ job: 5, done: 3, total: 10 }));
    expect(liveText(pill)).toContain("hello");
    act(() => useUi.getState().closeToast(useUi.getState().toast!.id));
    expect(liveText(pill)).toContain("3 / 10 files");
    expect(liveText(pill)).not.toContain("hello");
  });

  // Fails when a second pill stacks, a count appears, the newer message waits for the older one, or the pill
  // is re-created (re-animated) instead of swapping its content in place.
  it("shows one toast: a new message replaces it at once, in the same pill", async () => {
    const { renderApp } = await setup();
    renderApp();
    act(() => useUi.getState().showToast("info", "first"));
    const pill = within(slot()).getByRole("status");
    await dwell();
    act(() => useUi.getState().showToast("success", "second"));
    expect(within(slot()).getAllByRole("status")).toEqual([pill]);
    expect(liveText(pill)).toContain("second");
    expect(liveText(slot())).not.toContain("first");
    expect(slot()).not.toHaveTextContent(/\+\d/);
    // An error swaps into the same element and is announced as an alert; the live region stays the same aside.
    const live = slot();
    await dwell();
    act(() => useUi.getState().showToast("error", "third"));
    expect(within(slot()).getByRole("alert")).toBe(pill);
    expect(slot()).toBe(live);
    expect(live).toHaveAttribute("aria-live", "polite");
  });

  // Fails when a toast gets a control again (Undo, Deploy now, close): toasts are informational only.
  it("toasts in the bar hold no buttons or focus stops", async () => {
    const { renderApp, t } = await setup();
    renderApp();
    act(() => useUi.getState().showToast("success", "Uninstalled", { detail: "SkyUI" }));
    expect(within(slot()).queryAllByRole("button")).toEqual([]);
    act(() => {
      t.emit("vessel:job", { id: 7, kind: "deploy", state: "started", error: null });
    });
    expect(within(slot().parentElement!).queryAllByRole("button")).toEqual([]);
  });

  // Fails when long text widens the bar (no truncation) or its full text is lost (no tooltip / DOM text).
  it("truncates long text to one line, full text in the tooltip", async () => {
    const { renderApp } = await setup();
    renderApp();
    const long = "1200 files · 85 mods · 14 conflicts · 3 skipped · couldn't write plugins.txt: read-only";
    act(() => useUi.getState().showToast("success", "Deployed", { detail: long }));
    const text = within(slot()).getByText(long);
    expect(text.className).toMatch(/\btruncate\b/);
    expect(within(slot()).getByRole("status")).toHaveAttribute("title", `Deployed: ${long}`);
  });

  // Fails when a toast or the job pill changes the bar's height: row 1 gains a second line (the old
  // stack-below rule), the slot changes its classes, or the pill is not exactly the bar's control height.
  it("keeps the bar the same height with no toast, a notice and the job pill, at every width", async () => {
    const { renderApp, t } = await setup();
    renderApp();
    await within(bar()).findByText("Skyrim Special Edition");
    const row = () => bar().firstElementChild as HTMLElement;
    const cell = () => slot().parentElement!;
    const snapshot = () => [row().className, cell().className, row().children.length].join("|");
    const empty = snapshot();
    act(() => useUi.getState().showToast("info", "hello"));
    const withNotice = snapshot();
    const noticePill = within(slot()).getByRole("status").className;
    act(() => t.emit("vessel:job", { id: 3, kind: "deploy", state: "started", error: null }));
    const withJob = snapshot();
    const jobPill = within(slot()).getByRole("status").className;
    expect(withNotice).toBe(empty);
    expect(withJob).toBe(empty);
    // No responsive rule moves the slot out of row 1.
    expect(cell().className).not.toMatch(/row-start|col-span|max-md:|hidden/);
    // The pill is exactly Quit's height (the same h-control class), so the row height cannot change.
    const quit = within(bar()).getByRole("button", { name: "Quit" });
    for (const p of [noticePill, jobPill]) {
      expect(p.split(" ")).toContain("h-control");
      expect(p).not.toMatch(/\bmin-h-/);
    }
    // Narrow: the slot never drops below one control width (a disc-only pill), and is the container the
    // pill's @max-3xs disc-only rule reads.
    expect(row().className).toContain("minmax(var(--spacing-control),1fr)");
    expect(cell().className.split(" ")).toContain("@container");
    expect(quit.className).toMatch(/\bcol-start-6\b/);
  });

  // Fails when the toast pill and Quit stop sharing one height class, or stop being centred on the same row
  // line (their top and bottom edges would no longer match).
  it("aligns the pill with Quit: same height class, both centred in row 1", async () => {
    const { renderApp } = await setup();
    renderApp();
    act(() => useUi.getState().showToast("info", "hello"));
    const row = bar().firstElementChild as HTMLElement;
    const quit = within(bar()).getByRole("button", { name: "Quit" });
    const pill = within(slot()).getByRole("status");
    const height = (el: HTMLElement) => el.className.split(" ").filter((c) => /^(min-|max-)?h-/.test(c));
    expect(height(quit)).toEqual(["h-control"]);
    expect(height(pill)).toEqual(["h-control"]);
    // One row, centred: Quit and the slot cell are both direct grid items of row 1, with no self-alignment.
    expect(row.className.split(" ")).toContain("items-center");
    expect(quit.parentElement).toBe(row);
    const cell = slot().parentElement!;
    expect(cell.parentElement).toBe(row);
    for (const el of [quit, cell]) expect(el.className).not.toMatch(/\bself-|\brow-start|\bmt-|\bmb-/);
    // The cell is exactly the pill's height (no padding around it), so the pill's edges are the cell's.
    expect(cell.className).not.toMatch(/\bp[ytb]?-/);
  });

  // Fails when opening stops using the left-to-right expand, a write re-opens (re-mounts) the pill instead of
  // cycling its text, or the outgoing text is exposed to assistive tech (announced twice).
  it("opens with the expand motion; a write cycles the text in the same pill, old copy aria-hidden", async () => {
    const { renderApp } = await setup();
    renderApp();
    act(() => useUi.getState().showToast("info", "first"));
    const pill = within(slot()).getByRole("status");
    expect(pill.className.split(" ")).toContain("animate-toast-open");
    expect(pill.querySelector("[data-toast-out]")).toBeNull();
    await dwell();
    act(() => useUi.getState().showToast("success", "second"));
    expect(within(slot()).getByRole("status")).toBe(pill);
    expect(pill.className.split(" ")).toContain("animate-toast-open");
    const out = pill.querySelector<HTMLElement>("[data-toast-out]")!;
    expect(out).toHaveAttribute("aria-hidden", "true");
    expect(out).toHaveTextContent("first");
    expect(out.className).toMatch(/\banimate-toast-out\b/);
    expect(within(pill).getByText("second").parentElement!.className).toMatch(/\banimate-toast-in\b/);
    // Rapid writes: each shows in turn after the dwell, and only one outgoing copy exists at a time.
    await dwell();
    act(() => {
      useUi.getState().showToast("info", "third");
      useUi.getState().showToast("info", "fourth");
    });
    expect(liveText(pill)).toContain("third");
    expect(pill.querySelectorAll("[data-toast-out]")).toHaveLength(1);
    expect(pill.querySelector("[data-toast-out]")).toHaveTextContent("second");
    await waitFor(() => expect(liveText(pill)).toContain("fourth"));
    expect(pill.querySelectorAll("[data-toast-out]")).toHaveLength(1);
    expect(pill.querySelector("[data-toast-out]")).toHaveTextContent("third");
  });

  // Fails when closing snaps away instead of collapsing, the closing copy stays exposed to assistive tech,
  // or it never unmounts.
  it("closes with the collapse motion, hidden from assistive tech, then unmounts", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const { renderApp } = await setup();
      renderApp();
      act(() => useUi.getState().showToast("info", "bye"));
      const pill = within(slot()).getByRole("status");
      act(() => useUi.getState().closeToast(useUi.getState().toast!.id));
      expect(pill).toBeInTheDocument();
      expect(pill.className.split(" ")).toContain("animate-toast-close");
      expect(pill).toHaveAttribute("aria-hidden", "true");
      expect(within(slot()).queryByRole("status")).toBeNull();
      act(() => vi.advanceTimersByTime(TOAST_CLOSE_MS));
      expect(pill).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  // Fails when reduced motion still keeps the closing copy around for the collapse (it must go at once).
  it("reduced motion: closes at once, no collapse delay", async () => {
    const mm = window.matchMedia;
    window.matchMedia = ((q: string) => ({ matches: q.includes("reduce"), media: q, addEventListener() {}, removeEventListener() {} })) as unknown as typeof window.matchMedia;
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const { renderApp } = await setup();
      renderApp();
      act(() => useUi.getState().showToast("info", "bye"));
      const pill = within(slot()).getByRole("status");
      act(() => useUi.getState().closeToast(useUi.getState().toast!.id));
      act(() => vi.advanceTimersByTime(0));
      expect(pill).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
      window.matchMedia = mm;
    }
  });

  // Fails when a live toast's progress cycles (motion, outgoing copy) instead of updating in place, or when its
  // return after a transient does not cycle once.
  it("live toast: progress updates in place with no motion; returning after a transient cycles once", async () => {
    const { renderApp, t } = await setup();
    renderApp();
    act(() => t.emit("vessel:job", { id: 8, kind: "deploy", state: "started", error: null }));
    const pill = within(slot()).getByRole("status");
    act(() => useUi.getState().onDeployProgress({ job: 8, done: 1, total: 10 }));
    act(() => useUi.getState().onDeployProgress({ job: 8, done: 2, total: 10 }));
    expect(within(slot()).getByRole("status")).toBe(pill);
    expect(liveText(pill)).toContain("2 / 10 files");
    expect(pill.querySelector("[data-toast-out]")).toBeNull();
    expect(within(pill).getByText("2 / 10 files").parentElement!.className).not.toMatch(/animate-toast-in/);
    await dwell();
    act(() => useUi.getState().showToast("success", "Saved", { detail: "Profile x" }));
    expect(liveText(pill)).toContain("Saved");
    act(() => useUi.getState().closeToast(useUi.getState().toast!.id));
    expect(liveText(pill)).toContain("2 / 10 files");
    expect(pill.querySelector("[data-toast-out]")).toHaveTextContent("Saved");
  });
});

describe("controller mode (html[data-pad])", () => {
  afterEach(() => setPad(false));
  const tabStops = (list: HTMLElement) => focusables(document.body).filter((el) => list.contains(el));

  // Fails when the view bar stays a D-pad stop in controller mode, or LB/RB leave focus on a tab / nowhere.
  it("the view tabs leave focus and spatial nav; LB/RB put focus in the new view; B does nothing", async () => {
    const { renderApp } = await setup();
    renderApp();
    await heading("Games");
    expect(document.activeElement).toHaveAttribute("data-tab", "games");
    act(() => setPad(true));
    expect(tabStops(viewTabs())).toEqual([]);
    await waitFor(() => within(viewTabs()).getAllByRole("tab").forEach((t) => expect(t.tabIndex).toBe(-1)));
    act(() => performNav({ type: "tab", delta: -1 }));
    await heading("Settings");
    await waitFor(() => expect(screen.getByRole("main")).toContainElement(document.activeElement as HTMLElement));
    expect(document.activeElement).not.toHaveAttribute("role", "tab");
    // B without a dialog does nothing: focus and view stay.
    const before = document.activeElement;
    act(() => performNav({ type: "close" }));
    expect(document.activeElement).toBe(before);
    expect(useUi.getState().view).toBe("settings");
  });

  // Fails when the Mods sub-tabs (controllerFocusable) drop out with the view bar in controller mode.
  it("the Mods sub-tabs stay a roving stop", async () => {
    const { renderApp } = await setup();
    renderApp();
    act(() => useUi.getState().setView("mods"));
    const list = await screen.findByRole("tablist", { name: "Mods sections" });
    act(() => setPad(true));
    await act(async () => {});
    expect(tabStops(list)).toHaveLength(4);
    expect(within(list).getAllByRole("tab").filter((t) => t.tabIndex === 0)).toHaveLength(1);
  });
});
