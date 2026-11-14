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

export type NoticeTone = "info" | "success" | "warn" | "error";
export interface Notice {
  id: number;
  tone: NoticeTone;
  text: string;
  /** Offer a "Deploy now" button. */
  deploy?: boolean;
  /** The game (id) a "Deploy now" notice is about: shown and acted on only for it. */
  game?: string;
  /** A one-off action button (e.g. Undo) in place of "Deploy now". */
  action?: NoticeAction;
}
export interface NoticeAction {
  label: string;
  onAction(): void;
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
  /**
   * Per game: the "before-<name>" profiles the profile-load flow saved itself. Only these
   * are replaced by a later snapshot; any other profile of that name is the user's.
   */
  snapshots: Record<string, string[]>;
}
const modsUiDefaults = (): ModsUi => ({ tab: "list", filters: {}, snapshots: {} });

const MAX_ACTIVITY = 500;
/** Jobs kept for Activity; past this the oldest finished ones are dropped. */
export const MAX_JOBS = 200;
const MAX_NOTICES = 3;
/** Auto-dismiss after (ms); every notice closes on its own, "Deploy now" ones included. */
const NOTICE_TTL: Record<NoticeTone, number> = { info: 6000, success: 6000, warn: 10000, error: 10000 };
/** A notice with an action (Undo) stays at least this long: time to reach it with D-pad or Tab. */
export const ACTION_NOTICE_TTL = 15000;
/** Pending auto-dismiss timers, and the notices focus is in (their timer waits until focus leaves). */
const noticeTimers = new Map<number, ReturnType<typeof setTimeout>>();
const heldNotices = new Set<number>();

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
  /** The game a deploy started from the UI runs for (null: none, or started elsewhere). */
  deployGame: string | null;
  setDeployGame(game: string | null): void;

  nxm: NxmEvent[];
  onNxm(e: NxmEvent): void;

  /** undefined = not received yet; null = the check failed or was skipped. */
  startupCheck: StartupCheck | null | undefined;
  setStartupCheck(c: StartupCheck | null): void;

  notices: Notice[];
  /**
   * `game`: the notice belongs to that game (id) and, without `action`, offers "Deploy now".
   * `action`: one button (e.g. Undo); using it dismisses the notice, then calls `onAction`.
   */
  notify(tone: NoticeTone, text: string, game?: string, action?: NoticeAction): void;
  dismiss(id: number): void;
  /** Use a notice's action: dismiss it, then run the action. */
  runNoticeAction(id: number): void;
  /** Focus entered (true) or left (false) a notice: its auto-dismiss waits, then restarts in full. */
  holdNotice(id: number, held: boolean): void;

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
  /**
   * owned: the load flow saved this snapshot profile. Not owned: the user saved or deleted a
   * profile of that name, so it is theirs and is never replaced.
   */
  markProfileSnapshot(game: string, name: string, owned: boolean): void;
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
  deployProgress: null,
  deployGame: null,
  nxm: [],
  startupCheck: undefined,
  notices: [],
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
      const patch: Partial<UiState> = { jobs: capJobs({ ...s.jobs, [e.id]: entry }) };
      if (e.state !== "started") {
        if (s.deployProgress?.job === e.id) patch.deployProgress = null;
        // A question from a job that ended can no longer be answered: withdraw it.
        const orphans = s.prompts.filter((p) => p.job === e.id);
        if (orphans.length) {
          patch.prompts = s.prompts.filter((p) => p.job !== e.id);
          queueMicrotask(() =>
            get().notify("info", `${orphans.length === 1 ? "A question" : "Questions"} from job #${e.id} closed: the job ended.`),
          );
        }
      }
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
  setDeployGame: (deployGame) => set({ deployGame }),

  onNxm: (e) => set((s) => ({ nxm: [e, ...s.nxm].slice(0, 50) })),

  setStartupCheck: (c) => set({ startupCheck: c }),

  notify: (tone, text, game, action) => {
    const id = ++seq;
    const deploy = game !== undefined && !action;
    const notice: Notice = deploy ? { id, tone, text, deploy, game } : { id, tone, text };
    if (action) {
      notice.action = action;
      if (game !== undefined) notice.game = game;
    }
    set((s) => ({
      // The same notice again replaces the old one (moved to the end, fresh timeout) instead of stacking.
      notices: [...s.notices.filter((n) => !(n.tone === tone && n.text === text && n.game === game)), notice].slice(
        -MAX_NOTICES,
      ),
    }));
    // Drop the timers of notices that were replaced or pushed out.
    const live = new Set(get().notices.map((n) => n.id));
    for (const [old, t] of noticeTimers) {
      if (live.has(old)) continue;
      clearTimeout(t);
      noticeTimers.delete(old);
      heldNotices.delete(old);
    }
    armNotice(id, action ? Math.max(ACTION_NOTICE_TTL, NOTICE_TTL[tone]) : NOTICE_TTL[tone], get);
  },
  dismiss: (id) => {
    clearTimeout(noticeTimers.get(id));
    noticeTimers.delete(id);
    heldNotices.delete(id);
    set((s) => ({ notices: s.notices.filter((n) => n.id !== id) }));
  },
  runNoticeAction: (id) => {
    const action = get().notices.find((n) => n.id === id)?.action;
    if (!action) return;
    get().dismiss(id);
    action.onAction();
  },
  holdNotice: (id, held) => {
    const notice = get().notices.find((n) => n.id === id);
    if (!notice) return;
    if (held) {
      heldNotices.add(id);
      clearTimeout(noticeTimers.get(id));
      noticeTimers.delete(id);
    } else if (heldNotices.delete(id)) {
      const ttl = NOTICE_TTL[notice.tone];
      armNotice(id, notice.action ? Math.max(ACTION_NOTICE_TTL, ttl) : ttl, get);
    }
  },

  markDeployDue: (game) => set((s) => ({ deployDue: { ...s.deployDue, [game]: ++seq } })),
  clearDeployDue: (game, since) =>
    set((s) => {
      const at = s.deployDue[game];
      // Changed again while the deploy ran: still due, keep its notice.
      if (at !== undefined && since !== undefined && at > since) return s;
      const deployDue = { ...s.deployDue };
      delete deployDue[game];
      // A "Deploy now" notice for this game would offer a deploy that is no longer due.
      return { deployDue, notices: s.notices.filter((n) => !(n.deploy && n.game === game)) };
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
  markProfileSnapshot: (game, name, owned) =>
    set((s) => {
      const have = s.modsUi.snapshots[game] ?? [];
      if (have.includes(name) === owned) return s;
      const next = owned ? [...have, name] : have.filter((n) => n !== name);
      return { modsUi: { ...s.modsUi, snapshots: { ...s.modsUi.snapshots, [game]: next } } };
    }),
}));

/** Drop the oldest finished jobs past MAX_JOBS (running ones always stay). */
/** (Re)start a notice's auto-dismiss timer. */
function armNotice(id: number, ms: number, get: () => UiState): void {
  clearTimeout(noticeTimers.get(id));
  noticeTimers.set(
    id,
    setTimeout(() => get().dismiss(id), ms),
  );
}

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
  useUi.setState({ ...initial, modsUi: modsUiDefaults() });
}
