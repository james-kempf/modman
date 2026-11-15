// SPDX-License-Identifier: GPL-3.0-or-later
import type { Msg, Style } from "../../../ipc/types";

const SPAN: Record<Style, string> = {
  plain: "",
  bold: "font-semibold text-zinc-100",
  dim: "text-dim",
  green: "text-accent-300",
  yellow: "text-amber-300",
};
const LEVEL: Record<Msg["level"], string> = {
  heading: "font-sans font-medium text-zinc-100",
  info: "text-zinc-300",
  success: "text-accent-300",
  warn: "text-amber-300",
  error: "text-rose-300",
  line: "text-zinc-300",
  err_line: "text-rose-300",
};
export function MsgLine({ msg }: { msg: Msg }) {
  return (
    <span className={LEVEL[msg.level]}>
      {msg.spans.map((s, i) => (
        <span key={i} className={SPAN[s.style]}>
          {/* Streamed events stay in the session log: one malformed span must not break Activity for good. */}
          {typeof s.text === "string" ? s.text : JSON.stringify(s.text)}
        </span>
      ))}
    </span>
  );
}
