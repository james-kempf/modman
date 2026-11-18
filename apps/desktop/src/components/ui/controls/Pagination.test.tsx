// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Pagination } from "./Pagination";

const nav = () => screen.getByRole("navigation", { name: "Items pages" });
const button = (name: string) => within(nav()).getByRole("button", { name });

describe("Pagination", () => {
  // Fails when the default per-page count overshoots the total on a short last page.
  it("labels the page and its 1-based range, short last page included", () => {
    render(<Pagination label="Items pages" page={2} pages={3} size={50} total={112} onPage={() => {}} />);
    expect(nav()).toHaveTextContent("Page 3 of 3 · 101–112 of 112");
  });

  // Fails when a single page still offers paging buttons.
  it("shows only the count with one page", () => {
    render(<Pagination label="Items pages" page={0} pages={1} size={50} total={7} onPage={() => {}} />);
    expect(nav()).toHaveTextContent("Page 1 of 1 · 1–7 of 7");
    expect(within(nav()).queryByRole("button")).toBeNull();
  });

  // Fails when a button targets the wrong page, or an end button pages past the end instead of staying inert.
  it("calls onPage with the target page and step, and ignores inactive ends", async () => {
    const onPage = vi.fn();
    const user = userEvent.setup();
    const { rerender } = render(
      <Pagination label="Items pages" page={0} pages={5} size={10} total={45} onPage={onPage} />,
    );
    expect(button("First")).toHaveAttribute("aria-disabled", "true");
    await user.click(button("Previous"));
    expect(onPage).not.toHaveBeenCalled();
    await user.click(button("Next"));
    await user.click(button("Last"));
    expect(onPage.mock.calls).toEqual([
      [1, "next"],
      [4, "last"],
    ]);
    onPage.mockClear();
    rerender(<Pagination label="Items pages" page={2} pages={5} size={10} total={45} onPage={onPage} />);
    await user.click(button("First"));
    await user.click(button("Previous"));
    expect(onPage.mock.calls).toEqual([
      [0, "first"],
      [1, "prev"],
    ]);
  });

  // Fails when a click lands while the next page loads, or the label shows a stale total.
  it("is busy and ignores clicks while loading", async () => {
    const onPage = vi.fn();
    render(<Pagination label="Items pages" page={1} pages={3} size={10} total={30} loading onPage={onPage} />);
    expect(nav()).toHaveTextContent("Page 2 · loading…");
    expect(button("Next")).toHaveAttribute("aria-busy", "true");
    await userEvent.setup().click(button("Next"));
    expect(onPage).not.toHaveBeenCalled();
  });

  // Fails when the default bottom pinning is lost, or a caller's className replaces it instead of adding to it.
  it("pins to the bottom by default and appends className", () => {
    render(<Pagination label="Items pages" page={0} pages={2} size={10} total={15} className="mt-5" onPage={() => {}} />);
    const cls = nav().className.split(" ");
    expect(cls).toEqual(expect.arrayContaining(["sticky", "bottom-[calc(var(--main-pb,0px)*-1)]", "z-10", "mt-5"]));
  });

  // Fails when the label is centred only in the leftover space (a flex row), or drops out of the middle column
  // with one page (no button groups to hold columns 1 and 3).
  it("centres the label in the middle of a 1fr|auto|1fr grid, with or without buttons", () => {
    const { rerender } = render(
      <Pagination label="Items pages" page={0} pages={2} size={10} total={15} onPage={() => {}} />,
    );
    const check = () => {
      expect(nav()).toHaveClass("grid", "grid-cols-[1fr_minmax(0,auto)_1fr]");
      const label = within(nav()).getByText(/^Page 1 of/);
      expect(label).toHaveClass("col-start-2", "text-center", "min-w-0");
    };
    check();
    expect(button("Previous").parentElement).toHaveClass("col-start-1", "justify-self-start");
    expect(button("Next").parentElement).toHaveClass("col-start-3", "justify-self-end");
    rerender(<Pagination label="Items pages" page={0} pages={1} size={10} total={5} onPage={() => {}} />);
    check();
  });

  // Fails when per-button attributes (Browse's focus-parking keys) are not applied.
  it("applies buttonAttrs to each button", () => {
    render(
      <Pagination
        label="Items pages"
        page={1}
        pages={3}
        size={10}
        total={30}
        buttonAttrs={(step) => ({ "data-k": `page:${step}` })}
        onPage={() => {}}
      />,
    );
    expect(button("Next")).toHaveAttribute("data-k", "page:next");
    expect(button("First")).toHaveAttribute("data-k", "page:first");
  });
});
