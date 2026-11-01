-- SPDX-License-Identifier: GPL-3.0-or-later
-- Loaders (owner: loaders.rs): loader installs, the files they changed, and
-- the runs that changed them. Rollback data survives removing the game:
-- game_id is set to NULL and game_dir still says where the files are.

CREATE TABLE loader_installs (
    id             INTEGER PRIMARY KEY,
    game_id        TEXT REFERENCES games(id) ON DELETE SET NULL,
    game_dir       TEXT NOT NULL,                  -- absolute game folder at install time
    loader_key     TEXT NOT NULL,                  -- == requirements tool key
    version        TEXT,
    runtime        TEXT NOT NULL,                  -- native|proton|unknown
    strategy       TEXT NOT NULL,                  -- installer|deploy
    status         TEXT NOT NULL,                  -- installed|running|interrupted|uninstalled|failed
    nexus_mod_id   INTEGER,
    nexus_file_id  INTEGER,
    origin         TEXT NOT NULL,                  -- nexus|github|local
    archive_sha256 TEXT,
    archive_path   TEXT,                           -- copy under <data>/loaders/archives/<sha>
    vanilla_sha256 TEXT,                           -- game exe before the first install
    game_build     TEXT,                           -- Steam buildid at install
    launch         TEXT,                           -- JSON LaunchChange this install needs
    installed_at   INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);
-- One live install per game and loader; finished rows are history.
CREATE UNIQUE INDEX loader_installs_active ON loader_installs (game_id, loader_key)
    WHERE status IN ('installed', 'running', 'interrupted');
CREATE INDEX loader_installs_game ON loader_installs (game_id);

-- Files an install created, modified or deleted (lstat diff, game-root relative).
CREATE TABLE loader_files (
    install_id   INTEGER NOT NULL REFERENCES loader_installs(id) ON DELETE CASCADE,
    path         TEXT NOT NULL,                    -- '/' separated, relative to game_dir
    change       TEXT NOT NULL CHECK (change IN ('created', 'modified', 'deleted')),
    kind         TEXT NOT NULL CHECK (kind IN ('file', 'link')),
    pre_sha256   TEXT,                             -- link target hash for links
    post_sha256  TEXT,
    backup_ref   TEXT,                             -- backup store blob; a GC root
    out_of_scope INTEGER NOT NULL DEFAULT 0 CHECK (out_of_scope IN (0, 1)),
    PRIMARY KEY (install_id, path)
);

-- Runs of install/upgrade/repair/uninstall/adopt, for crash recovery.
CREATE TABLE loader_runs (
    id                INTEGER PRIMARY KEY,
    install_id        INTEGER NOT NULL REFERENCES loader_installs(id) ON DELETE CASCADE,
    op                TEXT NOT NULL
                      CHECK (op IN ('install', 'upgrade', 'repair', 'uninstall', 'adopt')),
    consent_hash      TEXT NOT NULL,
    phase             TEXT NOT NULL
                      CHECK (phase IN ('backing_up', 'spawned', 'diffing', 'done')),
    pid               INTEGER,
    pgid              INTEGER,
    proc_start        INTEGER,                     -- /proc/<pid>/stat starttime (clock ticks)
    pre_manifest_path TEXT,
    started_at        INTEGER NOT NULL,
    ended_at          INTEGER,
    exit_code         INTEGER,
    timed_out         INTEGER NOT NULL DEFAULT 0 CHECK (timed_out IN (0, 1)),
    log_path          TEXT,
    wineserver        TEXT,                        -- Proton runs: for `wineserver -k` in recovery
    wineprefix        TEXT
);
CREATE INDEX loader_runs_open ON loader_runs (phase) WHERE phase <> 'done';

-- Backups taken for a run before it spawns. GC roots until the op clears
-- them (once loader_files hold the refs, or the rollback is finished).
CREATE TABLE loader_run_backups (
    run_id     INTEGER NOT NULL REFERENCES loader_runs(id) ON DELETE CASCADE,
    backup_ref TEXT NOT NULL,                      -- <sha256>:<mode>:<mtime_ns>
    PRIMARY KEY (run_id, backup_ref)
);

-- Per-game launch options: what loaders need, what the user confirmed
-- setting by hand, and what Vessel wrote to localconfig.vdf.
ALTER TABLE games ADD COLUMN runtime_override TEXT;
ALTER TABLE games ADD COLUMN launch_options_desired TEXT;
ALTER TABLE games ADD COLUMN launch_options_acked TEXT;
ALTER TABLE games ADD COLUMN launch_options_applied TEXT;
