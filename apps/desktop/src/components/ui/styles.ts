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
/**
 * An activatable card (a <button>: game tiles, Browse results): the whole card is one control and takes the
 * shared highlight. Add the direction (flex-col / flex-row), padding, border colour and fill per use; an
 * inner picture is rounded-inset (rounded-card-lg 16px minus the p-3 padding = 4px) so the corners nest.
 * Press: Button's translate-y-px with scale-99, not 98 (98 pulls a ~300px card's edges in ~3px each). Transform
 * only (Tailwind v4 emits the translate/scale properties, hence the transition list), so the grid does not move.
 * `enabled:` skips disabled buttons and non-button shells (the inert Browse <div aria-disabled>);
 * `not-aria-disabled:` skips inactive buttons.
 */
export const CARD_TILE =
  "group flex h-full w-full min-w-0 gap-3 rounded-card-lg border text-left transition-[background-color,border-color,color,translate,scale] duration-base enabled:not-aria-disabled:active:translate-y-px enabled:not-aria-disabled:active:scale-99";
/** A compact status pill / chip row (mono log lines, repair steps). */
export const PILL = "rounded-pill border border-hairline bg-ink px-3 py-1.5 text-label";
/** A free-standing list row (jobs, entries); the shared highlight shows when it is interactive. */
export const LIST_ROW = "rounded-inner px-3 py-2.5 transition-colors duration-base";
/** Width of a settings row's control (input + Save group, dropdown): grows to fill the row, wraps below 14rem. */
export const SETTING_CONTROL = "min-w-0 flex-[1_1_14rem]";
/**
 * The one toast shape, used unchanged by every toast (results and running jobs alike, in the app bar's toast slot):
 * the job-island pill, fitted to the bar. It is exactly as tall as the bar's row floor (h-control, the
 * row's auto-rows minimum), so a toast never grows the bar; py-1 leaves the two text lines 40px, and pl-1.5 keeps the
 * 36px disc concentric in the 48px pill ((48 - 36) / 2). Inside a slot narrower than @3xs (16rem) the text goes
 * sr-only and the pill closes to a disc-only circle (pr-1.5); the slot is an @container.
 */
export const TOAST_SHELL =
  "glass pointer-events-auto relative flex h-control w-full max-w-xl items-center gap-3 overflow-hidden rounded-pill py-1 pr-5 pl-1.5 @max-3xs:pr-1.5";
/** The toast's open (clip reveal left→right) and close (reverse) motion; one of them goes on the shell. */
export const TOAST_OPEN = "animate-toast-open";
export const TOAST_CLOSE = "animate-toast-close";
/** Keep in step with --animate-toast-close (duration-fast): the closing copy unmounts after this. */
export const TOAST_CLOSE_MS = 150;
/** A toast's leading status disc (add its tint); holds a size 17 bold glyph. */
export const TOAST_DISC = "relative grid size-9 shrink-0 place-items-center rounded-pill";
/** A toast's text column (title, detail): visually hidden, still read, when the slot is too narrow. */
export const TOAST_TEXT = "relative min-w-0 flex-1 overflow-hidden @max-3xs:sr-only";
/** The text cycle: the incoming line rises in, the outgoing copy (aria-hidden, absolute) moves up and out. */
export const TOAST_IN = "block animate-toast-in";
export const TOAST_OUT = "pointer-events-none absolute inset-0 animate-toast-out";
/** A toast's main line. */
export const TOAST_TITLE = "block truncate text-[13.5px] font-medium text-zinc-100";
/** A view's outer frame (ViewFrame): one column that never widens past <main>, blocks 1.5rem apart. */
export const VIEW_FRAME = "grid grid-cols-[minmax(0,1fr)] gap-6";
/** Card title (non-inset header). */
export const CARD_TITLE = "text-title font-medium tracking-tight text-zinc-50";
/** Muted secondary line under a title (card intros, row help). */
export const MUTED_NOTE = "text-label leading-snug text-dim";
/**
 * How long a load may run before its loading state shows (skeletons, "Loading…"): a fast answer never flashes
 * one, and a page change keeps the previous page up (dimmed) until then.
 */
export const SKELETON_DELAY_MS = 300;
