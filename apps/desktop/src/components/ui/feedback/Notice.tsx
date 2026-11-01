// SPDX-License-Identifier: GPL-3.0-or-later
import { CheckCircleIcon, InfoIcon, WarningIcon, XCircleIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
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

/** Toast card in the bottom stack (optional action, e.g. "Deploy now"). Closes on a timer, so no dismiss button. Errors are announced as alerts. */
export function Notice({ tone, children, action }: { tone: NoticeTone; children: ReactNode; action?: ReactNode }) {
  return (
    <div
      role={tone === "error" ? "alert" : "status"}
      className={cx(
        "glass pointer-events-auto flex min-h-14 animate-island items-center gap-3 rounded-card px-4 py-2",
        TONE[tone].border,
      )}
    >
      {TONE[tone].icon}
      <p className="min-w-0 flex-1 text-[13.5px] wrap-break-word text-zinc-100">{children}</p>
      {action}
    </div>
  );
}
