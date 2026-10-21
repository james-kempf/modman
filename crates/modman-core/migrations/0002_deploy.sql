-- SPDX-License-Identifier: GPL-3.0-or-later
-- Deploy engine state (owner: deploy.rs).

-- Content-addressed backups of game files replaced by a deploy.
-- A row is "held" while restored_at IS NULL.
CREATE TABLE backups (
    id          INTEGER PRIMARY KEY,
    game        TEXT NOT NULL REFERENCES games(domain) ON DELETE CASCADE,
    target      TEXT NOT NULL CHECK (target IN ('game_dir', 'prefix_dir')),
    rel_path    TEXT NOT NULL,                     -- real spelling, relative to the target root
    sha256      TEXT NOT NULL,                     -- blob name in the backup store
    store_path  TEXT NOT NULL,                     -- absolute blob path
    size        INTEGER NOT NULL,
    mode        INTEGER NOT NULL,
    mtime_ns    INTEGER,
    created_at  INTEGER NOT NULL,
    restored_at INTEGER
);
CREATE UNIQUE INDEX backups_held ON backups (game, target, rel_path) WHERE restored_at IS NULL;
CREATE INDEX backups_sha ON backups (sha256);

-- Active deployment: one row per placed file.
CREATE TABLE deployments (
    id          INTEGER PRIMARY KEY,
    game        TEXT NOT NULL REFERENCES games(domain) ON DELETE CASCADE,
    target      TEXT NOT NULL CHECK (target IN ('game_dir', 'prefix_dir')),
    rel_path    TEXT NOT NULL,                     -- real spelling, relative to the target root
    mod_id      INTEGER REFERENCES mods(id) ON DELETE SET NULL,
    method      TEXT NOT NULL CHECK (method IN ('hardlink', 'reflink', 'symlink', 'copy')),
    source      TEXT NOT NULL,                     -- absolute staged source path
    dev         INTEGER,
    inode       INTEGER,
    size        INTEGER,
    mtime_ns    INTEGER,
    sha256      TEXT,
    backup_id   INTEGER REFERENCES backups(id),
    deployed_at INTEGER NOT NULL,
    UNIQUE (game, target, rel_path)
);

-- Directories created by a deploy (removed deepest first on purge).
CREATE TABLE deployment_dirs (
    game     TEXT NOT NULL REFERENCES games(domain) ON DELETE CASCADE,
    target   TEXT NOT NULL CHECK (target IN ('game_dir', 'prefix_dir')),
    rel_path TEXT NOT NULL,
    PRIMARY KEY (game, target, rel_path)
);

-- Journal of deploy/purge runs, for resume or rollback after a crash.
CREATE TABLE deploy_journal (
    id          INTEGER PRIMARY KEY,
    game        TEXT NOT NULL REFERENCES games(domain) ON DELETE CASCADE,
    op          TEXT NOT NULL CHECK (op IN ('deploy', 'purge', 'redeploy')),
    state       TEXT NOT NULL DEFAULT 'started'
                CHECK (state IN ('started', 'committed', 'rolled_back', 'failed')),
    method      TEXT,
    started_at  INTEGER NOT NULL,
    finished_at INTEGER,
    error       TEXT
);
CREATE INDEX deploy_journal_open ON deploy_journal (game, state);

-- Ordered steps of a journal entry. `done` flips after the step hit the disk.
CREATE TABLE deploy_journal_steps (
    id         INTEGER PRIMARY KEY,
    journal_id INTEGER NOT NULL REFERENCES deploy_journal(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    action     TEXT NOT NULL,                      -- backup|place|mkdir|remove|restore|rmdir
    target     TEXT NOT NULL CHECK (target IN ('game_dir', 'prefix_dir')),
    rel_path   TEXT NOT NULL,
    detail     TEXT,                               -- JSON (method, source, backup id, ...)
    done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1)),
    UNIQUE (journal_id, seq)
);
