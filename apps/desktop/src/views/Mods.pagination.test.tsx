// SPDX-License-Identifier: GPL-3.0-or-later
// Mods load order paging (client-side, MODS_PAGE_SIZE rows a page): pager, reset on filter change, clamp on shrink.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useUi } from "../app/store";
import type { Handler } from "../mock/fake";
import { mod } from "../mock/fixtures";
import { setup } from "../test/harness";
import { PAGE_EVENT } from "../nav/actions";
import { MODS_PAGE_SIZE } from "./mods/paginate";

async function open(handlers: Record<string, Handler>) {
  const env = await setup(handlers);
  env.renderApp();
  act(() => useUi.getState().setView("mods"));
  return { ...env, user: userEvent.setup() };
}

const many = (n: number) => Array.from({ length: n }, (_, i) => mod({ name: `Row ${i + 1}`, position: i + 1 }));
const rows = () => within(screen.getByRole("list", { name: "Load order" })).getAllByRole("listitem");
const pager = () => screen.getByRole("navigation", { name: "Load order pages" });

describe("load order paging", () => {
  // Fails when the whole list renders at once, the range label is wrong, or the current page is marked by colour only.
  it("shows one page with a range label and the current page marked by aria-current", async () => {
    await open({ mods_list: () => ({ mods: many(60), changed_since_deploy: false }) });
    await screen.findByTitle("Row 1");
    expect(rows()).toHaveLength(MODS_PAGE_SIZE);
    expect(pager()).toHaveTextContent("1–50 of 60");
    expect(within(pager()).getByRole("button", { name: "Page 1" })).toHaveAttribute("aria-current", "page");
    expect(within(pager()).getByRole("button", { name: "Page 2" })).not.toHaveAttribute("aria-current");
    expect(within(pager()).getByRole("button", { name: "Previous" })).toHaveAttribute("aria-disabled", "true");
  });

  // Fails when Next does not page, or focus is left on a pager button far below the new rows.
  it("Next shows the next page and focuses its first row", async () => {
    const { user } = await open({ mods_list: () => ({ mods: many(60), changed_since_deploy: false }) });
    await screen.findByTitle("Row 1");
    await user.click(within(pager()).getByRole("button", { name: "Next" }));
    expect(rows()).toHaveLength(10);
    expect(pager()).toHaveTextContent("51–60 of 60");
    expect(within(pager()).getByRole("button", { name: "Page 2" })).toHaveAttribute("aria-current", "page");
    expect(within(pager()).getByRole("button", { name: "Next" })).toHaveAttribute("aria-disabled", "true");
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("checkbox", { name: "Select Row 51" })));
  });

  // Fails when the gamepad's LT/RT (PAGE_EVENT) is ignored by the Mods list.
  it("pages on PAGE_EVENT", async () => {
    await open({ mods_list: () => ({ mods: many(60), changed_since_deploy: false }) });
    await screen.findByTitle("Row 1");
    act(() => void document.dispatchEvent(new CustomEvent(PAGE_EVENT, { detail: { delta: 1 } })));
    expect(screen.getByTitle("Row 51")).toBeInTheDocument();
  });

  // Fails when a new search keeps the old page (showing an empty or wrong slice of the new matches).
  it("goes back to page 1 when the search changes", async () => {
    const { user } = await open({ mods_list: () => ({ mods: many(120), changed_since_deploy: false }) });
    await screen.findByTitle("Row 1");
    await user.click(within(pager()).getByRole("button", { name: "Page 3" }));
    expect(screen.getByTitle("Row 101")).toBeInTheDocument();
    await user.type(screen.getByRole("searchbox", { name: "Search mods" }), "Row");
    expect(screen.getByTitle("Row 1")).toBeInTheDocument();
    expect(within(pager()).getByRole("button", { name: "Page 1" })).toHaveAttribute("aria-current", "page");
  });

  // Fails when moving a page's edge row onto the previous page drops focus to <body>.
  it("follows a row moved across a page edge and keeps focus on its arrow", async () => {
    let list = many(51);
    const { user } = await open({
      mods_list: () => ({ mods: list.map((m, i) => ({ ...m, position: i + 1 })), changed_since_deploy: false }),
      mod_move: () => {
        list = [...list.slice(0, 49), list[50], list[49]];
        return { uid: list[49].uid, from: 51, to: 50, of: 51, after: { deployed: null, needs_deploy: false } };
      },
    });
    await screen.findByTitle("Row 1");
    await user.click(within(pager()).getByRole("button", { name: "Next" }));
    await user.click(screen.getByRole("button", { name: "Move Row 51 up" }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "Move Row 51 up" })));
    expect(within(pager()).getByRole("button", { name: "Page 1" })).toHaveAttribute("aria-current", "page");
  });

  // Fails when the list shrinks under the last page and an empty page (or the no-match state) is shown.
  it("clamps to the last page when the list shrinks", async () => {
    let list = many(51);
    const { user } = await open({
      mods_list: () => ({ mods: list, changed_since_deploy: false }),
      mods_set_enabled: () => {
        list = list.slice(0, 50);
        return { deployed: null, needs_deploy: false };
      },
    });
    await screen.findByTitle("Row 1");
    await user.click(within(pager()).getByRole("button", { name: "Next" }));
    expect(rows()).toHaveLength(1);
    await user.click(screen.getByRole("switch", { name: "Disable Row 51" }));
    await waitFor(() => expect(screen.queryByTitle("Row 51")).toBeNull());
    expect(rows()).toHaveLength(50);
    expect(screen.queryByRole("navigation", { name: "Load order pages" })).toBeNull();
  });
});
