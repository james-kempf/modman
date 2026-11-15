-- SPDX-License-Identifier: GPL-3.0-or-later
-- Game identity: a game's key is its own stable id, separate from its Nexus
-- domain, so the domain can be corrected without touching dependent rows or
-- folders. Existing ids keep their old domain value (data folders, the active
-- game setting and every game column in other tables stay valid as they are).
-- RENAME COLUMN also rewrites the REFERENCES games(domain) clauses.

ALTER TABLE games RENAME COLUMN domain TO id;

-- Nexus game domain (nexusmods.com/<domain>); NULL = not linked to Nexus.
-- Not unique: two installs of one game may share a domain.
ALTER TABLE games ADD COLUMN nexus_domain TEXT;
UPDATE games SET nexus_domain = id;
CREATE INDEX games_nexus_domain ON games (nexus_domain);
