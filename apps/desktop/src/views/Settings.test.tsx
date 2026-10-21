// SPDX-License-Identifier: GPL-3.0-or-later
import { act, screen, waitFor } from "@testing-library/react";
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
    expect(document.activeElement).toBe(field);
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
    const runs = await screen.findByRole("list", { name: "Auto-update runs" });
    expect(runs).toHaveTextContent("acted");
    expect(runs).toHaveTextContent("steam build changed");
    const toggle = screen.getByRole("switch", { name: "Auto-update off" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await userEvent.setup().click(toggle);
    await waitFor(() => expect(t.callsOf("autoupdate_enable")).toHaveLength(1));
  });
});
