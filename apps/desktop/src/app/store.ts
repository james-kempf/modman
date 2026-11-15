// SPDX-License-Identifier: GPL-3.0-or-later
// UI state (Zustand): current view, live job/activity/prompt state fed by
// backend events, the one toast and the on-screen keyboard target. Server data
// lives in TanStack Query, not here.

import { create } from "zustand";
import { jobLabel, jobOutcome } from "./jobs";
import { cancelledMsg, failedMsg, jobDoneTitle, jobRunningTitle } from "./toastText";
import { bytes } from "../components/ui/format";
import type {
  ActivityEvent,
  DeployProgressEvent,
  DownloadProgress,
  JobEvent,
  LoaderLogEvent,
  NxmEvent,
  PromptEvent,
} from "../ipc/events";
import type { BrowseKind, BrowsePeriod, BrowseSort, LoaderPending, Preview, StartupCheck } from "../ipc/types";

export const VIEWS = [
  { id: "games", label: "Games" },
  { id: "overview", label: "Overview" },
  { id: "browse", label: "Browse" },
  { id: "mods", label: "Mods" },
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

export type ToastTone = "info" | "success" | "warn" | "error";
/**
 * The one toast: informational only (no controls), closes on its own after its tone's TOAST_TTL. Every writer
 * (components, hooks, store actions, IPC/event handlers, job progress) replaces it through `showToast`.
 */
export interface Toast {
  id: number;
  tone: ToastTone;
  text: string;
  /** A second, quieter line (a job's latest output, "deploying 3/10", an error). */
  detail?: string;
  /** Work in progress (a running job): the disc shows the animated rails instead of a tone glyph. */
  busy?: boolean;
  /**
   * A live (persistent) toast's key: it updates in place (no cycle, no dwell, no TTL) until its process ends
   * (`endProgress`). Absent on ordinary (transient) toasts.
   */
  persist?: string;
}
export interface ToastOpts {
  detail?: string;
  busy?: boolean;
}

/**
 * A loader op waiting for the consent dialog: a full preview (a Requirements
 * card op), or a pending one from install / a collection / an nxm link
 * (fetched in full when shown). Never run without the user's confirm.
 */
export interface LoaderConsent {
  id: number;
  game: string | null;
  preview?: Preview;
  pending?: LoaderPending;
}

/** The input the on-screen keyboard types into. */
export interface OskTarget {
  input: HTMLInputElement;
  label: string;
}

/**
 * Browse's listing state for this session (in memory): restored when the view
 * is entered again. Belongs to one game; another game starts from the defaults.
 */
export interface BrowseSession {
  game: string;
  kind: BrowseKind;
  period: BrowsePeriod;
  sort: BrowseSort;
  /** The search field's text (not submitted yet). */
  draft: string;
  /** The submitted search words. */
  text: string;
  /** Zero-based page of the listing. */
  page: number;
  /** data-browse-focus key of the last focused Browse control. */
  focus: string | null;
  /** <main>'s scrollTop when Browse was left. */
  scroll: number;
}
export const browseDefaults = (game: string): BrowseSession => ({
  game,
  kind: "mods",
  period: "all",
  sort: "endorsements",
  draft: "",
  text: "",
  page: 0,
  focus: null,
  scroll: 0,
});

/** The Mods view's sub-tabs. */
export type ModsTab = "list" | "conflicts" | "updates" | "profiles";
export type StateFilter = "all" | "enabled" | "disabled" | "conflicts" | "updates";
/** "" = any collection membership, "-" = in none, "*" = in any, else a collection slug. */
export type CollectionFilter = string;
export interface ModFilter {
  query: string;
  state: StateFilter;
  collection: CollectionFilter;
}
export const NO_FILTER: ModFilter = { query: "", state: "all", collection: "" };

/** The Mods view's UI state for this session (in memory): kept across view switches. */
export interface ModsUi {
  tab: ModsTab;
  /** The load order's search and filters, per game (collection slugs belong to one game). */
  filters: Record<string, ModFilter>;
}
const modsUiDefaults = (): ModsUi => ({ tab: "list", filters: {} });

const MAX_ACTIVITY = 500;
/** Jobs kept for Activity; past this the oldest finished ones are dropped. */
export const MAX_JOBS = 200;
/** Auto-close after (ms), counted from the latest write. */
export const TOAST_TTL: Record<ToastTone, number> = { info: 6000, success: 6000, warn: 10000, error: 10000 };
/** The one auto-close timer: it belongs to the current toast and is replaced with it. */
let toastTimer: ReturnType<typeof setTimeout> | undefined;
/** DESIGN.md `toast-min-dwell`: a shown message stays at least this long before the next replaces it. */
export const TOAST_MIN_DWELL_MS = 300;
/** Transient writes waiting for the shown message's dwell (FIFO, arrival order). Live toasts never enter it. */
const pending: Toast[] = [];
/** When the shown toast went up, and the timer that shows the next waiting one after its dwell. */
let shownAt = 0;
let dwellTimer: ReturnType<typeof setTimeout> | undefined;
/**
 * Live (persistent) toasts by key, in start order: the most recently started one is shown (when no transient
 * holds the slot); the others wait, counted as "+n running" in its detail. One slot per key: an update replaces
 * the entry's content, so updates received while hidden are applied, never queued.
 */
interface Live {
  order: number;
  toast: Toast;
}
const live = new Map<string, Live>();

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

  /** Deploy progress goes to the deploy job's live toast (the only progress indicator). */
  onDeployProgress(e: DeployProgressEvent): void;

  nxm: NxmEvent[];
  onNxm(e: NxmEvent): void;

  /** undefined = not received yet; null = the check failed or was skipped. */
  startupCheck: StartupCheck | null | undefined;
  setStartupCheck(c: StartupCheck | null): void;

  /** The one toast (no queue, no second source). Read it with `useToast()`, write it with `showToast()` (app/toast.ts). */
  toast: Toast | null;
  /** Replace the toast at once and restart its auto-close timer with the tone's TOAST_TTL. Returns the new id. */
  showToast(tone: ToastTone, text: string, opts?: ToastOpts): number;
  /** Close toast `id` if it is still the current one (its timeout calls this; a replaced id is a no-op). */
  closeToast(id: number): void;
  /** Start or update the live toast `key` (in place while shown; applied silently while hidden). */
  showProgress(key: string, tone: ToastTone, text: string, opts?: ToastOpts): void;
  /** End the live toast `key`; `outcome` (a normal transient toast) takes its place. */
  endProgress(key: string, outcome?: { tone: ToastTone; text: string; opts?: ToastOpts }): void;

  /**
   * Per game (id): a mod-set change left it out of sync with staging,
   * with the stamp of that change. Absent = nothing pending from this session
   * (the backend's changed_since_deploy still counts, see useDeployDue).
   */
  deployDue: Record<string, number>;
  markDeployDue(game: string): void;
  /**
   * In sync again (a deploy of `game` succeeded). With `since` (a stamp taken
   * when the deploy started), a change marked after it stays pending.
   */
  clearDeployDue(game: string, since?: number): void;
  /** A fresh ordering stamp (compare with deployDue values). */
  stamp(): number;

  /** Running command labels (button busy state). */
  busy: Record<string, number>;
  setBusy(label: string, on: boolean): void;

  loaderConsents: LoaderConsent[];
  /** Queue a consent dialog (the same preview once). */
  openLoaderConsent(c: Omit<LoaderConsent, "id">): void;
  dropLoaderConsent(id: number): void;
  /** Live installer output per preview id (redacted by the backend). */
  loaderLog: Record<string, string>;
  onLoaderLog(e: LoaderLogEvent): void;

  osk: OskTarget | null;
  openOsk(t: OskTarget): void;
  closeOsk(): void;

  /** The add-game dialog (the Games view's "Add game" tile) is open. */
  addGame: boolean;
  /** Prefill of the dialog's manual form (an unsupported game found in Steam). */
  addGamePrefill: AddGamePrefill | null;
  setAddGame(open: boolean, prefill?: AddGamePrefill): void;

  browse: BrowseSession | null;
  /** Merge into the game's Browse session (a session of another game is replaced). */
  saveBrowse(game: string, patch: Partial<Omit<BrowseSession, "game">>): void;

  modsUi: ModsUi;
  setModsTab(tab: ModsTab): void;
  setModsFilter(game: string, filter: ModFilter): void;
}

/** A game found in Steam with no preset: its name and install folder. */
export interface AddGamePrefill {
  name: string;
  path: string;
}

let seq = 0;

const initial = {
  view: "games" as ViewId,
  jobs: {},
  activity: [],
  prompts: [],
  downloads: {},
  nxm: [],
  startupCheck: undefined,
  toast: null,
  deployDue: {},
  busy: {},
  loaderConsents: [],
  loaderLog: {},
  osk: null,
  addGame: false,
  addGamePrefill: null as AddGamePrefill | null,
  browse: null as BrowseSession | null,
  modsUi: modsUiDefaults(),
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

  onJob: (e) => {
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
      const patch: Partial<UiState> = { jobs: capJobs({ ...s.jobs, [e.id]: entry }) };
      if (e.state !== "started") {
        // A question from a job that ended can no longer be answered: withdraw it.
        const orphans = s.prompts.filter((p) => p.job === e.id);
        if (orphans.length) {
          patch.prompts = s.prompts.filter((p) => p.job !== e.id);
          queueMicrotask(() =>
            get().showToast("info", "Question closed", {
              detail: `Job #${e.id} ended, so ${orphans.length === 1 ? "its question was" : "its questions were"} closed`,
            }),
          );
        }
      }
      return patch;
    });
    // The job's own toast (start or outcome), from the job entry just stored.
    jobToast(e.id, get);
  },

  onActivity: (e) => {
    set((s) => {
      const next = [...s.activity, { ...e, seq: ++seq, at: Date.now() }];
      return { activity: next.length > MAX_ACTIVITY ? next.slice(-MAX_ACTIVITY) : next };
    });
    const line = e.text.trim();
    if (e.job !== null && line) jobProgress(e.job, line, get);
  },

  onPrompt: (e) =>
    set((s) => (s.prompts.some((p) => p.id === e.id) ? s : { prompts: [...s.prompts, e] })),
  dropPrompt: (id) => set((s) => ({ prompts: s.prompts.filter((p) => p.id !== id) })),

  onDownload: (job, name, ev) => {
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
    });
    // The download's job shows it live: name and bytes so far (of the total when known).
    const d = get().downloads[name];
    if (job !== null && d && (ev.type === "started" || ev.type === "progress"))
      jobProgress(job, `${name} · ${bytes(d.done)}${d.total ? ` / ${bytes(d.total)}` : ""}`, get);
  },

  onDeployProgress: (e) => {
    if (e.job !== null) jobProgress(e.job, `${e.done} / ${e.total} files`, get);
  },

  onNxm: (e) => set((s) => ({ nxm: [e, ...s.nxm].slice(0, 50) })),

  setStartupCheck: (c) => set({ startupCheck: c }),

  showToast: (tone, text, opts = {}) => writeToast(makeToast(tone, text, opts)),
  closeToast: (id) => {
    const shown = get().toast;
    // Only the shown toast: a stale id (replaced, or still waiting) never closes or drops anything; a running
    // live toast closes only through endProgress.
    if (shown?.id !== id || (shown.persist && live.has(shown.persist))) return;
    clearTimeout(toastTimer);
    toastTimer = undefined;
    // Next: a waiting transient (its predecessor is gone), else the live toast returns, else nothing.
    if (pending.length) {
      clearTimeout(dwellTimer);
      dwellTimer = undefined;
      displayToast(pending.shift()!);
      scheduleNext();
    } else if (live.size) displayLive();
    else set({ toast: null });
  },
  showProgress: (key, tone, text, opts = {}) => {
    const prev = live.get(key);
    const toast = makeToast(tone, text, opts, prev?.toast.id);
    toast.persist = key;
    live.set(key, { order: prev?.order ?? ++seq, toast });
    refreshLive();
  },
  endProgress: (key, outcome) => {
    const was = live.delete(key);
    const shown = get().toast;
    // The outcome is written even if the process was never live (e.g. its start event was missed).
    if (outcome) writeToast(makeToast(outcome.tone, outcome.text, outcome.opts));
    if (!was) return;
    if (shown?.persist !== key || get().toast !== shown) {
      // Hidden, or already replaced by its outcome: just refresh the shown live toast's "+n running".
      refreshLive();
      return;
    }
    // Still showing the ended process (its outcome waits for the dwell, or there is none): the next live toast
    // takes over if nothing waits; else the frozen pill closes on a normal TTL unless the outcome replaces it.
    if (!pending.length && live.size) displayLive();
    else if (!pending.length) toastTimer = setTimeout(() => get().closeToast(shown.id), TOAST_TTL[shown.tone]);
  },

  markDeployDue: (game) => set((s) => ({ deployDue: { ...s.deployDue, [game]: ++seq } })),
  clearDeployDue: (game, since) =>
    set((s) => {
      const at = s.deployDue[game];
      // Changed again while the deploy ran: still due.
      if (at !== undefined && since !== undefined && at > since) return s;
      const deployDue = { ...s.deployDue };
      delete deployDue[game];
      return { deployDue };
    }),
  stamp: () => ++seq,

  setBusy: (label, on) =>
    set((s) => {
      const busy = { ...s.busy };
      const n = (busy[label] ?? 0) + (on ? 1 : -1);
      if (n > 0) busy[label] = n;
      else delete busy[label];
      return { busy };
    }),

  openLoaderConsent: (c) =>
    set((s) => {
      const pid = c.preview?.preview_id ?? c.pending?.preview_id;
      if (s.loaderConsents.some((x) => (x.preview?.preview_id ?? x.pending?.preview_id) === pid)) return s;
      return { loaderConsents: [...s.loaderConsents, { ...c, id: ++seq }] };
    }),
  dropLoaderConsent: (id) => set((s) => ({ loaderConsents: s.loaderConsents.filter((c) => c.id !== id) })),
  onLoaderLog: (e) =>
    set((s) => {
      const next = (s.loaderLog[e.preview_id] ?? "") + e.text;
      // Bounded: the full log stays on disk (loader_log_get).
      return { loaderLog: { ...s.loaderLog, [e.preview_id]: next.length > 200_000 ? next.slice(-200_000) : next } };
    }),

  openOsk: (t) => set({ osk: t }),
  closeOsk: () => set({ osk: null }),

  setAddGame: (open, prefill) => set({ addGame: open, addGamePrefill: (open && prefill) || null }),

  saveBrowse: (game, patch) =>
    set((s) => ({ browse: { ...(s.browse?.game === game ? s.browse : browseDefaults(game)), ...patch } })),

  setModsTab: (tab) => set((s) => ({ modsUi: { ...s.modsUi, tab } })),
  setModsFilter: (game, filter) =>
    set((s) => ({ modsUi: { ...s.modsUi, filters: { ...s.modsUi.filters, [game]: filter } } })),
}));

/** Drop the oldest finished jobs past MAX_JOBS (running ones always stay). */
function capJobs(jobs: Record<number, JobEntry>): Record<number, JobEntry> {
  const ids = Object.keys(jobs).map(Number);
  if (ids.length <= MAX_JOBS) return jobs;
  const drop = ids
    .filter((id) => jobs[id].state !== "started")
    .sort((a, b) => a - b)
    .slice(0, ids.length - MAX_JOBS);
  const out = { ...jobs };
  for (const id of drop) delete out[id];
  return out;
}

/** Test helper: back to the initial state. */
export function resetUi(): void {
  clearTimeout(toastTimer);
  clearTimeout(dwellTimer);
  dwellTimer = undefined;
  pending.length = 0;
  live.clear();
  shownAt = 0;
  useUi.setState({ ...initial, modsUi: modsUiDefaults() });
}

/** A job's live toast key. */
const jobKey = (job: number) => `job:${job}`;

/** A job's toast: live while it runs (start, then progress in place), then its outcome as a normal toast. */
function jobToast(job: number, get: () => UiState): void {
  const j = get().jobs[job];
  if (!j) return;
  const label = jobLabel(j.kind);
  if (j.state === "started") {
    get().showProgress(jobKey(job), "info", jobRunningTitle(j.kind), { detail: label, busy: true });
    return;
  }
  // Messages show in arrival order: the outcome takes the job's live toast's place like any other write.
  const outcome = jobOutcome(j);
  if (outcome === "finished") {
    // A deploy's success is reported once, by its command (useDeploy: "Deployed" · "N files · M mods…"), which
    // says more than the generic "Deployed" · "Deploy". Its failure and cancellation still come from here.
    get().endProgress(
      jobKey(job),
      j.kind === "deploy" ? undefined : { tone: "success", text: jobDoneTitle(j.kind), opts: { detail: label } },
    );
  } else if (outcome === "cancelled") {
    const m = cancelledMsg(label);
    get().endProgress(jobKey(job), { tone: "info", text: m.title, opts: m.detail ? { detail: m.detail } : {} });
  } else {
    const m = failedMsg(label, `${j.error ? `${j.error} · ` : ""}see Activity`);
    get().endProgress(jobKey(job), { tone: "error", text: m.title, opts: { detail: m.detail } });
  }
}

/** A running job's progress (output line, deploy n/total, download bytes): its live toast, updated in place. */
function jobProgress(job: number, detail: string, get: () => UiState): void {
  const j = get().jobs[job];
  if (!j || j.state !== "started") return;
  get().showProgress(jobKey(job), "info", jobRunningTitle(j.kind), { detail, busy: true });
}

function makeToast(tone: ToastTone, text: string, opts: ToastOpts = {}, id = ++seq): Toast {
  const toast: Toast = { id, tone, text };
  if (opts.detail) toast.detail = opts.detail;
  if (opts.busy) toast.busy = true;
  return toast;
}

/**
 * A transient write: show it now if the shown toast has had its TOAST_MIN_DWELL_MS and nothing is waiting,
 * else queue it (FIFO, arrival order). It takes the slot from a live toast too (which returns later).
 */
function writeToast(toast: Toast): number {
  const ui = useUi.getState();
  const dwelt = !ui.toast || Date.now() - shownAt >= TOAST_MIN_DWELL_MS;
  if (dwelt && !pending.length) {
    displayToast(toast);
    return toast.id;
  }
  pending.push(toast);
  scheduleNext();
  return toast.id;
}

/** Show a transient `toast` now: it replaces the shown one, and its auto-close (tone TTL) counts from now. */
function displayToast(toast: Toast): void {
  clearTimeout(toastTimer);
  shownAt = Date.now();
  useUi.setState({ toast });
  toastTimer = setTimeout(() => useUi.getState().closeToast(toast.id), TOAST_TTL[toast.tone]);
}

/** The live toast to show: the most recently started, with "+n running" for the others. */
function liveView(): Toast | null {
  let top: Live | undefined;
  for (const l of live.values()) if (!top || l.order > top.order) top = l;
  if (!top) return null;
  const n = live.size - 1;
  if (!n) return top.toast;
  const more = `+${n} running`;
  return { ...top.toast, detail: top.toast.detail ? `${top.toast.detail} · ${more}` : more };
}

/** Show the live toast now (no TTL: it stays until its process ends). */
function displayLive(): void {
  const t = liveView();
  if (!t) return;
  clearTimeout(toastTimer);
  toastTimer = undefined;
  shownAt = Date.now();
  useUi.setState({ toast: t });
}

/** A live toast changed: update it in place if it is the shown one, show it if the slot is free. */
function refreshLive(): void {
  const shown = useUi.getState().toast;
  const t = liveView();
  if (!t) return;
  if (!shown) {
    if (!pending.length) displayLive();
  } else if (shown.persist && live.has(shown.persist)) {
    // In place: same key, no new dwell (shownAt unchanged); a newer process takes over the slot.
    if (shown.persist === t.persist) useUi.setState({ toast: t });
    else displayLive();
  }
  // A transient holds the slot: the live toast returns, with its latest content, when the transients are done.
}

/** Show the next waiting toast when the shown one's dwell is up (one timer at a time). */
function scheduleNext(): void {
  if (dwellTimer || !pending.length) return;
  const wait = Math.max(0, shownAt + TOAST_MIN_DWELL_MS - Date.now());
  dwellTimer = setTimeout(() => {
    dwellTimer = undefined;
    const next = pending.shift();
    if (!next) return;
    displayToast(next);
    scheduleNext();
  }, wait);
}
