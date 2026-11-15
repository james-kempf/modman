// SPDX-License-Identifier: GPL-3.0-or-later
import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { Prompt } from "../../ipc/types";
import { installKeyboardNav } from "../../nav/actions";
import type { FakeTransport } from "../../mock/fake";
import { setup } from "../../test/harness";
import { PromptHost, fomodInitial, fomodProblem } from "./PromptDialog";

let next = 100;
async function ask(prompt: Prompt) {
  const env = await setup();
  env.renderIn(<PromptHost />);
  const id = ++next;
  act(() => env.t.emit("vessel:prompt", { id, job: 1, prompt }));
  await screen.findByRole("dialog");
  return { ...env, id, user: userEvent.setup() };
}
const answered = (t: FakeTransport) => waitFor(() => expect(t.callsOf("prompt_answer")).toHaveLength(1)).then(() => t.callsOf("prompt_answer")[0]);
const gone = () => waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

describe("prompt dialogs answer via prompt_answer", () => {
  it("confirm", async () => {
    const { t, id, user } = await ask({ type: "confirm", question: "Purge Skyrim?", default_yes: false });
    expect(screen.getByText("Purge Skyrim?")).toBeInTheDocument();
    // default_yes=false focuses No.
    expect(document.activeElement).toHaveTextContent("No");
    await user.click(screen.getByRole("button", { name: "Yes" }));
    expect(await answered(t)).toEqual({ id, answer: { type: "confirm", yes: true } });
    await gone();
  });

  it("text", async () => {
    const { t, id, user } = await ask({ type: "text", question: "Profile name", default: "main" });
    const input = screen.getByLabelText("Profile name");
    expect(input).toHaveValue("main");
    await user.clear(input);
    await user.type(input, "deck{Enter}");
    expect(await answered(t)).toEqual({ id, answer: { type: "text", value: "deck" } });
  });

  it("choose_file", async () => {
    const f = (file_id: number, name: string) => ({
      file_id,
      name,
      version: "1",
      category_name: "MAIN",
      file_name: `${name}.7z`,
      size_in_bytes: 10,
      uploaded_timestamp: null,
      is_primary: file_id === 1,
    });
    const { t, id, user } = await ask({ type: "choose_file", files: [f(1, "Main"), f(2, "Lite")], default: 0 });
    expect(screen.getByRole("radio", { name: /Main/ })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("radio", { name: /Lite/ }));
    await user.click(screen.getByRole("button", { name: "Install this file" }));
    expect(await answered(t)).toEqual({ id, answer: { type: "choice", index: 1 } });
  });

  it("choose_optional", async () => {
    const { t, id, user } = await ask({
      type: "choose_optional",
      collection: "Lorerim",
      mods: [
        { name: "A", version: null, size: 1 },
        { name: "B", version: "2", size: 2 },
        { name: "C", version: null, size: 3 },
      ],
    });
    await user.click(screen.getByRole("checkbox", { name: /C/ }));
    await user.click(screen.getByRole("checkbox", { name: /A/ }));
    await user.click(screen.getByRole("button", { name: /Install 2 optional/ }));
    expect(await answered(t)).toEqual({ id, answer: { type: "choices", indices: [0, 2] } });
  });

  it("manual_download", async () => {
    const { t, id, user } = await ask({
      type: "manual_download",
      name: "SKSE",
      instructions: "Get it from the site.",
      url: "https://skse.silverlock.org",
    });
    expect(screen.getByText("Get it from the site.")).toBeInTheDocument();
    // #8: inactive, not native disabled (focus must not drop to <body>).
    expect(screen.getByRole("button", { name: "Use this file" })).toHaveAttribute("aria-disabled", "true");
    expect(document.querySelector("button[disabled]")).toBeNull();
    await user.type(screen.getByLabelText("Path of the downloaded archive"), "/tmp/skse.7z");
    await user.click(screen.getByRole("button", { name: "Use this file" }));
    expect(await answered(t)).toEqual({ id, answer: { type: "path", path: "/tmp/skse.7z" } });
  });

  it("choose_variant", async () => {
    const { t, id, user } = await ask({ type: "choose_variant", choices: ["SE", "AE"] });
    await user.click(screen.getByRole("button", { name: "AE" }));
    expect(await answered(t)).toEqual({ id, answer: { type: "choice", index: 1 } });
  });

  it("fomod_group: defaults + required preselected, single choice replaces, answers indices", async () => {
    const { t, id, user } = await ask({
      type: "fomod_group",
      module: "Demo",
      step: "Main",
      group: "Textures",
      kind: "SelectExactlyOne",
      kind_label: "Select one",
      options: [
        { name: "1K", description: "small", kind: "Optional", kind_label: null },
        { name: "2K", description: "medium", kind: "Recommended", kind_label: "Recommended" },
        { name: "8K", description: "huge", kind: "NotUsable", kind_label: "Not usable" },
      ],
      defaults: [1],
      error: null,
    });
    expect(screen.getByRole("heading", { name: "FOMOD installer: Demo" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /2K/ })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: /8K/ })).toHaveAttribute("aria-disabled", "true");
    expect(document.querySelector("button[disabled]")).toBeNull();
    await user.click(screen.getByRole("radio", { name: /1K/ }));
    expect(screen.getByText("small")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await answered(t)).toEqual({ id, answer: { type: "choices", indices: [0] } });
  });

  it("fomod_group re-ask shows the backend error in a fresh dialog", async () => {
    const { t, user } = await ask({
      type: "fomod_group",
      module: "Demo",
      step: "Main",
      group: "Patches",
      kind: "SelectAny",
      kind_label: "Any",
      options: [{ name: "P1", description: "", kind: "Optional", kind_label: null }],
      defaults: [],
      error: null,
    });
    await user.click(screen.getByRole("button", { name: "Next" }));
    await gone();
    act(() =>
      t.emit("vessel:prompt", {
        id: 999,
        job: 1,
        prompt: {
          type: "fomod_group",
          module: "Demo",
          step: "Main",
          group: "Patches",
          kind: "SelectAny",
          kind_label: "Any",
          options: [{ name: "P1", description: "", kind: "Optional", kind_label: null }],
          defaults: [],
          error: "P1 conflicts with your other choice",
        },
      }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("P1 conflicts with your other choice");
  });

  it("Escape (gamepad B) answers cancel", async () => {
    const { t, id, user } = await ask({ type: "choose_variant", choices: ["SE"] });
    const stop = installKeyboardNav();
    await user.keyboard("{Escape}");
    stop();
    expect(await answered(t)).toEqual({ id, answer: { type: "cancel" } });
    await gone();
  });

  it("vessel:prompt-closed dismisses without answering", async () => {
    const { t, id } = await ask({ type: "confirm", question: "Still there?", default_yes: true });
    act(() => t.emit("vessel:prompt-closed", { id }));
    await gone();
    expect(t.callsOf("prompt_answer")).toHaveLength(0);
  });

  it("queues prompts and shows them in order", async () => {
    const { t, user } = await ask({ type: "confirm", question: "First?", default_yes: true });
    act(() => t.emit("vessel:prompt", { id: 5000, job: 2, prompt: { type: "confirm", question: "Second?", default_yes: true } }));
    expect(screen.getByText("First?")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Yes" }));
    expect(await screen.findByText("Second?")).toBeInTheDocument();
  });
});

describe("confirm: danger contract and busy (#1, #8)", () => {
  const danger: Prompt = {
    type: "confirm",
    question: "Uninstall 1 mod(s)?",
    default_yes: true,
    title: "Uninstall SkyUI?",
    yes_label: "Uninstall SkyUI",
    no_label: "Keep mod",
    danger: true,
    option: "Delete archive too",
  };

  // Fails when a backend without the new fields (missing or null) no longer gets the plain dialog.
  it.each([
    ["missing", { type: "confirm", question: "Proceed?", default_yes: true }],
    ["null", { type: "confirm", question: "Proceed?", default_yes: true, title: null, yes_label: null, no_label: null, danger: null, option: null }],
  ] as [string, Prompt][])("legacy confirm with the new fields %s renders as before", async (_, prompt) => {
    const { t, id, user } = await ask(prompt);
    expect(screen.getByRole("heading", { name: "Confirm" })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).toBeNull();
    // default_yes=true still focuses Yes.
    expect(document.activeElement).toHaveTextContent("Yes");
    expect(screen.getByRole("button", { name: "No" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Yes" }));
    expect(await answered(t)).toEqual({ id, answer: { type: "confirm", yes: true } });
  });

  // Fails when a danger confirm focuses the destructive button (default_yes is true for the CLI).
  it("danger focuses Keep mod, tones the yes button danger and leaves the option unchecked", async () => {
    const { t, id, user } = await ask(danger);
    expect(screen.getByRole("heading", { name: "Uninstall SkyUI?" })).toBeInTheDocument();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Keep mod" }));
    expect(screen.getByRole("button", { name: "Uninstall SkyUI" }).className).toContain("bg-rose-500/15");
    const box = screen.getByRole("checkbox", { name: "Delete archive too" });
    expect(box).toHaveAttribute("aria-checked", "false");
    await user.click(screen.getByRole("button", { name: "Uninstall SkyUI" }));
    expect(await answered(t)).toEqual({ id, answer: { type: "confirm", yes: true, option_checked: false } });
  });

  // Fails when the ticked option is not sent, or the checkbox cannot be reached by keyboard/pad.
  it("the Delete archive too option is focusable and sent as option_checked", async () => {
    const { t, id, user } = await ask(danger);
    const box = screen.getByRole("checkbox", { name: "Delete archive too" });
    await user.tab({ shift: true });
    expect(document.activeElement).toBe(box);
    await user.keyboard(" ");
    expect(box).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("button", { name: "Uninstall SkyUI" }));
    expect(await answered(t)).toEqual({ id, answer: { type: "confirm", yes: true, option_checked: true } });
  });

  // #8: fails when sending disables the buttons natively (focus drops to <body>).
  it("while sending, buttons are busy and focus stays on the pressed button", async () => {
    const { t, user } = await ask({ type: "confirm", question: "Proceed?", default_yes: true });
    t.handlers.prompt_answer = () => new Promise(() => {});
    const yes = screen.getByRole("button", { name: "Yes" });
    await user.click(yes);
    await waitFor(() => expect(yes).toHaveAttribute("aria-busy", "true"));
    for (const b of screen.getAllByRole("button")) {
      expect(b).not.toHaveAttribute("disabled");
      expect(b).toHaveAttribute("aria-disabled", "true");
    }
    expect(document.activeElement).toBe(yes);
    // A second press while busy sends nothing more.
    await user.click(yes);
    expect(t.callsOf("prompt_answer")).toHaveLength(1);
  });
});

describe("fomod helpers", () => {
  it("initial selection includes Required options", () => {
    const o = (kind: "Required" | "Optional") => ({ name: "", description: "", kind, kind_label: null });
    expect(fomodInitial([o("Optional"), o("Required")], [0])).toEqual([0, 1]);
  });
  it("group rules", () => {
    expect(fomodProblem("SelectExactlyOne", 0, 3)).not.toBeNull();
    expect(fomodProblem("SelectAtMostOne", 0, 3)).toBeNull();
    expect(fomodProblem("SelectAtLeastOne", 0, 3)).not.toBeNull();
    expect(fomodProblem("SelectAll", 2, 3)).not.toBeNull();
    expect(fomodProblem("SelectAny", 0, 3)).toBeNull();
  });
});
