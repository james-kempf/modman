// SPDX-License-Identifier: GPL-3.0-or-later
import { CheckCircleIcon, InfoIcon, WarningIcon, XCircleIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import { Button } from "../controls/Button";
import { cx } from "../styles";

export type NoticeTone = "info" | "success" | "warn" | "error";

const TONE: Record<NoticeTone, { border: string; icon: ReactNode }> = {
  info: { border: "border-white/10", icon: <InfoIcon aria-hidden size={18} weight="duotone" className="text-zinc-300" /> },
  success: {
    border: "border-accent-400/25",
    icon: <CheckCircleIcon aria-hidden size={18} weight="duotone" className="text-accent-300" />,
  },
  warn: { border: "border-amber-400/30", icon: <WarningIcon aria-hidden size={18} weight="duotone" className="text-amber-300" /> },
  error: { border: "border-rose-400/40", icon: <XCircleIcon aria-hidden size={18} weight="duotone" className="text-rose-300" /> },
};

/**
 * Toast card in the bottom stack. Closes on a timer, so no dismiss button. Errors are announced as alerts.
 * One optional action: `act` (label + handler, e.g. Undo) renders a small button; `action` takes a ready node
 * (e.g. "Deploy now"). Neither takes focus when the notice appears; it is a normal Tab / D-pad stop.
 * `onHold(true/false)` reports focus entering and leaving the card, so the caller can pause its timer.
 */
export function Notice({
  tone,
  children,
  action,
  act,
  onHold,
}: {
  tone: NoticeTone;
  children: ReactNode;
  action?: ReactNode;
  act?: { label: string; onAction: () => void };
  onHold?: (held: boolean) => void;
}) {
  return (
    <div
      role={tone === "error" ? "alert" : "status"}
      onFocus={onHold && (() => onHold(true))}
      onBlur={
        onHold &&
        ((e) => {
          // Moving between controls inside the card is not leaving it.
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) onHold(false);
        })
      }
      className={cx(
        "glass pointer-events-auto flex min-h-14 animate-island items-center gap-3 rounded-card px-4 py-2",
        TONE[tone].border,
      )}
    >
      {TONE[tone].icon}
      <p className="min-w-0 flex-1 text-[13.5px] wrap-break-word text-zinc-100">{children}</p>
      {act && (
        <Button size="sm" onClick={act.onAction}>
          {act.label}
        </Button>
      )}
      {action}
    </div>
  );
}
