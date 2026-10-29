// SPDX-License-Identifier: GPL-3.0-or-later
//! Mod requirements for `deps`, cached in `requirements_cache` (legacy
//! `deps_fetch` L2913-2947): GraphQL `modRequirements` in batches of 40,
//! re-queried after 7 days, plus availability of the required mods.

use std::collections::BTreeSet;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::graphql::lenient_u64;
use super::{NexusClient, NexusError, SharedStore, REQUIREMENT_FIELDS};
use crate::store::{self, Store};

pub const BATCH_SIZE: usize = 40;
/// Seconds before a cached lookup is repeated.
pub const REQUIREMENTS_TTL: i64 = 604_800;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Requirement {
    #[serde(default, deserialize_with = "lenient_u64")]
    pub mod_id: Option<u64>,
    #[serde(default)]
    pub mod_name: Option<String>,
    #[serde(default, deserialize_with = "lenient_u64")]
    pub game_id: Option<u64>,
    #[serde(default)]
    pub external_requirement: bool,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

impl Requirement {
    /// The required Nexus mod of `game` (by id, or linked by URL), if any.
    pub fn nexus_mod(&self, game: &str, game_id: u64) -> Option<u64> {
        let same_game = matches!(self.game_id, None | Some(0)) || self.game_id == Some(game_id);
        let id = if !self.external_requirement && same_game {
            self.mod_id
        } else {
            self.url.as_deref().and_then(|u| mod_from_url(u, game))
        };
        id.filter(|&m| m != 0)
    }
}

/// `nexusmods.com/[games/]<game>/mods/<id>` -> id, when `<game>` matches.
fn mod_from_url(url: &str, game: &str) -> Option<u64> {
    let lower = url.to_ascii_lowercase();
    let rest = &lower[lower.find("nexusmods.com/")? + "nexusmods.com/".len()..];
    let rest = rest.strip_prefix("games/").unwrap_or(rest);
    let (dom, rest) = rest.split_once('/')?;
    if dom != game {
        return None;
    }
    let digits: String = rest
        .strip_prefix("mods/")?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// One `requirements_cache` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedRequirements {
    /// `None` when only the availability is known.
    pub requirements: Option<Vec<Requirement>>,
    /// Whether the mod can still be downloaded from Nexus.
    pub available: Option<bool>,
    pub fetched_at: i64,
    pub checked_at: Option<i64>,
}

pub fn cached_requirements(
    store: &Store,
    game: &str,
    mod_id: u64,
) -> Result<Option<CachedRequirements>, NexusError> {
    let row = store
        .conn()
        .query_row(
            "SELECT body, available, fetched_at, checked_at FROM requirements_cache
             WHERE game = ?1 AND nexus_mod_id = ?2",
            params![game, mod_id as i64],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<bool>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .optional()?;
    Ok(row.map(
        |(body, available, fetched_at, checked_at)| CachedRequirements {
            requirements: body.and_then(|b| serde_json::from_str(&b).ok()),
            available,
            fetched_at,
            checked_at,
        },
    ))
}

fn lock(store: &SharedStore) -> std::sync::MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(|p| p.into_inner())
}

/// Errors that stop the whole refresh; others skip one batch like legacy.
fn fatal(e: &NexusError) -> bool {
    matches!(
        e,
        NexusError::Unauthorized
            | NexusError::NotLoggedIn
            | NexusError::RateLimited { .. }
            | NexusError::RateLimitLow { .. }
    )
}

/// Makes sure the cache knows the requirements of `mod_ids` and whether the
/// Nexus mods they require are still available. `refresh` ignores the TTL.
pub async fn refresh_requirements(
    client: &NexusClient,
    store: &SharedStore,
    game: &str,
    game_id: u64,
    mod_ids: &[u64],
    refresh: bool,
) -> Result<(), NexusError> {
    let now = store::now();
    let ids: BTreeSet<u64> = mod_ids.iter().copied().collect();
    let stale: Vec<u64> = {
        let s = lock(store);
        let mut v = Vec::new();
        for &m in &ids {
            let c = cached_requirements(&s, game, m)?;
            let fresh = c.is_some_and(|c| {
                c.requirements.is_some() && now - c.fetched_at <= REQUIREMENTS_TTL
            });
            if refresh || !fresh {
                v.push(m);
            }
        }
        v
    };
    for chunk in stale.chunks(BATCH_SIZE) {
        let res = match client.mod_batch(game_id, chunk, REQUIREMENT_FIELDS).await {
            Ok(r) => r,
            Err(e) if fatal(&e) => return Err(e),
            Err(e) => {
                tracing::warn!(error = %e, "requirement lookup failed for one batch");
                continue;
            }
        };
        let s = lock(store);
        for (m, v) in res {
            let nodes = v
                .as_ref()
                .and_then(|v| v.pointer("/modRequirements/nexusRequirements/nodes"))
                .cloned()
                .unwrap_or_else(|| Value::Array(vec![]));
            let reqs: Vec<Requirement> = serde_json::from_value(nodes).unwrap_or_default();
            let body =
                serde_json::to_string(&reqs).map_err(|e| NexusError::Decode(e.to_string()))?;
            s.conn().execute(
                "INSERT INTO requirements_cache (game, nexus_mod_id, body, available, fetched_at, checked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT (game, nexus_mod_id) DO UPDATE SET body = excluded.body,
                    available = excluded.available, fetched_at = excluded.fetched_at,
                    checked_at = excluded.checked_at",
                params![game, m as i64, body, v.is_some(), now],
            )?;
        }
    }

    let need: Vec<u64> = {
        let s = lock(store);
        let mut required = BTreeSet::new();
        for &m in &ids {
            if let Some(reqs) = cached_requirements(&s, game, m)?.and_then(|c| c.requirements) {
                required.extend(reqs.iter().filter_map(|r| r.nexus_mod(game, game_id)));
            }
        }
        let mut v = Vec::new();
        for m in required {
            let c = cached_requirements(&s, game, m)?;
            let known = c.is_some_and(|c| {
                c.available.is_some() && now - c.checked_at.unwrap_or(0) <= REQUIREMENTS_TTL
            });
            if refresh || !known {
                v.push(m);
            }
        }
        v
    };
    for chunk in need.chunks(BATCH_SIZE) {
        let res = match client.mod_batch(game_id, chunk, "modId").await {
            Ok(r) => r,
            Err(e) if fatal(&e) => return Err(e),
            Err(e) => {
                tracing::warn!(error = %e, "availability lookup failed for one batch");
                continue;
            }
        };
        let s = lock(store);
        for (m, v) in res {
            s.conn().execute(
                "INSERT INTO requirements_cache (game, nexus_mod_id, body, available, fetched_at, checked_at)
                 VALUES (?1, ?2, NULL, ?3, 0, ?4)
                 ON CONFLICT (game, nexus_mod_id) DO UPDATE SET
                    available = excluded.available, checked_at = excluded.checked_at",
                params![game, m as i64, v.is_some(), now],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_mod_from_id_or_url() {
        let r = |ext: bool, gid: Option<u64>, id: Option<u64>, url: Option<&str>| Requirement {
            mod_id: id,
            mod_name: None,
            game_id: gid,
            external_requirement: ext,
            url: url.map(str::to_owned),
            notes: None,
        };
        assert_eq!(
            r(false, Some(1704), Some(5), None).nexus_mod("sse", 1704),
            Some(5)
        );
        assert_eq!(
            r(false, None, Some(5), None).nexus_mod("sse", 1704),
            Some(5)
        );
        assert_eq!(
            r(false, Some(1), Some(5), None).nexus_mod("sse", 1704),
            None
        );
        let u = Some("https://www.NexusMods.com/sse/mods/266?tab=files");
        assert_eq!(r(true, None, None, u).nexus_mod("sse", 1704), Some(266));
        let u = Some("https://next.nexusmods.com/games/sse/mods/12");
        assert_eq!(r(true, None, None, u).nexus_mod("sse", 1704), Some(12));
        let u = Some("https://www.nexusmods.com/other/mods/12");
        assert_eq!(r(true, None, None, u).nexus_mod("sse", 1704), None);
        assert_eq!(r(false, None, Some(0), None).nexus_mod("sse", 1704), None);
    }
}
