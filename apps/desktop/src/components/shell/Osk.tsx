// SPDX-License-Identifier: GPL-3.0-or-later
// On-screen keyboard fallback for gamepad text entry when Steam's keyboard
// does not appear. Types into the target input on "Done".

import { useState } from "react";
import { flushSync } from "react-dom";
import { useUi } from "../../app/store";
import { ArrowFatUpIcon, BackspaceIcon, ClipboardTextIcon } from "@phosphor-icons/react";
import { Dialog } from "../dialogs/Dialog";
import { Button, cx, TILE } from "../ui";

const ROWS = ["1234567890", "qwertyuiop", "asdfghjkl", "zxcvbnm", "-_+/=.:@"];

/** Set an input's value so React's onChange sees it. */
export function setNativeValue(input: HTMLInputElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  setter?.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

/** Read the clipboard; null when the webview refuses. */
export async function readClipboard(): Promise<string | null> {
  try {
    return (await navigator.clipboard?.readText()) ?? null;
  } catch {
    return null;
  }
}

export function OskHost() {
  const target = useUi((s) => s.osk);
  if (!target) return null;
  // Remount per target so the buffer starts from that input's value.
  return <Osk key={target.input.id || target.label} />;
}

function Osk() {
  const target = useUi((s) => s.osk)!;
  const close = useUi((s) => s.closeOsk);
  const notify = useUi((s) => s.notify);
  const secret = target.input.type === "password";
  const [value, setValue] = useState(target.input.value);
  const [shift, setShift] = useState(false);
  const [show, setShow] = useState(!secret);

  const type = (ch: string) => setValue((v) => v + (shift ? ch.toUpperCase() : ch));
  // Inputs marked `data-osk-submit="<action>"` get a button that also submits their form.
  const submitLabel = target.input.dataset.oskSubmit;
  const done = (submit = false) => {
    const input = target.input;
    // Commit the owner's state now so its onSubmit sees the new value.
    flushSync(() => setNativeValue(input, value));
    close(); // the Dialog returns focus to the input once the app root is no longer inert
    if (submit) input.form?.requestSubmit();
  };
  const paste = async () => {
    const t = await readClipboard();
    if (t === null) notify("warn", "Clipboard not readable here; paste into the field with Ctrl+V.");
    else setValue((v) => v + t.trim());
  };

  return (
    <Dialog title={`Keyboard: ${target.label}`} onBack={close} wide returnFocus={target.input}>
      <output
        aria-label="Typed text"
        className={cx(TILE, "mb-5 block min-h-control-lg border-line px-4 py-3.5 font-mono text-lg break-all text-zinc-100")}
      >
        {show ? value : "•".repeat(value.length)}
        <span aria-hidden className="ml-px inline-block h-5 w-0.5 translate-y-0.5 animate-pulse bg-accent-400" />
      </output>
      <div className="grid gap-2">
        {ROWS.map((row) => (
          <div key={row} className="flex justify-center gap-2">
            {[...row].map((ch, i) => {
              const k = shift ? ch.toUpperCase() : ch;
              return (
                <button
                  key={ch}
                  type="button"
                  data-autofocus={row === ROWS[0] && i === 0 ? true : undefined}
                  onClick={() => type(ch)}
                  className="min-h-control min-w-control rounded-control border border-hairline bg-fill-2 font-mono text-lg text-zinc-100 transition-colors duration-fast hover:bg-fill-hover active:scale-95"
                >
                  {k}
                </button>
              );
            })}
          </div>
        ))}
      </div>
      <div className="mt-4 flex flex-wrap justify-center gap-2">
        <button
          type="button"
          aria-pressed={shift}
          onClick={() => setShift((s) => !s)}
          className={cx(
            "inline-flex min-h-control items-center gap-2 rounded-control border px-4 text-body font-medium transition-colors duration-base",
            shift
              ? "border-accent-400/40 bg-accent-400/10 text-accent-200"
              : "border-hairline bg-fill-2 text-zinc-100 hover:bg-fill-hover",
          )}
        >
          <ArrowFatUpIcon aria-hidden size={16} weight={shift ? "fill" : "regular"} />
          Shift
        </button>
        <Button icon={<BackspaceIcon aria-hidden size={16} />} onClick={() => setValue((v) => v.slice(0, -1))}>
          Backspace
        </Button>
        <Button onClick={() => setValue("")}>Clear</Button>
        <Button icon={<ClipboardTextIcon aria-hidden size={16} />} onClick={paste}>
          Paste
        </Button>
        {secret && <Button onClick={() => setShow((s) => !s)}>{show ? "Hide" : "Show"}</Button>}
        <Button tone="ghost" onClick={close}>
          Cancel
        </Button>
        <Button tone={submitLabel ? "default" : "primary"} onClick={() => done()}>
          Done
        </Button>
        {submitLabel && (
          <Button tone="primary" disabled={!value.trim()} onClick={() => done(true)}>
            {submitLabel}
          </Button>
        )}
      </div>
    </Dialog>
  );
}
