// SPDX-License-Identifier: GPL-3.0-or-later
// The shared Card: header placement and that the default look is unchanged.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Card, CardSection, Row, SECTION_PAD } from ".";

const surface = (title: string) => screen.getByRole("heading", { name: title }).closest("section")!.lastElementChild!;

describe("Card", () => {
  // Fails when the default header moves into the surface (changes every card's look).
  it("puts the header above the surface by default", () => {
    render(
      <Card title="Above">
        <CardSection>body</CardSection>
      </Card>,
    );
    expect(surface("Above").contains(screen.getByRole("heading", { name: "Above" }))).toBe(false);
  });

  // Fails when `inset` is ignored: side-by-side heroes (one untitled) lose aligned tops.
  it("puts the header inside the surface, before the body, with inset", () => {
    render(
      <Card title="Inside" inset>
        <CardSection>body</CardSection>
      </Card>,
    );
    const s = screen.getByRole("heading", { name: "Inside" }).closest("section")!;
    expect(s.children).toHaveLength(1);
    const box = s.firstElementChild!;
    expect(box.className).toMatch(/\brounded-card\b/);
    expect(box.firstElementChild!.contains(screen.getByRole("heading", { name: "Inside" }))).toBe(true);
    expect(box.lastElementChild!.textContent).toBe("body");
  });

  // Fails when the inset header is a divided section again: a line would sit between title and content.
  it("keeps the inset header out of the dividers, which still split later sections", () => {
    render(
      <Card title="Plain" inset>
        <CardSection>one</CardSection>
        <CardSection>two</CardSection>
      </Card>,
    );
    const heading = screen.getByRole("heading", { name: "Plain" });
    expect(heading.closest(".divide-y")).toBeNull();
    const divided = screen.getByText("one").parentElement!;
    expect(divided.className).toMatch(/divide-y/);
    expect(divided).toBe(screen.getByText("two").parentElement);
  });
  // Fails when inset titles lose the mono eyebrow look, or when it leaks onto default (non-inset) titles.
  it("styles only inset titles as the mono eyebrow", () => {
    render(
      <>
        <Card title="Boxed" inset>
          <CardSection>a</CardSection>
        </Card>
        <Card title="Open">
          <CardSection>b</CardSection>
        </Card>
      </>,
    );
    const boxed = screen.getByRole("heading", { level: 2, name: "Boxed" });
    expect(boxed.className).toMatch(/\beyebrow\b/);
    const open = screen.getByRole("heading", { level: 2, name: "Open" });
    expect(open.className).not.toMatch(/\beyebrow\b|font-mono/);
    expect(open.className).toMatch(/\btext-title\b/);
  });

  // Fails when a section drifts back to uneven padding (e.g. px-4 py-3) or Row/sections/inset header disagree.
  it("pads every section evenly from one shared value", () => {
    expect(SECTION_PAD).toBe("p-section");
    render(
      <Card title="Even" inset>
        <CardSection>sec</CardSection>
        <Row title="row" />
      </Card>,
    );
    const pad = (el: Element) => el.className.split(/\s+/).filter((c) => /^!?[p][xytblr]?-/.test(c));
    expect(pad(screen.getByText("sec"))).toEqual(["p-section"]);
    expect(pad(screen.getByText("row").closest(".divide-y > *")!)).toEqual(["p-section"]);
    // Inset header: same top/side padding, no bottom (the first section's top padding is the gap).
    expect(pad(screen.getByRole("heading", { name: "Even" }).parentElement!.parentElement!).sort()).toEqual(["pt-section", "px-section"]);
  });
});
