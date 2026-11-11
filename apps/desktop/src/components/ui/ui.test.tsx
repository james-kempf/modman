// SPDX-License-Identifier: GPL-3.0-or-later
// The shared Card: header placement and that the default look is unchanged.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Button, Card, CardSection, Row, SECTION_PAD, Segmented, Toggle } from ".";

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

describe("Toggle focus highlight padding", () => {
  // Fails when the nav highlight hugs the switch sideways: the button is min-h-control (48px) around an
  // h-7 (28px) track, i.e. 10px above and below, so it needs px-2.5 (10px) left and right too.
  it.each([true, false])("pads the track as much sideways as vertically (showLabel=%s)", (showLabel) => {
    render(<Toggle label="Deploy" checked={false} onChange={() => {}} showLabel={showLabel} />);
    const cls = screen.getByRole("switch", { name: "Deploy" }).className;
    expect(cls).toMatch(/\bmin-h-control\b/);
    expect(cls).toMatch(/\bpx-2\.5\b/);
    expect(cls).not.toMatch(/\bpx-(?!2\.5\b)/);
  });
});

describe("Button nav focus", () => {
  const cls = (name: string) => screen.getByRole("button", { name }).className;

  // Fails when a coloured button loses its tone on the shared white focus fill (text must carry it, >= 4.5:1).
  it.each([
    ["primary", "[--focus-text:var(--color-accent-700)]"],
    ["danger", "[--focus-text:var(--color-rose-700)]"],
  ] as const)("%s carries its tone in the focus text colour", (tone, text) => {
    render(<Button tone={tone}>Go</Button>);
    expect(cls("Go")).toContain(text);
  });

  // Fails when a neutral button overrides the shared dark focus ink.
  it.each(["default", "ghost"] as const)("%s uses the shared focus ink", (tone) => {
    render(<Button tone={tone}>Go</Button>);
    expect(cls("Go")).not.toMatch(/(^|\s)\[--focus-text/);
  });

  // Fails when the selected segment falls back to a tint that barely differs from its track (the old accent-400/15
  // is 1.3:1), drifts from the Overview Play button's green (Button tone="primary"), or loses the rim that keeps
  // "selected + highlighted" apart from "highlighted" under the white fill. accent-400 fill vs ink track 10.3:1,
  // accent-950 text on it 8.5:1; the rim is accent-400 at rest (invisible) and accent-700 under the highlight
  // (5.2:1; accent-400 there would be 1.7:1).
  it("a selected segment is a solid Play-green thumb (>= 3:1 vs track) with a darker rim under the highlight", () => {
    render(
      <>
        <Button tone="primary">Play</Button>
        <Segmented label="Show" value="a" options={[{ value: "a", label: "A" }, { value: "b", label: "B" }]} onChange={() => {}} />
      </>,
    );
    const on = screen.getByRole("button", { name: "A" });
    expect(on).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "B" })).toHaveAttribute("aria-pressed", "false");
    const play = cls("Play").split(" ");
    for (const c of ["bg-accent-400", "text-accent-950", "[--focus-text:var(--color-accent-700)]"]) {
      expect(play).toContain(c);
      expect(on.className.split(" ")).toContain(`aria-pressed:${c}`);
    }
    expect(on.className.split(" ")).toEqual(
      expect.arrayContaining([
        "rounded-pill",
        "aria-pressed:outline-2",
        "aria-pressed:-outline-offset-2",
        "aria-pressed:outline-accent-400",
        "aria-pressed:highlighted:outline-accent-700",
      ]),
    );
    // A translucent fill (bg-accent-400/15, bg-fill-3) is below 3:1 on the ink track; ring-* would replace the highlight shadow.
    expect(on.className).not.toMatch(/aria-pressed:bg-[\w-]+\/\d+|aria-pressed:bg-fill-|\bring-/);
    // Concentric: pill segments in a pill track with a 4px gap.
    expect(on.parentElement?.className.split(" ")).toEqual(expect.arrayContaining(["rounded-pill", "p-1", "gap-1"]));
  });

  // Fails when a plain ghost button picks up the segment's selected look.
  it("keeps the ghost tone free of aria-pressed styles", () => {
    render(<Button tone="ghost">Go</Button>);
    expect(cls("Go")).not.toMatch(/aria-pressed:/);
  });

  // Fails when danger goes back to a saturated or ring-bordered fill at rest.
  it("keeps danger tonal at rest without a coloured border", () => {
    render(<Button tone="danger">Remove</Button>);
    expect(cls("Remove")).toMatch(/\bbg-rose-500\/15\b/);
    expect(cls("Remove")).not.toMatch(/\bborder-rose-/);
  });

  // Fails when the toggle track keeps its at-rest colours on the white focus fill (the off track is a 10% white, invisible).
  it.each([
    [true, "in-highlight:bg-accent-700"],
    [false, "in-highlight:bg-zinc-500"],
  ] as const)("toggle track stays >= 3:1 on the white focus fill (checked=%s)", (checked, track) => {
    render(<Toggle label="T" checked={checked} onChange={() => {}} />);
    expect(screen.getByRole("switch").innerHTML).toContain(track);
  });
});
