// SPDX-License-Identifier: GPL-3.0-or-later
// Shared class-name helper and Tailwind class strings for the UI primitives.

export const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(" ");

/** Padding of every Card section (CardSection, Row, card-level list rows): equal on both axes. */
export const SECTION_PAD = "p-section";
/** The inset header: the section padding minus the bottom (the first section's top padding is the gap). */
export const INSET_HEADER_PAD = "px-section pt-section";
/** The card surface: radius, hairline border, raised fill. */
export const CARD_SHELL = "rounded-card border border-hairline bg-ink-2";
/** Hairline dividers between stacked children (card sections, lists). */
export const DIVIDE = "divide-y divide-hairline";
/** A sunken well inside a card or dialog (tiles, typed-text output, side panels); add padding per use. */
export const TILE = "rounded-card border border-hairline bg-ink";
/** A compact status pill / chip row (mono log lines, repair steps). */
export const PILL = "rounded-control border border-hairline bg-ink px-3 py-1.5 text-label";
/** A free-standing list row (jobs, entries); the shared highlight shows when it is interactive. */
export const LIST_ROW = "rounded-control px-3 py-2.5 transition-colors duration-base";
/** Width of a settings row's control (input + Save group, dropdown): grows to fill the row, wraps below 14rem. */
export const SETTING_CONTROL = "min-w-0 flex-[1_1_14rem]";
/** A view's outer frame (ViewFrame): one column that never widens past <main>, blocks 1.5rem apart. */
export const VIEW_FRAME = "grid grid-cols-[minmax(0,1fr)] gap-6";
/** Card title (non-inset header). */
export const CARD_TITLE = "text-title font-medium tracking-tight text-zinc-50";
/** Muted secondary line under a title (card intros, row help). */
export const MUTED_NOTE = "text-label leading-snug text-dim";
