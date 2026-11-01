// SPDX-License-Identifier: GPL-3.0-or-later
// The About section of the browse details dialogs: the item's Nexus
// description (mod: BBCode, collection: Markdown), fetched when the dialog
// opens and cached; the short summary stands in while it is missing, empty
// or failed to load.

import { useQuery } from "@tanstack/react-query";
import { errMsg } from "../../app/bridge";
import * as ipc from "../../ipc/commands";
import type { AboutTarget } from "../../ipc/types";
import { ErrorNote } from "../ui";
import { BBCode } from "./bbcode";
import { MarkdownText } from "./markdown";

function Summary({ text }: { text: string | null }) {
  return <p className="text-body leading-relaxed whitespace-pre-line text-zinc-300">{text?.trim() || "No description on Nexus."}</p>;
}

/** `target` null (no Nexus id): the summary only. */
export function About({ game, target, summary }: { game: string | null; target: AboutTarget | null; summary: string | null }) {
  const q = useQuery({
    queryKey: ["nexus-about", game, target?.kind, target ? (target.kind === "mod" ? target.mod_id : target.slug) : null],
    queryFn: () => ipc.browseAbout(target!),
    enabled: target !== null,
    staleTime: 30 * 60_000,
  });
  const description = target && q.data;
  return (
    <section data-slot="about" aria-label="About" className="mt-5 grid gap-2">
      <h3 className="eyebrow">About</h3>
      {target && q.isPending && (
        <p role="status" className="text-label text-zinc-400">
          Loading description…
        </p>
      )}
      {target && q.isError && <ErrorNote>Could not load the description: {errMsg(q.error)}</ErrorNote>}
      {description ? (
        <div className="text-body leading-relaxed text-zinc-300">
          {target.kind === "mod" ? <BBCode source={description} /> : <MarkdownText source={description} />}
        </div>
      ) : (
        <Summary text={summary} />
      )}
    </section>
  );
}
