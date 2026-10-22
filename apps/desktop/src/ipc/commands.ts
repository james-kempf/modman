// SPDX-License-Identifier: GPL-3.0-or-later
// Typed wrappers of every Tauri command (apps/desktop/src-tauri/src/commands.rs).
// Arguments are snake_case. `game` is a Nexus domain; omitted/null = the
// active game. Every command rejects with a `CmdError` ({ message }).
// Long commands ("job") emit `modman:job` start/finish events, stream
// `modman:activity` messages and may raise `modman:prompt` questions that
// must be answered with `promptAnswer`.

import { transport } from "./transport";
import type {
  AccountInfo,
  AfterChange,
  Answer,
  AppInfo,
  AutoupdateStatus,
  CollectionInfo,
  CollectionRemoved,
  CollectionResult,
  CollectionSummary,
  ConfigChange,
  ConfigEntry,
  ConflictData,
  DeployReport,
  DeployStatus,
  Detected,
  Download,
  FilesView,
  GameConfig,
  GameView,
  InstallResult,
  Launched,
  LoginResult,
  ModDetail,
  ModsView,
  MoveResult,
  NxmReceived,
  PluginLine,
  PluginStage,
  ProfileLoaded,
  ProfileSummary,
  PurgeReport,
  ReadyInfo,
  Reinstalled,
  ReqRow,
  RunLogEntry,
  SearchView,
  StartupCheck,
  UninstallResult,
  UpdateReport,
  VerifyReport,
} from "./types";

type Game = string | null | undefined;

const call = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> =>
  transport().invoke<T>(cmd, args);

/** Every command name, as registered in the backend. */
export const COMMANDS = [
  "app_info",
  "app_ready",
  "startup_check",
  "prompt_answer",
  "log_tail",
  "config_list",
  "config_set",
  "account",
  "login",
  "logout",
  "games_list",
  "games_detect",
  "game_add",
  "game_use",
  "game_remove",
  "game_set",
  "game_launch",
  "mods_list",
  "mod_detail",
  "mods_set_enabled",
  "mod_move",
  "mods_uninstall",
  "mod_reinstall",
  "thumbnail",
  "install",
  "mod_files",
  "nxm_submit",
  "downloads_list",
  "deploy",
  "purge",
  "verify",
  "deploy_status",
  "conflicts",
  "deps",
  "deps_ack",
  "deps_unack",
  "profiles_list",
  "profile_save",
  "profile_load",
  "profile_delete",
  "collections_list",
  "collection_info",
  "collection_install",
  "collection_update",
  "collection_remove",
  "plugins_list",
  "plugins_sort",
  "plugins_masterlist",
  "updates_check",
  "updates_install",
  "search",
  "autoupdate_status",
  "autoupdate_enable",
  "autoupdate_disable",
  "autoupdate_runs",
] as const;
export type CommandName = (typeof COMMANDS)[number];

// ── app ─────────────────────────────────────────────────────

export const appInfo = () => call<AppInfo>("app_info");
/** Call once listeners are registered: start-up check + queued nxm links start processing. */
export const appReady = () => call<ReadyInfo>("app_ready");
/** The one auto-update check of this app start (cached). */
export const startupCheck = () => call<StartupCheck | null>("startup_check");
/** false = that prompt is gone. */
export const promptAnswer = (id: number, answer: Answer) =>
  call<boolean>("prompt_answer", { id, answer });
export const logTail = (lines?: number) => call<string[]>("log_tail", { lines: lines ?? null });
export const configList = () => call<ConfigEntry[]>("config_list");
export const configSet = (key: string, value: string) =>
  call<ConfigChange>("config_set", { key, value });

// ── account ─────────────────────────────────────────────────

export const account = () => call<AccountInfo>("account");
/** The key is validated with Nexus and stored; it is never returned. */
export const login = (apiKey: string) => call<LoginResult>("login", { api_key: apiKey });
/** true = NEXUS_API_KEY is still set in the environment. */
export const logout = () => call<boolean>("logout");

// ── games ───────────────────────────────────────────────────

export const gamesList = () => call<GameView[]>("games_list");
export const gamesDetect = () => call<Detected[]>("games_detect");
/** Job (prompts: folder, profile, active game). */
export const gameAdd = (a: {
  domain: string;
  path?: string | null;
  subdir?: string | null;
  profile?: string | null;
}) =>
  call<GameConfig>("game_add", {
    domain: a.domain,
    path: a.path ?? null,
    subdir: a.subdir ?? null,
    profile: a.profile ?? null,
  });
export const gameUse = (domain: string) => call<null>("game_use", { domain });
/** Job (confirm prompt). false = declined. */
export const gameRemove = (domain: string, deleteData: boolean) =>
  call<boolean>("game_remove", { domain, delete_data: deleteData });
/** Keys: folder, subdir, profile, appid, method, prefix, plugins-file, paks-dir, name. */
export const gameSet = (key: string, value: string, game?: Game) =>
  call<GameConfig>("game_set", { game: game ?? null, key, value });
/** Job: start the game through Steam (asks to deploy pending changes first). */
export const gameLaunch = (game?: Game) => call<Launched>("game_launch", { game: game ?? null });

// ── mods ────────────────────────────────────────────────────

export const modsList = (game?: Game) => call<ModsView>("mods_list", { game: game ?? null });
/** reference: #position, uid, name, Nexus id or a unique name part. */
export const modDetail = (reference: string, game?: Game) =>
  call<ModDetail>("mod_detail", { game: game ?? null, reference });
/** Job (auto-deploy may run). ids = ModRecord.id. */
export const modsSetEnabled = (ids: number[], enabled: boolean, game?: Game) =>
  call<AfterChange>("mods_set_enabled", { game: game ?? null, ids, enabled });
/** Job. spec: "#N" | first | last | up | down | before | after; arg: count or mod. */
export const modMove = (reference: string, spec: string, arg?: string | null, game?: Game) =>
  call<MoveResult>("mod_move", { game: game ?? null, reference, spec, arg: arg ?? null });
/** Job (confirm prompt). refs: uids or other references. */
export const modsUninstall = (refs: string[], deleteArchive: boolean, game?: Game) =>
  call<UninstallResult>("mods_uninstall", {
    game: game ?? null,
    refs,
    delete_archive: deleteArchive,
  });
/** Job (FOMOD/variant prompts). fomod: ask | default | none | reuse. */
export const modReinstall = (
  reference: string,
  opts: { fomod?: string | null; ask?: boolean } = {},
  game?: Game,
) =>
  call<Reinstalled>("mod_reinstall", {
    game: game ?? null,
    reference,
    fomod: opts.fomod ?? null,
    ask: opts.ask ?? false,
  });
/** data: URL of the mod picture (cached), null when it has none. */
export const thumbnail = (game: string, modId: number) =>
  call<string | null>("thumbnail", { game, mod_id: modId });

// ── install & downloads ─────────────────────────────────────

export interface InstallArgs {
  /** Mod id, Nexus URL, <game>:<id>, nxm:// link, archive path or collection URL. */
  target: string;
  game?: Game;
  fileId?: number | null;
  /** ask | default | none | reuse */
  fomod?: string | null;
  name?: string | null;
  deploy?: boolean;
  /** Collections: all | none | ask */
  optional?: string | null;
  revision?: number | null;
  downloadOnly?: boolean;
}
/** Job (prompts: file choice, reinstall/replace, FOMOD, variants, optional mods). */
export const install = (a: InstallArgs) =>
  call<InstallResult>("install", {
    game: a.game ?? null,
    target: a.target,
    file_id: a.fileId ?? null,
    fomod: a.fomod ?? null,
    name: a.name ?? null,
    deploy: a.deploy ?? false,
    optional: a.optional ?? null,
    revision: a.revision ?? null,
    download_only: a.downloadOnly ?? false,
  });
export const modFiles = (modId: number, game?: Game) =>
  call<FilesView>("mod_files", { game: game ?? null, mod_id: modId });
/** Paste-link box: same intake as a link forwarded by `modman nxm`. */
export const nxmSubmit = (url: string) => call<NxmReceived>("nxm_submit", { url });
export const downloadsList = () => call<Download[]>("downloads_list");

// ── deploy ──────────────────────────────────────────────────

/** Job (progress: modman:deploy-progress). method: auto | hardlink | reflink | symlink | copy. */
export const deploy = (opts: { dryRun?: boolean; method?: string | null } = {}, game?: Game) =>
  call<DeployReport>("deploy", {
    game: game ?? null,
    dry_run: opts.dryRun ?? false,
    method: opts.method ?? null,
  });
/** Job (confirm prompt). null = nothing deployed or declined. */
export const purge = (game?: Game) => call<PurgeReport | null>("purge", { game: game ?? null });
export const verify = (game?: Game) => call<VerifyReport>("verify", { game: game ?? null });
export const deployStatus = (game?: Game) =>
  call<DeployStatus>("deploy_status", { game: game ?? null });
export const conflicts = (game?: Game) => call<ConflictData>("conflicts", { game: game ?? null });

// ── requirements ────────────────────────────────────────────

/** All enabled mods, or one mod (modRef). */
export const deps = (opts: { modRef?: string | null; refresh?: boolean } = {}, game?: Game) =>
  call<ReqRow[]>("deps", {
    game: game ?? null,
    mod_ref: opts.modRef ?? null,
    refresh: opts.refresh ?? false,
  });
export const depsAck = (name: string, game?: Game) =>
  call<ReqRow>("deps_ack", { game: game ?? null, name });
export const depsUnack = (name: string, game?: Game) =>
  call<ReqRow>("deps_unack", { game: game ?? null, name });

// ── profiles ────────────────────────────────────────────────

export const profilesList = (game?: Game) =>
  call<ProfileSummary[]>("profiles_list", { game: game ?? null });
/** Job (overwrite confirm). null = declined; else the mod count. */
export const profileSave = (name: string, game?: Game) =>
  call<number | null>("profile_save", { game: game ?? null, name });
/** Job (auto-deploy may run). */
export const profileLoad = (name: string, game?: Game) =>
  call<ProfileLoaded>("profile_load", { game: game ?? null, name });
export const profileDelete = (name: string, game?: Game) =>
  call<null>("profile_delete", { game: game ?? null, name });

// ── collections ─────────────────────────────────────────────

export const collectionsList = (game?: Game) =>
  call<CollectionSummary[]>("collections_list", { game: game ?? null });
/** reference: URL, nxm:// link or slug (of the active game). */
export const collectionInfo = (reference: string, game?: Game) =>
  call<CollectionInfo>("collection_info", { game: game ?? null, reference });
export interface CollectionArgs {
  reference: string;
  game?: Game;
  /** all | none | ask */
  optional?: string | null;
  /** collection | ask | default */
  fomod?: string | null;
  revision?: number | null;
  deploy?: boolean;
  downloadOnly?: boolean;
}
/** Job (prompts: proceed, optional mods, manual downloads, FOMOD). */
export const collectionInstall = (a: CollectionArgs) =>
  call<CollectionResult>("collection_install", {
    game: a.game ?? null,
    reference: a.reference,
    optional: a.optional ?? null,
    fomod: a.fomod ?? null,
    revision: a.revision ?? null,
    deploy: a.deploy ?? false,
    download_only: a.downloadOnly ?? false,
  });
/** Job: latest revision; dropped mods are removed after a confirm. */
export const collectionUpdate = (a: Omit<CollectionArgs, "revision" | "downloadOnly">) =>
  call<CollectionResult>("collection_update", {
    game: a.game ?? null,
    reference: a.reference,
    optional: a.optional ?? null,
    fomod: a.fomod ?? null,
    deploy: a.deploy ?? false,
  });
/** Job (confirm prompt). */
export const collectionRemove = (reference: string, keepMods: boolean, game?: Game) =>
  call<CollectionRemoved>("collection_remove", {
    game: game ?? null,
    reference,
    keep_mods: keepMods,
  });

// ── plugins ─────────────────────────────────────────────────

/** null = no plugins.txt yet. Rejects for games without plugins.txt. */
export const pluginsList = (game?: Game) =>
  call<PluginLine[] | null>("plugins_list", { game: game ?? null });
export const pluginsSort = (updateMasterlist: boolean, game?: Game) =>
  call<PluginStage>("plugins_sort", { game: game ?? null, update_masterlist: updateMasterlist });
/** Path of the downloaded LOOT masterlist. */
export const pluginsMasterlist = (game?: Game) =>
  call<string>("plugins_masterlist", { game: game ?? null });

// ── updates & search ────────────────────────────────────────

export const updatesCheck = (refresh: boolean, game?: Game) =>
  call<UpdateReport>("updates_check", { game: game ?? null, refresh });
/** Job: check, then install (pinned ones only with all). */
export const updatesInstall = (opts: { all?: boolean; refresh?: boolean } = {}, game?: Game) =>
  call<UpdateReport>("updates_install", {
    game: game ?? null,
    all: opts.all ?? false,
    refresh: opts.refresh ?? false,
  });
export const search = (query: string, game?: Game) =>
  call<SearchView>("search", { game: game ?? null, query });

// ── auto-update ─────────────────────────────────────────────

export const autoupdateStatus = (runs?: number) =>
  call<AutoupdateStatus>("autoupdate_status", { runs: runs ?? null });
/** Installs the systemd user timer (runs the `modman` CLI) and records the baseline. */
export const autoupdateEnable = () => call<AutoupdateStatus>("autoupdate_enable");
/** false = it was not enabled. */
export const autoupdateDisable = () => call<boolean>("autoupdate_disable");
/** Last-run log, newest first. */
export const autoupdateRuns = (limit?: number) =>
  call<RunLogEntry[]>("autoupdate_runs", { limit: limit ?? null });
