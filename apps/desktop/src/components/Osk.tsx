// SPDX-License-Identifier: GPL-3.0-or-later
// On-screen keyboard fallback for gamepad text entry when Steam's keyboard
// does not appear. Types into the target input on "Done".

import { useState } from "react";
import { flushSync } from "react-dom";
import { useUi } from "../app/store";
import { Dialog } from "./Dialog";
import { Button, cx } from "./ui";

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
        className="mb-4 block min-h-12 rounded-xl border border-white/10 bg-ink px-4 py-3 font-mono text-lg break-all text-zinc-100"
      >
        {show ? value : "•".repeat(value.length)}
        <span className="text-accent-400">▏</span>
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
                  className="min-h-12 min-w-12 rounded-lg border border-white/10 bg-ink-3 font-mono text-lg text-zinc-100"
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
            "min-h-12 rounded-xl border px-5",
            shift ? "border-accent-500/60 bg-accent-950 text-accent-200" : "border-white/10 bg-ink-3",
          )}
        >
          Shift
        </button>
        <Button onClick={() => setValue((v) => v.slice(0, -1))}>Backspace</Button>
        <Button onClick={() => setValue("")}>Clear</Button>
        <Button onClick={paste}>Paste</Button>
        {secret && <Button onClick={() => setShow((s) => !s)}>{show ? "Hide" : "Show"}</Button>}
        <Button onClick={close}>Cancel</Button>
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
