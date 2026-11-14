// SPDX-License-Identifier: GPL-3.0-or-later
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { focusables } from "../../nav/spatial";
import { MoveToPosition, positionError } from "./MoveToPosition";

const mount = (props: Partial<Parameters<typeof MoveToPosition>[0]> = {}) => {
  const onMove = vi.fn();
  render(<MoveToPosition name="SkyUI" position={3} total={10} onMove={onMove} {...props} />);
  return { onMove, user: userEvent.setup(), field: () => screen.getByRole("spinbutton", { name: "Position" }) };
};

describe("positionError", () => {
  // Fails when out-of-range, fractional, signed, exponent or junk input passes as a position.
  it("accepts whole numbers 1..N only", () => {
    const msg = "Enter a position from 1 to 10.";
    for (const ok of ["1", "10", "7", " 4 "]) expect(positionError(ok, 10)).toBeNull();
    for (const bad of ["0", "11", "-1", "2.5", "1e1", "+3", "abc", "", " "]) expect(positionError(bad, 10)).toBe(msg);
  });
});

describe("MoveToPosition", () => {
  // Fails when top/bottom emit the wrong spec for modMove.
  it("Move to top emits first, Move to bottom emits last", async () => {
    const { onMove, user } = mount();
    await user.click(screen.getByRole("button", { name: "Move SkyUI to top" }));
    await user.click(screen.getByRole("button", { name: "Move SkyUI to bottom" }));
    expect(onMove.mock.calls).toEqual([["first"], ["last"]]);
  });

  // Fails when a valid position does not emit #N, or the field keeps its value after a move.
  it("submitting a valid position emits #N and clears the field", async () => {
    const { onMove, user, field } = mount();
    await user.type(field(), "7{Enter}");
    expect(onMove).toHaveBeenCalledWith("#7");
    expect(field()).toHaveValue(null);
  });

  // Fails when the error shows while still typing the first time, or does not follow typing after the first blur.
  it("shows the inline error on blur, then live", async () => {
    const { user, field } = mount();
    await user.type(field(), "12");
    expect(field()).not.toHaveAttribute("aria-invalid");
    fireEvent.blur(field());
    expect(field()).toHaveAttribute("aria-invalid", "true");
    expect(field()).toHaveAccessibleDescription(/Enter a position from 1 to 10\./);
    await user.clear(field());
    await user.type(field(), "9");
    expect(field()).not.toHaveAttribute("aria-invalid");
    await user.type(field(), "9");
    expect(field()).toHaveAttribute("aria-invalid", "true");
  });

  // Fails when an invalid submit still moves the mod or shows no error.
  it("rejects out-of-range and fractional input on submit", async () => {
    const { onMove, user, field } = mount();
    for (const bad of ["11", "0", "2.5"]) {
      await user.clear(field());
      await user.type(field(), `${bad}{Enter}`);
      expect(field()).toHaveAttribute("aria-invalid", "true");
    }
    expect(onMove).not.toHaveBeenCalled();
  });

  // Fails when the range helper is missing (the user would not know N).
  it("describes the range", () => {
    const { field } = mount();
    expect(field()).toHaveAccessibleDescription("1–10");
  });

  // Fails when the ends use native disabled (focus to <body>) or still emit a no-op move.
  it("top is inactive at the top, bottom at the bottom; they stay focusable", async () => {
    const { onMove, user } = mount({ position: 1, total: 1 });
    for (const name of ["Move SkyUI to top", "Move SkyUI to bottom"]) {
      const b = screen.getByRole("button", { name });
      expect(b).not.toBeDisabled();
      expect(b).toHaveAttribute("aria-disabled", "true");
      await user.click(b);
    }
    expect(onMove).not.toHaveBeenCalled();
  });

  // Fails when Move fires with an empty field, or busy uses native disabled.
  it("Move is inactive when empty; busy blocks every control without disabling", async () => {
    const { onMove, user } = mount({ busy: true });
    for (const b of screen.getAllByRole("button")) {
      expect(b).not.toBeDisabled();
      expect(b).toHaveAttribute("aria-disabled", "true");
      await user.click(b);
    }
    expect(onMove).not.toHaveBeenCalled();
  });

  // Fails when a control is not a keyboard/gamepad stop.
  it("every control is reachable by Tab and spatial nav", async () => {
    const { user, field } = mount();
    const nav = focusables(document.body);
    const order = [
      screen.getByRole("button", { name: "Move SkyUI to top" }),
      screen.getByRole("button", { name: "Move SkyUI to bottom" }),
      field(),
      screen.getByRole("button", { name: "Move" }),
    ];
    for (const el of order) {
      await user.tab();
      expect(el).toHaveFocus();
      expect(nav).toContain(el);
    }
  });
});
