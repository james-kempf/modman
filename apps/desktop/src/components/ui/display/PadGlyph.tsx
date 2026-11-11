// SPDX-License-Identifier: GPL-3.0-or-later

/** Controller-mode keycap (LB/RB) at an end of a tab row: the pad: variant shows it only under gamepad input. */
export function PadGlyph({ children }: { children: string }) {
  return (
    <kbd className="hidden min-w-8 items-center justify-center rounded-control-sm border border-line bg-fill-1 px-1.5 py-0.5 font-mono text-micro font-semibold text-zinc-300 pad:inline-flex">
      {children}
    </kbd>
  );
}
