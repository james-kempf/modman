// SPDX-License-Identifier: GPL-3.0-or-later
//! `x-rl-*` rate-limit headers, persisted in the single-row `ratelimit` table.

use reqwest::header::HeaderMap;
use rusqlite::{params, OptionalExtension};

use super::NexusError;
use crate::store::Store;

/// Hourly requests kept back by bulk operations (`updates` stops below
/// this, legacy L3751-3754).
pub const BULK_RESERVE: u32 = 20;

/// How long an exhausted state is trusted when the reset time is unknown.
const WINDOW_SECS: i64 = 3600;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RateLimit {
    pub hourly_limit: Option<u32>,
    pub hourly_remaining: Option<u32>,
    pub hourly_reset: Option<String>,
    pub daily_limit: Option<u32>,
    pub daily_remaining: Option<u32>,
    pub daily_reset: Option<String>,
    /// Unix time the headers were seen.
    pub updated_at: i64,
}

fn header_u32(h: &HeaderMap, name: &str) -> Option<u32> {
    h.get(name)?.to_str().ok()?.trim().parse().ok()
}

fn header_str(h: &HeaderMap, name: &str) -> Option<String> {
    let v = h.get(name)?.to_str().ok()?.trim();
    (!v.is_empty() && v.len() <= 64).then(|| v.to_owned())
}

impl RateLimit {
    /// Parses the headers; `None` when the response carries no remaining counts.
    pub fn from_headers(h: &HeaderMap, now: i64) -> Option<Self> {
        let rl = Self {
            hourly_limit: header_u32(h, "x-rl-hourly-limit"),
            hourly_remaining: header_u32(h, "x-rl-hourly-remaining"),
            hourly_reset: header_str(h, "x-rl-hourly-reset"),
            daily_limit: header_u32(h, "x-rl-daily-limit"),
            daily_remaining: header_u32(h, "x-rl-daily-remaining"),
            daily_reset: header_str(h, "x-rl-daily-reset"),
            updated_at: now,
        };
        (rl.hourly_remaining.is_some() || rl.daily_remaining.is_some()).then_some(rl)
    }

    /// Nexus serves requests while either bucket has requests left. Refuses
    /// locally when both are known to be empty within the last hour.
    pub fn check(&self, now: i64) -> Result<(), NexusError> {
        let exhausted = self.hourly_remaining == Some(0) && self.daily_remaining == Some(0);
        if exhausted && now - self.updated_at < WINDOW_SECS {
            return Err(self.limited());
        }
        Ok(())
    }

    /// Fails when fewer than `reserve` hourly requests are left (bulk work).
    pub fn ensure_budget(&self, reserve: u32) -> Result<(), NexusError> {
        match self.hourly_remaining {
            Some(remaining) if remaining < reserve => {
                Err(NexusError::RateLimitLow { remaining, reserve })
            }
            _ => Ok(()),
        }
    }

    pub(crate) fn limited(&self) -> NexusError {
        let show = |v: Option<u32>| v.map_or_else(|| "?".to_owned(), |n| n.to_string());
        NexusError::RateLimited {
            hourly: show(self.hourly_remaining),
            daily: show(self.daily_remaining),
            reset: self
                .hourly_reset
                .clone()
                .unwrap_or_else(|| "unknown".into()),
        }
    }

    /// `status` line, e.g. "80 hourly / 2400 daily requests left".
    pub fn summary(&self) -> String {
        let show = |v: Option<u32>| v.map_or_else(|| "?".to_owned(), |n| n.to_string());
        format!(
            "{} hourly / {} daily requests left",
            show(self.hourly_remaining),
            show(self.daily_remaining)
        )
    }

    pub fn load(store: &Store) -> Result<Option<Self>, NexusError> {
        let row = store
            .conn()
            .query_row(
                "SELECT hourly_limit, hourly_remaining, hourly_reset, daily_limit,
                        daily_remaining, daily_reset, updated_at FROM ratelimit WHERE id = 1",
                [],
                |r| {
                    Ok(Self {
                        hourly_limit: r.get(0)?,
                        hourly_remaining: r.get(1)?,
                        hourly_reset: r.get(2)?,
                        daily_limit: r.get(3)?,
                        daily_remaining: r.get(4)?,
                        daily_reset: r.get(5)?,
                        updated_at: r.get(6)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn save(&self, store: &Store) -> Result<(), NexusError> {
        store.conn().execute(
            "INSERT INTO ratelimit (id, hourly_limit, hourly_remaining, hourly_reset,
                                    daily_limit, daily_remaining, daily_reset, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (id) DO UPDATE SET
                hourly_limit = excluded.hourly_limit, hourly_remaining = excluded.hourly_remaining,
                hourly_reset = excluded.hourly_reset, daily_limit = excluded.daily_limit,
                daily_remaining = excluded.daily_remaining, daily_reset = excluded.daily_reset,
                updated_at = excluded.updated_at",
            params![
                self.hourly_limit,
                self.hourly_remaining,
                self.hourly_reset,
                self.daily_limit,
                self.daily_remaining,
                self.daily_reset,
                self.updated_at
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, v.parse().unwrap());
        }
        h
    }

    #[test]
    fn parses_headers_and_ignores_responses_without_them() {
        assert_eq!(RateLimit::from_headers(&HeaderMap::new(), 1), None);
        let rl = RateLimit::from_headers(
            &headers(&[
                ("x-rl-hourly-limit", "100"),
                ("x-rl-hourly-remaining", "42"),
                ("x-rl-daily-remaining", "2400"),
                ("x-rl-hourly-reset", "2026-10-20T13:00:00+00:00"),
            ]),
            7,
        )
        .unwrap();
        assert_eq!(rl.hourly_remaining, Some(42));
        assert_eq!(rl.daily_remaining, Some(2400));
        assert_eq!(rl.updated_at, 7);
        assert_eq!(rl.summary(), "42 hourly / 2400 daily requests left");
    }

    #[test]
    fn exhausted_state_expires_after_an_hour() {
        let rl = RateLimit {
            hourly_remaining: Some(0),
            daily_remaining: Some(0),
            updated_at: 1000,
            ..Default::default()
        };
        assert!(matches!(
            rl.check(1001),
            Err(NexusError::RateLimited { .. })
        ));
        assert!(rl.check(1000 + WINDOW_SECS).is_ok());
        let daily_left = RateLimit {
            daily_remaining: Some(5),
            ..rl.clone()
        };
        assert!(daily_left.check(1001).is_ok());
    }

    #[test]
    fn budget_below_reserve_is_typed_error() {
        let rl = RateLimit {
            hourly_remaining: Some(19),
            ..Default::default()
        };
        assert!(matches!(
            rl.ensure_budget(BULK_RESERVE),
            Err(NexusError::RateLimitLow {
                remaining: 19,
                reserve: 20
            })
        ));
        assert!(RateLimit::default().ensure_budget(BULK_RESERVE).is_ok());
    }

    #[test]
    fn roundtrips_through_store() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(RateLimit::load(&store).unwrap(), None);
        let rl = RateLimit {
            hourly_remaining: Some(3),
            daily_remaining: Some(9),
            hourly_reset: Some("r".into()),
            updated_at: 11,
            ..Default::default()
        };
        rl.save(&store).unwrap();
        let rl2 = RateLimit {
            hourly_remaining: Some(2),
            ..rl.clone()
        };
        rl2.save(&store).unwrap();
        assert_eq!(RateLimit::load(&store).unwrap(), Some(rl2));
    }
}
