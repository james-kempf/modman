-- SPDX-License-Identifier: GPL-3.0-or-later
-- Nexus client and download state (owners: nexus.rs, download.rs).

-- Download queue, resume state and nxm intake. A row is committed before any
-- network I/O so a killed process leaves a resumable row.
CREATE TABLE downloads (
    id                  INTEGER PRIMARY KEY,
    game                TEXT,                      -- Nexus domain (may not be configured yet)
    source              TEXT NOT NULL,             -- nxm|nexus|direct|collection|manual
    url                 TEXT,                      -- without secrets (no key/expires query)
    nexus_mod_id        INTEGER,
    nexus_file_id       INTEGER,
    collection_slug     TEXT,
    collection_revision INTEGER,
    nxm_key             TEXT,
    nxm_expires         INTEGER,
    nxm_user_id         INTEGER,
    file_name           TEXT,
    dest_path           TEXT,
    part_path           TEXT,
    size                INTEGER,
    bytes_done          INTEGER NOT NULL DEFAULT 0,
    etag                TEXT,
    md5                 TEXT,
    sha256              TEXT,
    status              TEXT NOT NULL DEFAULT 'queued'
                        CHECK (status IN ('queued', 'downloading', 'paused', 'done', 'failed', 'cancelled')),
    error               TEXT,
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL
);
CREATE INDEX downloads_status ON downloads (status);
CREATE INDEX downloads_file ON downloads (game, nexus_mod_id, nexus_file_id);

-- Cached Nexus responses for update checks.
-- kind 'updated': key = period (e.g. '1m'), body = updated.json response.
-- kind 'files':   key = mod id,             body = files.json response.
CREATE TABLE updates_cache (
    game       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('updated', 'files')),
    key        TEXT NOT NULL,
    body       TEXT NOT NULL,
    fetched_at INTEGER NOT NULL,
    PRIMARY KEY (game, kind, key)
);

-- Cached mod requirements for `deps` (7-day TTL in legacy).
CREATE TABLE requirements_cache (
    game         TEXT NOT NULL,
    nexus_mod_id INTEGER NOT NULL,
    body         TEXT,                             -- JSON requirement nodes; NULL = lookup failed
    available    INTEGER CHECK (available IN (0, 1)),
    fetched_at   INTEGER NOT NULL,
    checked_at   INTEGER,                          -- availability check time
    PRIMARY KEY (game, nexus_mod_id)
);

-- Rate-limit state from x-rl-* headers (single row).
CREATE TABLE ratelimit (
    id               INTEGER PRIMARY KEY CHECK (id = 1),
    hourly_limit     INTEGER,
    hourly_remaining INTEGER,
    hourly_reset     TEXT,
    daily_limit      INTEGER,
    daily_remaining  INTEGER,
    daily_reset      TEXT,
    updated_at       INTEGER NOT NULL
);
