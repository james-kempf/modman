// SPDX-License-Identifier: GPL-3.0-or-later
// Browse: single-control cards, their details dialogs (Details / About / Mods
// tabs switched by LB/RB or a click, Install / Remove, a collection's mod
// grid), the header search (gamepad Y), the session-restored listing
// (filters, page, scroll, focus) and Previous/Next paging.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useUi, type ViewId } from "../app/store";
import { MOD_GRID, RESULTS_GRID } from "../components/browse/Tile";
import { bytes } from "../components/ui";
import type { Handler } from "../mock/fake";
import { game } from "../mock/fixtures";
import { performNav } from "../nav/actions";
import { focusables } from "../nav/spatial";
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
/** Open a dialog's tab by clicking it (the tabs are no focus stops). */
const clickTab = (user: ReturnType<typeof userEvent.setup>, dlg: HTMLElement, name: string | RegExp) =>
  user.click(within(dlg).getByRole("tab", { name }));
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
    await screen.findByText("Mod 7");
    const mods = screen.getByRole("list", { name: "Mods" });
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

describe("Browse dialogs close on install/uninstall (T9)", () => {
  /** A command held until release (the job keeps running after the dialog closed). */
  const slow = <T,>(value: T) => {
    let open!: () => void;
    const gate = new Promise<void>((r) => (open = r));
    return { handler: async () => (await gate, value), release: () => act(async () => open()) };
  };
  const installResult = { game: null, installed: null, collection: null, after: { deployed: null, needs_deploy: false } };
  /** The backend's uninstall confirmation for job `job` of `kind`. */
  const confirmFor = (t: { emit(e: string, p: unknown): void }, job: number, kind: string) =>
    act(() => {
      t.emit("vessel:job", { id: job, kind, state: "started", error: null });
      t.emit("vessel:prompt", { id: job * 10, job, prompt: { type: "confirm", question: "Sure?", default_yes: false } });
    });
  const browseBoth = (a: unknown) =>
    reqOf(a).kind === "mods"
      ? { kind: "mods", total: 2, mods: [hit(7), hit(8, { installed: true })] }
      : { kind: "collections", total: 2, collections: [coll("new"), coll("cur", { installed_revision: 3 })] };

  // Fails when the mod dialog stays open after Install (until the job ends), or focus does not return to the card.
  it("mod Install closes the dialog at once and focuses the card", async () => {
    const inst = slow(installResult);
    const { t, user } = await open("browse", { browse: browseBoth, install: inst.handler });
    const card = await screen.findByRole("button", { name: "Mod 7" });
    await user.click(card);
    const dlg = await screen.findByRole("dialog", { name: "Mod 7" });
    await user.click(within(dlg).getByRole("button", { name: "Install Mod 7" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(t.callsOf("install")).toHaveLength(1);
    await waitFor(() => expect(document.activeElement).toBe(card));
    await inst.release();
  });

  // Fails when Remove closes the dialog before the confirmation is answered, keeps it open after Yes, or
  // closes it on No.
  it("mod Remove: No keeps the dialog, Yes closes it and focuses the card", async () => {
    // First run: answered No, the job ends with nothing done; second run: Yes.
    const first = slow({ done: false, mods: 0, archives: [], after: { deployed: null, needs_deploy: false } });
    const rm = slow({ done: true, mods: 1, archives: [], after: { deployed: null, needs_deploy: false } });
    let runs = 0;
    const { t, user } = await open("browse", { browse: browseBoth, mods_uninstall: () => (runs++ ? rm.handler() : first.handler()) });
    const card = await screen.findByRole("button", { name: "Mod 8" });
    await user.click(card);
    const dlg = await screen.findByRole("dialog", { name: "Mod 8" });
    await user.click(within(dlg).getByRole("button", { name: "Remove Mod 8" }));
    confirmFor(t, 5, "mods_uninstall");
    expect(screen.getByRole("dialog", { name: "Mod 8" })).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "No" }));
    await first.release();
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(screen.getByRole("dialog", { name: "Mod 8" })).toBeInTheDocument();

    await user.click(within(screen.getByRole("dialog", { name: "Mod 8" })).getByRole("button", { name: "Remove Mod 8" }));
    confirmFor(t, 6, "mods_uninstall");
    await user.click(await screen.findByRole("button", { name: "Yes" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(card));
    await rm.release();
  });

  // Fails when a collection's Install or Remove leaves its dialog open.
  it("collection Install and Remove close the dialog and focus the card", async () => {
    const inst = slow({ ...installResult });
    const rm = slow({ removed: true, uninstalled: 1, after: { deployed: null, needs_deploy: false } });
    const { t, user } = await open("browse", { browse: browseBoth, collection_install: inst.handler, collection_remove: rm.handler });
    await showCollections(user);
    const fresh = await screen.findByRole("button", { name: "Coll new" });
    await user.click(fresh);
    let dlg = await screen.findByRole("dialog", { name: "Coll new" });
    await user.click(within(dlg).getByRole("button", { name: "Install Coll new" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(fresh));

    const cur = screen.getByRole("button", { name: "Coll cur" });
    await user.click(cur);
    dlg = await screen.findByRole("dialog", { name: "Coll cur" });
    await user.click(within(dlg).getByRole("button", { name: "Remove Coll cur" }));
    confirmFor(t, 8, "collection_remove");
    await user.click(await screen.findByRole("button", { name: "Yes" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(cur));
    await inst.release();
    await rm.release();
  });

  // Fails when Install on a mod inside a collection's Mods tab closes the collection dialog too (or nothing).
  it("a mod installed from a collection's Mods tab closes only its own dialog", async () => {
    const inst = slow(installResult);
    const { user } = await open("browse", {
      browse: browseBoth,
      install: inst.handler,
      collection_info: () => ({
        game: "skyrimspecialedition",
        slug: "new",
        revision: {
          name: "Coll new", author: "c", game_name: "Skyrim SE", number: 3, status: "published", latest: 3, mod_count: 1, optional_count: 0, total_size: 1, summary: "", url: "",
          mods: [{ name: "SkyUI", version: "5.2", author: "s", mod_id: 12604, size: 1, optional: false }],
        },
        installed: null,
        update_available: null,
      }),
    });
    await showCollections(user);
    await user.click(await screen.findByRole("button", { name: "Coll new" }));
    const outer = await screen.findByRole("dialog", { name: "Coll new" });
    await user.click(await within(outer).findByRole("tab", { name: /^Mods/ }));
    const tile = await within(outer).findByRole("button", { name: "SkyUI" });
    await user.click(tile);
    const inner = await screen.findByRole("dialog", { name: "SkyUI" });
    await user.click(within(inner).getByRole("button", { name: "Install SkyUI" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "SkyUI" })).toBeNull());
    expect(screen.getByRole("dialog", { name: "Coll new" })).toBeInTheDocument();
    await waitFor(() => expect(document.activeElement).toBe(tile));
    await inst.release();
  });

  const twoFiles = () => ({
    files: [1, 2].map((i) => ({ file_id: i, name: `File ${i}`, version: null, category_name: "MAIN", file_name: null, size_in_bytes: null, uploaded_timestamp: null, is_primary: i === 1 })),
    default: 0,
  });
  /** The backend's file choice for install job `job` (after the job started). */
  const chooseFileFor = (t: { emit(e: string, p: unknown): void }, job: number) =>
    act(() => {
      t.emit("vessel:job", { id: job, kind: "install", state: "started", error: null });
      t.emit("vessel:prompt", { id: job * 10, job, prompt: { type: "choose_file", files: twoFiles().files, default: 0 } });
    });

  // T10: fails when a mod with several files closes its dialog before the file is picked, stays open after
  // the pick, or focus does not end on the card.
  it("a mod with file options: the dialog stays open while choosing, closes on the pick", async () => {
    const inst = slow(installResult);
    const { t, user } = await open("browse", { browse: browseBoth, install: inst.handler, mod_files: twoFiles });
    const card = await screen.findByRole("button", { name: "Mod 7" });
    await user.click(card);
    const dlg = await screen.findByRole("dialog", { name: "Mod 7" });
    await user.click(within(dlg).getByRole("button", { name: "Install Mod 7" }));
    await waitFor(() => expect(t.callsOf("mod_files")).toHaveLength(1));
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(screen.getByRole("dialog", { name: "Mod 7" })).toBeInTheDocument();
    chooseFileFor(t, 5);
    const chooser = await screen.findByRole("dialog", { name: "Choose a file" });
    expect(screen.getByRole("dialog", { name: "Mod 7" })).toBeInTheDocument();
    await user.click(within(chooser).getByRole("button", { name: "Install this file" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(card));
    await inst.release();
  });

  // T10: fails when cancelling the file choice closes the mod dialog or leaves focus outside it.
  it("cancelling the file choice keeps the dialog open, focus on Install", async () => {
    const inst = slow(installResult);
    const { t, user } = await open("browse", { browse: browseBoth, install: inst.handler, mod_files: twoFiles });
    await user.click(await screen.findByRole("button", { name: "Mod 7" }));
    const dlg = await screen.findByRole("dialog", { name: "Mod 7" });
    const install = within(dlg).getByRole("button", { name: "Install Mod 7" });
    await user.click(install);
    await waitFor(() => expect(t.callsOf("mod_files")).toHaveLength(1));
    chooseFileFor(t, 6);
    const chooser = await screen.findByRole("dialog", { name: "Choose a file" });
    await user.click(within(chooser).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Choose a file" })).toBeNull());
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(screen.getByRole("dialog", { name: "Mod 7" })).toBeInTheDocument();
    await waitFor(() => expect(document.activeElement).toBe(install));
    await inst.release();
  });

  // T10: fails when the nested collection-mod dialog ignores the file choice (closes early) or closes the collection.
  it("nested: a collection's mod with file options closes only after the pick, collection stays", async () => {
    const inst = slow(installResult);
    const { t, user } = await open("browse", {
      browse: browseBoth,
      install: inst.handler,
      mod_files: twoFiles,
      collection_info: () => ({
        game: "skyrimspecialedition",
        slug: "new",
        revision: {
          name: "Coll new", author: "c", game_name: "Skyrim SE", number: 3, status: "published", latest: 3, mod_count: 1, optional_count: 0, total_size: 1, summary: "", url: "",
          mods: [{ name: "SkyUI", version: "5.2", author: "s", mod_id: 12604, size: 1, optional: false }],
        },
        installed: null,
        update_available: null,
      }),
    });
    await showCollections(user);
    await user.click(await screen.findByRole("button", { name: "Coll new" }));
    const outer = await screen.findByRole("dialog", { name: "Coll new" });
    await user.click(await within(outer).findByRole("tab", { name: /^Mods/ }));
    const tile = await within(outer).findByRole("button", { name: "SkyUI" });
    await user.click(tile);
    const inner = await screen.findByRole("dialog", { name: "SkyUI" });
    await user.click(within(inner).getByRole("button", { name: "Install SkyUI" }));
    await waitFor(() => expect(t.callsOf("mod_files")).toHaveLength(1));
    chooseFileFor(t, 7);
    const chooser = await screen.findByRole("dialog", { name: "Choose a file" });
    expect(screen.getByRole("dialog", { name: "SkyUI" })).toBeInTheDocument();
    await user.click(within(chooser).getByRole("button", { name: "Install this file" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "SkyUI" })).toBeNull());
    expect(screen.getByRole("dialog", { name: "Coll new" })).toBeInTheDocument();
    await waitFor(() => expect(document.activeElement).toBe(tile));
    await inst.release();
  });

  // Fails when a non-install action (Close excepted) such as the About tab closes the dialog.
  it("switching tabs does not close the dialog", async () => {
    const { user } = await open("browse", { browse: browseBoth });
    await user.click(await screen.findByRole("button", { name: "Mod 7" }));
    const dlg = await screen.findByRole("dialog", { name: "Mod 7" });
    await user.click(within(dlg).getByRole("tab", { name: "About" }));
    expect(screen.getByRole("dialog", { name: "Mod 7" })).toBeInTheDocument();
  });
});

describe("Browse tile stats row height (T11)", () => {
  /** The flex line that holds a tile's stats and its status badges. */
  const statsLine = (card: HTMLElement) => within(card).getByText("downloads", { selector: ".sr-only" }).closest<HTMLElement>("[data-tile-stats]")!;
  // Fails when the stats line is only as tall as its text without the Installed badge (h-badge), so an
  // installed tile's line is taller than its neighbour's.
  it("is at least badge-tall with and without Installed, mods and collections", async () => {
    const { user } = await open("browse", {
      browse: (a) =>
        reqOf(a).kind === "mods"
          ? { kind: "mods", total: 2, mods: [hit(7), hit(8, { installed: true })] }
          : { kind: "collections", total: 2, collections: [coll("new"), coll("cur", { installed_revision: 3 })] },
    });
    const lines = async (a: string, b: string) => [statsLine(await screen.findByRole("button", { name: a })), statsLine(screen.getByRole("button", { name: b }))];
    for (const [plain, installed] of [await lines("Mod 7", "Mod 8")]) {
      expect(within(installed).getByText("Installed")).toBeInTheDocument();
      expect(within(plain).queryByText("Installed")).toBeNull();
      for (const l of [plain, installed]) expect(l.className.split(" ")).toEqual(expect.arrayContaining(["flex", "min-h-badge", "items-center"]));
      expect(plain.className).toBe(installed.className);
    }
    await showCollections(user);
    for (const [plain, installed] of [await lines("Coll new", "Coll cur")]) {
      for (const l of [plain, installed]) expect(l.className.split(" ")).toEqual(expect.arrayContaining(["flex", "min-h-badge", "items-center"]));
      expect(plain.className).toBe(installed.className);
    }
  });

  // Fails when the skeleton's stats line is shorter than a real tile's (the grid jumps when tiles arrive).
  it("the skeleton's stats line is badge-tall too", async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    await open("browse", { browse: async (a) => (await gate, paged(a)) });
    const sk = (await screen.findAllByTestId("tile-skeleton"))[0];
    expect(sk.querySelector(".min-h-badge")).not.toBeNull();
    await act(async () => release());
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
    await user.click(within(dlg).getByRole("tab", { name: "Mods (2)" }));
    const rows = within(within(dlg).getByRole("list", { name: "Collection mods" })).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("SkyUI");
    expect(rows[0]).toHaveTextContent("v5.2 · schlangster");
    expect(rows[0]).toHaveTextContent(bytes(2048));
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

const infoWith = (mods: Record<string, unknown>[]) => () => ({
  game: "skyrimspecialedition",
  slug: "abc",
  revision: {
    name: "Coll abc", author: "c", game_name: "Skyrim SE", number: 3, status: "published", latest: 3, mod_count: mods.length, optional_count: 0, total_size: 100, summary: "About text.", url: "",
    mods,
  },
  installed: null,
  update_available: null,
});
const collOnly: Handler = (a) =>
  reqOf(a).kind === "mods" ? { kind: "mods", total: 0, mods: [] } : { kind: "collections", total: 1, collections: [coll("abc")] };

describe("Browse toolbar and collection details layout (T3)", () => {
  // B1. Fails when the Sort label goes back above the dropdown (stacked grid, text-label) instead of inline-left
  // with the Segmented labels' eyebrow style, or the dropdown loses its "Sort" name.
  it("the Sort label sits left of the dropdown with the Segmented label style", async () => {
    await open("browse", { browse: collOnly });
    const sort = await screen.findByRole("button", { name: /^Sort/ });
    const label = screen.getByText("Sort", { selector: "span" });
    const showLabel = within(screen.getByRole("group", { name: "Show" })).getByText("Show");
    expect(label.className.split(" ")).toContain("eyebrow");
    expect(showLabel.className.split(" ")).toContain("eyebrow");
    expect(label.parentElement).toBe(sort.parentElement);
    expect(label.parentElement!.className.split(" ")).toEqual(expect.arrayContaining(["flex", "items-center"]));
    expect(label.parentElement!.className.split(" ")).not.toContain("grid");
    expect(label.compareDocumentPosition(sort) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  // A1/A2. Fails when the mod grid shows by default, the description stays on Details instead of its own About tab,
  // the summary leaves Details, the tabs lose their order or Mods count, go back to the Segmented pill look or do not
  // stretch across the dialog, or the picture and facts are not side by side (stacking only when narrow).
  it("collection details: Details (picture, facts, summary) | About (description) | Mods tabs, full width", async () => {
    const { t, user } = await open("browse", {
      browse: (a) =>
        (a as { request: { about?: unknown } }).request.about ? { kind: "about", description: "Long **description**." } : collOnly(a),
      collection_info: infoWith([
        { name: "SkyUI", version: "5.2", author: "s", mod_id: 12604, size: 2048, optional: false },
        { name: "Manual", version: null, author: null, mod_id: null, size: 0, optional: false },
      ]),
    });
    await showCollections(user);
    await user.click(await screen.findByRole("button", { name: "Coll abc" }));
    const dlg = await screen.findByRole("dialog", { name: "Coll abc" });
    expect(await within(dlg).findByText("About text.")).toBeInTheDocument();
    const tablist = within(dlg).getByRole("tablist", { name: "Collection" });
    expect(within(tablist).getAllByRole("tab").map((x) => x.textContent)).toEqual(["Details", "About", "Mods (2)"]);
    expect(within(tablist).getByRole("tab", { name: "Details" })).toHaveAttribute("aria-selected", "true");
    expect(within(dlg).getByRole("tabpanel")).toHaveAccessibleName("Details");
    expect(within(dlg).queryByRole("region", { name: "About" })).toBeNull();
    expect(within(dlg).queryByRole("list", { name: "Collection mods" })).toBeNull();
    // Tab look: an underlined strip across the dialog, equal-width tabs.
    expect(tablist.className.split(" ")).toEqual(expect.arrayContaining(["flex", "w-full", "border-b"]));
    for (const tab of within(tablist).getAllByRole("tab")) {
      expect(tab.className.split(" ")).toEqual(expect.arrayContaining(["flex-1", "border-b-2", "aria-selected:border-accent-400"]));
      expect(tab).not.toHaveAttribute("aria-pressed");
    }
    // Picture and facts share a row from sm up, stacked below; the facts column can shrink (no horizontal scroll).
    const facts = within(dlg).getByText("Latest revision").closest("dl")!;
    const row = facts.parentElement!;
    for (const c of ["flex", "flex-col", "sm:flex-row"]) expect(row.className.split(" ")).toContain(c);
    expect(row.firstElementChild).not.toBe(facts);
    expect(facts.className.split(" ")).toEqual(expect.arrayContaining(["min-w-0", "flex-1"]));
    for (const k of ["Author", "Category", "Latest revision", "Status", "Mods", "Revision status"]) expect(facts).toHaveTextContent(k);

    await clickTab(user, dlg, "About");
    expect(within(dlg).getByRole("tabpanel")).toHaveAccessibleName("About");
    const about = within(dlg).getByRole("region", { name: "About" });
    expect(await within(about).findByText("description")).toHaveProperty("tagName", "STRONG");
    expect(within(dlg).queryByText("Latest revision")).toBeNull();
    expect(t.callsOf("browse").filter((a) => (a as { request: { about?: unknown } }).request.about)).toHaveLength(1);
  });

  // A1. Fails when the mod dialog has no Details | About tabs, or the description renders on Details.
  it("mod details: Details (picture, facts, summary) | About (description)", async () => {
    const { user } = await open("browse", {
      browse: (a) =>
        (a as { request: { about?: unknown } }).request.about
          ? { kind: "about", description: "[b]Full text[/b]" }
          : { kind: "mods", total: 1, mods: [hit(7, { summary: "Short summary." })] },
    });
    await user.click(await screen.findByRole("button", { name: "Mod 7" }));
    const dlg = await screen.findByRole("dialog", { name: "Mod 7" });
    const tablist = within(dlg).getByRole("tablist", { name: "Mod" });
    expect(within(tablist).getAllByRole("tab").map((x) => x.textContent)).toEqual(["Details", "About"]);
    expect(within(dlg).getByRole("tabpanel")).toHaveTextContent("Short summary.");
    expect(within(dlg).getByRole("tabpanel")).toHaveTextContent("Nexus id");
    expect(within(dlg).queryByRole("region", { name: "About" })).toBeNull();
    await clickTab(user, dlg, "About");
    expect(await within(within(dlg).getByRole("region", { name: "About" })).findByText("Full text")).toBeInTheDocument();
    expect(within(dlg).queryByText("Nexus id")).toBeNull();
  });

  // A3. Fails when the tabs are focus stops (Tab or spatial nav), LB/RB (PageDown/PageUp under Steam Input) do not
  // switch them or do not wrap, the app's view tabs switch too, or focus is stranded when its panel goes away.
  it("dialog tabs switch with LB/RB, wrap, are no focus stops, and leave the app's views alone", async () => {
    const { user } = await open("browse", {
      browse: collOnly,
      collection_info: infoWith([{ name: "SkyUI", version: "5.2", author: "s", mod_id: 12604, size: 2048, optional: false }]),
    });
    await showCollections(user);
    await user.click(await screen.findByRole("button", { name: "Coll abc" }));
    const dlg = await screen.findByRole("dialog", { name: "Coll abc" });
    await within(dlg).findByText("About text.");
    const tabs = within(dlg).getAllByRole("tab");
    for (const tab of tabs) {
      expect(tab).toHaveAttribute("tabindex", "-1");
      expect(focusables(dlg)).not.toContain(tab);
    }
    const selected = () => within(dlg).getAllByRole("tab").find((x) => x.getAttribute("aria-selected") === "true")!.textContent;
    await user.keyboard("{PageDown}");
    expect(selected()).toBe("About");
    act(() => performNav({ type: "tab", delta: 1 }));
    expect(selected()).toBe("Mods (1)");
    await user.keyboard("{PageDown}");
    expect(selected()).toBe("Details");
    await user.keyboard("{PageUp}");
    expect(selected()).toBe("Mods (1)");
    expect(useUi.getState().view).toBe("browse");
    // Focus on a mod card, then LB: the card goes with its panel and focus lands on the dialog's Close.
    act(() => within(dlg).getByRole("button", { name: "SkyUI" }).focus());
    act(() => performNav({ type: "tab", delta: -1 }));
    expect(selected()).toBe("About");
    await waitFor(() => expect(document.activeElement).toBe(within(dlg).getByRole("button", { name: "Close" })));
    // Clicking a tab switches without focusing it.
    await clickTab(user, dlg, "Details");
    expect(selected()).toBe("Details");
    expect(document.activeElement).not.toHaveAttribute("role", "tab");
  });

  // A4. Fails when the Mods tab is not the Browse results grid of cards, or a mod without a Nexus id looks or acts
  // like an activatable card.
  it("the Mods tab is a grid of browse cards; mods without a Nexus id are dimmed, non-activatable cards", async () => {
    const { user } = await open("browse", {
      browse: collOnly,
      collection_info: infoWith([
        { name: "SkyUI", version: "5.2", author: "s", mod_id: 12604, size: 2048, optional: true },
        { name: "Manual", version: null, author: null, mod_id: null, size: 0, optional: false },
      ]),
    });
    await showCollections(user);
    await user.click(await screen.findByRole("button", { name: "Coll abc" }));
    const dlg = await screen.findByRole("dialog", { name: "Coll abc" });
    await clickTab(user, dlg, "Mods (2)");
    const list = await within(dlg).findByRole("list", { name: "Collection mods" });
    expect(list.className).toBe(MOD_GRID);
    const sky = within(list).getByRole("button", { name: "SkyUI" });
    expect(sky).toHaveAttribute("aria-haspopup", "dialog");
    expect(sky.closest("li")).toHaveTextContent("Optional");
    const manual = within(list).getByText("Manual").closest("li")!;
    expect(within(manual).queryByRole("button")).toBeNull();
    expect(manual).toHaveTextContent("Not on Nexus");
    const card = manual.firstElementChild!;
    expect(card).toHaveAttribute("aria-disabled", "true");
    expect(card.className.split(" ")).toContain("opacity-60");
    expect(focusables(dlg)).not.toContain(card);
  });

  // B2. Fails when a listed mod does not open its details over the collection, or Back loses the Mods tab or
  // strands focus instead of returning it to the mod's row.
  it("a listed mod opens its details; Back returns to the Mods tab with focus on the card", async () => {
    const { user } = await open("browse", {
      browse: collOnly,
      collection_info: infoWith([{ name: "SkyUI", version: "5.2", author: "schlangster", mod_id: 12604, size: 2048, optional: false }]),
    });
    await showCollections(user);
    await user.click(await screen.findByRole("button", { name: "Coll abc" }));
    let dlg = await screen.findByRole("dialog", { name: "Coll abc" });
    await user.click(await within(dlg).findByRole("tab", { name: "Mods (1)" }));
    const row = within(dlg).getByRole("button", { name: "SkyUI" });
    // Controller: A on the focused card opens it.
    act(() => row.focus());
    act(() => performNav({ type: "select" }));
    const mod = await screen.findByRole("dialog", { name: "SkyUI" });
    expect(mod).toHaveTextContent("12604");
    expect(mod).toHaveTextContent("schlangster");
    // RB in the mod's dialog switches its tabs only, not the collection's underneath.
    act(() => performNav({ type: "tab", delta: 1 }));
    expect(within(mod).getByRole("tab", { name: "About" })).toHaveAttribute("aria-selected", "true");
    expect(within(dlg).getByRole("tab", { name: "Mods (1)" })).toHaveAttribute("aria-selected", "true");
    act(() => performNav({ type: "back" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "SkyUI" })).toBeNull());
    dlg = screen.getByRole("dialog", { name: "Coll abc" });
    expect(within(dlg).getByRole("tab", { name: "Mods (1)" })).toHaveAttribute("aria-selected", "true");
    await waitFor(() => expect(document.activeElement).toBe(row));
  });
});

describe("Browse search (A5/A6)", () => {
  // Fails when the search goes back into the listing card instead of the header's top right (as in Games), loses
  // the shared wide search box, gamepad Y does not focus it, Enter does not search, or emptying it keeps the filter.
  it("sits top right in the header, Y focuses it, Enter searches, emptying it lists all again", async () => {
    // "sky" matches one mod, Mod 99; no words: the paged listing.
    const { t, user } = await open("browse", {
      browse: (a) => (reqOf(a).text ? { kind: "mods", total: 1, mods: [hit(99)] } : paged(a)),
    });
    await screen.findByText("Mod 1");
    const box = screen.getByRole("searchbox", { name: "Search" });
    const header = box.closest("header")!;
    expect(header).toContainElement(screen.getByRole("heading", { level: 1, name: "Browse" }));
    expect(screen.getByRole("search", { name: "Search Nexus" }).className.split(" ")).toEqual(expect.arrayContaining(["w-96", "max-w-full"]));
    expect(screen.queryByRole("button", { name: "Search" })).toBeNull();
    act(() => performNav({ type: "search" }));
    expect(document.activeElement).toBe(box);
    await user.type(box, "sky{Enter}");
    await waitFor(() => expect(lastReq(t)).toMatchObject({ text: "sky", offset: 0 }));
    expect(await screen.findByText("Mod 99")).toBeInTheDocument();
    expect(screen.queryByText("Mod 1")).toBeNull();
    const calls = t.callsOf("browse").length;
    await user.clear(box);
    // The unfiltered listing comes back (from cache: no new request needed).
    expect(await screen.findByText("Mod 1")).toBeInTheDocument();
    expect(screen.queryByText("Mod 99")).toBeNull();
    expect(t.callsOf("browse")).toHaveLength(calls);
  });
});

describe("Browse session restore (Browse-3)", () => {
  // Fails when leaving Browse forgets the tab, period, sort, search words, page, scroll or focused tile.
  it("restores tab, filters, search, page, scroll and focus on returning", async () => {
    const { t, user } = await open("browse", { browse: paged });
    await screen.findByText("Mod 1");
    await showCollections(user);
    await user.click(screen.getByRole("button", { name: "Month" }));
    await user.type(screen.getByLabelText("Search"), "lore{Enter}");
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

describe("Browse LT/RT paging", () => {
  // Fails when the triggers do not page Browse, page past the first/last page, or ask for another offset
  // than the Previous/Next buttons.
  it("RT next, LT previous; nothing past the first or last page", async () => {
    const { t } = await open("browse", { browse: paged });
    await screen.findByText("Mod 1");
    const pager = screen.getByRole("navigation", { name: "Mods pages" });
    const calls = () => t.callsOf("browse").length;

    let before = calls();
    act(() => performNav({ type: "page", delta: -1 }));
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(calls()).toBe(before);
    expect(pager).toHaveTextContent("Page 1 of 2");

    act(() => performNav({ type: "page", delta: 1 }));
    expect(await screen.findByText("Mod 25")).toBeInTheDocument();
    expect(lastReq(t).offset).toBe(24);
    expect(pager).toHaveTextContent("Page 2 of 2");

    before = calls();
    act(() => performNav({ type: "page", delta: 1 }));
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(calls()).toBe(before);
    expect(pager).toHaveTextContent("Page 2 of 2");

    act(() => performNav({ type: "page", delta: -1 }));
    expect(await screen.findByText("Mod 1")).toBeInTheDocument();
    expect(pager).toHaveTextContent("Page 1 of 2");
  });

  // Fails when a trigger with no results listed still changes the page (a request for an empty offset).
  it("does nothing when there are no results", async () => {
    const { t } = await open("browse", { browse: () => ({ kind: "mods", total: 0, mods: [] }) });
    await screen.findByText(/No mods were created/);
    const before = t.callsOf("browse").length;
    act(() => performNav({ type: "page", delta: 1 }));
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(t.callsOf("browse")).toHaveLength(before);
  });

  // Fails when the pager does not span the content width with Next pushed to the right edge.
  it("the pager spans the full width, Next on the right", async () => {
    await open("browse", { browse: paged });
    await screen.findByText("Mod 1");
    const pager = screen.getByRole("navigation", { name: "Mods pages" });
    expect(pager.className).toMatch(/(^|\s)-mx-section(\s|$)/);
    expect(within(pager).getByRole("button", { name: /Next/ }).className).toMatch(/\bml-auto\b/);
  });
});

describe("Browse page change: top and first tile", () => {
  /** jsdom has no layout: give <main> a settable scrollTop, scrolled down to 900. */
  const scrolled = () => {
    const main = document.querySelector("main")!;
    let top = 900;
    Object.defineProperty(main, "scrollTop", { configurable: true, get: () => top, set: (v: number) => (top = v) });
    return { top: () => top, down: () => (top = 900) };
  };
  const tile = (name: string) => screen.getByRole("button", { name });

  // Fails when a page change leaves the view scrolled down at the old position, or the highlight on the
  // pager (or a removed tile) instead of the new page's first result.
  it("Next/RT and Previous/LT scroll to the top and focus the first tile (mods)", async () => {
    const { user } = await open("browse", { browse: paged });
    await screen.findByText("Mod 1");
    const pos = scrolled();
    const pager = screen.getByRole("navigation", { name: "Mods pages" });

    await user.click(within(pager).getByRole("button", { name: /Next/ }));
    await waitFor(() => expect(document.activeElement).toBe(tile("Mod 25")));
    expect(pos.top()).toBe(0);

    pos.down();
    await user.click(within(pager).getByRole("button", { name: /Previous/ }));
    await waitFor(() => expect(document.activeElement).toBe(tile("Mod 1")));
    expect(pos.top()).toBe(0);

    pos.down();
    act(() => performNav({ type: "page", delta: 1 }));
    await waitFor(() => expect(document.activeElement).toBe(tile("Mod 25")));
    expect(pos.top()).toBe(0);

    pos.down();
    act(() => performNav({ type: "page", delta: -1 }));
    await waitFor(() => expect(document.activeElement).toBe(tile("Mod 1")));
    expect(pos.top()).toBe(0);
  });

  it("collections: RT focuses the first collection of the new page", async () => {
    const { user } = await open("browse", { browse: paged });
    await showCollections(user);
    await screen.findByText("Coll s1");
    const pos = scrolled();
    act(() => performNav({ type: "page", delta: 1 }));
    await waitFor(() => expect(document.activeElement).toBe(tile("Coll s25")));
    expect(pos.top()).toBe(0);
  });

  // Fails when a filter change (not a page change) moves the focus into the results.
  it("a filter change does not move the focus to the first tile", async () => {
    const { user } = await open("browse", { browse: paged });
    await screen.findByText("Mod 1");
    const month = screen.getByRole("button", { name: "Month" });
    await user.click(month);
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(document.activeElement).toBe(month);
  });

  // Fails when an empty next page throws or strands the focus on <body>: the listing falls back to the last
  // non-empty page and the focus lands on its first tile.
  it("an empty new page keeps the focus in the view", async () => {
    let total = 30;
    const { t, user } = await open("browse", {
      browse: (a) => {
        const r = reqOf(a);
        if (r.offset >= total) return { kind: "mods", total, mods: [] };
        return { ...(paged(a) as object), total };
      },
    });
    await screen.findByText("Mod 1");
    total = 24; // the listing shrank on the server: page 2 now comes back empty
    const next = within(screen.getByRole("navigation", { name: "Mods pages" })).getByRole("button", { name: /Next/ });
    await user.click(next);
    await waitFor(() => expect(document.activeElement).toBe(tile("Mod 1")));
    expect(t.callsOf("browse").some((c) => reqOf(c).offset === 24)).toBe(true);
    expect(screen.getByRole("navigation", { name: "Mods pages" })).toHaveTextContent(/^Previous\s*Page 1 of/);
  });
});

describe("Browse pager toolbar (sticky bottom)", () => {
  const place = (el: HTMLElement, x: number, y: number, w = 100, h = 40) => {
    el.getBoundingClientRect = () => ({ left: x, top: y, right: x + w, bottom: y + h, width: w, height: h, x, y, toJSON() {} }) as DOMRect;
  };
  // Fails when the pager scrolls away with the results (not sticky to the bottom of <main>), stops spanning the
  // card section's width, or stops being the section's last child (sticky would then pin it mid-card).
  it("is a sticky bottom toolbar across the results section, only on Browse", async () => {
    await open("browse", { browse: paged });
    await screen.findByText("Mod 1");
    const pager = screen.getByRole("navigation", { name: "Mods pages" });
    expect(pager.tagName).toBe("DIV");
    expect(pager.className).toMatch(/\bsticky\b/);
    expect(pager.className).toMatch(/(^|\s)bottom-\[calc\(var\(--main-pb,0px\)\*-1\)\](\s|$)/);
    expect(pager.className).toMatch(/(^|\s)-mx-section(\s|$)/);
    expect(pager.parentElement!.lastElementChild).toBe(pager);
    expect(pager.parentElement!.contains(screen.getByRole("list", { name: "Mods" }))).toBe(true);
    act(() => useUi.getState().setView("activity"));
    await screen.findByRole("heading", { name: "Activity" });
    expect(screen.queryByRole("navigation", { name: /pages$/ })).toBeNull();
  });

  // Fails when the always-on-screen pager steals Down from a tile whose next row is still visible above it,
  // or Up from the pager leaves the grid.
  it("pinned over the grid: Down goes row by row, Up from the pager returns to the grid", async () => {
    await open("browse", { browse: paged });
    await screen.findByText("Mod 1");
    const pager = screen.getByRole("navigation", { name: "Mods pages" });
    const prev = within(pager).getByRole("button", { name: /Previous/ });
    const next = within(pager).getByRole("button", { name: /Next/ });
    const tiles = within(screen.getByRole("list", { name: "Mods" })).getAllByRole("button");
    // Viewport: rows at 100 and 450, the pager pinned at 600, every later row below it (off screen).
    tiles.forEach((t, i) => place(t, 0, i === 0 ? 100 : i === 1 ? 450 : 900 + i * 300, 100, 120));
    place(prev, 0, 600);
    place(next, 300, 600);

    tiles[0].focus();
    act(() => performNav({ type: "move", dir: "down" }));
    expect(document.activeElement).toBe(tiles[1]);
    act(() => performNav({ type: "move", dir: "down" }));
    expect(document.activeElement).toBe(prev);
    act(() => performNav({ type: "move", dir: "up" }));
    expect(document.activeElement).toBe(tiles[1]);
  });
});

describe("Browse pager toolbar: pinned while scrolled up, page margin at the end", () => {
  const cls = (el: Element, c: string) => el.className.split(/\s+/).includes(c);
  // Fails when Browse loses the shared page margin at the end of the scroll (<main>'s pb-*), or the card
  // puts its own padding/border under the toolbar (a gap under it while it is pinned flush to <main>'s
  // scrollport: sticky ignores the scroll container's padding, so <main>'s pb only shows at the end).
  it("keeps <main>'s bottom padding; nothing of the card sits below the toolbar", async () => {
    await open("browse", { browse: paged });
    await screen.findByText("Mod 1");
    const main = document.querySelector("main")!;
    expect(main.className).toMatch(/\bpb-10\b/);
    expect(main.className).toMatch(/\bscroll-pb-32\b/);
    // WebKit insets the sticky bar by <main>'s padding-bottom: the pager pulls itself down by exactly that
    // padding, published by <main> as --main-pb with the same number as its pb-* class.
    const pbMatches = () => {
      const pb = /(^|\s)pb-(\d+)(\s|$)/.exec(main.className)![2];
      expect(main.className).toContain(`[--main-pb:calc(var(--spacing)*${pb})]`);
    };
    pbMatches();
    act(() => useUi.getState().notify("info", "hello"));
    await waitFor(() => expect(main.className).toMatch(/\bpb-28\b/));
    pbMatches();
    const pager = screen.getByRole("navigation", { name: "Mods pages" });
    const section = pager.parentElement!;
    expect(cls(section, "pb-0")).toBe(true);
    expect(cls(pager, "-mb-px")).toBe(true);
    expect(cls(pager, "bottom-[calc(var(--main-pb,0px)*-1)]")).toBe(true);
    expect(cls(pager, "bottom-0")).toBe(false);
    // The results card is the view's last block: nothing after it in the frame.
    expect(section.closest("[data-view-frame] > *")!.nextElementSibling).toBeNull();
    act(() => useUi.getState().setView("activity"));
    await screen.findByRole("heading", { name: "Activity" });
    expect(main.className).toMatch(/\bpb-28\b/);
  });
});

describe("Browse skeleton while loading", () => {
  /** A browse handler held at `gate` for the offsets in `slow` (all when omitted). */
  const gated = (slow?: (offset: number) => boolean, fail = false) => {
    let open!: () => void;
    const gate = new Promise<void>((r) => (open = r));
    const handler: Handler = async (a) => {
      if (!slow || slow(reqOf(a).offset)) await gate;
      if (fail) throw { message: "nexus down" };
      return paged(a);
    };
    return { handler, release: () => act(async () => open()) };
  };
  const skeletons = () => screen.queryAllByTestId("tile-skeleton");
  const results = () => screen.getByRole("list", { name: /^(Mods|Collections)$/, hidden: true });

  // Fails when the first load shows a text line / blank space instead of tile-shaped skeletons, the
  // skeletons can take focus or are exposed to assistive tech, or the region is not marked busy.
  it("first load: 24 aria-hidden, unfocusable skeleton tiles in a busy grid, then the real tiles", async () => {
    const g = gated();
    await open("browse", { browse: g.handler });
    await waitFor(() => expect(skeletons()).toHaveLength(24));
    const ul = results();
    expect(ul).toHaveAttribute("aria-busy", "true");
    expect(ul.className).toBe(MOD_GRID);
    for (const li of skeletons()) expect(li).toHaveAttribute("aria-hidden", "true");
    expect(focusables(ul)).toHaveLength(0);
    expect(screen.queryByText(/Loading mods/)).toBeNull();
    await g.release();
    expect(await screen.findByText("Mod 1")).toBeInTheDocument();
    expect(skeletons()).toHaveLength(0);
    expect(results()).not.toHaveAttribute("aria-busy");
  });

  // Fails when a page change keeps the previous page's tiles up while the next loads, the pager goes away
  // (or pages again) meanwhile, or focus ends anywhere but the new page's first tile.
  it("page change: skeletons replace the old page, the pager stays inert, then the first tile is focused", async () => {
    const g = gated((o) => o > 0);
    const { t, user } = await open("browse", { browse: g.handler });
    await screen.findByText("Mod 1");
    const pager = screen.getByRole("navigation", { name: "Mods pages" });
    const next = within(pager).getByRole("button", { name: /Next/ });
    await user.click(next);
    await waitFor(() => expect(skeletons()).toHaveLength(24));
    expect(screen.queryByText("Mod 1")).toBeNull();
    expect(results()).toHaveAttribute("aria-busy", "true");
    expect(pager).toBeInTheDocument();
    expect(pager).toHaveTextContent("Page 2 · loading");
    const calls = t.callsOf("browse").length;
    act(() => performNav({ type: "page", delta: 1 }));
    act(() => performNav({ type: "page", delta: -1 }));
    await user.click(next);
    expect(t.callsOf("browse")).toHaveLength(calls);
    await g.release();
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "Mod 25" })));
    expect(skeletons()).toHaveLength(0);
  });

  // Fails when RT/LT pressed on a tile drops focus to <body> as the tiles turn into skeletons.
  it("RT from a tile parks focus on Next while loading", async () => {
    const g = gated((o) => o > 0);
    await open("browse", { browse: g.handler });
    const tile = await screen.findByRole("button", { name: "Mod 3" });
    tile.focus();
    act(() => performNav({ type: "page", delta: 1 }));
    await waitFor(() => expect(skeletons()).toHaveLength(24));
    expect(document.activeElement).toBe(within(screen.getByRole("navigation", { name: "Mods pages" })).getByRole("button", { name: /Next/ }));
    await g.release();
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "Mod 25" })));
  });

  // Fails when a cached page flashes skeletons on the way back to it.
  it("no skeleton for a cached page", async () => {
    const { user } = await open("browse", { browse: paged });
    await screen.findByText("Mod 1");
    const pager = screen.getByRole("navigation", { name: "Mods pages" });
    await user.click(within(pager).getByRole("button", { name: /Next/ }));
    await screen.findByText("Mod 25");
    await user.click(within(pager).getByRole("button", { name: /Previous/ }));
    expect(skeletons()).toHaveLength(0);
    expect(screen.getByText("Mod 1")).toBeInTheDocument();
  });

  // Fails when a collections load shows mod-shaped skeletons (wrong grid / tile shape).
  it("collections skeletons use the collections grid", async () => {
    const g = gated((o) => o > 0);
    const { user } = await open("browse", { browse: g.handler });
    await screen.findByText("Mod 1");
    await showCollections(user);
    await screen.findByText("Coll s1");
    act(() => performNav({ type: "page", delta: 1 }));
    await waitFor(() => expect(skeletons()).toHaveLength(24));
    expect(results().className).toBe(RESULTS_GRID);
    expect(results()).toHaveAttribute("aria-label", "Collections");
  });

  // Fails when a failed load leaves the skeletons up instead of the error state.
  it("an error replaces the skeletons", async () => {
    const g = gated(undefined, true);
    await open("browse", { browse: g.handler });
    await waitFor(() => expect(skeletons()).toHaveLength(24));
    await g.release();
    expect(await screen.findByText(/Could not load mods: nexus down/)).toBeInTheDocument();
    expect(skeletons()).toHaveLength(0);
  });
});

describe("Browse pager keyboard reach", () => {
  const place = (el: HTMLElement, x: number, y: number, w = 100, h = 40) => {
    el.getBoundingClientRect = () => ({ left: x, top: y, right: x + w, bottom: y + h, width: w, height: h, x, y, toJSON() {} }) as DOMRect;
  };
  // Fails when the pager is its own landmark region for spatial nav: Down from the grid then skips past
  // Previous/Next to the next control in <main> (the "below" stand-in), so the arrows never reach the pager.
  it("Down from the last tile reaches the pager, Right moves Previous -> Next, Enter pages", async () => {
    const { t, user } = await open("browse", { browse: paged });
    const tile = await screen.findByRole("button", { name: "Mod 24" });
    const pager = screen.getByRole("navigation", { name: "Mods pages" });
    const prev = within(pager).getByRole("button", { name: /Previous/ });
    const next = within(pager).getByRole("button", { name: /Next/ });
    const below = document.createElement("button");
    below.textContent = "below";
    document.querySelector("main")!.appendChild(below);
    place(tile, 0, 100);
    place(prev, 0, 300);
    place(next, 300, 300);
    place(below, 0, 500);

    tile.focus();
    act(() => performNav({ type: "move", dir: "down" }));
    expect(document.activeElement).toBe(prev);
    act(() => performNav({ type: "move", dir: "right" }));
    expect(document.activeElement).toBe(next);
    await user.keyboard("{Enter}");
    expect(await screen.findByText("Mod 25")).toBeInTheDocument();
    expect(lastReq(t).offset).toBe(24);
  });
});

describe("Browse details About (T5)", () => {
  type About = { kind: "mod"; mod_id: number } | { kind: "collection"; slug: string };
  const aboutOf = (a: unknown) => (a as { request: { about?: About } }).request.about;
  /** Pages for listings; `describe` answers about requests. */
  const withAbout =
    (page: Handler, describe: (about: About) => unknown): Handler =>
    (a) => {
      const about = aboutOf(a);
      return about ? describe(about) : page(a);
    };
  const oneMod: Handler = () => ({ kind: "mods", total: 1, mods: [hit(7, { summary: "Short summary." })] });

  // Fails when the description is fetched with the listing (not on open), the about request does not name the
  // mod, or the BBCode shows as raw markup instead of formatted content in the About section.
  it("mod details fetch the description on open and render its BBCode", async () => {
    const { t, user } = await open("browse", {
      browse: withAbout(oneMod, () => ({ kind: "about", description: "[b]Bold intro[/b]\n<br />[list][*]Feature[/list]" })),
    });
    const card = await screen.findByRole("button", { name: "Mod 7" });
    expect(t.callsOf("browse").filter((a) => aboutOf(a))).toHaveLength(0);
    await user.click(card);
    const dlg = await screen.findByRole("dialog", { name: "Mod 7" });
    // Fetched when About opens, not with the Details tab.
    expect(t.callsOf("browse").filter((a) => aboutOf(a))).toHaveLength(0);
    await clickTab(user, dlg, "About");
    const about = within(dlg).getByRole("region", { name: "About" });
    expect(await within(about).findByText("Bold intro")).toHaveProperty("tagName", "STRONG");
    expect(within(about).getByRole("listitem")).toHaveTextContent("Feature");
    expect(about).not.toHaveTextContent("[b]");
    expect(about).not.toHaveTextContent("Short summary.");
    expect(t.callsOf("browse").map(aboutOf).filter(Boolean)).toEqual([{ kind: "mod", mod_id: 7 }]);
  });

  // Fails when a link in the description navigates the webview or skips the backend open_url command.
  it("description links open through open_url", async () => {
    const { t, user } = await open("browse", {
      browse: withAbout(oneMod, () => ({ kind: "about", description: "[url=https://smapi.io]SMAPI[/url]" })),
      open_url: (a) => (a as { url: string }).url,
    });
    await user.click(await screen.findByRole("button", { name: "Mod 7" }));
    const dlg = await screen.findByRole("dialog", { name: "Mod 7" });
    await clickTab(user, dlg, "About");
    const link = await within(dlg).findByRole("link", { name: "SMAPI" });
    await user.click(link);
    await waitFor(() => expect(t.callsOf("open_url")).toEqual([{ url: "https://smapi.io/" }]));
  });

  // Fails when an empty description or a failed fetch leaves About blank (no summary fallback) or hides the error.
  it("falls back to the summary when the description is empty", async () => {
    const { user } = await open("browse", { browse: withAbout(oneMod, () => ({ kind: "about", description: null })) });
    await user.click(await screen.findByRole("button", { name: "Mod 7" }));
    const dlg = await screen.findByRole("dialog", { name: "Mod 7" });
    await clickTab(user, dlg, "About");
    const about = within(dlg).getByRole("region", { name: "About" });
    expect(await within(about).findByText("Short summary.")).toBeInTheDocument();
    expect(within(dlg).queryByText("Loading description…")).toBeNull();
  });

  it("shows the error and the summary when the description fails to load", async () => {
    const { user } = await open("browse", {
      browse: withAbout(oneMod, () => {
        throw { message: "Nexus down" };
      }),
    });
    await user.click(await screen.findByRole("button", { name: "Mod 7" }));
    const dlg = await screen.findByRole("dialog", { name: "Mod 7" });
    await clickTab(user, dlg, "About");
    const about = within(dlg).getByRole("region", { name: "About" });
    expect(await within(about).findByText(/Could not load the description: Nexus down/)).toBeInTheDocument();
    expect(within(about).getByText("Short summary.")).toBeInTheDocument();
  });

  // Fails when the loading state is missing while the description is on its way.
  it("shows a loading state while fetching", async () => {
    let release!: (v: unknown) => void;
    const { user } = await open("browse", {
      browse: withAbout(oneMod, () => new Promise((r) => (release = r))),
    });
    await user.click(await screen.findByRole("button", { name: "Mod 7" }));
    const dlg = await screen.findByRole("dialog", { name: "Mod 7" });
    await clickTab(user, dlg, "About");
    expect(await within(dlg).findByText("Loading description…")).toBeInTheDocument();
    act(() => release({ kind: "about", description: "Ready text" }));
    expect(await within(dlg).findByText("Ready text")).toBeInTheDocument();
    expect(within(dlg).queryByText("Loading description…")).toBeNull();
  });

  // Fails when collection details do not ask for the collection's description by slug, or its Markdown shows raw.
  it("collection details render the Markdown description in About", async () => {
    const { t, user } = await open("browse", {
      browse: withAbout(collOnly, () => ({ kind: "about", description: "### Install notes\n\n**Read me** first.\n\n<script>alert(1)</script>" })),
      collection_info: infoWith([]),
    });
    await showCollections(user);
    await user.click(await screen.findByRole("button", { name: "Coll abc" }));
    const dlg = await screen.findByRole("dialog", { name: "Coll abc" });
    await clickTab(user, dlg, "About");
    const about = within(dlg).getByRole("region", { name: "About" });
    expect(await within(about).findByRole("heading", { name: "Install notes" })).toBeInTheDocument();
    expect(within(about).getByText("Read me").tagName).toBe("STRONG");
    expect(about).toHaveTextContent("<script>alert(1)</script>");
    expect(dlg.querySelector("script")).toBeNull();
    expect(t.callsOf("browse").map(aboutOf).filter(Boolean)).toEqual([{ kind: "collection", slug: "abc" }]);
  });
});
