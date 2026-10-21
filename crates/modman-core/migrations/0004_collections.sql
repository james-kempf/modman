-- SPDX-License-Identifier: GPL-3.0-or-later
-- Collections, FOMOD choices and acknowledged requirements.

CREATE TABLE collections (
    id                 INTEGER PRIMARY KEY,
    game               TEXT NOT NULL REFERENCES games(domain) ON DELETE CASCADE,
    slug               TEXT NOT NULL,
    name               TEXT,
    author             TEXT,
    revision           INTEGER NOT NULL,
    dir                TEXT,                       -- local folder with revision.json etc.
    manifest           TEXT,                       -- collection.json (JSON)
    rules              TEXT,                       -- modRules (JSON)
    external_resources TEXT,                       -- externalResources (JSON)
    failed             TEXT,                       -- JSON array of mod names that failed
    installed_at       INTEGER NOT NULL,
    UNIQUE (game, slug)
);

-- Membership (legacy mods[].collections) and collection order.
CREATE TABLE collection_mods (
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    mod_id        INTEGER NOT NULL REFERENCES mods(id) ON DELETE CASCADE,
    position      INTEGER NOT NULL,
    PRIMARY KEY (collection_id, mod_id)
);
CREATE INDEX collection_mods_mod ON collection_mods (mod_id);

-- Recorded FOMOD choices, Vortex format {"type":"fomod","options":[...]}.
CREATE TABLE fomod_choices (
    mod_id      INTEGER PRIMARY KEY REFERENCES mods(id) ON DELETE CASCADE,
    choices     TEXT NOT NULL,
    mode        TEXT,                              -- mode that produced them (ask|default|choices)
    recorded_at INTEGER NOT NULL
);

-- `deps ack` keys per game (tool:<k>, nexus:<id>, url:<...>).
CREATE TABLE deps_ack (
    game     TEXT NOT NULL REFERENCES games(domain) ON DELETE CASCADE,
    key      TEXT NOT NULL,
    acked_at INTEGER NOT NULL,
    PRIMARY KEY (game, key)
);
