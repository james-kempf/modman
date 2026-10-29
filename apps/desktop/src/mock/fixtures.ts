// SPDX-License-Identifier: GPL-3.0-or-later
// Sample payloads matching the backend types (tests and the demo backend).

import type {
  AfterChange,
  AutoupdateStatus,
  DeployReport,
  DeployStatus,
  GameView,
  ModView,
} from "../ipc/types";

export const deployStatus = (o: Partial<DeployStatus> = {}): DeployStatus => ({
  active: true,
  files: 1234,
  by_method: { hardlink: 1234 },
  backups_held: 0,
  dirs: 12,
  last_deployed_at: 1_760_000_000,
  interrupted: false,
  enabled_mods: 2,
  deployed_mods: 2,
  ...o,
});

export const game = (o: Partial<GameView> = {}): GameView => ({
  domain: "skyrimspecialedition",
  name: "Skyrim Special Edition",
  folder: "/home/deck/.local/share/Steam/steamapps/common/Skyrim Special Edition",
  subdir: "Data",
  profile: "bethesda",
  target: "game_dir",
  appid: 489830,
  method: null,
  prefix: null,
  library: null,
  plugins_file: null,
  plugins_format: "star",
  appdata_folder: "Skyrim Special Edition",
  paks_dir: null,
  nexus_game_id: 1704,
  definition: null,
  added_at: 1_750_000_000,
  active: true,
  support: "tested",
  mod_count: 2,
  deploy: deployStatus(),
  data_dir: "/home/deck/.local/share/vessel/skyrimspecialedition",
  plugins_txt: null,
  ...o,
});

export const experimentalGame = (o: Partial<GameView> = {}): GameView =>
  game({
    domain: "cyberpunk2077",
    name: "Cyberpunk 2077",
    profile: "redengine",
    appid: 1091500,
    support: "experimental",
    ...o,
  });

let modId = 0;
export const mod = (o: Partial<ModView> = {}): ModView => {
  const id = ++modId;
  return {
    id,
    game: "skyrimspecialedition",
    uid: `mod-${id}`,
    name: `Mod ${id}`,
    version: "1.0",
    nexus_mod_id: null,
    nexus_file_id: null,
    md5: null,
    source: "nexus",
    archive: null,
    author: "someone",
    category: null,
    update_policy: null,
    root: "",
    variant: null,
    staging_dir: null,
    enabled: true,
    priority: id,
    file_count: 10,
    size: 1024 * 1024,
    installed_at: 1_760_000_000,
    position: id,
    collections: [],
    ...o,
  };
};

export const deployReport = (o: Partial<DeployReport> = {}): DeployReport => ({
  dry_run: false,
  method: "auto",
  mods: 2,
  skipped: [],
  conflicts: [],
  placed: 20,
  backed_up: 0,
  new_dirs: 1,
  by_method: { hardlink: 20 },
  purged: null,
  recovered: null,
  plugins: null,
  ...o,
});

export const afterChange = (o: Partial<AfterChange> = {}): AfterChange => ({
  deployed: null,
  needs_deploy: true,
  ...o,
});

export const autoupdateStatus = (o: Partial<AutoupdateStatus> = {}): AutoupdateStatus => ({
  timer: { installed: false, active: null, next_elapse: null, every_hours: null },
  collections: false,
  interval_hours: 0,
  last_system_change: 1_760_000_000,
  games: [],
  runs: [],
  ...o,
});
