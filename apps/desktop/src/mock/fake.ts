// SPDX-License-Identifier: GPL-3.0-or-later
// In-memory Transport: command handlers + an event emitter. Used by the
// tests and by the demo backend under plain `vite dev`.

import type { EventName, EventPayloads } from "../ipc/events";
import type { Transport, Unlisten } from "../ipc/transport";

export type Args = Record<string, unknown> | undefined;
export type Handler = (args: Args) => unknown;

export interface FakeTransport extends Transport {
  /** Every invoke, in order. */
  calls: { cmd: string; args: Args }[];
  /** Every listen, in order (event names). */
  listened: string[];
  handlers: Record<string, Handler>;
  emit<E extends EventName>(event: E, payload: EventPayloads[E]): void;
  callsOf(cmd: string): Args[];
}

export function createFakeTransport(handlers: Record<string, Handler> = {}): FakeTransport {
  const listeners = new Map<string, Set<(p: unknown) => void>>();
  const t: FakeTransport = {
    calls: [],
    listened: [],
    handlers,
    async invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
      t.calls.push({ cmd, args });
      const h = t.handlers[cmd];
      if (!h) throw { message: `no mock for ${cmd}` };
      return (await h(args)) as T;
    },
    async listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten> {
      t.listened.push(event);
      const set = listeners.get(event) ?? new Set();
      listeners.set(event, set);
      const fn = handler as (p: unknown) => void;
      set.add(fn);
      return () => set.delete(fn);
    },
    emit(event, payload) {
      listeners.get(event)?.forEach((fn) => fn(payload));
    },
    callsOf(cmd) {
      return t.calls.filter((c) => c.cmd === cmd).map((c) => c.args);
    },
  };
  return t;
}
