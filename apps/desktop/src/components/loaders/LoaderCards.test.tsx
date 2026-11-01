// SPDX-License-Identifier: GPL-3.0-or-later
// The Requirements loader card: states come from loader_status, every op
// goes through the consent dialog, launch options (copy, ack, apply with an
// account picker, disabled while Steam runs), the runtime picker, the
// offered rollback, junk cleanup and typed errors.
import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useUi } from "../../app/store";
import type { ApplyOutcome, LoaderCard, LoaderStatus } from "../../ipc/types";
import type { Handler } from "../../mock/fake";
import { demoPreview } from "../../mock/loaders";
import { setup } from "../../test/harness";

function card(status: LoaderStatus, over: Partial<LoaderCard> = {}): LoaderCard {
  return {
    key: "smapi",
    name: "SMAPI",
    nexus_mod_id: 2400,
    status,
    runtime: "native",
    runtime_override: null,
    launch: { desired: null, acked: null, applied: null },
    steam_running: false,
    last_run: null,
    recovered: null,
    ...over,
  };
}

const installed: LoaderStatus = { state: "installed", install_id: 3, version: "4.5.2" };

async function open(c: LoaderCard, handlers: Record<string, Handler> = {}) {
  const env = await setup({
    loader_status: () => [c],
    loader_preview: (a) => demoPreview({ op: a!.op as never, loader_key: "smapi" }),
    loader_preview_reopen: () => demoPreview({ loader_key: "smapi", preview_id: "p2", plan_hash: "h2" }),
    ...handlers,
  });
  env.renderApp();
  act(() => useUi.getState().setView("requirements"));
  const region = await screen.findByRole("region", { name: "SMAPI" });
  return { ...env, region, user: userEvent.setup() };
}

describe("loader card", () => {
  // Fails when a status from loader_status is shown wrongly (badge, version, the ops it offers).
  it("renders each state from loader_status with its ops", async () => {
    const a = await open(card(installed));
    expect(a.region).toHaveTextContent("installed");
    expect(a.region).toHaveTextContent("4.5.2");
    for (const b of ["Upgrade", "Repair", "Uninstall"]) expect(within(a.region).getByRole("button", { name: b })).toBeInTheDocument();
    expect(within(a.region).queryByRole("button", { name: "Install" })).toBeNull();
    a.qc.setQueryData(["loaders", "skyrimspecialedition"], [card({ state: "drifted", install_id: 3, version: "4.5.2", reason: "StardewValley is vanilla again" })]);
    expect(await within(a.region).findByText(/StardewValley is vanilla again/)).toBeInTheDocument();
    expect(within(a.region).getByText("needs repair")).toBeInTheDocument();
    a.qc.setQueryData(["loaders", "skyrimspecialedition"], [card({ state: "manual_detected", evidence: ["StardewModdingAPI.dll"] })]);
    expect(await within(a.region).findByRole("button", { name: "Adopt" })).toBeInTheDocument();
    a.qc.setQueryData(["loaders", "skyrimspecialedition"], [card({ state: "not_installed" })]);
    expect(await within(a.region).findByRole("button", { name: "Install" })).toBeInTheDocument();
  });

  // Fails when an op runs (or previews nothing) instead of opening the consent dialog with its preview.
  it("Repair and Uninstall preview and open the consent dialog; nothing runs", async () => {
    for (const op of ["repair", "uninstall"] as const) {
      const { t, region, user } = await open(card(installed));
      await user.click(within(region).getByRole("button", { name: op === "repair" ? "Repair" : "Uninstall" }));
      const dialog = await screen.findByRole("dialog");
      expect(dialog).toHaveAccessibleName(new RegExp(`^${op === "repair" ? "Repair" : "Uninstall"} SMAPI`));
      expect(t.callsOf("loader_preview")).toEqual([{ game: "skyrimspecialedition", op, loader_key: "smapi", archive: null, unverified_ack: false }]);
      expect(t.callsOf("loader_run")).toEqual([]);
      cleanup();
    }
  });

  // Fails when Install / Upgrade / Adopt run the loader instead of opening the dialog with the pending preview.
  it("Install, Upgrade and Adopt download from Nexus and open the consent dialog", async () => {
    const cases = [
      [card({ state: "not_installed" }), "Install", "install"],
      [card(installed), "Upgrade", "upgrade"],
      [card({ state: "manual_detected", evidence: ["StardewModdingAPI.dll"] }), "Adopt", "adopt"],
    ] as const;
    for (const [c, label, op] of cases) {
      const { t, region, user } = await open(c, {
        install: () => ({
          game: "skyrimspecialedition",
          installed: null,
          loader: { loader_key: "smapi", preview_id: "p0", plan_hash: "h0", op, version: "4.5.2" },
          collection: null,
          after: { needs_deploy: false, deployed: null },
        }),
        loader_preview_reopen: () => demoPreview({ loader_key: "smapi", op, preview_id: "p2", plan_hash: "h2" }),
      });
      await user.click(within(region).getByRole("button", { name: label }));
      expect(await screen.findByRole("dialog")).toHaveAccessibleName(new RegExp(`^${label} SMAPI`));
      expect(t.callsOf("install")[0]).toMatchObject({ target: "2400", game: "skyrimspecialedition" });
      expect(t.callsOf("loader_preview_reopen")).toEqual([{ preview_id: "p0", unverified_ack: false }]);
      expect(t.callsOf("loader_run")).toEqual([]);
      cleanup();
    }
  });

  // Fails when Copy does not put the string on the clipboard or "I've set it" does not record the ack.
  it("launch options: Copy and I've set it", async () => {
    const { t, region, user } = await open(card(installed, { launch: { desired: "%command% --smapi", acked: null, applied: null } }));
    expect(region).toHaveTextContent("not set yet");
    await user.click(within(region).getByRole("button", { name: "Copy" }));
    expect(await navigator.clipboard.readText()).toBe("%command% --smapi");
    await user.click(within(region).getByRole("button", { name: "I've set it" }));
    await waitFor(() => expect(t.callsOf("launch_options_ack")).toEqual([{ game: "skyrimspecialedition" }]));
  });

  // Fails when Apply can write localconfig.vdf while Steam runs (Steam would overwrite it on exit).
  it("Apply is disabled while Steam is running", async () => {
    const { region } = await open(card(installed, { steam_running: true, launch: { desired: "x", acked: null, applied: null } }));
    expect(within(region).getByRole("button", { name: "Apply to Steam" })).toBeDisabled();
    expect(region).toHaveTextContent("Close Steam to apply");
  });

  // Fails when several accounts write to a guessed one instead of showing the picker (preselecting the default).
  it("Apply shows the account picker for several accounts and applies for the chosen one", async () => {
    const acct = (id: number, name: string) => ({ account_id: id, steam_id: String(id), account_name: name, persona_name: name, timestamp: null, localconfig: "/x" });
    const { t, region, user } = await open(card(installed, { launch: { desired: "x", acked: null, applied: null } }), {
      launch_options_apply: (a): ApplyOutcome =>
        a!.account == null
          ? { kind: "choose_account", accounts: [acct(1, "alice"), acct(2, "bob")], default: 2 }
          : { kind: "applied", account_id: 2, path: "/x", value: "x", backup: null },
    });
    await user.click(within(region).getByRole("button", { name: "Apply to Steam" }));
    const picker = await within(region).findByRole("button", { name: /Steam account/ });
    expect(picker).toHaveTextContent("bob");
    await user.click(within(region).getByRole("button", { name: "Apply for this account" }));
    await waitFor(() => expect(t.callsOf("launch_options_apply")).toEqual([{ game: "skyrimspecialedition", account: null }, { game: "skyrimspecialedition", account: 2 }]));
  });

  // Fails when an Unknown runtime offers no way out, or the picker does not store the override.
  it("Unknown runtime: the picker calls game_runtime_set", async () => {
    const { t, region, user } = await open(card({ state: "not_installed" }, { runtime: "unknown" }), { game_runtime_set: () => null });
    expect(region).toHaveTextContent("cannot tell whether the game runs natively or through Proton");
    await user.click(within(region).getByRole("button", { name: /Game runtime/ }));
    await user.click(screen.getByRole("option", { name: "Proton (Windows version)" }));
    await waitFor(() => expect(t.callsOf("game_runtime_set")).toEqual([{ game: "skyrimspecialedition", runtime: "proton" }]));
  });

  // Fails when an interrupted run offers no rollback decision, or the buttons call the wrong command.
  it("an interrupted run offers Roll back and Keep files", async () => {
    const c = card({ state: "busy", install_id: 3, status: "interrupted" }, {
      recovered: { run_id: 5, install_id: 3, action: { kind: "rollback_offered", killed: true } },
    });
    const { t, region, user } = await open(c, { loader_rollback: () => [], loader_decline_rollback: () => null });
    expect(region).toHaveTextContent("Vessel stopped the leftover installer");
    await user.click(within(region).getByRole("button", { name: "Roll back" }));
    await waitFor(() => expect(t.callsOf("loader_rollback")).toEqual([{ install_id: 3 }]));
    await user.click(within(region).getByRole("button", { name: "Keep files" }));
    await waitFor(() => expect(t.callsOf("loader_decline_rollback")).toEqual([{ install_id: 3 }]));
  });

  // Fails when leftover loader archives are listed but cannot be removed through loader_junk_remove.
  it("the junk-cleanup prompt calls loader_junk_remove", async () => {
    const { t, user } = await open(card(installed), {
      loader_junk_scan: () => [{ mod_id: 41, uid: "u", name: "SMAPI 4.1.0 installer", loader_key: "smapi" }],
      loader_junk_remove: () => 1,
    });
    const junk = await screen.findByRole("region", { name: "Loader leftovers" });
    expect(junk).toHaveTextContent("SMAPI 4.1.0 installer");
    await user.click(within(junk).getByRole("button", { name: "Remove them" }));
    await waitFor(() => expect(t.callsOf("loader_junk_remove")).toEqual([{ game: "skyrimspecialedition", mod_ids: [41] }]));
  });

  // Fails when a typed error shows only its raw message instead of the mapped title and action.
  it("a typed error renders its mapped action", async () => {
    const { region, user } = await open(card(installed), {
      loader_preview: () => Promise.reject({ message: "cannot tell whether ...", code: "loader_unknown_runtime" }),
    });
    await user.click(within(region).getByRole("button", { name: "Repair" }));
    expect(await within(region).findByText(/Native or Proton\?/)).toBeInTheDocument();
    await user.click(within(region).getByRole("button", { name: "Choose runtime" }));
    expect(within(region).getByRole("button", { name: /Game runtime/ })).toBeInTheDocument();
  });

  // Fails when View log shows nothing or asks for the wrong run.
  it("View log reads the last run's redacted log", async () => {
    const { t, region, user } = await open(card(installed, { last_run: 9 }), { loader_log_get: () => "line one\n" });
    await user.click(within(region).getByRole("button", { name: "View log" }));
    expect(await screen.findByLabelText("Installer log")).toHaveTextContent("line one");
    expect(t.callsOf("loader_log_get")).toEqual([{ run_id: 9 }]);
  });
});
