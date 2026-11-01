// SPDX-License-Identifier: GPL-3.0-or-later
// Browse: single-control cards, their details dialogs (Install / Remove, a
// collection's mods), the session-restored
// listing (filters, page, scroll, focus) and Previous/Next paging.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useUi, type ViewId } from "../app/store";
import { bytes } from "../components/ui";
import type { Handler } from "../mock/fake";
import { game } from "../mock/fixtures";
import { setup } from "../test/harness";

async function open(view: ViewId, handlers: Record<string, Handler> = {}) {
  const env = await setup(handlers, [game()]);
  env.renderApp();
  act(() => useUi.getState().setView(view));
  return { ...env, user: userEvent.setup() };
}

const hit = (modId: number, o: Record<string, unknown> = {}) => ({
  modId,
  name: `Mod ${modId}`,
  author: "a",
  version: "1",
  summary: null,
  downloads: 1,
  endorsements: 1,
  thumbnailUrl: null,
  category: null,
  updatedAt: null,
  installed: false,
  ...o,
});
const coll = (slug: string, o: Record<string, unknown> = {}) => ({
  slug,
  name: `Coll ${slug}`,
  summary: null,
  author: "c",
  category: null,
  endorsements: 1,
  downloads: 1,
  revision: 3,
  mod_count: 10,
  total_size: 1,
  image_url: null,
  installed_revision: null,
  ...o,
});
type Req = { kind: string; period: string; sort: string; text: string; offset: number };
const reqOf = (a: unknown) => (a as { request: Req }).request;

/** 30 mods / 30 collections: page 1 holds 24, page 2 the last 6. */
const paged: Handler = (a) => {
  const r = reqOf(a);
  const n = r.offset === 0 ? 24 : 6;
  const ids = Array.from({ length: n }, (_, i) => r.offset + i + 1);
  return r.kind === "mods"
    ? { kind: "mods", total: 30, mods: ids.map((i) => hit(i)) }
    : { kind: "collections", total: 30, collections: ids.map((i) => coll(`s${i}`)) };
};
const lastReq = (t: { callsOf(c: string): unknown[] }) => reqOf(t.callsOf("browse").at(-1));
const showCollections = async (user: ReturnType<typeof userEvent.setup>) =>
  user.click(within(await screen.findByRole("group", { name: "Show" })).getByRole("button", { name: "Collections" }));

describe("Browse cards (Browse-1)", () => {
  // Fails when a card grows its own Install/Details/Installed buttons again (more than one control per card),
  // or the Installed marker becomes a focusable control instead of a plain badge.
  it("each card is one focusable button with a non-interactive Installed badge", async () => {
    const { user } = await open("browse", {
      browse: (a) =>
        reqOf(a).kind === "mods"
          ? { kind: "mods", total: 2, mods: [hit(7), hit(8, { installed: true })] }
          : { kind: "collections", total: 1, collections: [coll("cur", { installed_revision: 3 })] },
    });
    const mods = await screen.findByRole("list", { name: "Mods" });
    for (const li of within(mods).getAllByRole("listitem")) expect(within(li).getAllByRole("button")).toHaveLength(1);
    const card = within(mods).getByRole("button", { name: "Mod 8" });
    expect(card.tagName).toBe("BUTTON");
    expect(card).toHaveAttribute("data-browse-focus", "m8");
    expect(card).toHaveAccessibleDescription("Installed");
    expect(within(mods).getByRole("button", { name: "Mod 7" })).not.toHaveAccessibleDescription("Installed");
    expect(within(mods).queryByRole("button", { name: /^(Install|Details for|Installed: remove)/ })).toBeNull();

    await showCollections(user);
    const ccard = await screen.findByRole("button", { name: "Coll cur" });
    expect(ccard).toHaveAccessibleDescription("Installed");
    expect(within(ccard.closest("li")!).getAllByRole("button")).toHaveLength(1);
  });

  // Fails when Remove in the details dialog removes through anything but mods_uninstall (by Nexus id) /
  // collection_remove (by slug, mods not kept), or an installed item's dialog still offers Install.
  it("the details dialog of an installed mod or collection offers Remove and removes it", async () => {
    const { t, user } = await open("browse", {
      browse: (a) =>
        reqOf(a).kind === "mods"
          ? { kind: "mods", total: 1, mods: [hit(8, { installed: true })] }
          : { kind: "collections", total: 1, collections: [coll("cur", { installed_revision: 3 })] },
      mods_uninstall: () => ({ done: true, mods: 1, archives: [], after: { deployed: null, needs_deploy: true } }),
      collection_remove: () => ({ removed: true, uninstalled: 10, after: { deployed: null, needs_deploy: true } }),
    });
    await user.click(await screen.findByRole("button", { name: "Mod 8" }));
    let dlg = await screen.findByRole("dialog", { name: "Mod 8" });
    expect(within(dlg).queryByRole("button", { name: /^Install/ })).toBeNull();
    await user.click(within(dlg).getByRole("button", { name: "Remove Mod 8" }));
    await waitFor(() => expect(t.callsOf("mods_uninstall")).toEqual([{ game: null, refs: ["8"], delete_archive: false }]));
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    await showCollections(user);
    await user.click(await screen.findByRole("button", { name: "Coll cur" }));
    dlg = await screen.findByRole("dialog", { name: "Coll cur" });
    expect(within(dlg).queryByRole("button", { name: /^Install/ })).toBeNull();
    await user.click(within(dlg).getByRole("button", { name: "Remove Coll cur" }));
    await waitFor(() => expect(t.callsOf("collection_remove")).toEqual([{ game: null, reference: "cur", keep_mods: false }]));
  });
});

describe("Browse collection card height (Browse-5)", () => {
  // Fails when a long collection description can make the card taller than its picture: the text column must be
  // taken out of flow (absolute in a relative column, overflow hidden) and the summary must shrink and clamp with an
  // ellipsis, while title, byline and the stats/badges row keep their size.
  it("collection cards clamp the description to the picture's height", async () => {
    const { user } = await open("browse", {
      browse: (a) =>
        reqOf(a).kind === "mods"
          ? { kind: "mods", total: 0, mods: [] }
          : { kind: "collections", total: 1, collections: [coll("long", { summary: "word ".repeat(400), installed_revision: 3 })] },
    });
    await showCollections(user);
    const card = await screen.findByRole("button", { name: "Coll long" });
    const summary = within(card).getByTestId("tile-summary");
    for (const c of ["line-clamp-4", "overflow-hidden", "text-ellipsis", "min-h-0", "shrink"]) expect(summary.className.split(" ")).toContain(c);
    const column = summary.parentElement!;
    for (const c of ["absolute", "inset-0", "overflow-hidden", "flex-col"]) expect(column.className.split(" ")).toContain(c);
    expect(column.parentElement!.className.split(" ")).toContain("relative");
    // Title, byline and the badge row do not shrink away.
    expect(within(card).getByText("Coll long").className).toMatch(/\bshrink-0\b/);
    expect(within(card).getByText("Installed").closest("span.mt-auto")!.className).toMatch(/\bshrink-0\b/);
  });
});

describe("Browse details (Browse-2)", () => {
  // Fails when activating a card (click or Enter) does not open a dialog with the mod's full info, or closing it
  // strands focus instead of returning to the card.
  it("activating a mod card shows the full info and returns focus to the card on close", async () => {
    const { user } = await open("browse", {
      browse: () => ({
        kind: "mods",
        total: 1,
        mods: [hit(7, { summary: "Better skies.", author: "skyler", version: "2.1", category: "Visuals", endorsements: 1234, downloads: 98765, updatedAt: "2026-01-02T00:00:00Z", installed: true })],
      }),
    });
    const card = await screen.findByRole("button", { name: "Mod 7" });
    await user.click(card);
    const dlg = await screen.findByRole("dialog", { name: "Mod 7" });
    expect(dlg).toHaveTextContent("Better skies.");
    for (const v of ["skyler", "2.1", "Visuals", "1,234", "98,765", "Installed"]) expect(dlg).toHaveTextContent(v);
    expect(dlg).toHaveTextContent(new Date("2026-01-02T00:00:00Z").toLocaleDateString());
    await user.click(within(dlg).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(card));

    // Keyboard / controller A (Enter on the focused card) opens it too.
    act(() => card.focus());
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("dialog", { name: "Mod 7" })).toBeInTheDocument();
  });

  // Fails when a not-installed mod's dialog lacks Install, or Install skips the existing flow (install by Nexus id
  // with the FOMOD wizard).
  it("a not-installed mod's dialog installs it", async () => {
    const { t, user } = await open("browse", {
      browse: () => ({ kind: "mods", total: 1, mods: [hit(7)] }),
      install: () => ({ game: "skyrimspecialedition", installed: { mod_id: 1, uid: "u", name: "Mod 7", files: 2, size: 1, fomod_found: false, replaced: null, warnings: [] }, collection: null, after: { deployed: null, needs_deploy: true } }),
    });
    await user.click(await screen.findByRole("button", { name: "Mod 7" }));
    const dlg = await screen.findByRole("dialog", { name: "Mod 7" });
    expect(within(dlg).queryByRole("button", { name: /^Remove/ })).toBeNull();
    await user.click(within(dlg).getByRole("button", { name: "Install Mod 7" }));
    await waitFor(() => expect(t.callsOf("install")).toEqual([expect.objectContaining({ target: "7", fomod: "ask" })]));
  });

  // Fails when collection details does not ask collection_info for the slug, drops its revision details, or does
  // not list the collection's mods (name, version, author, size, optional flag).
  it("collection details loads the revision and its mods from collection_info", async () => {
    const { t, user } = await open("browse", {
      browse: (a) =>
        reqOf(a).kind === "mods" ? { kind: "mods", total: 0, mods: [] } : { kind: "collections", total: 1, collections: [coll("abc", { installed_revision: 2 })] },
      collection_info: () => ({
        game: "skyrimspecialedition",
        slug: "abc",
        revision: {
          name: "Coll abc", author: "c", game_name: "Skyrim SE", number: 3, status: "published", latest: 3, mod_count: 2, optional_count: 1, total_size: 100, summary: "Full curated list.", url: "",
          mods: [
            { name: "SkyUI", version: "5.2", author: "schlangster", mod_id: 12604, size: 2048, optional: false },
            { name: "Extra Bits", version: null, author: null, mod_id: 5, size: 0, optional: true },
          ],
        },
        installed: null,
        update_available: 3,
      }),
    });
    await showCollections(user);
    const card = await screen.findByRole("button", { name: "Coll abc" });
    await user.click(card);
    const dlg = await screen.findByRole("dialog", { name: "Coll abc" });
    expect(await within(dlg).findByText("Full curated list.")).toBeInTheDocument();
    expect(t.callsOf("collection_info")).toEqual([{ game: null, reference: "abc" }]);
    expect(dlg).toHaveTextContent("2 (1 optional)");
    expect(dlg).toHaveTextContent("Revision 3 available");
    const rows = within(within(dlg).getByRole("list", { name: "Collection mods" })).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("SkyUI");
    expect(rows[0]).toHaveTextContent(`v5.2 · schlangster · ${bytes(2048)}`);
    expect(rows[0]).not.toHaveTextContent("Optional");
    expect(rows[1]).toHaveTextContent("Extra Bits");
    expect(rows[1]).toHaveTextContent("Optional");
    // Installed revision 2 of 3: Update and Remove, no Install.
    expect(within(dlg).getByRole("button", { name: "Update Coll abc" })).toBeInTheDocument();
    expect(within(dlg).getByRole("button", { name: "Remove Coll abc" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(card));
  });
});

describe("Browse session restore (Browse-3)", () => {
  // Fails when leaving Browse forgets the tab, period, sort, search words, page, scroll or focused tile.
  it("restores tab, filters, search, page, scroll and focus on returning", async () => {
    const { t, user } = await open("browse", { browse: paged });
    await screen.findByText("Mod 1");
    await showCollections(user);
    await user.click(screen.getByRole("button", { name: "Month" }));
    await user.type(screen.getByLabelText("Search"), "lore");
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.click(await screen.findByRole("button", { name: /^Next/ }));
    const card = await screen.findByRole("button", { name: "Coll s27" });
    act(() => card.focus());
    // jsdom has no layout: give <main> a settable scrollTop.
    const main = screen.getByRole("main");
    let top = 0;
    Object.defineProperty(main, "scrollTop", { configurable: true, get: () => top, set: (v: number) => (top = v) });
    top = 640;
    main.dispatchEvent(new Event("scroll"));

    act(() => useUi.getState().setView("mods"));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Coll s27" })).toBeNull());
    top = 0;
    act(() => useUi.getState().setView("browse"));

    const back = await screen.findByRole("button", { name: "Coll s27" });
    expect(within(screen.getByRole("group", { name: "Show" })).getByRole("button", { name: "Collections" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Month" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("Search")).toHaveValue("lore");
    expect(screen.getByText(/Page 2 of 2/)).toBeInTheDocument();
    expect(lastReq(t)).toEqual({ kind: "collections", period: "month", sort: "endorsements", text: "lore", offset: 24 });
    await waitFor(() => expect(document.activeElement).toBe(back));
    expect(top).toBe(640);
  });
});

describe("Browse paging (Browse-4)", () => {
  // Fails when Load more comes back, Next/Previous ask for the wrong offset, the last page still offers Next,
  // or a filter change keeps a stale page.
  it("pages mods and collections with Previous/Next by offset", async () => {
    const { t, user } = await open("browse", { browse: paged });
    await screen.findByText("Mod 1");
    expect(screen.queryByRole("button", { name: "Load more" })).toBeNull();
    const pager = screen.getByRole("navigation", { name: "Mods pages" });
    const prev = within(pager).getByRole("button", { name: /Previous/ });
    const next = within(pager).getByRole("button", { name: /Next/ });
    expect(prev).toHaveAttribute("aria-disabled", "true");
    expect(pager).toHaveTextContent("Page 1 of 2");

    await user.click(next);
    expect(await screen.findByText("Mod 25")).toBeInTheDocument();
    expect(screen.queryByText("Mod 1")).toBeNull();
    expect(lastReq(t).offset).toBe(24);
    expect(pager).toHaveTextContent("Page 2 of 2 · 25–30 of 30");
    expect(next).toHaveAttribute("aria-disabled", "true");
    expect(document.activeElement).toBe(next);

    await user.click(prev);
    expect(await screen.findByText("Mod 1")).toBeInTheDocument();
    expect(pager).toHaveTextContent("Page 1 of 2 · 1–24 of 30");

    await user.click(next);
    await screen.findByText("Mod 25");
    await showCollections(user);
    expect(await screen.findByText("Coll s1")).toBeInTheDocument();
    expect(lastReq(t)).toMatchObject({ kind: "collections", offset: 0 });
    await user.click(within(screen.getByRole("navigation", { name: "Collections pages" })).getByRole("button", { name: /Next/ }));
    expect(await screen.findByText("Coll s25")).toBeInTheDocument();
    expect(lastReq(t)).toMatchObject({ kind: "collections", offset: 24 });
  });
});
