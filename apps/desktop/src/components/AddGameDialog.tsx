// SPDX-License-Identifier: GPL-3.0-or-later
// Add a game: supported games found in the Steam libraries (games_detect),
// or any Nexus game domain with an optional folder (game_add). game_add is a
// job: it may ask for the folder, profile or the active-game switch through
// prompts, which open on top of this dialog.

import { MagnifyingGlassIcon, PlusIcon } from "@phosphor-icons/react";
import { useQuery } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { errMsg } from "../app/bridge";
import { useBusy, useRun } from "../app/run";
import { useUi } from "../app/store";
import * as ipc from "../ipc/commands";
import { Dialog } from "./Dialog";
import { Button, Empty, ErrorNote, SupportBadge, TextField } from "./ui";

/** A Nexus Mods game domain: lowercase letters and digits (e.g. skyrimspecialedition, fallout4). */
const DOMAIN = /^[a-z0-9]+$/;

/** null = valid; else the inline error for the domain field. */
export function domainProblem(raw: string): string | null {
  const d = raw.trim();
  if (!d) return "Enter the game's Nexus Mods domain.";
  if (!DOMAIN.test(d)) return "A Nexus domain uses only lowercase letters and digits, e.g. stardewvalley.";
  return null;
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
  const detect = useQuery({ queryKey: ["detect"], queryFn: ipc.gamesDetect });
  const fresh = detect.data?.filter((d) => !d.configured) ?? [];
  const [domain, setDomain] = useState("");
  const [folder, setFolder] = useState("");
  const [domainError, setDomainError] = useState<string | null>(null);

  const add = async (a: { domain: string; path?: string | null }) => {
    const r = await run("Add game", () => ipc.gameAdd(a));
    if (r) close();
  };
  const submit = (e: FormEvent) => {
    e.preventDefault();
    const problem = domainProblem(domain);
    setDomainError(problem);
    if (problem) {
      // The domain field is the form's first input.
      (e.currentTarget as HTMLFormElement).querySelector("input")?.focus();
      return;
    }
    void add({ domain: domain.trim(), path: folder.trim() || null });
  };

  return (
    <Dialog
      title="Add a game"
      subtitle="Pick a game found in your Steam libraries, or add any game by its Nexus Mods domain."
      onBack={close}
    >
      <section aria-labelledby="add-game-found" className="grid gap-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h3 id="add-game-found" className="eyebrow">
            Found in Steam
          </h3>
          <Button
            size="sm"
            tone="ghost"
            busy={detect.isFetching}
            icon={<MagnifyingGlassIcon aria-hidden size={15} />}
            onClick={() => void detect.refetch()}
          >
            Detect again
          </Button>
        </div>
        {detect.isError ? (
          <ErrorNote>{errMsg(detect.error)}</ErrorNote>
        ) : detect.isPending ? (
          <p className="text-body text-zinc-400">Looking for Steam games…</p>
        ) : !fresh.length ? (
          <Empty>No other supported games found in Steam.</Empty>
        ) : (
          <ul className="rows" aria-label="Detected games">
            {fresh.map((d) => (
              <li key={d.domain} className="flex flex-wrap items-center gap-4 py-3">
                <span className="min-w-0 flex-1">
                  <span className="flex flex-wrap items-center gap-2 text-body text-zinc-100">
                    {d.name} <SupportBadge support={d.support} />
                  </span>
                  <span className="block font-mono text-caption break-all text-dim">{d.install_dir}</span>
                </span>
                <Button
                  size="sm"
                  tone="primary"
                  busy={adding}
                  aria-label={`Add ${d.name}`}
                  icon={<PlusIcon aria-hidden size={14} />}
                  onClick={() => void add({ domain: d.domain, path: d.install_dir })}
                >
                  Add
                </Button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <form onSubmit={submit} aria-labelledby="add-game-manual" className="mt-7 grid gap-4">
        <h3 id="add-game-manual" className="eyebrow">
          Add by Nexus domain
        </h3>
        <TextField
          label="Nexus game domain"
          placeholder="e.g. stardewvalley"
          helper="The part after nexusmods.com/ in the game's URL."
          value={domain}
          error={domainError}
          onChange={(e) => {
            setDomain(e.target.value);
            setDomainError(null);
          }}
        />
        <TextField
          label="Game folder (optional)"
          placeholder="Detected through Steam when empty"
          value={folder}
          onChange={(e) => setFolder(e.target.value)}
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
