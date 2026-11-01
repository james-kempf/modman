// SPDX-License-Identifier: GPL-3.0-or-later
// A failed loader command, rendered through its typed code
// (ipc/loaderErrors.ts): title, explanation and the suggested next step.
// Errors without a known code show their message.

import { errMsg } from "../../app/bridge";
import { useUi } from "../../app/store";
import { loaderErrorInfo, type LoaderErrorAction } from "../../ipc/loaderErrors";
import { Button, ErrorNote } from "../ui";

const ACTION_LABEL: Partial<Record<LoaderErrorAction, string>> = {
  retry: "Try again",
  pick_runtime: "Choose runtime",
  adopt: "Adopt",
  open_log: "Open log",
  open_settings: "Open Settings",
};

export type LoaderErrorHandlers = Partial<Record<LoaderErrorAction, () => void>>;

/** `handlers`: the actions this place can carry out (open_settings always can). */
export function LoaderErrorNote({ error, handlers = {} }: { error: unknown; handlers?: LoaderErrorHandlers }) {
  const setView = useUi((s) => s.setView);
  const info = loaderErrorInfo(error);
  const msg = errMsg(error);
  if (!info) return <ErrorNote>{msg}</ErrorNote>;
  const run = info.action === "open_settings" ? () => setView("settings") : handlers[info.action];
  const label = ACTION_LABEL[info.action];
  return (
    <ErrorNote
      actions={
        run && label ? (
          <Button size="sm" onClick={run}>
            {label}
          </Button>
        ) : undefined
      }
    >
      <strong className="font-medium">{info.title}.</strong> {info.explanation}
      <span className="mt-1 block font-mono text-caption wrap-anywhere">{msg}</span>
    </ErrorNote>
  );
}
