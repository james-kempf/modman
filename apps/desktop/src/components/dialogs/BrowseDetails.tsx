// SPDX-License-Identifier: GPL-3.0-or-later
// Browse details dialogs, opened by activating a browse card: everything known
// about a Nexus mod or collection from its card (Details tab), its Nexus
// description (About tab), plus the collection's revision from collection_info
// and its mods as the same mod cards Browse lists (Mods tab; a listed mod opens
// its own details dialog inside the collection), and the item's actions
// (Install / Remove, passed in by Browse). Tabs switch with LB/RB
// (PageUp/PageDown) or a click. The dialogs keep their full height across tabs.
// Focus returns to the card on close.

import { useQuery } from "@tanstack/react-query";
import { errMsg } from "../../app/bridge";
import * as ipc from "../../ipc/commands";
import type { CollectionHit, RevisionMod, SearchHit } from "../../ipc/types";
import { DownloadSimpleIcon, HardDrivesIcon, ThumbsUpIcon } from "@phosphor-icons/react";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { TAB_EVENT, type TabEventDetail } from "../../nav/actions";
import { focusables } from "../../nav/spatial";
import { useBusy } from "../../app/run";
import { installKey } from "../../views/Requirements";
import { ModActions } from "../browse/ModActions";
import { InstalledBadge, MOD_GRID, MOD_PICTURE, Stat, Tile, WorkingBadge, compact, useDetails } from "../browse/Tile";
import { About } from "../rich/About";
import { Badge, Button, ErrorNote, Fact, Picture, bytes } from "../ui";
import { Dialog } from "./Dialog";

const num = (n: number | null) => (n === null ? "?" : n.toLocaleString());

/** Nexus ISO timestamp -> local date; unparseable values are shown as sent. */
function day(iso: string | null): string {
  if (!iso) return "?";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString();
}

/** The item's actions, then Close (focused on open: activating a card never lands on Install or Remove). */
function ActionRow({ actions, onClose }: { actions?: ReactNode; onClose: () => void }) {
  return (
    <div className="flex flex-wrap justify-end gap-3">
      {actions}
      <Button tone={actions ? "ghost" : "primary"} data-autofocus onClick={onClose}>
        Close
      </Button>
    </div>
  );
}

/** A collection mod as a browse hit, for the mod details dialog (no summary, stats or picture in a revision). */
const revisionHit = (m: RevisionMod): SearchHit => {
  const c = m.card;
  return {
    modId: m.mod_id,
    name: c?.name ?? m.name,
    author: c?.author ?? m.author,
    version: c?.version ?? m.version,
    summary: c?.summary ?? null,
    downloads: c?.downloads ?? null,
    endorsements: c?.endorsements ?? null,
    thumbnailUrl: c?.thumbnailUrl ?? null,
    category: c?.category ?? null,
    updatedAt: c?.updatedAt ?? null,
    installed: m.installed,
    loader: m.loader ?? undefined,
  };
};

/**
 * A mod card, the same one the Browse list and a collection's Mods tab show: its installed tag, its
 * details dialog (with Install / Remove). One without a Nexus id is a dimmed card (`inert`);
 * `extraStats` and `extraBadges` add what only a collection lists.
 */
export function ModTile({
  game,
  m,
  focusKey,
  inert,
  extraStats,
  extraBadges,
}: {
  game: string;
  m: SearchHit;
  /** The card's session focus key (Browse remembers it). */
  focusKey?: string;
  /** A mod with no Nexus id: a dimmed card that does not open. */
  inert?: boolean;
  extraStats?: ReactNode;
  extraBadges?: ReactNode;
}) {
  const details = useDetails();
  const name = m.name ?? `Mod ${m.modId ?? "?"}`;
  const busy = useBusy(installKey(String(m.modId)));
  const installed = m.installed && m.modId !== null;
  return (
    <Tile
      picture={<Picture url={m.thumbnailUrl} name={name} className={MOD_PICTURE} />}
      name={name}
      byline={[m.version ? `v${m.version.replace(/^v/i, "")}` : null, m.author, m.category].filter(Boolean).join(" · ")}
      summary={m.summary}
      stats={
        <>
          {m.downloads !== null && <Stat icon={<DownloadSimpleIcon aria-hidden size={13} />} value={compact(m.downloads)} label="downloads" />}
          {m.endorsements !== null && <Stat icon={<ThumbsUpIcon aria-hidden size={13} />} value={compact(m.endorsements)} label="endorsements" />}
          {extraStats}
        </>
      }
      badges={
        <>
          {busy && <WorkingBadge />}
          {installed && <InstalledBadge />}
          {extraBadges}
        </>
      }
      focusKey={focusKey}
      onOpen={inert ? undefined : details.show}
      cardRef={details.card}
      dialog={
        details.open && <ModDetailsDialog game={game} m={m} onClose={details.hide} returnFocus={details.card.current} />
      }
    />
  );
}

type ModTab = "details" | "about";
type CollectionTab = ModTab | "mods";
const MOD_TABS = [
  { value: "details", label: "Details" },
  { value: "about", label: "About" },
] as const;

/**
 * The dialog's tabs, full width with an underline on the selected one, and the selected tab's panel. Tabs are
 * not focus stops (tabindex -1: spatial nav and Tab skip them): LB/RB (the dialog's TAB_EVENT) or a click
 * switch them, wrapping like the app's view tabs. Focus inside the old panel moves into the new one.
 */
function DialogTabs<T extends string>({
  label,
  tabs,
  value,
  onChange,
  children,
}: {
  label: string;
  tabs: readonly { value: T; label: string }[];
  value: T;
  onChange: (t: T) => void;
  /** The selected tab's panel content. */
  children: ReactNode;
}) {
  const id = useId();
  const list = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const refocus = useRef(false);
  const latest = useRef({ tabs, value, onChange });
  latest.current = { tabs, value, onChange };
  const [select] = useState(() => (t: T) => {
    const cur = latest.current;
    if (t === cur.value) return;
    // The focused control goes away with its panel: focus the new panel once it is rendered.
    refocus.current = !!panel.current?.contains(document.activeElement);
    cur.onChange(t);
  });

  useEffect(() => {
    const scope = list.current?.closest<HTMLElement>("[data-nav-scope]");
    if (!scope) return;
    const onTab = (e: Event) => {
      const { tabs: all, value: cur } = latest.current;
      const i = all.findIndex((t) => t.value === cur);
      const { delta } = (e as CustomEvent<TabEventDetail>).detail;
      select(all[(i + delta + all.length) % all.length].value);
    };
    scope.addEventListener(TAB_EVENT, onTab);
    return () => scope.removeEventListener(TAB_EVENT, onTab);
  }, [select]);

  useEffect(() => {
    if (!refocus.current) return;
    refocus.current = false;
    const scope = list.current?.closest<HTMLElement>("[data-nav-scope]");
    const target = (panel.current && focusables(panel.current)[0]) ?? scope?.querySelector<HTMLElement>("[data-autofocus]");
    target?.focus();
  }, [value]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div ref={list} role="tablist" aria-label={label} className="flex w-full shrink-0 border-b border-hairline">
        {tabs.map((t) => (
          <button
            key={t.value}
            type="button"
            role="tab"
            tabIndex={-1}
            id={`${id}-tab-${t.value}`}
            aria-selected={t.value === value}
            aria-controls={`${id}-panel-${t.value}`}
            // No focus on click either: the tabs stay out of the focus order.
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => select(t.value)}
            className="-mb-px min-w-0 flex-1 truncate border-b-2 border-transparent px-3 py-3 text-body font-medium text-zinc-400 aria-selected:border-accent-400 aria-selected:text-zinc-50"
          >
            {t.label}
          </button>
        ))}
      </div>
      {/* The tab's contents are the only part that scrolls (the tabs, title and actions stay in place). */}
      <div
        ref={panel}
        data-scroll-pane
        role="tabpanel"
        id={`${id}-panel-${value}`}
        aria-labelledby={`${id}-tab-${value}`}
        className="mt-5 min-h-0 flex-1 overflow-y-auto overscroll-contain"
      >
        {children}
      </div>
    </div>
  );
}

/** The short summary under the facts, when there is one (the full description is the About tab). */
function Summary({ text }: { text: string | null | undefined }) {
  const t = text?.trim();
  return t ? <p className="mt-5 text-body leading-relaxed whitespace-pre-line text-zinc-300">{t}</p> : null;
}

interface DialogProps {
  onClose: () => void;
  /** Install / Remove buttons, before Close. */
  actions?: ReactNode;
  /** Where focus goes on close (the card). */
  returnFocus?: HTMLElement | null;
}

export function ModDetailsDialog({
  game,
  m,
  onClose,
  returnFocus,
}: {
  game: string;
  m: SearchHit;
  onClose: () => void;
  /** Where focus goes on close (the card). */
  returnFocus?: HTMLElement | null;
}) {
  const name = m.name ?? `Mod ${m.modId ?? "?"}`;
  const [tab, setTab] = useState<ModTab>("details");
  return (
    <Dialog
      title={name}
      subtitle={m.author ? `by ${m.author}` : undefined}
      onBack={onClose}
      returnFocus={returnFocus}
      wide
      fixedHeight
      footer={<ActionRow actions={<ModActions game={game} m={m} onAccepted={onClose} />} onClose={onClose} />}
    >
      <DialogTabs label="Mod" tabs={MOD_TABS} value={tab} onChange={setTab}>
        {tab === "details" ? (
          <>
            <Picture url={m.thumbnailUrl} name={name} className="aspect-video w-full max-w-md rounded-card" />
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
            <Summary text={m.summary} />
          </>
        ) : (
          <About game={game} target={m.modId !== null ? { kind: "mod", mod_id: m.modId } : null} summary={m.summary} />
        )}
      </DialogTabs>
    </Dialog>
  );
}

export function CollectionDetailsDialog({ game, c, onClose, actions, returnFocus }: DialogProps & { game: string; c: CollectionHit }) {
  const q = useQuery({
    queryKey: ["collection-info", game, c.slug],
    queryFn: () => ipc.collectionInfo(c.slug),
    staleTime: 5 * 60_000,
  });
  const r = q.data?.revision;
  const latest = r?.latest ?? c.revision;
  const installed = q.data ? (q.data.installed?.revision ?? null) : c.installed_revision;
  const mods = r?.mods ?? [];
  const [tab, setTab] = useState<CollectionTab>("details");
  const tabs = [
    ...MOD_TABS,
    { value: "mods", label: r ? `Mods (${r.mod_count})` : c.mod_count !== null ? `Mods (${c.mod_count})` : "Mods" },
  ] as const;
  return (
    <Dialog
      title={c.name}
      subtitle={c.author ? `by ${c.author}` : undefined}
      onBack={onClose}
      returnFocus={returnFocus}
      wide
      fixedHeight
      footer={<ActionRow actions={actions} onClose={onClose} />}
    >
      <DialogTabs label="Collection" tabs={tabs} value={tab} onChange={setTab}>
        {tab === "details" ? (
          <>
            {/* Picture left, facts right; stacked below sm. */}
            <div className="flex flex-col gap-6 sm:flex-row sm:items-start">
              <Picture url={c.image_url} name={c.name} className="aspect-[4/5] w-40 shrink-0 rounded-card" />
              <dl className="grid min-w-0 flex-1 grid-cols-2 gap-x-6 gap-y-4 lg:grid-cols-3">
                <Fact k="Author" v={r?.author ?? c.author ?? "?"} />
                <Fact k="Category" v={c.category ?? "?"} />
                <Fact k="Latest revision" v={latest ?? "?"} />
                <Fact k="Status" v={installed === null ? "Not installed" : `Installed, revision ${installed}`} />
                <Fact k="Mods" v={r ? `${r.mod_count}${r.optional_count ? ` (${r.optional_count} optional)` : ""}` : num(c.mod_count)} />
                <Fact k="Size" v={bytes(r?.total_size ?? c.total_size)} />
                <Fact k="Endorsements" v={num(c.endorsements)} />
                <Fact k="Downloads" v={num(c.downloads)} />
                <Fact k="Slug" v={c.slug} />
                {r && <Fact k="Revision status" v={r.status} />}
                {q.data?.update_available != null && <Fact k="Update" v={`Revision ${q.data.update_available} available`} />}
              </dl>
            </div>
            <Summary text={r?.summary || c.summary} />
          </>
        ) : tab === "about" ? (
          <About game={game} target={{ kind: "collection", slug: c.slug }} summary={r?.summary || c.summary} />
        ) : mods.length > 0 ? (
          <ul aria-label="Collection mods" className={MOD_GRID}>
            {mods.map((m, i) => (
              <ModTile
                key={`${m.mod_id ?? "x"}-${i}`}
                game={game}
                m={revisionHit(m)}
                inert={m.mod_id === null}
                extraStats={m.size ? <Stat icon={<HardDrivesIcon aria-hidden size={13} />} value={bytes(m.size)} label="size" /> : null}
                extraBadges={
                  <>
                    {m.optional && <Badge>Optional</Badge>}
                    {m.mod_id === null && <Badge>Not on Nexus</Badge>}
                  </>
                }
              />
            ))}
          </ul>
        ) : (
          !q.isPending && !q.isError && <p className="text-label text-zinc-400">No mods listed for this revision.</p>
        )}
      </DialogTabs>
      {q.isPending && <p className="mt-4 text-label text-zinc-400">Loading revision details…</p>}
      {q.isError && (
        <div className="mt-4">
          <ErrorNote>Could not load revision details: {errMsg(q.error)}</ErrorNote>
        </div>
      )}
    </Dialog>
  );
}
