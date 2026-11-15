// SPDX-License-Identifier: GPL-3.0-or-later
import { CheckIcon, InfoIcon, WarningCircleIcon, WarningIcon } from "@phosphor-icons/react";
import { useState, type ReactNode } from "react";
import { TOAST_CLOSE, TOAST_DISC, TOAST_IN, TOAST_OPEN, TOAST_OUT, TOAST_SHELL, TOAST_TEXT, TOAST_TITLE, cx } from "../styles";

/** Same union as the store's ToastTone (the UI kit does not import app state). */
export type NoticeTone = "info" | "success" | "warn" | "error";

// Tone shows in the leading disc, glyph + tint (never colour alone).
const TONE: Record<NoticeTone, { tint: string; icon: ReactNode }> = {
  info: { tint: "bg-white/8 text-zinc-300", icon: <InfoIcon size={17} weight="bold" /> },
  success: { tint: "bg-accent-400/15 text-accent-300", icon: <CheckIcon size={17} weight="bold" /> },
  warn: { tint: "bg-amber-400/15 text-amber-300", icon: <WarningIcon size={17} weight="bold" /> },
  error: { tint: "bg-rose-400/15 text-rose-300", icon: <WarningCircleIcon size={17} weight="bold" /> },
};
/** Running work (the former job island's disc): accent tint, three animated rails. */
const BUSY_TINT = "bg-accent-400/15 text-accent-300";

/** One rendered message: its text lines. */
interface Line {
  text: ReactNode;
  detail?: string;
}

/**
 * Presentational only: the previous message while a new one cycles in. `cycleKey` changes on every write
 * (the toast id); the line shown before it becomes the outgoing copy, anything older is dropped (latest wins).
 */
function useCycle(cycleKey: string | number | undefined, line: Line): (Line & { key: string | number }) | null {
  const [s, set] = useState<{ key: string | number | undefined; last: Line; prev: (Line & { key: string | number }) | null }>({
    key: cycleKey,
    last: line,
    prev: null,
  });
  // Derived state, set during render (React's documented pattern): no extra commit, no effect lag.
  if (s.key !== cycleKey) {
    const prev = s.key === undefined || cycleKey === undefined ? null : { ...s.last, key: s.key };
    set({ key: cycleKey, last: line, prev });
    return prev;
  }
  return s.prev;
}

function Lines({ text, detail }: Line) {
  return (
    <>
      <span className={TOAST_TITLE}>{text}</span>
      {detail && <span className="block truncate font-mono text-micro text-dim">{detail}</span>}
    </>
  );
}

/**
 * The toast pill: informational only, no controls (it closes on its own; app/toast.ts). TOAST_SHELL with a tone
 * disc (or the running-work rails when `busy`), the text line and an optional quieter `detail` line, both
 * truncated; the full text stays in the DOM (read by screen readers, also when a narrow slot shows the disc only)
 * and in the pill's tooltip. Errors are announced as alerts. There is only ever one: each write swaps this one's
 * content in place, cycling the text (TOAST_IN / TOAST_OUT); the shell opens with TOAST_OPEN and closes with
 * TOAST_CLOSE (DESIGN.md §8 toast motion).
 */
export function Notice({
  tone,
  children,
  detail,
  busy = false,
  cycleKey,
  closing = false,
}: {
  tone: NoticeTone;
  children: ReactNode;
  detail?: string;
  busy?: boolean;
  /**
   * Changes on every new message (the toast id; a live toast's key): the text cycles (old line out, new line
   * in). Updates under the same key (a live toast's progress) change the text in place, with no motion.
   */
  cycleKey?: string | number;
  /** The toast is closing: the collapse motion, and hidden from assistive tech (its text was already read). */
  closing?: boolean;
}) {
  const text = typeof children === "string" ? children : undefined;
  const prev = useCycle(cycleKey, { text: children, detail });
  const disc = busy ? "busy" : tone;
  return (
    // title on the pill: the full text as a tooltip, also when only the disc shows (narrow slot).
    <div
      role={tone === "error" ? "alert" : "status"}
      aria-busy={busy || undefined}
      aria-hidden={closing || undefined}
      title={text && (detail ? `${text}: ${detail}` : text)}
      className={cx(TOAST_SHELL, closing ? TOAST_CLOSE : TOAST_OPEN)}
    >
      {/* key: a tone change re-mounts the disc, which fades in (the tint itself snaps; see DESIGN.md §8). */}
      <span
        key={disc}
        aria-hidden
        data-tone={disc}
        className={cx(TOAST_DISC, busy ? BUSY_TINT : TONE[tone].tint, prev && "animate-fade")}
      >
        {busy ? (
          <span className="flex gap-rail">
            <span className="h-3.5 w-rail animate-bar rounded-pill bg-current" />
            <span className="h-3.5 w-rail animate-bar rounded-pill bg-current [animation-delay:150ms]" />
            <span className="h-3.5 w-rail animate-bar rounded-pill bg-current [animation-delay:300ms]" />
          </span>
        ) : (
          TONE[tone].icon
        )}
      </span>
      <span className={TOAST_TEXT}>
        {/* key = the write: each write restarts the incoming motion, never queues one. */}
        <span key={cycleKey} className={prev ? TOAST_IN : "block"}>
          <Lines text={children} detail={detail} />
        </span>
        {prev && (
          <span key={`out-${prev.key}`} aria-hidden data-toast-out className={TOAST_OUT}>
            <Lines text={prev.text} detail={prev.detail} />
          </span>
        )}
      </span>
    </div>
  );
}
