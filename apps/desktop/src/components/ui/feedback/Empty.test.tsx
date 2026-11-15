// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Empty, TILE } from "..";

const classesIn = (root: Element) => [root, ...root.querySelectorAll("*")].map((e) => e.getAttribute("class") ?? "").join(" ");

describe("Empty", () => {
  // Fails when a dashed or dotted outline comes back on either variant.
  it("has no dashed or dotted outline", () => {
    const { container } = render(
      <>
        <Empty>Nothing here.</Empty>
        <Empty icon={<svg />} title="No mods" action={<button type="button">Go</button>}>
          Install one.
        </Empty>
      </>,
    );
    expect(classesIn(container)).not.toMatch(/\b(border-dashed|border-dotted|outline-dashed|outline-dotted)\b/);
  });

  // Fails when the one-line variant stops being a quiet, centred well from the shared TILE surface.
  it("renders a message alone as one centred line in a sunken well", () => {
    render(<Empty>Nothing here.</Empty>);
    const p = screen.getByText("Nothing here.");
    expect(p.tagName).toBe("P");
    for (const c of TILE.split(" ")) expect(p).toHaveClass(c);
    expect(p).toHaveClass("text-center", "text-zinc-400");
  });

  // Fails when the titled variant loses its centred icon -> title -> message -> action order or the decorative icon tile.
  it("stacks a muted icon tile, title, message and action, centred", () => {
    const { container } = render(
      <Empty icon={<svg data-testid="icon" />} title="No mods" action={<button type="button">Go</button>}>
        Install one.
      </Empty>,
    );
    const title = screen.getByRole("heading", { level: 2, name: "No mods" });
    const root = container.firstElementChild!;
    expect(root).toHaveClass("text-center");
    const tile = screen.getByTestId("icon").parentElement!;
    expect(tile).toHaveAttribute("aria-hidden");
    expect(tile).toHaveClass("rounded-card", "text-dim");
    const order = [tile, title, screen.getByText("Install one."), screen.getByRole("button", { name: "Go" })];
    for (let i = 1; i < order.length; i++)
      expect(order[i - 1].compareDocumentPosition(order[i]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});
