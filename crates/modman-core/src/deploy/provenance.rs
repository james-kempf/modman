// SPDX-License-Identifier: GPL-3.0-or-later
//! Journal reads: which game files modman itself placed. The deploy backup
//! step uses this so a file modman placed earlier (with its tracking lost)
//! is never stored as the game's "original". Provenance comes from the
//! journal, never from comparing bytes: a mod may ship bytes identical to
//! the vanilla file.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::Connection;

use super::place::sha256_file;
use super::DeployError;

/// Lower-cased `rel_path` -> SHA-256 of each placement there (`None` when
/// the record does not say).
pub(crate) type Placements = HashMap<String, Vec<Option<String>>>;

/// Placements of committed deploys of `domain` into `target` that nothing
/// undid since: no committed purge and no rollback came after their
/// journal (both remove every recorded placement). With intact tracking a
/// redeploy always purges first, so this is non-empty only when the
/// `deployments` rows of an earlier deploy were lost.
pub(crate) fn untracked_placements(
    conn: &Connection,
    domain: &str,
    target: &str,
) -> Result<Placements, DeployError> {
    // Older records carry no sha256 in `detail`; the mod's file list
    // (mod_files of the recorded mod at the recorded source) fills it in.
    let mut stmt = conn.prepare(
        "SELECT s.rel_path,
                COALESCE(json_extract(s.detail, '$.sha256'),
                         (SELECT f.sha256 FROM mod_files f JOIN mods m ON m.id = f.mod_id
                          WHERE m.id = json_extract(s.detail, '$.mod_id')
                            AND m.staging_dir || '/' || f.path = json_extract(s.detail, '$.source')))
         FROM deploy_journal_steps s JOIN deploy_journal j ON j.id = s.journal_id
         WHERE j.game = ?1 AND j.op IN ('deploy', 'redeploy') AND j.state = 'committed'
           AND s.action = 'place' AND s.done = 1 AND s.target = ?2
           AND NOT EXISTS (
               SELECT 1 FROM deploy_journal u
               WHERE u.game = j.game AND u.id > j.id
                 AND ((u.op = 'purge' AND u.state = 'committed') OR u.state = 'rolled_back'))",
    )?;
    let rows = stmt.query_map([domain, target], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;
    let mut out = Placements::new();
    for row in rows {
        let (rel, sha) = row?;
        out.entry(rel.to_lowercase()).or_default().push(sha);
    }
    Ok(out)
}

/// True when the journal shows modman placed the file at `rel` (real file
/// at `abs`). When the records name what was placed, the file must still
/// hold one of those placements; something else there (a game update, a
/// user edit) is backed up as an original.
pub(crate) fn placed_by_modman(placements: &Placements, rel: &str, abs: &Path) -> bool {
    let Some(shas) = placements.get(&rel.to_lowercase()) else {
        return false;
    };
    let known: Vec<&str> = shas.iter().flatten().map(String::as_str).collect();
    known.is_empty() || sha256_file(abs).is_ok_and(|s| known.contains(&s.as_str()))
}
