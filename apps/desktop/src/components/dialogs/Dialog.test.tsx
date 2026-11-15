// SPDX-License-Identifier: GPL-3.0-or-later
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StrictMode, useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Dialog } from "./Dialog";
import { Button } from "../ui";

describe("Dialog focus restore (T16 F1)", () => {
  // Fails when the opener is recorded in an effect: StrictMode's re-run sees the dialog's own button.
  it("returns focus to its opener under StrictMode with an inert background", async () => {
    function Host() {
      const [open, setOpen] = useState(false);
      return (
        <>
          {/* Like App: the background is inert while a dialog is open. */}
          <div inert={open}>
            <button onClick={() => setOpen(true)}>opener</button>
          </div>
          {open && (
            <Dialog title="t" onBack={() => setOpen(false)}>
              <button data-autofocus onClick={() => setOpen(false)}>
                close
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
    // jsdom ignores `inert`; emulate it: the opener cannot take focus while the dialog is open.
    const inertFocus = vi.spyOn(opener, "focus").mockImplementation(() => {});
    act(() => opener.click());
    expect(document.activeElement).toBe(screen.getByText("close"));
    inertFocus.mockRestore();
    act(() => screen.getByText("close").click());
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });

  it("returns focus to `returnFocus` when given", async () => {
    function Host() {
      const [open, setOpen] = useState(false);
      const [target, setTarget] = useState<HTMLInputElement | null>(null);
      return (
        <>
          <input aria-label="field" ref={setTarget} />
          <button onClick={() => setOpen(true)}>opener</button>
          {open && (
            <Dialog title="t" onBack={() => setOpen(false)} returnFocus={target}>
              <button onClick={() => setOpen(false)}>close</button>
            </Dialog>
          )}
        </>
      );
    }
    render(<Host />);
    screen.getByText("opener").focus();
    act(() => screen.getByText("opener").click());
    act(() => screen.getByText("close").click());
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText("field")));
  });
});

describe("busy Button (T16 F1)", () => {
  // Fails when busy sets native `disabled`: the browser drops focus to <body>.
  it("stays focusable and does not fire onClick while busy", async () => {
    const onClick = vi.fn();
    render(
      <Button busy onClick={onClick}>
        Purge
      </Button>,
    );
    const b = screen.getByRole("button", { name: /Purge/ });
    expect(b).not.toBeDisabled();
    expect(b).toHaveAttribute("aria-disabled", "true");
    expect(b).toHaveAttribute("aria-busy", "true");
    await userEvent.setup().click(b);
    expect(onClick).not.toHaveBeenCalled();
  });

  // Fails when a busy submit button still submits its form.
  it("does not submit its form while busy", async () => {
    const onSubmit = vi.fn((e: { preventDefault(): void }) => e.preventDefault());
    render(
      <form onSubmit={onSubmit}>
        <Button type="submit" busy>
          Go
        </Button>
      </form>,
    );
    await userEvent.setup().click(screen.getByRole("button", { name: /Go/ }));
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
