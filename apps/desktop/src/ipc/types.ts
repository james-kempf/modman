// SPDX-License-Identifier: GPL-3.0-or-later
// Payload types of the Tauri commands and events, mirroring the serde
// output of modman-core / modman-desktop exactly (snake_case fields,
// `null` for `None`, paths as strings, integers as numbers).

// ── messages ────────────────────────────────────────────────

export type Level = "heading" | "info" | "success" | "warn" | "error" | "line" | "err_line";
export type Style = "plain" | "bold" | "dim" | "green" | "yellow";
export interface Span {
  text: string;
  style: Style;
}
/** A core message (CLI-equivalent text with styles). */
export interface Msg {
  level: Level;
  spans: Span[];
}

// ── games ───────────────────────────────────────────────────

export type Support = "tested" | "experimental";
export type Target = "game_dir" | "prefix_dir";
export type PluginsFormat = "star" | "plain";

export interface GameConfig {
  domain: string;
  name: string;
  folder: string | null;
  subdir: string;
  profile: string;
  target: Target;
  appid: number | null;
  method: string | null;
  prefix: string | null;
  library: string | null;
  plugins_file: string | null;
  plugins_format: PluginsFormat | null;
  appdata_folder: string | null;
  paks_dir: string | null;
  nexus_game_id: number | null;
  definition: string | null;
  added_at: number;
}

export interface GameView extends GameConfig {
  active: boolean;
  support: Support;
  mod_count: number;
  deploy: DeployStatus;
  data_dir: string;
  plugins_txt: string | null;
}

export interface Detected {
  domain: string;
  name: string;
  install_dir: string;
  support: Support;
  configured: boolean;
}

/** game_launch result. */
export interface Launched {
  appid: number;
  /** Pending mod changes were deployed first. */
  deployed: boolean;
}

// ── deploy ──────────────────────────────────────────────────

export type Method = "hardlink" | "reflink" | "symlink" | "copy";
export type MethodChoice = "auto" | Method;

export interface DeployStatus {
  active: boolean;
  files: number;
  by_method: Partial<Record<Method, number>>;
  backups_held: number;
  dirs: number;
  last_deployed_at: number | null;
  interrupted: boolean;
}

export interface ModRef {
  id: number;
  uid: string;
  name: string;
  priority: number;
}
export interface SkippedMod {
  uid: string;
  reason: string;
}
export interface Conflict {
  rel_path: string;
  winner: ModRef;
  losers: ModRef[];
}

export interface RecoveryReport {
  journal_id: number;
  op: string;
  outcome: "rolled_back" | "resumed";
  undo: PurgeReport;
}

export interface PurgeReport {
  nothing_deployed: boolean;
  removed: number;
  restored: number;
  missing: number;
  changed: number;
  kept: number;
  recovered: RecoveryReport | null;
  plugins: PluginStage | null;
}

export interface DeployReport {
  dry_run: boolean;
  method: MethodChoice;
  mods: number;
  skipped: SkippedMod[];
  conflicts: Conflict[];
  placed: number;
  backed_up: number;
  new_dirs: number;
  by_method: Partial<Record<Method, number>>;
  purged: PurgeReport | null;
  recovered: RecoveryReport | null;
  plugins: PluginStage | null;
}

export interface VerifyIssue {
  target: Target;
  rel_path: string;
  method: Method;
  kind: "missing" | "modified";
}
export interface VerifyReport {
  intact: number;
  issues: VerifyIssue[];
  interrupted: boolean;
}

/** After a change of the mod set: auto-deploy ran, or a deploy is due. */
export interface AfterChange {
  deployed: DeployReport | null;
  needs_deploy: boolean;
}

// ── plugins & conflicts ─────────────────────────────────────

export type SkipReason =
  | { kind: "not_bethesda" }
  | { kind: "no_plugins_file" }
  | { kind: "folder_missing"; path: string };
export type SortMethod = { loot: { masterlist: boolean } } | "rules";
export interface PluginsReport {
  path: string;
  provided: number;
  added: string[];
  removed: string[];
  written: boolean;
  sorted_by: SortMethod | null;
  warnings: string[];
}
export type PluginStage =
  | { state: "skipped"; detail: SkipReason }
  | { state: "updated"; detail: PluginsReport }
  | { state: "failed"; detail: string };

export interface PluginLine {
  name: string;
  active: boolean;
  managed: boolean;
}

export interface ConflictMod {
  id: number;
  uid: string;
  name: string;
  priority: number;
}
export interface FileConflict {
  rel_path: string;
  winner: ConflictMod;
  losers: ConflictMod[];
  plugin: boolean;
}
export interface PluginEntry {
  name: string;
  provider: ConflictMod | null;
  overridden: ConflictMod[];
  load_index: number | null;
  active: boolean;
  managed: boolean;
}
export interface ConflictData {
  files: FileConflict[];
  plugins: PluginEntry[];
}

// ── mods ────────────────────────────────────────────────────

export interface ModRecord {
  id: number;
  game: string;
  uid: string;
  name: string;
  version: string | null;
  nexus_mod_id: number | null;
  nexus_file_id: number | null;
  md5: string | null;
  source: string;
  archive: string | null;
  author: string | null;
  category: string | null;
  update_policy: string | null;
  /** "game" = relative to the game root, "" = deploy folder. */
  root: string;
  variant: string | null;
  staging_dir: string | null;
  enabled: boolean;
  priority: number;
  file_count: number;
  size: number;
  installed_at: number;
  /** 1-based load-order position. */
  position: number;
}

export interface ModView extends ModRecord {
  collections: string[];
}
export interface ModsView {
  mods: ModView[];
  changed_since_deploy: boolean;
}
export interface FomodStepView {
  step: string;
  options: string[];
}
export interface ModDetail {
  mod: ModRecord;
  collections: string[];
  fomod: FomodStepView[];
  wins: number;
  loses: number;
  url: string | null;
}
export interface MoveResult {
  uid: string;
  from: number;
  to: number;
  of: number;
  after: AfterChange;
}
export interface UninstallResult {
  /** false = the user declined. */
  done: boolean;
  mods: number;
  archives: string[];
  after: AfterChange;
}
export interface ProfileSummary {
  name: string;
  mods: number;
  enabled: number;
  saved_at: number;
}
export interface ProfileLoaded {
  missing: number;
  added_disabled: number;
  after: AfterChange;
}

// ── install & downloads ─────────────────────────────────────

export interface InstalledMod {
  mod_id: number;
  uid: string;
  name: string;
  files: number;
  size: number;
  fomod_found: boolean;
  replaced: string | null;
  warnings: string[];
}
export interface InstallResult {
  game: string | null;
  installed: InstalledMod | null;
  collection: CollectionResult | null;
  after: AfterChange;
}
export interface Reinstalled {
  installed: InstalledMod;
  after: AfterChange;
}

export interface FileInfo {
  file_id: number;
  name: string;
  version: string | null;
  category_name: string | null;
  file_name: string | null;
  size_in_bytes: number | null;
  uploaded_timestamp: number | null;
  is_primary: boolean;
}
export interface FilesView {
  files: FileInfo[];
  /** Index of the default (primary) file. */
  default: number;
}

export type DownloadSource = "nxm" | "nexus" | "direct" | "collection" | "manual";
export type DownloadStatus = "queued" | "downloading" | "paused" | "done" | "failed" | "cancelled";
/** A `downloads` row (the nxm key is never sent). */
export interface Download {
  id: number;
  game: string | null;
  source: DownloadSource;
  url: string | null;
  nexus_mod_id: number | null;
  nexus_file_id: number | null;
  collection_slug: string | null;
  collection_revision: number | null;
  file_name: string | null;
  dest_path: string | null;
  part_path: string | null;
  size: number | null;
  bytes_done: number;
  md5: string | null;
  sha256: string | null;
  status: DownloadStatus;
  error: string | null;
  created_at: number;
  updated_at: number;
}

/** Result of `nxm_submit` and of a forwarded link. */
export interface NxmReceived {
  row: number;
  /** Redacted link (no key/expires). */
  link: string;
  kind: "mod" | "collection";
}

// ── collections ─────────────────────────────────────────────

export interface CollectionFailure {
  name: string;
  reason: string;
  url: string | null;
}
export interface CollectionResult {
  game: string | null;
  slug: string;
  revision: number | null;
  cancelled: boolean;
  download_only: boolean;
  installed: number;
  present: number;
  failed: CollectionFailure[];
  removed: number;
  up_to_date: boolean;
  deployed: DeployReport | null;
}
export interface CollectionRecord {
  id: number;
  game: string;
  slug: string;
  name: string | null;
  author: string | null;
  revision: number;
  dir: string | null;
  failed: string[];
  installed_at: number;
  /** Member mod row ids. */
  mods: number[];
}
export interface CollectionSummary {
  record: CollectionRecord;
  enabled: number;
}
export interface RevisionInfo {
  name: string;
  author: string;
  game_name: string;
  number: number;
  status: string;
  latest: number | null;
  mod_count: number;
  optional_count: number;
  total_size: number;
  summary: string;
  url: string;
}
export interface CollectionInfo {
  game: string;
  slug: string;
  revision: RevisionInfo;
  installed: CollectionRecord | null;
  update_available: number | null;
}
export interface CollectionRemoved {
  /** false = the user declined. */
  removed: boolean;
  uninstalled: number;
  after: AfterChange;
}

// ── requirements, updates, search ───────────────────────────

export type ReqStatus = "missing" | "disabled" | "external" | "gone" | "ok";
export interface ReqRow {
  status: ReqStatus;
  /** Ack key. */
  key: string;
  name: string;
  /** modman arguments that fix it, e.g. "install 123". */
  hint: string;
  urls: string[];
  by: string[];
  detail: string;
  acked: boolean;
}

export interface Update {
  mod: ModRecord;
  file: FileInfo;
  pinned: boolean;
}
export interface UpdateReport {
  checked: number;
  fetched: number;
  stopped_early: boolean;
  updates: Update[];
  installed: number;
}

/** Nexus GraphQL search hit (camelCase as Nexus sends it). */
export interface SearchHit {
  modId: number | null;
  name: string | null;
  author: string | null;
  version: string | null;
  summary: string | null;
  downloads: number | null;
  endorsements: number | null;
  thumbnailUrl: string | null;
  category: string | null;
  updatedAt: string | null;
  installed: boolean;
}
export interface SearchView {
  total: number;
  mods: SearchHit[];
}

// ── account & settings ──────────────────────────────────────

export interface AccountInfo {
  /** Where the key comes from; null = not logged in. */
  key_source: string | null;
  user: string | null;
  premium: boolean | null;
  quota: string | null;
}
/** Never contains the key. */
export interface LoginResult {
  user: string;
  premium: boolean;
  backend: "keyring" | "file";
  file: string | null;
  keyring_unavailable: string | null;
}
export interface ConfigEntry {
  key: string;
  value: string;
  description: string;
}
export interface ConfigChange extends ConfigEntry {
  /** data-dir moved away from a folder that still holds data. */
  old_data_left: string | null;
}

export interface AppInfo {
  version: string;
  config_dir: string;
  data_dir: string;
  log_path: string;
  db_path: string;
  legacy_state_dir: string | null;
}

// ── auto-update ─────────────────────────────────────────────

export type Skip =
  | { kind: "not_deployed" }
  | { kind: "baseline" }
  | { kind: "no_reasons" }
  | { kind: "back_off"; retry_at: number }
  | { kind: "folder_missing" }
  | { kind: "steam_updating"; state_flags: number }
  | { kind: "game_running" };
export type Busy = { kind: "already_running" } | { kind: "system_updating"; by: string | null };
export interface Pending {
  domain: string;
  name: string;
  reasons: string[];
  /** null = a run now would update and redeploy it. */
  blocked: Skip | null;
}
export interface StartupCheck {
  system_busy: Busy | null;
  pending: Pending[];
}
export interface ReadyInfo {
  startup_check: StartupCheck | null;
  queued: number;
}
export interface TimerStatus {
  installed: boolean;
  active: string | null;
  next_elapse: string | null;
  every_hours: number | null;
}
export interface AutoupdateGameStatus {
  domain: string;
  name: string;
  last_ok: number | null;
  last_failed: boolean;
  steam_build: string | null;
  pending: string[];
}
export type RunOutcome = "nothing" | "acted" | "failed" | "skipped";
export interface RunLogEntry {
  id: number;
  /** null = summary row of a whole run. */
  game: string | null;
  started_at: number;
  finished_at: number | null;
  forced: boolean;
  dry_run: boolean;
  outcome: RunOutcome;
  reasons: string[];
  actions: string[];
  summary: string | null;
}
export interface AutoupdateStatus {
  timer: TimerStatus;
  collections: boolean;
  interval_hours: number;
  last_system_change: number;
  games: AutoupdateGameStatus[];
  runs: RunLogEntry[];
}

// ── prompts ─────────────────────────────────────────────────

export type GroupType =
  | "SelectExactlyOne"
  | "SelectAtMostOne"
  | "SelectAtLeastOne"
  | "SelectAll"
  | "SelectAny";
export type PluginType = "Required" | "Optional" | "Recommended" | "NotUsable" | "CouldBeUsable";
export interface FomodOption {
  name: string;
  description: string;
  kind: PluginType;
  kind_label: string | null;
}
export interface OptionalMod {
  name: string;
  version: string | null;
  size: number;
}

/** A question from a running job (`modman:prompt`). */
export type Prompt =
  | { type: "confirm"; question: string; default_yes: boolean }
  | { type: "text"; question: string; default: string }
  | { type: "choose_file"; files: FileInfo[]; default: number }
  | { type: "choose_optional"; collection: string; mods: OptionalMod[] }
  | { type: "manual_download"; name: string; instructions: string | null; url: string | null }
  | { type: "choose_variant"; choices: string[] }
  | {
      type: "fomod_group";
      module: string;
      step: string;
      group: string;
      kind: GroupType;
      kind_label: string;
      options: FomodOption[];
      defaults: number[];
      /** Why the previous answer was rejected (asked again). */
      error: string | null;
    };

/** Reply to a prompt (`prompt_answer`). */
export type Answer =
  | { type: "confirm"; yes: boolean }
  | { type: "text"; value: string }
  | { type: "choice"; index: number }
  | { type: "choices"; indices: number[] }
  | { type: "path"; path: string }
  | { type: "cancel" };

/** Rejection value of every command. */
export interface CmdError {
  message: string;
}
