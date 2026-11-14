// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Notice } from "./Notice";

describe("Notice action (T03)", () => {
  // Fails when the action steals focus on appear, is not a Tab stop, or Enter does not run it.
  it("is a keyboard stop that does not take focus when shown", async () => {
    const onAction = vi.fn();
    render(
      <>
        <button type="button">Before</button>
        <Notice tone="success" act={{ label: "Undo", onAction }}>
          Uninstalled SkyUI
        </Notice>
      </>,
    );
    const undo = screen.getByRole("button", { name: "Undo" });
    expect(undo).not.toHaveFocus();
    expect(undo).not.toBeDisabled();
    const user = userEvent.setup();
    await user.tab();
    await user.tab();
    expect(undo).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(onAction).toHaveBeenCalledTimes(1);
  });

  // Fails when focus moving within the card reports a release, or leaving it does not (the timer would never restart).
  it("reports focus entering and leaving the card once", async () => {
    const onHold = vi.fn();
    render(
      <>
        <Notice tone="info" act={{ label: "Undo", onAction: () => {} }} onHold={onHold}>
          Removed
        </Notice>
        <button type="button">After</button>
      </>,
    );
    const user = userEvent.setup();
    await user.tab();
    expect(onHold.mock.calls).toEqual([[true]]);
    await user.tab();
    expect(onHold.mock.calls).toEqual([[true], [false]]);
  });
});
