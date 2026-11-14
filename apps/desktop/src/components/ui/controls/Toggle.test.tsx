// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Toggle } from "./Toggle";

describe("Toggle busy (T03)", () => {
  // Fails when busy shows no spinner, uses native disabled (drops focus), or still applies a click.
  it("shows a spinner, stays focusable and ignores clicks", async () => {
    const onChange = vi.fn();
    render(<Toggle label="Enable SkyUI" checked={false} onChange={onChange} busy />);
    const sw = screen.getByRole("switch", { name: "Enable SkyUI" });
    expect(screen.getByTestId("toggle-spinner")).toBeInTheDocument();
    expect(sw).toHaveAttribute("aria-disabled", "true");
    expect(sw).toHaveAttribute("aria-busy", "true");
    expect(sw).not.toBeDisabled();
    sw.focus();
    expect(sw).toHaveFocus();
    await userEvent.setup().click(sw);
    expect(onChange).not.toHaveBeenCalled();
  });

  // Fails when the spinner spins unconditionally (ignores prefers-reduced-motion).
  it("spins only under motion-safe", () => {
    render(<Toggle label="T" checked onChange={() => {}} busy />);
    const cls = screen.getByTestId("toggle-spinner").getAttribute("class") ?? "";
    expect(cls).toMatch(/\bmotion-safe:animate-spin\b/);
    expect(cls).not.toMatch(/(^|\s)animate-spin\b/);
  });

  // Fails when the spinner shows on an idle toggle.
  it("has no spinner when idle", () => {
    render(<Toggle label="T" checked={false} onChange={() => {}} />);
    expect(screen.queryByTestId("toggle-spinner")).toBeNull();
  });
});
