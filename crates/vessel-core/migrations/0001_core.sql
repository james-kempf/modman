-- SPDX-License-Identifier: GPL-3.0-or-later
-- Core state: settings, games, mods, staged files, profiles.
-- Timestamps are Unix epoch seconds unless the column name ends in _ns.

-- Global key/value settings (`vessel config` keys, active game, account info,
-- auth.backend, ...). Values are text; the owner parses them.
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Configured games (replaces legacy games/<domain>.cfg).
CREATE TABLE games (
    domain         TEXT PRIMARY KEY,               -- Nexus game domain
    name           TEXT NOT NULL,
    folder         TEXT,                           -- game install dir (absolute)
    subdir         TEXT NOT NULL DEFAULT '.',      -- deploy root relative to folder
    profile        TEXT NOT NULL DEFAULT 'generic',-- layout profile name (staging.rs)
    target         TEXT NOT NULL DEFAULT 'game_dir' CHECK (target IN ('game_dir', 'prefix_dir')),
    appid          INTEGER,                        -- Steam appid
    method         TEXT,                           -- per-game deploy method; NULL = global
    prefix         TEXT,                           -- Proton prefix (.../compatdata/<appid>/pfx)
    library        TEXT,                           -- Steam library root holding the game
    plugins_file   TEXT,                           -- plugins.txt path override
    plugins_format TEXT CHECK (plugins_format IN ('star', 'plain')),
    appdata_folder TEXT,                           -- AppData/Local/<folder> for plugins.txt
    paks_dir       TEXT,                           -- ue pak mods folder relative to folder
    nexus_game_id  INTEGER,
    definition     TEXT,                           -- game definition domain; NULL = custom
    added_at       INTEGER NOT NULL
);

-- Installed mods. `priority` is the deploy order per game: 0 = lowest, last wins.
CREATE TABLE mods (
    id             INTEGER PRIMARY KEY,
    game           TEXT NOT NULL REFERENCES games(domain) ON DELETE CASCADE,
    uid            TEXT NOT NULL,                  -- user-facing id, e.g. <name>-<modId>-<fileId>
    name           TEXT NOT NULL,
    version        TEXT,
    nexus_mod_id   INTEGER,
    nexus_file_id  INTEGER,
    md5            TEXT,
    source         TEXT NOT NULL DEFAULT 'local',  -- local|nexus|bundle|direct|manual
    archive        TEXT,                           -- absolute archive path
    author         TEXT,
    category       TEXT,
    update_policy  TEXT,                           -- e.g. 'exact' (pinned by collection)
    root           TEXT NOT NULL DEFAULT '',       -- 'game' = relative to game root, '' = deploy root
    variant        TEXT,
    staging_dir    TEXT,                           -- absolute staging folder
    status         TEXT NOT NULL DEFAULT 'installed',
    enabled        INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    priority       INTEGER NOT NULL,
    file_count     INTEGER NOT NULL DEFAULT 0,
    size           INTEGER NOT NULL DEFAULT 0,
    installed_at   INTEGER NOT NULL,
    UNIQUE (game, uid)
);
CREATE INDEX mods_game_priority ON mods (game, priority);
CREATE INDEX mods_game_nexus ON mods (game, nexus_mod_id);

-- Per-mod staged file list with hashes.
CREATE TABLE mod_files (
    mod_id  INTEGER NOT NULL REFERENCES mods(id) ON DELETE CASCADE,
    path    TEXT NOT NULL,                         -- relative to the staging dir, '/' separated
    size    INTEGER NOT NULL,
    sha256  TEXT NOT NULL,
    mode    INTEGER,
    PRIMARY KEY (mod_id, path)
);

-- Saved profiles: mod order + enabled state, plus plugin order.
CREATE TABLE profiles (
    id           INTEGER PRIMARY KEY,
    game         TEXT NOT NULL REFERENCES games(domain) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    plugin_order TEXT,                             -- JSON array of plugin names
    saved_at     INTEGER NOT NULL,
    UNIQUE (game, name)
);

CREATE TABLE profile_mods (
    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    position   INTEGER NOT NULL,
    mod_uid    TEXT NOT NULL,                      -- by uid so entries survive reinstall
    enabled    INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    PRIMARY KEY (profile_id, position)
);
