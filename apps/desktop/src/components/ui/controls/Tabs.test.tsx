// SPDX-License-Identifier: GPL-3.0-or-later
// Tabs: one shared look (view bar and dialogs), slots, roving tabindex (APG manual activation), dialog LB/RB scoping.
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { PadGlyph, Tabs, type TabItem } from "..";
import { TAB_EVENT } from "../../../nav/actions";
import { focusables } from "../../../nav/spatial";

type V = "a" | "b" | "c";
const TABS: readonly TabItem<V>[] = [
  { value: "a", label: "Alpha", icon: <svg data-testid="icon-a" aria-hidden /> },
  { value: "b", label: "Beta", badge: <span data-testid="badge-b">3</span> },
  { value: "c", label: "Gamma" },
];

function Harness(props: { focusable?: boolean; panel?: boolean; start?: string; end?: string }) {
  const [v, setV] = useState<V>("a");
  return (
    <Tabs
      label="Views"
      tabs={TABS}
      value={v}
      onChange={setV}
      focusable={props.focusable}
      start={props.start && <PadGlyph>{props.start}</PadGlyph>}
      end={props.end && <PadGlyph>{props.end}</PadGlyph>}
    >
      {props.panel ? <button type="button">in {v}</button> : undefined}
    </Tabs>
  );
}

const tabindexes = () => screen.getAllByRole("tab").map((t) => t.getAttribute("tabindex"));
const cls = (el: Element) => el.className.split(" ");

describe("Tabs", () => {
  // Fails when dialog tabs (panel, not focusable) get a second visual style, or become focus stops.
  it("dialog tabs share the view bar's tab and highlight classes and stay out of the focus order", () => {
    const { unmount } = render(<Harness focusable />);
    const nav = {
      list: screen.getByRole("tablist").className,
      tabs: screen.getAllByRole("tab").map((t) => t.className),
      ind: screen.getAllByRole("tab").map((t) => t.querySelector("[data-tab-highlight]")!.className),
    };
    unmount();
    render(
      <div data-nav-scope>
        <Harness panel start="LB" end="RB" />
      </div>,
    );
    const list = screen.getByRole("tablist", { name: "Views" });
    expect(list.className).toBe(nav.list);
    expect(screen.getAllByRole("tab").map((t) => t.className)).toEqual(nav.tabs);
    expect(screen.getAllByRole("tab").map((t) => t.querySelector("[data-tab-highlight]")!.className)).toEqual(nav.ind);
    // Panel below: a hairline under the bar separates it.
    expect(cls(list.parentElement!)).toEqual(expect.arrayContaining(["border-b", "border-hairline"]));
    for (const t of screen.getAllByRole("tab")) expect(t).toHaveAttribute("tabindex", "-1");
    expect(list).not.toHaveAttribute("data-nav-roving");
    const scope = document.querySelector<HTMLElement>("[data-nav-scope]")!;
    expect(focusables(scope).filter((el) => el.getAttribute("role") === "tab")).toEqual([]);
    expect(screen.getByRole("tabpanel")).toHaveAccessibleName("Alpha");
    expect(screen.getByRole("tab", { name: "Alpha" })).toHaveAttribute("aria-controls", screen.getByRole("tabpanel").id);
    // LB/RB still switch them (wrapping), and the keycaps show only under the pad variant.
    act(() => {
      scope.dispatchEvent(new CustomEvent(TAB_EVENT, { detail: { delta: 1 } }));
    });
    expect(document.querySelector("[data-tab=b]")).toHaveAttribute("aria-selected", "true");
    for (const k of Array.from(scope.querySelectorAll("kbd"))) expect(cls(k)).toEqual(expect.arrayContaining(["hidden", "pad:inline-flex"]));
  });

  // Fails when icon/badge slots or the data-tab hook for the shell are dropped.
  it("renders icon and badge slots and a data-tab per tab", () => {
    render(<Harness />);
    const [a, b] = screen.getAllByRole("tab");
    expect(a).toHaveAttribute("data-tab", "a");
    expect(within(a).getByTestId("icon-a")).toBeInTheDocument();
    expect(within(b).getByTestId("badge-b")).toHaveTextContent("3");
    expect(screen.queryByRole("tabpanel")).toBeNull();
  });

  // Fails when tabs lose the pill shape, the selected highlight animates anything but transform/opacity,
  // loses reduced motion, is not driven by aria-selected, or a selected+focused tab looks like a focused one.
  it("pill tabs: selected pill highlight by class (scale + opacity, spring), distinct under nav focus", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    const tab = (v: string) => document.querySelector<HTMLElement>(`[data-tab=${v}]`)!;
    const hl = (v: string) => tab(v).querySelector("[data-tab-highlight]")!;
    for (const c of [
      "rounded-pill",
      "scale-95",
      "opacity-0",
      "bg-accent-400/15",
      "group-aria-selected:scale-100",
      "group-aria-selected:opacity-100",
      "transition-[transform,opacity]",
      "ease-[cubic-bezier(0.16,1,0.3,1)]",
      "duration-[250ms]",
      "motion-reduce:transition-none",
      "in-highlight:ring-2",
      "in-highlight:ring-inset",
    ])
      expect(cls(hl("a"))).toContain(c);
    expect(hl("a")).toHaveAttribute("aria-hidden", "true");
    // The highlight is the first child; the label sits in a positioned wrapper painted above it.
    expect(tab("a").firstElementChild).toBe(hl("a"));
    expect(cls(tab("a").lastElementChild!)).toContain("relative");
    expect(tab("a").querySelector("[data-tab-indicator]")).toBeNull();
    expect(cls(tab("a"))).toEqual(
      expect.arrayContaining(["group", "rounded-pill", "min-h-control-xs", "active:scale-98", "aria-selected:text-accent-200", "aria-selected:[--focus-text:var(--color-accent-700)]"]),
    );
    expect(cls(tab("a"))).not.toContain("rounded-control");
    await user.click(tab("b"));
    expect(tab("b")).toHaveAttribute("aria-selected", "true");
    expect(tab("a")).toHaveAttribute("aria-selected", "false");
  });

  // Fails when tabindex 0 follows focus (automatic activation) or does not follow activation.
  it("focusable: one tabindex=0 on the selected tab; focus moves alone; activation moves both", async () => {
    const user = userEvent.setup();
    render(<Harness focusable />);
    expect(screen.getByRole("tablist")).toHaveAttribute("data-nav-roving");
    expect(tabindexes()).toEqual(["0", "-1", "-1"]);

    const [a, b, c] = screen.getAllByRole("tab");
    act(() => b.focus());
    expect(b).toHaveFocus();
    expect(tabindexes()).toEqual(["0", "-1", "-1"]);
    expect(a).toHaveAttribute("aria-selected", "true");

    fireEvent.keyDown(b, { key: "ArrowRight" });
    expect(c).toHaveFocus();
    expect(tabindexes()).toEqual(["0", "-1", "-1"]);
    fireEvent.keyDown(c, { key: "ArrowRight" });
    expect(a).toHaveFocus();
    fireEvent.keyDown(a, { key: "ArrowLeft" });
    expect(c).toHaveFocus();

    await user.keyboard("{Enter}");
    expect(c).toHaveAttribute("aria-selected", "true");
    expect(tabindexes()).toEqual(["-1", "-1", "0"]);

    await user.click(b);
    expect(b).toHaveAttribute("aria-selected", "true");
    expect(b).toHaveFocus();
    expect(tabindexes()).toEqual(["-1", "0", "-1"]);
  });

  // Fails when the arrow key also reaches the app's spatial-nav handler (it skips prevented events).
  it("focusable: handled arrow keys are prevented", () => {
    render(<Harness focusable />);
    const ev = new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true });
    act(() => {
      screen.getAllByRole("tab")[0].dispatchEvent(ev);
    });
    expect(ev.defaultPrevented).toBe(true);
  });

  // Fails when glyph adornments land inside the tablist, are focusable, or are exposed to AT.
  it("start/end adornments are aria-hidden siblings of the tablist", () => {
    render(<Harness focusable start="LB" end="RB" />);
    const list = screen.getByRole("tablist");
    for (const child of Array.from(list.children)) expect(child).toHaveAttribute("role", "tab");
    const wrap = list.parentElement!;
    const [first, , last] = Array.from(wrap.children);
    expect(wrap.children).toHaveLength(3);
    for (const [el, text] of [
      [first, "LB"],
      [last, "RB"],
    ] as const) {
      expect(el).toHaveTextContent(text);
      expect(el).toHaveAttribute("aria-hidden", "true");
      expect(el).not.toHaveAttribute("tabindex");
      expect(el.tagName).not.toBe("BUTTON");
      expect(el.querySelector("button,[tabindex]")).toBeNull();
    }
    expect(screen.queryByText("LB")).not.toBeNull();
    expect(screen.getAllByRole("tab")).toHaveLength(3);
  });

  // Fails when the LB/RB listener attaches outside a dialog scope (it would hijack the app's view tabs).
  it("TAB_EVENT switches (wrapping) only inside a data-nav-scope", () => {
    const { unmount } = render(<Harness />);
    act(() => {
      document.body.dispatchEvent(new CustomEvent(TAB_EVENT, { detail: { delta: 1 } }));
    });
    expect(screen.getByRole("tab", { name: "Alpha" })).toHaveAttribute("aria-selected", "true");
    unmount();

    render(
      <div data-nav-scope>
        <Harness panel />
      </div>,
    );
    const scope = document.querySelector<HTMLElement>("[data-nav-scope]")!;
    act(() => {
      scope.dispatchEvent(new CustomEvent(TAB_EVENT, { detail: { delta: -1 } }));
    });
    expect(screen.getByRole("tab", { name: "Gamma" })).toHaveAttribute("aria-selected", "true");
  });
});
