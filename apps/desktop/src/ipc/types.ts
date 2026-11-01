// SPDX-License-Identifier: GPL-3.0-or-later
// Payload types of the Tauri commands and events, mirroring the serde
// output of vessel-core / vessel-desktop exactly (snake_case fields,
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
  /** Stable key; every `game` argument and field names a game by it. */
  id: string;
  /** Nexus game domain (nexusmods.com/<domain>); null = not linked. */
  nexus_domain: string | null;
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
  /** The preset's Nexus domain; `steam-<appid>` (a key only) when there is no preset. */
  domain: string;
  name: string;
  appid: number;
  install_dir: string;
  /** null: no preset (unsupported); adding asks for the Nexus domain. */
  support: Support | null;
  configured: boolean;
}

/** game_art: portrait (library tiles) or wide hero (Overview background). */
export type ArtKind = "library_600x900" | "library_hero";
/** open_folder: the game's install folder or its Proton prefix. */
export type GameFolder = "game" | "prefix" | "mods";

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
  /** Enabled mods of the game. */
  enabled_mods: number;
  /** Enabled mods with files in the current deployment. */
  deployed_mods: number;
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
  /** Mod files not placed because an installed loader owns the path. */
  loader_owned?: string[];
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
/** A mod installed as a loader (e.g. SMAPI): an active loader install record, no mods row. */
export interface InstalledLoader {
  key: string;
  name: string;
  version: string | null;
  nexus_mod_id: number | null;
  install_id: number;
  status: "installed" | "running" | "interrupted";
}
export interface ModsView {
  mods: ModView[];
  changed_since_deploy: boolean;
  /** Mods installed as a loader; removed through the loader uninstall flow. */
  loaders: InstalledLoader[];
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
/** A mod loader archive held for consent: nothing was installed or run. */
export interface LoaderPending {
  loader_key: string;
  preview_id: string;
  plan_hash: string;
  op: "install" | "upgrade" | "repair" | "uninstall" | "adopt";
  version: string | null;
}
export interface InstallResult {
  game: string | null;
  installed: InstalledMod | null;
  /** The target was a mod loader (pending preview). */
  loader?: LoaderPending | null;
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
  /** Mod loaders in the collection, pending consent (not failed). */
  loaders_pending?: LoaderPending[];
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
  /** The revision's mods, in collection order. */
  mods: RevisionMod[];
}
export interface RevisionMod {
  name: string;
  version: string | null;
  author: string | null;
  mod_id: number | null;
  /** Bytes; 0 = unknown. */
  size: number;
  optional: boolean;
  /** The mod's Nexus browse card (picture, summary, counts); absent when Nexus has none. */
  card?: Omit<SearchHit, "installed" | "loader"> | null;
  /** Installed for this game (managed, or as a loader; see `loader`). */
  installed: boolean;
  /** Key of the loader it is installed as (its Remove goes through the loader flow). */
  loader?: string | null;
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
  /** vessel arguments that fix it, e.g. "install 123". */
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

/** Nexus GraphQL mod in a browse page (camelCase as Nexus sends it). */
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
  /** Key of the loader this mod is installed as (absent for managed mods); Remove uses the loader flow. */
  loader?: string;
}
/** Nexus collection in a browse page. */
export interface CollectionHit {
  slug: string;
  name: string;
  summary: string | null;
  author: string | null;
  category: string | null;
  endorsements: number | null;
  downloads: number | null;
  /** Latest published revision. */
  revision: number | null;
  mod_count: number | null;
  total_size: number | null;
  /** Tile thumbnail (load it through `picture`). */
  image_url: string | null;
  /** null = not installed. */
  installed_revision: number | null;
}
export type BrowseKind = "mods" | "collections";
export type BrowsePeriod = "all" | "year" | "month" | "week" | "day";
export type BrowseSort = "endorsements" | "downloads" | "newest";
export interface BrowseRequest {
  kind: BrowseKind;
  period: BrowsePeriod;
  sort: BrowseSort;
  /** Name words; empty = everything. */
  text: string;
  offset: number;
}
/** One page; `total` counts every match. */
export type BrowseView =
  | { kind: "mods"; total: number; mods: SearchHit[] }
  | { kind: "collections"; total: number; collections: CollectionHit[] };
/** The item whose description `browse` fetches instead of a page. */
export type AboutTarget = { kind: "mod"; mod_id: number } | { kind: "collection"; slug: string };
/** `browse` with an about target: BBCode (mod) or Markdown (collection); null = none. */
export interface AboutView {
  kind: "about";
  description: string | null;
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
/** What a setting's value is: picked from `options`, a switch, or typed. */
export type ConfigKind = "enum" | "bool" | "number" | "path" | "text";
export interface ConfigEntry {
  key: string;
  value: string;
  description: string;
  kind: ConfigKind;
  /** Accepted values of an enum or bool key; empty for the others. */
  options: string[];
}
export interface ConfigChange extends ConfigEntry {
  /** data-dir moved away from a folder that still holds data. */
  old_data_left: string | null;
}

/** The Vessel shortcut in Steam (as a non-Steam game). */
export interface ShortcutStatus {
  accounts: number;
  added: number;
  steam_running: boolean;
  exe: string;
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
  game: string;
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
  /** Loader runs startup recovery finished (a rollback may be offered). */
  loader_recovered?: Recovered[];
}
export interface TimerStatus {
  installed: boolean;
  active: string | null;
  next_elapse: string | null;
  every_hours: number | null;
}
export interface AutoupdateGameStatus {
  game: string;
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

/** A question from a running job (`vessel:prompt`). */
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
  /** Stable code of a typed error (see `loaderErrors.ts`); absent otherwise. */
  code?: string;
}

// ── backup cleanup and restage ──────────────────────────────

/** confirmed: Vessel placed the path before the backup was taken; ambiguous: no earlier record. */
export type PoisonClass = "confirmed" | "ambiguous";

/** A held backup holding the bytes a mod ships for the same path (`backups_poisoned`). */
export interface PoisonedBackup {
  backup_id: number;
  target: Target;
  /** Relative to the target root. */
  path: string;
  sha256: string;
  class: PoisonClass;
  reason: string;
  /** Mod uids shipping these bytes at this path. */
  mods: string[];
  deployed: boolean;
  /** Placed by an enabled mod: the game file stays, only the backup goes. */
  deployed_by_enabled_mod: boolean;
}

export interface QuarantineReport {
  /** null = nothing was quarantined. */
  id: string | null;
  dir: string | null;
  backups: number;
  game_files: number;
  skipped_ambiguous: number;
}

export interface QuarantineItem {
  path: string;
  target: Target;
  sha256: string;
  origin: "backup" | "game_file";
  class: PoisonClass;
  reason: string;
  backup: { id: number; size: number; mode: number; mtime_ns: number | null; created_at: number } | null;
  deployment_ids: number[];
}

export interface QuarantineManifest {
  id: string;
  game: string;
  created_at: number;
  restored_at: number | null;
  items: QuarantineItem[];
}

export interface RestoreReport {
  id: string;
  backups: number;
  game_files: number;
}

export interface RestageReport {
  restaged: { id: number; uid: string; files: number; carried: { from: string; to: string }[]; backed_up: string[] }[];
  /** Mod uids whose archive is gone. */
  no_archive: string[];
  failed: { uid: string; error: string }[];
  backup_dir: string | null;
  poisoned_confirmed: number;
  poisoned_ambiguous: number;
}

// ── loaders ─────────────────────────────────────────────────

export type LoaderOp = LoaderPending["op"];

export type DownloadOrigin =
  | { kind: "nexus"; domain: string; mod_id: number; file_id: number }
  | { kind: "github"; repo: string; tag: string; asset: string }
  | { kind: "local"; path: string };

export type Provenance =
  | { kind: "verified"; origin: DownloadOrigin }
  | { kind: "unverified"; origin: DownloadOrigin; reason: string };

export type Runtime =
  | { kind: "native" }
  | { kind: "proton"; proton: string; compat_data: string; client_root: string; app_id: number; slr: string | null }
  | { kind: "unknown" };

/** One process a loader op starts, exactly as it will run. */
export interface PlannedCommand {
  program: string;
  args: string[];
  cwd: string;
  /** [name, value] pairs: the whole environment the installer gets. */
  env: [string, string][];
  /** The cmd.exe line (Proton .bat targets). */
  cmd_line: string | null;
}

/** A consentable loader op (`loader_preview`, `loader_preview_reopen`). Nothing ran yet. */
export interface Preview {
  preview_id: string;
  plan_hash: string;
  loader_key: string;
  op: LoaderOp;
  version: string | null;
  runtime: Runtime;
  commands: PlannedCommand[];
  /** Game-folder globs the op may change (backed up, tracked, rolled back on failure). */
  touch_scope: string[];
  /** Changes outside the game folder: cannot be rolled back. */
  external_side_effects: string[];
  /** null = no archive involved. */
  provenance: Provenance | null;
  warnings: string[];
  permissions: string;
  /** Unix seconds. */
  expires_at: number;
  /** The installer is stopped after this long. */
  timeout_secs: number;
}

export type LoaderRunOutcome = { kind: "succeeded" } | { kind: "failed"; reason: string };
export interface RunReport {
  install_id: number;
  run_id: number;
  outcome: LoaderRunOutcome;
  status: string;
  version: string | null;
  log_dir: string;
  warnings: string[];
  /** Launch options the game now needs (null = none). */
  launch_desired: string | null;
}

export type RecoveryAction = { kind: "discarded" } | { kind: "rollback_offered"; killed: boolean };
export interface Recovered {
  run_id: number;
  install_id: number;
  action: RecoveryAction;
}

export type LoaderStatus =
  | { state: "not_installed" }
  | { state: "installed"; install_id: number; version: string | null }
  /** status: running | interrupted (interrupted = a rollback is offered). */
  | { state: "busy"; install_id: number; status: string }
  | { state: "manual_detected"; evidence: string[] }
  | { state: "drifted"; install_id: number; version: string | null; reason: string };

export interface LaunchState {
  /** What the installed loaders need. */
  desired: string | null;
  /** What the user confirmed setting by hand. */
  acked: string | null;
  /** What Vessel last wrote to Steam. */
  applied: string | null;
}

/** A loader of the game (`loader_status`). */
export interface LoaderCard {
  key: string;
  name: string;
  nexus_mod_id: number | null;
  status: LoaderStatus;
  runtime: "native" | "proton" | "unknown";
  runtime_override: string | null;
  launch: LaunchState;
  steam_running: boolean;
  last_run: number | null;
  recovered: Recovered | null;
}

export interface SteamAccount {
  account_id: number;
  steam_id: string;
  account_name: string;
  persona_name: string;
  timestamp: number | null;
  localconfig: string;
}
export type ApplyOutcome =
  | { kind: "applied"; account_id: number; path: string; value: string; backup: string | null }
  | { kind: "choose_account"; accounts: SteamAccount[]; default: number | null };

/** A mod row that is really a loader archive (`loader_junk_scan`). */
export interface LoaderJunk {
  mod_id: number;
  uid: string;
  name: string;
  loader_key: string;
}
