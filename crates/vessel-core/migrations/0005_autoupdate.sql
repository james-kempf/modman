-- SPDX-License-Identifier: GPL-3.0-or-later
-- Auto-update state per game and last-run log (owner: autoupdate).

CREATE TABLE autoupdate_state (
    game      TEXT PRIMARY KEY REFERENCES games(domain) ON DELETE CASCADE,
    system_fp TEXT,                                -- system fingerprint at last success/baseline
    build     TEXT,                                -- Steam buildid at last success/baseline
    ok_at     INTEGER,
    tried_at  INTEGER,
    failed    INTEGER NOT NULL DEFAULT 0 CHECK (failed IN (0, 1))
);

-- Last-run log shown in the GUI and `autoupdate status`.
CREATE TABLE autoupdate_runs (
    id          INTEGER PRIMARY KEY,
    game        TEXT,                              -- NULL = whole run summary
    started_at  INTEGER NOT NULL,
    finished_at INTEGER,
    forced      INTEGER NOT NULL DEFAULT 0 CHECK (forced IN (0, 1)),
    dry_run     INTEGER NOT NULL DEFAULT 0 CHECK (dry_run IN (0, 1)),
    outcome     TEXT NOT NULL,                     -- nothing|acted|failed|skipped
    reasons     TEXT,                              -- JSON array (system updated, game updated, ...)
    actions     TEXT,                              -- JSON array of actions taken
    summary     TEXT
);
CREATE INDEX autoupdate_runs_started ON autoupdate_runs (started_at);
