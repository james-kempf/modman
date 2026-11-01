// SPDX-License-Identifier: GPL-3.0-or-later
// Browse details dialogs, opened by activating a browse card: everything known
// about a Nexus mod or collection from its card, plus the collection's revision
// and mod list from collection_info, and the item's actions (Install / Remove,
// passed in by Browse). Focus returns to the card on close.

import { useQuery } from "@tanstack/react-query";
import { errMsg } from "../../app/bridge";
import * as ipc from "../../ipc/commands";
import type { CollectionHit, RevisionMod, SearchHit } from "../../ipc/types";
import type { ReactNode } from "react";
import { Badge, Button, ErrorNote, Fact, LIST_ROW, Picture, bytes, cx } from "../ui";
import { Dialog } from "./Dialog";

const num = (n: number | null) => (n === null ? "?" : n.toLocaleString());

/** Nexus ISO timestamp -> local date; unparseable values are shown as sent. */
function day(iso: string | null): string {
  if (!iso) return "?";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString();
}

function Summary({ text }: { text: string | null }) {
  return (
    <p className="mt-5 text-body leading-relaxed whitespace-pre-line text-zinc-300">
      {text?.trim() || "No description on Nexus."}
    </p>
  );
}

/** The item's actions, then Close (focused on open: activating a card never lands on Install or Remove). */
function ActionRow({ actions, onClose }: { actions?: ReactNode; onClose: () => void }) {
  return (
    <div className="mt-6 flex flex-wrap justify-end gap-3">
      {actions}
      <Button tone={actions ? "ghost" : "primary"} data-autofocus onClick={onClose}>
        Close
      </Button>
    </div>
  );
}

/** A collection's mods; rows are focusable so a controller can scroll a long list. */
function ModList({ mods }: { mods: RevisionMod[] }) {
  return (
    <section className="mt-6">
      <h3 className="eyebrow">Mods in this collection ({mods.length})</h3>
      <ul aria-label="Collection mods" className="mt-3 grid gap-1">
        {mods.map((m, i) => (
          <li key={`${m.mod_id ?? "x"}-${i}`} tabIndex={0} className={cx(LIST_ROW, "flex flex-wrap items-center gap-x-3 gap-y-1")}>
            <span className="min-w-0 flex-1 truncate text-body text-zinc-100" title={m.name}>
              {m.name}
            </span>
            <span className="font-mono text-caption text-dim">
              {[m.version ? `v${m.version.replace(/^v/i, "")}` : null, m.author, m.size ? bytes(m.size) : null].filter(Boolean).join(" · ")}
            </span>
            {m.optional && <Badge>Optional</Badge>}
          </li>
        ))}
      </ul>
    </section>
  );
}

interface DialogProps {
  onClose: () => void;
  /** Install / Remove buttons, before Close. */
  actions?: ReactNode;
  /** Where focus goes on close (the card). */
  returnFocus?: HTMLElement | null;
}

export function ModDetailsDialog({ m, onClose, actions, returnFocus }: DialogProps & { m: SearchHit }) {
  const name = m.name ?? `Mod ${m.modId ?? "?"}`;
  return (
    <Dialog title={name} subtitle={m.author ? `by ${m.author}` : undefined} onBack={onClose} returnFocus={returnFocus} wide>
      <Picture url={m.thumbnailUrl} name={name} className="aspect-video w-full max-w-md rounded-card" />
      <Summary text={m.summary} />
      <dl className="mt-5 grid grid-cols-2 gap-x-6 gap-y-4 md:grid-cols-3">
        <Fact k="Author" v={m.author ?? "?"} />
        <Fact k="Version" v={m.version ?? "?"} />
        <Fact k="Category" v={m.category ?? "?"} />
        <Fact k="Endorsements" v={num(m.endorsements)} />
        <Fact k="Downloads" v={num(m.downloads)} />
        <Fact k="Updated" v={day(m.updatedAt)} />
        <Fact k="Nexus id" v={m.modId ?? "?"} />
        <Fact k="Status" v={m.installed ? "Installed" : "Not installed"} />
      </dl>
      <ActionRow actions={actions} onClose={onClose} />
    </Dialog>
  );
}

export function CollectionDetailsDialog({
  game,
  c,
  onClose,
  actions,
  returnFocus,
}: DialogProps & { game: string; c: CollectionHit }) {
  const q = useQuery({
    queryKey: ["collection-info", game, c.slug],
    queryFn: () => ipc.collectionInfo(c.slug),
    staleTime: 5 * 60_000,
  });
  const r = q.data?.revision;
  const latest = r?.latest ?? c.revision;
  const installed = q.data ? (q.data.installed?.revision ?? null) : c.installed_revision;
  const mods = r?.mods ?? [];
  return (
    <Dialog title={c.name} subtitle={c.author ? `by ${c.author}` : undefined} onBack={onClose} returnFocus={returnFocus} wide>
      <Picture url={c.image_url} name={c.name} className="aspect-[4/5] w-40 rounded-card" />
      <Summary text={r?.summary || c.summary} />
      <dl className="mt-5 grid grid-cols-2 gap-x-6 gap-y-4 md:grid-cols-3">
        <Fact k="Author" v={r?.author ?? c.author ?? "?"} />
        <Fact k="Category" v={c.category ?? "?"} />
        <Fact k="Latest revision" v={latest ?? "?"} />
        <Fact k="Status" v={installed === null ? "Not installed" : `Installed, revision ${installed}`} />
        <Fact
          k="Mods"
          v={r ? `${r.mod_count}${r.optional_count ? ` (${r.optional_count} optional)` : ""}` : num(c.mod_count)}
        />
        <Fact k="Size" v={bytes(r?.total_size ?? c.total_size)} />
        <Fact k="Endorsements" v={num(c.endorsements)} />
        <Fact k="Downloads" v={num(c.downloads)} />
        <Fact k="Slug" v={c.slug} />
        {r && <Fact k="Revision status" v={r.status} />}
        {q.data?.update_available != null && <Fact k="Update" v={`Revision ${q.data.update_available} available`} />}
      </dl>
      {q.isPending && <p className="mt-4 text-label text-zinc-400">Loading revision details…</p>}
      {q.isError && (
        <div className="mt-4">
          <ErrorNote>Could not load revision details: {errMsg(q.error)}</ErrorNote>
        </div>
      )}
      <ActionRow actions={actions} onClose={onClose} />
      {mods.length > 0 && <ModList mods={mods} />}
    </Dialog>
  );
}
