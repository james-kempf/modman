// SPDX-License-Identifier: GPL-3.0-or-later
// UI state (Zustand): current view, live job/activity/prompt state fed by
// backend events, notices and the on-screen keyboard target. Server data
// lives in TanStack Query, not here.

import { create } from "zustand";
import type {
  ActivityEvent,
  DeployProgressEvent,
  DownloadProgress,
  JobEvent,
  NxmEvent,
  PromptEvent,
} from "../ipc/events";
import type { StartupCheck } from "../ipc/types";

export const VIEWS = [
  { id: "overview", label: "Overview" },
  { id: "mods", label: "Mods" },
  { id: "install", label: "Install" },
  { id: "collections", label: "Collections" },
  { id: "requirements", label: "Requirements" },
  { id: "activity", label: "Activity" },
  { id: "settings", label: "Settings" },
] as const;
export type ViewId = (typeof VIEWS)[number]["id"];

export interface JobEntry {
  id: number;
  kind: string;
  state: JobEvent["state"];
  error: string | null;
  startedAt: number;
  finishedAt: number | null;
}

export interface ActivityEntry extends ActivityEvent {
  seq: number;
  at: number;
}

export interface DownloadEntry {
  name: string;
  job: number | null;
  last: DownloadProgress;
  done: number;
  total: number | null;
}

export type NoticeTone = "info" | "success" | "warn" | "error";
export interface Notice {
  id: number;
  tone: NoticeTone;
  text: string;
  /** Offer a "Deploy now" button. */
  deploy?: boolean;
}

/** The input the on-screen keyboard types into. */
export interface OskTarget {
  input: HTMLInputElement;
  label: string;
}

const MAX_ACTIVITY = 500;
const MAX_NOTICES = 3;
/** Auto-dismiss after (ms); null = stays until dismissed. "Deploy now" notices always stay. */
const NOTICE_TTL: Record<NoticeTone, number | null> = { info: 6000, success: 6000, warn: 10000, error: null };

export interface UiState {
  view: ViewId;
  setView(view: ViewId): void;
  cycleView(delta: 1 | -1): ViewId;

  jobs: Record<number, JobEntry>;
  onJob(e: JobEvent): void;

  activity: ActivityEntry[];
  onActivity(e: ActivityEvent): void;

  prompts: PromptEvent[];
  onPrompt(e: PromptEvent): void;
  dropPrompt(id: number): void;

  downloads: Record<string, DownloadEntry>;
  onDownload(job: number | null, name: string, ev: DownloadProgress): void;

  deployProgress: DeployProgressEvent | null;
  onDeployProgress(e: DeployProgressEvent): void;

  nxm: NxmEvent[];
  onNxm(e: NxmEvent): void;

  /** undefined = not received yet; null = the check failed or was skipped. */
  startupCheck: StartupCheck | null | undefined;
  setStartupCheck(c: StartupCheck | null): void;

  notices: Notice[];
  notify(tone: NoticeTone, text: string, deploy?: boolean): void;
  dismiss(id: number): void;

  /** A mod-set change left the game out of sync with staging. */
  needsDeploy: boolean;
  setNeedsDeploy(v: boolean): void;

  /** Running command labels (button busy state). */
  busy: Record<string, number>;
  setBusy(label: string, on: boolean): void;

  osk: OskTarget | null;
  openOsk(t: OskTarget): void;
  closeOsk(): void;

  /** The add-game dialog (sidebar "Add game", Overview empty state) is open. */
  addGame: boolean;
  setAddGame(open: boolean): void;
}

let seq = 0;

const initial = {
  view: "overview" as ViewId,
  jobs: {},
  activity: [],
  prompts: [],
  downloads: {},
  deployProgress: null,
  nxm: [],
  startupCheck: undefined,
  notices: [],
  needsDeploy: false,
  busy: {},
  osk: null,
  addGame: false,
};

export const useUi = create<UiState>()((set, get) => ({
  ...initial,

  setView: (view) => set({ view }),
  cycleView: (delta) => {
    const i = VIEWS.findIndex((v) => v.id === get().view);
    const view = VIEWS[(i + delta + VIEWS.length) % VIEWS.length].id;
    set({ view });
    return view;
  },

  onJob: (e) =>
    set((s) => {
      const prev = s.jobs[e.id];
      const now = Date.now();
      const entry: JobEntry = {
        id: e.id,
        kind: e.kind,
        state: e.state,
        error: e.error,
        startedAt: prev?.startedAt ?? now,
        finishedAt: e.state === "started" ? null : now,
      };
      const patch: Partial<UiState> = { jobs: { ...s.jobs, [e.id]: entry } };
      if (e.state !== "started" && s.deployProgress?.job === e.id) patch.deployProgress = null;
      return patch;
    }),

  onActivity: (e) =>
    set((s) => {
      const next = [...s.activity, { ...e, seq: ++seq, at: Date.now() }];
      return { activity: next.length > MAX_ACTIVITY ? next.slice(-MAX_ACTIVITY) : next };
    }),

  onPrompt: (e) =>
    set((s) => (s.prompts.some((p) => p.id === e.id) ? s : { prompts: [...s.prompts, e] })),
  dropPrompt: (id) => set((s) => ({ prompts: s.prompts.filter((p) => p.id !== id) })),

  onDownload: (job, name, ev) =>
    set((s) => {
      const prev = s.downloads[name];
      let done = prev?.done ?? 0;
      let total = prev?.total ?? null;
      if (ev.type === "started") {
        done = ev.resumed_from;
        total = ev.total;
      } else if (ev.type === "progress") {
        done = ev.done;
        total = ev.total ?? total;
      } else if (ev.type === "finished") {
        done = ev.size;
        total = ev.size;
      }
      return { downloads: { ...s.downloads, [name]: { name, job, last: ev, done, total } } };
    }),

  onDeployProgress: (e) => set({ deployProgress: e }),

  onNxm: (e) => set((s) => ({ nxm: [e, ...s.nxm].slice(0, 50) })),

  setStartupCheck: (c) => set({ startupCheck: c }),

  notify: (tone, text, deploy) => {
    const id = ++seq;
    set((s) => ({
      // The same notice again replaces the old one (moved to the end, fresh timeout) instead of stacking.
      notices: [...s.notices.filter((n) => !(n.tone === tone && n.text === text)), { id, tone, text, deploy }].slice(
        -MAX_NOTICES,
      ),
    }));
    const ttl = NOTICE_TTL[tone];
    if (ttl && !deploy) setTimeout(() => get().dismiss(id), ttl);
  },
  dismiss: (id) => set((s) => ({ notices: s.notices.filter((n) => n.id !== id) })),

  setNeedsDeploy: (v) => set({ needsDeploy: v }),

  setBusy: (label, on) =>
    set((s) => {
      const busy = { ...s.busy };
      const n = (busy[label] ?? 0) + (on ? 1 : -1);
      if (n > 0) busy[label] = n;
      else delete busy[label];
      return { busy };
    }),

  openOsk: (t) => set({ osk: t }),
  closeOsk: () => set({ osk: null }),

  setAddGame: (open) => set({ addGame: open }),
}));

/** Test helper: back to the initial state. */
export function resetUi(): void {
  useUi.setState({ ...initial });
}
