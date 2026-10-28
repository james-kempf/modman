// SPDX-License-Identifier: GPL-3.0-or-later
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useUi } from "../app/store";
import { autoupdateStatus } from "../mock/fixtures";
import { performNav } from "../nav/actions";
import { BUTTON, GamepadNav } from "../nav/gamepad";
import { setup } from "../test/harness";

const login = () => ({ user: "deck", premium: false, backend: "keyring", file: null, keyring_unavailable: null });

/** A mocked pad driven button by button through GamepadNav -> performNav. */
function gamepad() {
  const buttons = Array.from({ length: 17 }, () => ({ pressed: false }));
  const pads = [{ connected: true, buttons, axes: [0, 0] }];
  Object.defineProperty(navigator, "getGamepads", { configurable: true, value: () => pads });
  const nav = new GamepadNav();
  let now = 0;
  const step = () => act(() => nav.poll((now += 16)).forEach((a) => performNav(a)));
  return {
    press(b: number) {
      buttons[b].pressed = true;
      step();
      buttons[b].pressed = false;
      step();
    },
    /** D-pad down until `match` has focus (bounded). */
    downUntil(match: (el: Element | null) => boolean) {
      for (let i = 0; i < 60 && !match(document.activeElement); i++) this.press(BUTTON.DOWN);
      expect(match(document.activeElement)).toBe(true);
    },
  };
}

afterEach(() => {
  delete (navigator as { getGamepads?: unknown }).getGamepads;
});

describe("API key entry", () => {
  it("is reachable and submittable with the gamepad alone via the on-screen keyboard", async () => {
    const { renderApp, t } = await setup({ login });
    renderApp();
    const pad = gamepad();
    // Overview -> Settings is one LB press (tabs wrap).
    pad.press(BUTTON.LB);
    await screen.findByRole("heading", { level: 1, name: "Settings" });
    const field = await screen.findByLabelText("Nexus API key");
    pad.downUntil((el) => el === field);
    pad.press(BUTTON.A);
    await screen.findByRole("dialog", { name: /Keyboard: Nexus API key/ });
    // First key ("1") has focus: type "1", then move right and type "2".
    pad.press(BUTTON.A);
    pad.press(BUTTON.RIGHT);
    pad.press(BUTTON.A);
    pad.downUntil((el) => el?.textContent === "Done");
    pad.press(BUTTON.A);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(field).toHaveValue("12");
    // Restored after the dialog is gone and the app root is no longer inert.
    await waitFor(() => expect(document.activeElement).toBe(field));
    pad.downUntil((el) => el?.textContent === "Sign in");
    pad.press(BUTTON.A);
    await waitFor(() => expect(t.callsOf("login")).toEqual([{ api_key: "12" }]));
    // Cleared from state right after submit.
    expect(field).toHaveValue("");
  });

  it("B closes the on-screen keyboard without typing", async () => {
    const { renderApp } = await setup();
    renderApp();
    act(() => useUi.getState().setView("settings"));
    const field = (await screen.findByLabelText("Nexus API key")) as HTMLInputElement;
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Keyboard" }));
    await screen.findByRole("dialog");
    await user.click(screen.getByRole("button", { name: "q" }));
    const pad = gamepad();
    pad.press(BUTTON.B);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(field.value).toBe("");
  });

  it("Paste fills the key from the clipboard; submit sends it and clears the field", async () => {
    const { renderApp, t } = await setup({ login });
    // After userEvent.setup(), which installs its own clipboard stub.
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { readText: vi.fn(async () => "  PASTED-KEY==\n") },
    });
    renderApp();
    act(() => useUi.getState().setView("settings"));
    const field = await screen.findByLabelText("Nexus API key");
    await user.click(screen.getByRole("button", { name: "Paste" }));
    await waitFor(() => expect(field).toHaveValue("PASTED-KEY=="));
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    await waitFor(() => expect(t.callsOf("login")).toEqual([{ api_key: "PASTED-KEY==" }]));
    expect(field).toHaveValue("");
    expect(document.body.innerHTML).not.toContain("PASTED-KEY");
  });

  it("an unreadable clipboard falls back to a hint and focuses the field", async () => {
    const { renderApp } = await setup();
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { readText: vi.fn(async () => Promise.reject(new Error("denied"))) },
    });
    renderApp();
    act(() => useUi.getState().setView("settings"));
    const field = await screen.findByLabelText("Nexus API key");
    await user.click(screen.getByRole("button", { name: "Paste" }));
    await screen.findByText(/Clipboard not readable/);
    expect(document.activeElement).toBe(field);
  });
});

describe("on-screen keyboard (T16 F1/F2b)", () => {
  // Fails when Done focuses the input while the App root is still inert (focus lands on <body>).
  it("Done never focuses the input while the app root is still inert", async () => {
    const { renderApp } = await setup();
    renderApp();
    act(() => useUi.getState().setView("settings"));
    const field = (await screen.findByLabelText("Nexus API key")) as HTMLInputElement;
    act(() => useUi.getState().openOsk({ input: field, label: "Nexus API key" }));
    await screen.findByRole("dialog");
    const calls: boolean[] = [];
    const orig = field.focus.bind(field);
    vi.spyOn(field, "focus").mockImplementation((o) => {
      calls.push(!!field.closest("[inert]"));
      orig(o);
    });
    act(() => screen.getByRole("button", { name: "Done" }).click());
    await waitFor(() => expect(document.activeElement).toBe(field));
    expect(calls).not.toContain(true);
  });

  // Fails when the keyboard can only fill the field: the gamepad user then has to find Sign in.
  it("the keyboard's Sign in button fills the key and submits the form", async () => {
    const { renderApp, t } = await setup({ login });
    renderApp();
    act(() => useUi.getState().setView("settings"));
    const field = (await screen.findByLabelText("Nexus API key")) as HTMLInputElement;
    const pad = gamepad();
    act(() => field.focus());
    pad.press(BUTTON.A);
    await screen.findByRole("dialog", { name: /Keyboard: Nexus API key/ });
    pad.press(BUTTON.A); // "1"
    pad.press(BUTTON.RIGHT);
    pad.press(BUTTON.A); // "2"
    const dlg = screen.getByRole("dialog");
    act(() => within(dlg).getByRole("button", { name: "Sign in" }).click());
    await waitFor(() => expect(t.callsOf("login")).toEqual([{ api_key: "12" }]));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("nothing typed: Sign in is disabled and Done does not submit", async () => {
    const { renderApp, t } = await setup({ login });
    renderApp();
    act(() => useUi.getState().setView("settings"));
    const field = (await screen.findByLabelText("Nexus API key")) as HTMLInputElement;
    act(() => useUi.getState().openOsk({ input: field, label: "Nexus API key" }));
    const dlg = await screen.findByRole("dialog");
    expect(within(dlg).getByRole("button", { name: "Sign in" })).toBeDisabled();
    act(() => screen.getByRole("button", { name: "Done" }).click());
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(t.callsOf("login")).toEqual([]);
  });
});

describe("About card (T16 F4)", () => {
  // Fails when the <dl> holds bare <div>s (axe definition-list violation).
  it("uses dt/dd pairs inside its <dl>", async () => {
    const { renderApp } = await setup();
    renderApp();
    act(() => useUi.getState().setView("settings"));
    const dt = await screen.findByText("database");
    const dl = dt.closest("dl")!;
    expect(dt.tagName).toBe("DT");
    const groups = dl.querySelectorAll(":scope > div");
    expect(groups).toHaveLength(5);
    for (const g of groups) {
      expect(g.querySelectorAll(":scope > dt")).toHaveLength(1);
      expect(g.querySelectorAll(":scope > dd")).toHaveLength(1);
    }
  });
});

describe("auto-update", () => {
  it("toggle enables the timer and the last-run log is listed", async () => {
    const { renderApp, t } = await setup({
      autoupdate_status: () =>
        autoupdateStatus({
          runs: [
            {
              id: 1,
              game: "skyrimspecialedition",
              started_at: 1_760_000_000,
              finished_at: 1_760_000_100,
              forced: false,
              dry_run: false,
              outcome: "acted",
              reasons: ["steam build changed"],
              actions: ["updated 2 mods", "redeployed"],
              summary: null,
            },
          ],
        }),
      autoupdate_enable: () => autoupdateStatus(),
    });
    renderApp();
    act(() => useUi.getState().setView("settings"));
    // Collapsed behind a count; the latest outcome shows without expanding.
    const more = await screen.findByRole("button", { name: "Auto-update runs list (1)" });
    expect(screen.queryByRole("list", { name: "Auto-update runs" })).toBeNull();
    expect(screen.getByText(/^Latest .*: acted$/)).toBeInTheDocument();
    await userEvent.setup().click(more);
    const runs = await screen.findByRole("list", { name: "Auto-update runs" });
    expect(runs).toHaveTextContent("acted");
    expect(runs).toHaveTextContent("steam build changed");
    const toggle = screen.getByRole("switch", { name: "Auto-update off" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await userEvent.setup().click(toggle);
    await waitFor(() => expect(t.callsOf("autoupdate_enable")).toHaveLength(1));
  });
});

describe("Steam shortcut", () => {
  const status = (o = {}) => ({ accounts: 1, added: 0, steam_running: false, exe: "/x/modman.AppImage", ...o });

  // Fails when the toggle does not add the shortcut, or works while Steam runs.
  it("adds the shortcut from the toggle", async () => {
    let added = 0;
    const { renderApp, t } = await setup({
      steam_shortcut_status: () => status({ added }),
      steam_shortcut_set: () => status({ added: (added = 1) }),
    });
    renderApp();
    act(() => useUi.getState().setView("settings"));
    const toggle = screen.getByRole("switch", { name: "Steam shortcut off" });
    await waitFor(() => expect(toggle).toBeEnabled());
    // The per-account count and executable path are no longer shown under the toggle.
    expect(screen.queryByText(/have it$/)).toBeNull();
    expect(screen.queryByText("/x/modman.AppImage")).toBeNull();
    await userEvent.setup().click(toggle);
    await waitFor(() => expect(t.callsOf("steam_shortcut_set")).toHaveLength(1));
    await screen.findByRole("switch", { name: "Steam shortcut on" });
  });

  // Fails when a running Steam blocks the toggle or the restart note is missing.
  it("stays usable while Steam is running and says to restart it", async () => {
    const { renderApp } = await setup({ steam_shortcut_status: () => status({ steam_running: true }) });
    renderApp();
    act(() => useUi.getState().setView("settings"));
    expect(await screen.findByText(/restart Steam/)).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Steam shortcut off" })).toBeEnabled();
  });
});
