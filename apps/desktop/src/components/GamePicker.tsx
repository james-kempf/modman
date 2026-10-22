// SPDX-License-Identifier: GPL-3.0-or-later
// Sidebar game picker: a button showing the active game that opens a menu of
// every added game (games_list); picking one runs game_use and refreshes all
// queries. The open menu is a spatial-navigation scope, so arrows / D-pad
// move inside it, Enter / A picks, Escape / B closes it and focus returns
// to the button.

import { CaretUpDownIcon, CheckIcon } from "@phosphor-icons/react";
import { useEffect, useId, useRef, useState, type FocusEvent } from "react";
import { useRun } from "../app/run";
import * as ipc from "../ipc/commands";
import type { GameView } from "../ipc/types";
import { BACK_EVENT } from "../nav/actions";
import { focusables } from "../nav/spatial";
import { StatusDot, SupportBadge, cx, type DotTone } from "./ui";

export function GamePicker({ games, tone }: { games: GameView[]; tone: DotTone }) {
  const active = games.find((g) => g.active) ?? null;
  const [open, setOpen] = useState(false);
  const run = useRun();
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const menuId = useId();

  /** Close; `refocus` puts focus back on the button (keyboard / gamepad close). */
  const close = (refocus: boolean) => {
    // Focus moves before the menu unmounts, so focus never falls to <body>.
    if (refocus) trigger.current?.focus();
    setOpen(false);
  };

  useEffect(() => {
    if (!open) return;
    const el = menu.current!;
    (el.querySelector<HTMLElement>("[aria-checked=true]") ?? focusables(el)[0])?.focus();
    const onBack = () => close(true);
    const onPointer = (e: PointerEvent) => {
      const t = e.target as Node;
      if (!el.contains(t) && !trigger.current?.contains(t)) close(false);
    };
    el.addEventListener(BACK_EVENT, onBack);
    document.addEventListener("pointerdown", onPointer, true);
    return () => {
      el.removeEventListener(BACK_EVENT, onBack);
      document.removeEventListener("pointerdown", onPointer, true);
    };
  }, [open]);

  // Tab (or anything else) moving focus out of the menu closes it.
  const onBlur = (e: FocusEvent) => {
    const to = e.relatedTarget as Node | null;
    if (to && !menu.current?.contains(to) && !trigger.current?.contains(to)) setOpen(false);
  };

  const pick = (g: GameView) => {
    close(true);
    if (!g.active) void run("Switch game", () => ipc.gameUse(g.domain));
  };

  return (
    <div className="relative">
      <button
        ref={trigger}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        aria-label={active ? `Game: ${active.name}` : "Choose a game"}
        onClick={() => (open ? close(true) : setOpen(true))}
        className={cx(
          "flex min-h-12 w-full items-center gap-3 rounded-xl border px-3 text-left transition-colors duration-150",
          open ? "border-white/15 bg-white/[0.06]" : "border-white/[0.06] bg-ink-2 hover:bg-white/[0.04]",
        )}
      >
        {active ? (
          <>
            <StatusDot tone={tone} live={active.deploy.active} />
            <span className="min-w-0 flex-1 truncate text-[14.5px] text-zinc-50">{active.name}</span>
            <span className="font-mono text-[11px] text-dim">{active.mod_count}</span>
          </>
        ) : (
          <span className="min-w-0 flex-1 truncate text-[14px] text-zinc-400">No game selected</span>
        )}
        <CaretUpDownIcon aria-hidden size={16} className="shrink-0 text-dim" />
      </button>
      {open && (
        <div
          ref={menu}
          id={menuId}
          role="menu"
          aria-label="Games"
          data-nav-scope
          onBlur={onBlur}
          className="glass absolute inset-x-0 top-full z-30 mt-2 grid max-h-[60vh] animate-fade grid-cols-[minmax(0,1fr)] gap-0.5 overflow-x-hidden overflow-y-auto rounded-2xl p-1.5"
        >
          {games.map((g) => (
            <button
              key={g.domain}
              type="button"
              role="menuitemradio"
              aria-checked={g.active}
              onClick={() => pick(g)}
              className={cx(
                "flex min-h-12 w-full min-w-0 items-center gap-3 rounded-xl px-3 py-2 text-left transition-colors duration-150",
                g.active ? "bg-white/[0.07]" : "hover:bg-white/[0.04]",
              )}
            >
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[14px] text-zinc-100" title={g.name}>
                  {g.name}
                </span>
                <span className="mt-1 flex flex-wrap items-center gap-2 font-mono text-[11px] text-dim">
                  {g.mod_count} mods
                  <SupportBadge support={g.support} />
                </span>
              </span>
              {g.active && <CheckIcon aria-hidden size={16} weight="bold" className="shrink-0 text-accent-300" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
