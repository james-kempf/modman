// SPDX-License-Identifier: GPL-3.0-or-later
// The loader consent dialog: what it must show before anything runs, the
// unverified acknowledgement (a re-requested preview, since the flag is in
// the hashed plan), confirm/cancel semantics, the live log and typed errors.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useUi } from "../../app/store";
import { demoPreview } from "../../mock/loaders";
import type { Handler } from "../../mock/fake";
import { setup } from "../../test/harness";
import type { Preview, RunReport } from "../../ipc/types";
import { LoaderConsentHost } from "./LoaderConsent";

const smapi = (over: Partial<Preview> = {}) =>
  demoPreview({
    preview_id: "p1",
    plan_hash: "h1",
    loader_key: "smapi",
    version: "4.5.2",
    runtime: { kind: "native" },
    commands: [
      {
        program: "/data/loaders/scratch/p1/internal/linux/SMAPI.Installer",
        args: ["--install", "--no-prompt", "--game-path", "/games/Stardew Valley"],
        cwd: "/data/loaders/scratch/p1",
        env: [
          ["PATH", "/usr/bin:/bin"],
          ["SMAPI_NO_TERMINAL", "true"],
        ],
        cmd_line: null,
      },
    ],
    external_side_effects: ["~/.smapi (moved or created)"],
    ...over,
  });

const report: RunReport = {
  install_id: 1,
  run_id: 7,
  outcome: { kind: "succeeded" },
  status: "installed",
  version: "4.5.2",
  log_dir: "/d",
  warnings: [],
  launch_desired: null,
};

async function open(preview: Preview | null, handlers: Record<string, Handler> = {}, pending = false) {
  const env = await setup({ loader_run: () => report, ...handlers });
  env.renderIn(<LoaderConsentHost />);
  const before = env.t.calls.length;
  act(() =>
    useUi.getState().openLoaderConsent(
      pending
        ? { game: "stardewvalley", pending: { loader_key: "smapi", preview_id: "p0", plan_hash: "h0", op: "install", version: "4.5.2" } }
        : { game: "stardewvalley", preview: preview! },
    ),
  );
  const dialog = await screen.findByRole("dialog");
  return { ...env, before, dialog, user: userEvent.setup() };
}

describe("loader consent dialog", () => {
  // Fails when the user could confirm without seeing the no-sandbox notice, the exact argv or the unrecoverable effects.
  it("shows the permissions notice, every argument as a list item and the unrecoverable side effects", async () => {
    const { dialog } = await open(smapi());
    expect(within(dialog).getByText("Runs with your user permissions (no sandbox)")).toBeInTheDocument();
    const args = within(within(dialog).getByRole("list", { name: "Arguments" })).getAllByRole("listitem");
    expect(args.map((a) => a.textContent)).toEqual(["--install", "--no-prompt", "--game-path", "/games/Stardew Valley"]);
    expect(within(dialog).getByText("/data/loaders/scratch/p1/internal/linux/SMAPI.Installer")).toBeInTheDocument();
    expect(within(dialog).getByText("SMAPI_NO_TERMINAL=true")).toBeInTheDocument();
    const fx = within(dialog).getByRole("list", { name: "Side effects outside the game folder" });
    expect(fx).toHaveTextContent("unrecoverable");
    expect(fx).toHaveTextContent("~/.smapi (moved or created)");
    expect(within(dialog).getByText("verified source")).toBeInTheDocument();
    expect(within(dialog).getByText("Stopped after 5 min")).toBeInTheDocument();
  });

  // Fails when Confirm sends anything but the reviewed token, or the live log panel ignores loader-log events.
  it("Confirm calls loader_run with the preview id and hash and shows the live log", async () => {
    let release!: (r: RunReport) => void;
    const { t, dialog, user } = await open(smapi(), { loader_run: () => new Promise<RunReport>((r) => (release = r)) });
    await user.click(within(dialog).getByRole("button", { name: "Install" }));
    expect(t.callsOf("loader_run")).toEqual([{ preview_id: "p1", plan_hash: "h1" }]);
    act(() => t.emit("vessel:loader-log", { preview_id: "p1", step: 1, text: "Installing SMAPI...\n" }));
    act(() => t.emit("vessel:loader-log", { preview_id: "other", step: 1, text: "not ours\n" }));
    const log = within(dialog).getByRole("log", { name: "Installer output" });
    expect(log).toHaveTextContent("Installing SMAPI...");
    expect(log).not.toHaveTextContent("not ours");
    await act(async () => release(report));
    expect(await within(dialog).findByText(/Done: version 4.5.2/)).toBeInTheDocument();
  });

  // Fails when Cancel previews, discards or runs anything (the preview simply expires).
  it("Cancel calls nothing and closes", async () => {
    const { t, before, dialog, user } = await open(smapi());
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(t.calls.length).toBe(before);
  });

  // Fails when an unverified archive can be confirmed without the box, or the box does not re-request the preview with the ack.
  it("unverified: confirm waits for the box, which re-requests the preview with unverified_ack", async () => {
    const unverified = smapi({ provenance: { kind: "unverified", origin: { kind: "local", path: "/dl/SMAPI.zip" }, reason: "a local file" } });
    const { t, dialog, user } = await open(unverified, {
      loader_preview_reopen: () => ({ ...unverified, preview_id: "p2", plan_hash: "h2" }),
    });
    const confirm = within(dialog).getByRole("button", { name: "Install" });
    expect(confirm).toBeDisabled();
    await user.click(confirm);
    expect(t.callsOf("loader_run")).toEqual([]);
    const box = within(dialog).getByRole("checkbox", { name: /source is unverified/ });
    expect(box).toHaveAttribute("aria-checked", "false");
    await user.click(box);
    await waitFor(() => expect(box).toHaveAttribute("aria-checked", "true"));
    expect(t.callsOf("loader_preview_reopen")).toEqual([{ preview_id: "p1", unverified_ack: true }]);
    await user.click(within(dialog).getByRole("button", { name: "Install" }));
    expect(t.callsOf("loader_run")).toEqual([{ preview_id: "p2", plan_hash: "h2" }]);
  });

  // Fails when a pending preview (install / nxm / collection) is shown from its bare token instead of in full.
  it("a pending preview is fetched in full (no ack) before it can be confirmed", async () => {
    const { t, dialog } = await open(null, { loader_preview_reopen: () => smapi({ preview_id: "p9", plan_hash: "h9" }) }, true);
    expect(await within(dialog).findByText("Runs with your user permissions (no sandbox)")).toBeInTheDocument();
    expect(t.callsOf("loader_preview_reopen")).toEqual([{ preview_id: "p0", unverified_ack: false }]);
    expect(t.callsOf("loader_run")).toEqual([]);
  });

  // Fails when Tab escapes the dialog or a control has no accessible name (keyboard / screen reader users).
  it("traps focus and labels its controls", async () => {
    const unverified = smapi({ provenance: { kind: "unverified", origin: { kind: "local", path: "/x" }, reason: "a local file" } });
    const { dialog, user } = await open(unverified);
    expect(dialog).toHaveAccessibleName("Install SMAPI 4.5.2");
    expect(dialog).toContainElement(document.activeElement as HTMLElement);
    expect(document.activeElement).toHaveAccessibleName("Cancel");
    for (let i = 0; i < 8; i++) {
      await user.tab();
      expect(dialog).toContainElement(document.activeElement as HTMLElement);
    }
    for (const el of within(dialog).getAllByRole("button")) expect(el).toHaveAccessibleName();
    expect(within(dialog).getByRole("region", { name: "Command 1" })).toHaveAttribute("tabindex", "0");
  });

  // Fails when a typed run error shows a raw message instead of its mapped title and explanation.
  it("renders a typed run error through loaderErrors", async () => {
    const { dialog, user } = await open(smapi(), {
      loader_run: () => Promise.reject({ message: "the game is running (pid 7); close it first", code: "loader_game_running" }),
    });
    await user.click(within(dialog).getByRole("button", { name: "Install" }));
    expect(await within(dialog).findByText(/Game is running\./)).toBeInTheDocument();
    expect(within(dialog).getByText(/Close the game, then try again/)).toBeInTheDocument();
  });
});
