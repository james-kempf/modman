// SPDX-License-Identifier: GPL-3.0-or-later
// Add a game manually by its Nexus game domain and its folder (game_add), for games Vessel does not detect on its own (detected games are
// offered in the Games grid). Opened for an unsupported Steam game, the folder
// is prefilled and the user supplies the domain. game_add is a job: it may ask for the folder, profile or the active-game switch through
// prompts, which open on top of this dialog.

import { PlusIcon } from "@phosphor-icons/react";
import { useState, type FormEvent } from "react";
import { useBusy, useRun } from "../../app/run";
import { useUi } from "../../app/store";
import * as ipc from "../../ipc/commands";
import { Dialog } from "./Dialog";
import { Button, TextField } from "../ui";

/** A Nexus Mods game domain: lowercase letters and digits (e.g. skyrimspecialedition, fallout4). */
const DOMAIN = /^[a-z0-9]+$/;

/** null = valid; else the inline error for the domain field. */
export function domainProblem(raw: string): string | null {
  const d = raw.trim();
  if (!d) return "Enter the game's Nexus Mods domain.";
  if (!DOMAIN.test(d)) return "A Nexus domain uses only lowercase letters and digits, e.g. stardewvalley.";
  return null;
}

/** null = valid; else the inline error for the folder field. */
export function folderProblem(raw: string): string | null {
  return raw.trim() ? null : "Enter the game's folder.";
}

/** Rendered by the shell while `addGame` is set in the UI store. */
export function AddGameHost() {
  const open = useUi((s) => s.addGame);
  return open ? <AddGameDialog /> : null;
}

export function AddGameDialog() {
  const close = () => useUi.getState().setAddGame(false);
  const run = useRun();
  const adding = useBusy("Add game");
  const prefill = useUi((s) => s.addGamePrefill);
  const [domain, setDomain] = useState("");
  const [folder, setFolder] = useState(prefill?.path ?? "");
  const [domainError, setDomainError] = useState<string | null>(null);
  const [folderError, setFolderError] = useState<string | null>(null);

  const add = async (a: { domain: string; path: string }) => {
    const r = await run("Add game", () => ipc.gameAdd(a));
    if (r) close();
  };
  const submit = (e: FormEvent) => {
    e.preventDefault();
    const dProblem = domainProblem(domain);
    const fProblem = folderProblem(folder);
    setDomainError(dProblem);
    setFolderError(fProblem);
    if (dProblem || fProblem) {
      // Focus the first invalid field.
      const form = e.currentTarget as HTMLFormElement;
      form.querySelector<HTMLInputElement>(`input[name="${dProblem ? "domain" : "folder"}"]`)?.focus();
      return;
    }
    void add({ domain: domain.trim(), path: folder.trim() });
  };

  return (
    <Dialog
      title="Add a game"
      subtitle="For games not detected automatically: add one by its Nexus Mods domain."
      onBack={close}
    >
      {/* noValidate: required fields get the inline errors below, not the browser's bubble. */}
      <form onSubmit={submit} noValidate aria-label="Add a game manually" className="grid gap-4">
        {prefill && (
          <p className="text-body text-zinc-300">
            Vessel has no preset for {prefill.name}. Enter its Nexus Mods domain to add it with generic settings.
          </p>
        )}
        <TextField
          autoFocus={!!prefill}
          name="domain"
          required
          label="Nexus game domain"
          placeholder="nexusmods.com/<domain>, e.g. skyrimspecialedition"
          value={domain}
          error={domainError}
          onChange={(e) => {
            setDomain(e.target.value);
            setDomainError(null);
          }}
        />
        <TextField
          name="folder"
          required
          label="Game folder"
          placeholder="/path/to/game"
          value={folder}
          error={folderError}
          onChange={(e) => {
            setFolder(e.target.value);
            setFolderError(null);
          }}
        />
        <div className="flex flex-wrap justify-end gap-2">
          <Button tone="ghost" onClick={close}>
            Close
          </Button>
          <Button type="submit" tone="primary" busy={adding} icon={<PlusIcon aria-hidden size={15} />}>
            Add game
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
