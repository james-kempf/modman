// SPDX-License-Identifier: GPL-3.0-or-later
//! Update checks via `updated.json` + `files.json`, cached in `updates_cache`
//! (legacy `cmd_updates` L3730-3790).

use std::collections::{BTreeMap, HashMap};

use rusqlite::{params, OptionalExtension};

use super::ratelimit::BULK_RESERVE;
use super::{FileInfo, ModFiles, NexusClient, NexusError, SharedStore};
use crate::store::{self, Store};

/// A cached file list younger than this is never re-fetched.
const FRESH_SECS: i64 = 21_600;
/// Older than this it is re-fetched even when `updated.json` is silent.
const MAX_AGE_SECS: i64 = 2_419_200;
const PERIOD: &str = "1m";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateCheck {
    /// Cached file lists of the checked mods.
    pub files: BTreeMap<u64, ModFiles>,
    /// Mods whose list was fetched from Nexus this run.
    pub fetched: usize,
    /// Stopped because the hourly budget fell below [`BULK_RESERVE`].
    pub stopped_early: bool,
}

fn lock(store: &SharedStore) -> std::sync::MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(|p| p.into_inner())
}

fn cached_files(s: &Store, game: &str, mod_id: u64) -> Result<Option<(ModFiles, i64)>, NexusError> {
    let row: Option<(String, i64)> = s
        .conn()
        .query_row(
            "SELECT body, fetched_at FROM updates_cache WHERE game = ?1 AND kind = 'files' AND key = ?2",
            params![game, mod_id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(b, t)| serde_json::from_str(&b).ok().map(|f| (f, t))))
}

fn put_cache(
    s: &Store,
    game: &str,
    kind: &str,
    key: &str,
    body: &str,
    now: i64,
) -> Result<(), NexusError> {
    s.conn().execute(
        "INSERT INTO updates_cache (game, kind, key, body, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (game, kind, key) DO UPDATE SET body = excluded.body, fetched_at = excluded.fetched_at",
        params![game, kind, key, body, now],
    )?;
    Ok(())
}

/// Refreshes the file lists of `mod_ids` as needed and returns them.
/// `refresh` ignores the cache ages.
pub async fn check_updates(
    client: &NexusClient,
    store: &SharedStore,
    game: &str,
    mod_ids: &[u64],
    refresh: bool,
) -> Result<UpdateCheck, NexusError> {
    let mut out = UpdateCheck::default();
    let updated: HashMap<u64, i64> = match client.updated_mods(game, PERIOD).await {
        Ok(list) => {
            let body =
                serde_json::to_string(&list).map_err(|e| NexusError::Decode(e.to_string()))?;
            put_cache(&lock(store), game, "updated", PERIOD, &body, store::now())?;
            list.into_iter()
                .map(|u| (u.mod_id, u.latest_file_update))
                .collect()
        }
        Err(e @ (NexusError::Unauthorized | NexusError::RateLimited { .. })) => return Err(e),
        Err(e) => {
            tracing::warn!(error = %e, "updated.json failed; checking every mod");
            HashMap::new()
        }
    };
    for &m in mod_ids {
        let now = store::now();
        let cached = cached_files(&lock(store), game, m)?;
        if let (Some((_, t)), false) = (&cached, refresh) {
            let age = now - t;
            if age < FRESH_SECS {
                continue;
            }
            if age < MAX_AGE_SECS && updated.get(&m).is_none_or(|u| u < t) {
                lock(store).conn().execute(
                    "UPDATE updates_cache SET fetched_at = ?1 WHERE game = ?2 AND kind = 'files' AND key = ?3",
                    params![now, game, m.to_string()],
                )?;
                continue;
            }
        }
        if client.ensure_budget(BULK_RESERVE).is_err() {
            out.stopped_early = true;
            break;
        }
        match client.mod_files(game, m).await {
            Ok(files) => {
                let body =
                    serde_json::to_string(&files).map_err(|e| NexusError::Decode(e.to_string()))?;
                put_cache(&lock(store), game, "files", &m.to_string(), &body, now)?;
                out.fetched += 1;
            }
            Err(e @ (NexusError::Unauthorized | NexusError::RateLimited { .. })) => return Err(e),
            Err(e) => tracing::warn!(mod_id = m, error = %e, "could not list files"),
        }
        if client.ensure_budget(BULK_RESERVE).is_err() {
            out.stopped_early = true;
            break;
        }
    }
    let s = lock(store);
    for &m in mod_ids {
        if let Some((f, _)) = cached_files(&s, game, m)? {
            out.files.insert(m, f);
        }
    }
    Ok(out)
}

/// The newest live file at the end of `file_id`'s update chain, when it
/// differs from `file_id` (archived and deleted files are skipped).
pub fn latest_file(files: &ModFiles, file_id: u64) -> Option<&FileInfo> {
    let next: HashMap<u64, u64> = files
        .file_updates
        .iter()
        .map(|u| (u.old_file_id, u.new_file_id))
        .collect();
    let live = |id: u64| {
        files.files.iter().find(|f| {
            f.file_id == id && !matches!(f.category_name.as_deref(), Some("ARCHIVED" | "DELETED"))
        })
    };
    let mut cur = file_id;
    let mut best = None;
    // Bounded like legacy (`limit(100; …)`) so a cyclic chain terminates.
    for _ in 0..100 {
        if let Some(f) = live(cur) {
            best = Some(f);
        }
        match next.get(&cur) {
            Some(&n) => cur = n,
            None => break,
        }
    }
    best.filter(|f| f.file_id != file_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nexus::FileUpdate;

    fn file(id: u64, cat: &str) -> FileInfo {
        FileInfo {
            file_id: id,
            name: format!("f{id}"),
            version: None,
            category_name: Some(cat.into()),
            file_name: None,
            size_in_bytes: None,
            uploaded_timestamp: None,
        }
    }

    #[test]
    fn latest_file_follows_chain_and_skips_archived() {
        let files = ModFiles {
            files: vec![file(1, "OLD_VERSION"), file(2, "MAIN"), file(3, "ARCHIVED")],
            file_updates: vec![
                FileUpdate {
                    old_file_id: 1,
                    new_file_id: 2,
                },
                FileUpdate {
                    old_file_id: 2,
                    new_file_id: 3,
                },
            ],
        };
        assert_eq!(latest_file(&files, 1).map(|f| f.file_id), Some(2));
        assert_eq!(latest_file(&files, 2), None);
    }

    #[test]
    fn cyclic_chain_terminates() {
        let files = ModFiles {
            files: vec![file(1, "MAIN"), file(2, "MAIN")],
            file_updates: vec![
                FileUpdate {
                    old_file_id: 1,
                    new_file_id: 2,
                },
                FileUpdate {
                    old_file_id: 2,
                    new_file_id: 1,
                },
            ],
        };
        assert!(latest_file(&files, 1).is_some());
    }
}
