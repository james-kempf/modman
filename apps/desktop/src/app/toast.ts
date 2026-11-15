// SPDX-License-Identifier: GPL-3.0-or-later
// The toast API: one shown toast, writes in arrival order; live toasts (showProgress/endProgress) for ongoing
// processes stay up and update in place until they end. Anything may write it (components, hooks, store
// actions, IPC/event handlers). A write replaces the shown message once that one has been up for
// TOAST_MIN_DWELL_MS (300 ms; at once if it already has), else it waits its turn (FIFO, capped: see store.ts).
// The auto-close timer (TOAST_TTL: 6 s info/success, 10 s warn/error) starts when a message is shown. There is
// no other toast source: running jobs write their progress here too (store.ts, throttled, merged).

import { useUi, type Toast, type ToastOpts, type ToastTone } from "./store";
import type { ToastMsg } from "./toastText";

export type { Toast, ToastOpts, ToastTone };
export { cancelledMsg, failedMsg, type ToastMsg } from "./toastText";

/**
 * Show `text`: now, or after the shown message's minimum dwell. `opts.detail` adds a quieter second line;
 * `opts.busy` shows the running-work disc. Returns the toast's id.
 *
 *   showToast("success", "Uninstalled SkyUI");
 *   showToast("info", "Deploy", { detail: "deploying 3/10", busy: true });
 */
export function showToast(tone: ToastTone, text: string, opts?: ToastOpts): number {
  return useUi.getState().showToast(tone, text, opts);
}

/** `showToast` for a title + detail message (app/toastText.ts copy rule): `showMsg("success", { title: "Uninstalled", detail: "SkyUI" })`. */
export function showMsg(tone: ToastTone, m: ToastMsg, opts?: Omit<ToastOpts, "detail">): number {
  return showToast(tone, m.title, m.detail ? { ...opts, detail: m.detail } : opts);
}

/**
 * A live (persistent) toast for an ongoing process, keyed by `key`: the first call shows it (or queues it behind
 * transient toasts), later calls update it in place with no motion, no dwell and no auto-close. Ordinary toasts
 * take the slot meanwhile; it returns with its latest content when they are done.
 *
 *   showProgress("download:x", "info", "Downloading", { detail: "SkyUI · 2 MB / 5 MB", busy: true });
 */
export function showProgress(key: string, tone: ToastTone, text: string, opts?: ToastOpts): void {
  useUi.getState().showProgress(key, tone, text, opts);
}

/** End live toast `key`; `outcome` (a normal toast: dwell, TTL, cycle) takes its place. */
export function endProgress(key: string, outcome?: { tone: ToastTone; text: string; opts?: ToastOpts }): void {
  useUi.getState().endProgress(key, outcome);
}

/** Close toast `id` now, if it is still the one showing (a replaced id is ignored). */
export function closeToast(id: number): void {
  useUi.getState().closeToast(id);
}

/** The current toast (null when none), for rendering. */
export function useToast(): Toast | null {
  return useUi((s) => s.toast);
}
