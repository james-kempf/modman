// SPDX-License-Identifier: GPL-3.0-or-later
// The two primitives the IPC layer needs. Defaults to Tauri; tests and the
// plain Vite dev server can swap in a mock with `setTransport`.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";

export type Unlisten = () => void;

export interface Transport {
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten>;
}

const tauri: Transport = {
  invoke: (cmd, args) => tauriInvoke(cmd, args),
  listen: (event, handler) => tauriListen(event, (e) => handler(e.payload as never)),
};

let current: Transport = tauri;

export function setTransport(t: Transport): void {
  current = t;
}

export function resetTransport(): void {
  current = tauri;
}

export function transport(): Transport {
  return current;
}
