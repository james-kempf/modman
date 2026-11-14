// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { focusables } from "../../nav/spatial";
import { BulkBar } from "./BulkBar";

const handlers = () => ({
  onEnable: vi.fn(),
  onDisable: vi.fn(),
  onUninstall: vi.fn(),
  onClear: vi.fn(),
});
const NAMES = ["Enable", "Disable", "Uninstall", "Clear"] as const;

describe("BulkBar", () => {
  // Fails when an empty selection still shows the bar (and its focus stops).
  it("renders nothing at 0 selected", () => {
    const { container } = render(<BulkBar count={0} {...handlers()} />);
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByRole("region")).toBeNull();
  });

  // Fails when the bar is not a labelled region, loses the count, or stops being sticky.
  it("is a labelled sticky region with the count", () => {
    render(<BulkBar count={3} {...handlers()} />);
    const bar = screen.getByRole("region", { name: "Selected mods" });
    expect(bar).toHaveTextContent("3 selected");
    expect(bar.className).toMatch(/\bsticky\b/);
    expect(bar.className).not.toMatch(/\bring-/);
  });

  // Fails when a button is wired to the wrong callback.
  it("each action calls its own callback", async () => {
    const h = handlers();
    const user = userEvent.setup();
    render(<BulkBar count={2} {...h} />);
    for (const name of NAMES) await user.click(screen.getByRole("button", { name }));
    expect(h.onEnable).toHaveBeenCalledTimes(1);
    expect(h.onDisable).toHaveBeenCalledTimes(1);
    expect(h.onUninstall).toHaveBeenCalledTimes(1);
    expect(h.onClear).toHaveBeenCalledTimes(1);
  });

  // Fails when Uninstall is not the danger tone (it removes mods) or Clear is not ghost.
  it("Uninstall is danger, Clear is ghost", () => {
    render(<BulkBar count={1} {...handlers()} />);
    expect(screen.getByRole("button", { name: "Uninstall" }).className).toMatch(/rose/);
    expect(screen.getByRole("button", { name: "Clear" }).className).toMatch(/text-zinc-300/);
  });

  // Fails when a control is not a keyboard/gamepad stop (Tab order or spatial nav).
  it("every action is reachable by Tab and spatial nav", async () => {
    const user = userEvent.setup();
    render(<BulkBar count={2} {...handlers()} />);
    const nav = focusables(document.body);
    for (const name of NAMES) {
      await user.tab();
      const b = screen.getByRole("button", { name });
      expect(b).toHaveFocus();
      expect(nav).toContain(b);
    }
  });

  // Fails when a running action uses native disabled (focus drops to <body>) or still fires callbacks.
  it("while busy: the running action spins, all stay focusable and ignore presses", async () => {
    const h = handlers();
    const user = userEvent.setup();
    render(<BulkBar count={2} busy="disable" {...h} />);
    const dis = screen.getByRole("button", { name: "Disable" });
    expect(dis).toHaveAttribute("aria-busy", "true");
    dis.focus();
    for (const name of NAMES) {
      const b = screen.getByRole("button", { name });
      expect(b).not.toBeDisabled();
      expect(b).toHaveAttribute("aria-disabled", "true");
      await user.click(b);
    }
    expect(screen.getByRole("button", { name: "Enable" })).not.toHaveAttribute("aria-busy");
    for (const fn of Object.values(h)) expect(fn).not.toHaveBeenCalled();
  });
});
