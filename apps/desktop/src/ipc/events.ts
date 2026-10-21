// SPDX-License-Identifier: GPL-3.0-or-later
// Backend events (apps/desktop/src-tauri/src/events.rs) and typed listeners.

import { transport, type Unlisten } from "./transport";
import type { Msg, Prompt, StartupCheck } from "./types";

export const EVENTS = {
  activity: "modman:activity",
  job: "modman:job",
  prompt: "modman:prompt",
  promptClosed: "modman:prompt-closed",
  download: "modman:download",
  deployProgress: "modman:deploy-progress",
  nxm: "modman:nxm",
  startupCheck: "modman:startup-check",
} as const;

/** A core message for the activity log. */
export interface ActivityEvent {
  job: number | null;
  msg: Msg;
  /** The message without styles. */
  text: string;
}

export interface JobEvent {
  id: number;
  /** Command that started it, e.g. "install", "deploy", "nxm". */
  kind: string;
  state: "started" | "finished" | "failed";
  error: string | null;
}

/** Answer with `promptAnswer(id, ...)`. */
export interface PromptEvent {
  id: number;
  job: number | null;
  prompt: Prompt;
}

/** The prompt was withdrawn (job ended or app closing). */
export interface PromptClosedEvent {
  id: number;
}

export type DownloadProgress =
  | { type: "started"; resumed_from: number; total: number | null }
  | { type: "progress"; done: number; total: number | null }
  | { type: "retrying"; attempt: number; reason: string }
  | { type: "finished"; size: number; sha256: string };

export interface DownloadEvent {
  job: number | null;
  name: string;
  event: DownloadProgress;
}

export interface DeployProgressEvent {
  job: number | null;
  done: number;
  total: number;
}

/** Never carries the link's key/expires. */
export interface NxmEvent {
  status: "accepted" | "rejected";
  link: string | null;
  row: number | null;
  error: string | null;
}

export interface EventPayloads {
  "modman:activity": ActivityEvent;
  "modman:job": JobEvent;
  "modman:prompt": PromptEvent;
  "modman:prompt-closed": PromptClosedEvent;
  "modman:download": DownloadEvent;
  "modman:deploy-progress": DeployProgressEvent;
  "modman:nxm": NxmEvent;
  /** null = the check failed (see the log). */
  "modman:startup-check": StartupCheck | null;
}
export type EventName = keyof EventPayloads;

export function on<E extends EventName>(
  event: E,
  handler: (payload: EventPayloads[E]) => void,
): Promise<Unlisten> {
  return transport().listen<EventPayloads[E]>(event, handler);
}
