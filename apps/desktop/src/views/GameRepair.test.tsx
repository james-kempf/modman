// SPDX-License-Identifier: GPL-3.0-or-later
// Poisoned-backup cleanup, quarantine restore and restage, against the demo
// backend only (no Tauri): a file-changing call is made only after the
// dialog is confirmed, and always with confirmed=true.
import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { resetBridge, startBridge } from "../app/bridge";
import { makeQueryClient } from "../app/queries";
import { resetUi, useUi } from "../app/store";
import { setTransport } from "../ipc/transport";
import type { QuarantineManifest, RestoreReport } from "../ipc/types";
import { createDemoBackend } from "../mock/demo";
import { focusables } from "../nav/spatial";
import { GameRepairCard } from "./GameRepair";

const GAME = "skyrimspecialedition";

async function mount() {
  resetUi();
  resetBridge();
  const t = createDemoBackend();
  setTransport(t);
  const qc = makeQueryClient();
  await startBridge(qc);
  render(
    <QueryClientProvider client={qc}>
      <GameRepairCard />
    </QueryClientProvider>,
  );
  await screen.findByRole("list", { name: "Confirmed poisoned backups" });
  return { t, user: userEvent.setup() };
}

const rows = (name: string) => within(screen.getByRole("list", { name })).getAllByRole("listitem");

describe("GameRepair (demo backend)", () => {
  // Fails when cancelling the dialog still quarantines (the GUI must never act without an explicit yes).
  it("cancel: no backups_clean call and the list is unchanged", async () => {
    const { t, user } = await mount();
    await user.click(screen.getByRole("button", { name: "Quarantine…" }));
    const dlg = await screen.findByRole("dialog", { name: "Quarantine poisoned backups?" });
    // Cancel has focus first: an accidental Enter does not confirm.
    expect(within(dlg).getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.click(within(dlg).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(t.callsOf("backups_clean")).toEqual([]);
    expect(rows("Confirmed poisoned backups")).toHaveLength(2);
  });

  // Fails when confirm does not pass confirmed=true, quarantines ambiguous entries without the opt-in, or the view does not refresh.
  it("confirm: quarantines the confirmed entries with confirmed=true; the list shrinks and a quarantine appears", async () => {
    const { t, user } = await mount();
    await user.click(screen.getByRole("button", { name: "Quarantine…" }));
    const dlg = await screen.findByRole("dialog");
    await user.click(within(dlg).getByRole("button", { name: "Quarantine" }));
    await waitFor(() => expect(t.callsOf("backups_clean")).toEqual([{ game: GAME, include_ambiguous: false, confirmed: true }]));
    await waitFor(() => expect(screen.queryByRole("list", { name: "Confirmed poisoned backups" })).toBeNull());
    expect(rows("Ambiguous poisoned backups")).toHaveLength(1);
    const qs = await screen.findByRole("list", { name: "Quarantines" });
    expect(within(qs).getAllByRole("listitem")).toHaveLength(1);
    expect(within(qs).getByText("2 item(s)", { exact: false })).toBeInTheDocument();
  });

  // Fails when the ambiguous opt-in is not passed through.
  it("the ambiguous opt-in sends include_ambiguous=true and empties the list", async () => {
    const { t, user } = await mount();
    await user.click(screen.getByRole("switch", { name: "Include ambiguous" }));
    await user.click(screen.getByRole("button", { name: "Quarantine…" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Quarantine" }));
    await waitFor(() => expect(t.callsOf("backups_clean")).toEqual([{ game: GAME, include_ambiguous: true, confirmed: true }]));
    await screen.findByText(/^None: every held backup differs/);
  });

  // Fails when restore skips the dialog, omits confirmed=true, or the entries do not come back.
  it("restore: confirm puts the quarantined entries back", async () => {
    const { t, user } = await mount();
    await user.click(screen.getByRole("button", { name: "Quarantine…" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Quarantine" }));
    const restoreBtn = await screen.findByRole("button", { name: /^Restore quarantine / });
    await user.click(restoreBtn);
    const dlg = await screen.findByRole("dialog", { name: "Restore this quarantine?" });
    expect(t.callsOf("quarantine_restore")).toEqual([]);
    await user.click(within(dlg).getByRole("button", { name: "Restore" }));
    const [q] = await t.invoke<QuarantineManifest[]>("quarantines_list", { game: GAME });
    await waitFor(() => expect(t.callsOf("quarantine_restore")).toEqual([{ game: GAME, id: q.id, confirmed: true }]));
    await waitFor(() => expect(rows("Confirmed poisoned backups")).toHaveLength(2));
    expect(within(screen.getByRole("list", { name: "Quarantines" })).getByText(/^restored/)).toBeInTheDocument();
  });

  // Fails when restage does not surface the poisoned-backup block, or does not mark the deploy due once it runs.
  it("restage shows the block message, then runs after cleanup and marks the deploy due", async () => {
    const { t, user } = await mount();
    await user.click(screen.getByRole("button", { name: "Restage" }));
    expect(await screen.findByText(/^Restage blocked: 2 held backup\(s\)/)).toBeInTheDocument();
    expect(useUi.getState().deployDue[GAME]).toBeUndefined();
    await user.click(screen.getByRole("button", { name: "Quarantine…" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Quarantine" }));
    await waitFor(() => expect(screen.queryByText(/^Restage blocked/)).toBeNull());
    await user.click(screen.getByRole("button", { name: "Restage" }));
    await waitFor(() => expect(useUi.getState().deployDue[GAME]).toBeDefined());
    expect(t.callsOf("restage").at(-1)).toEqual({ game: GAME, ids: [], all: false, ignore_poisoned: false });
  });
});

describe("GameRepair navigation", () => {
  // Fails when a control or entry is not reachable by keyboard/gamepad spatial navigation or lacks an accessible name.
  it("every control and entry is a spatial-nav focusable with a name", async () => {
    await mount();
    const nav = focusables();
    for (const el of [
      screen.getByRole("switch", { name: "Include ambiguous" }),
      screen.getByRole("button", { name: "Quarantine…" }),
      screen.getByRole("button", { name: "Restage" }),
      ...rows("Confirmed poisoned backups"),
      ...rows("Ambiguous poisoned backups"),
    ])
      expect(nav).toContain(el);
  });
});

describe("demo backend: cleanup contract", () => {
  // Fails when the mock acts without confirmed=true (it must refuse like the core).
  it("refuses unconfirmed cleanup and restore; restore returns the entries", async () => {
    const t = createDemoBackend();
    await expect(t.invoke("backups_clean", { game: GAME, include_ambiguous: false, confirmed: false })).rejects.toHaveProperty("message");
    expect(await t.invoke<unknown[]>("backups_poisoned", { game: GAME })).toHaveLength(3);
    const r = await t.invoke<{ id: string; backups: number }>("backups_clean", { game: GAME, include_ambiguous: true, confirmed: true });
    expect(r.backups).toBe(3);
    expect(await t.invoke<unknown[]>("backups_poisoned", { game: GAME })).toHaveLength(0);
    await expect(t.invoke("quarantine_restore", { game: GAME, id: r.id, confirmed: false })).rejects.toHaveProperty("message");
    const back = await t.invoke<RestoreReport>("quarantine_restore", { game: GAME, id: r.id, confirmed: true });
    expect(back).toEqual({ id: r.id, backups: 3, game_files: 0 });
    expect(await t.invoke<unknown[]>("backups_poisoned", { game: GAME })).toHaveLength(3);
  });
});
