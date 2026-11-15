// SPDX-License-Identifier: GPL-3.0-or-later
//! Per-game auto-update state (`autoupdate_state`, legacy `autoupdate.json`
//! L4465-4471) and the last-run log (`autoupdate_runs`; new, legacy had none:
//! ambiguity A8).

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// Rows kept in the last-run log; older rows are pruned on insert.
pub const LOG_KEEP: i64 = 500;

/// One row of `autoupdate_state`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GameState {
    /// System fingerprint at the last success or baseline.
    pub system: Option<String>,
    /// Steam buildid at the last success or baseline.
    pub build: Option<String>,
    pub ok_at: Option<i64>,
    /// Last real (not forced) attempt.
    pub tried_at: Option<i64>,
    pub failed: bool,
}

pub fn load_state(conn: &Connection, game: &str) -> rusqlite::Result<GameState> {
    Ok(conn
        .query_row(
            "SELECT system_fp, build, ok_at, tried_at, failed FROM autoupdate_state WHERE game = ?1",
            [game],
            |r| {
                Ok(GameState {
                    system: r.get(0)?,
                    build: r.get(1)?,
                    ok_at: r.get(2)?,
                    tried_at: r.get(3)?,
                    failed: r.get::<_, i64>(4)? != 0,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

pub fn save_state(conn: &Connection, game: &str, s: &GameState) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO autoupdate_state (game, system_fp, build, ok_at, tried_at, failed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (game) DO UPDATE SET system_fp = excluded.system_fp,
             build = excluded.build, ok_at = excluded.ok_at,
             tried_at = excluded.tried_at, failed = excluded.failed",
        params![
            game,
            s.system,
            s.build,
            s.ok_at,
            s.tried_at,
            i64::from(s.failed)
        ],
    )?;
    Ok(())
}

/// Outcome stored in the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// Nothing to do (incl. a recorded baseline).
    Nothing,
    /// Mods updated and/or redeployed (or would be, on a dry run).
    Acted,
    Failed,
    /// Due, but blocked (system or Steam updating, game running, back-off,
    /// folder missing); retried on the next run.
    Skipped,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Nothing => "nothing",
            Outcome::Acted => "acted",
            Outcome::Failed => "failed",
            Outcome::Skipped => "skipped",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "acted" => Outcome::Acted,
            "failed" => Outcome::Failed,
            "skipped" => Outcome::Skipped,
            _ => Outcome::Nothing,
        }
    }
}

/// One row of the last-run log. `game = None` is the summary of a whole run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunLogEntry {
    pub id: i64,
    pub game: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub forced: bool,
    pub dry_run: bool,
    pub outcome: Outcome,
    pub reasons: Vec<String>,
    pub actions: Vec<String>,
    pub summary: Option<String>,
}

fn json_list(v: &[String]) -> Option<String> {
    (!v.is_empty()).then(|| serde_json::to_string(v).unwrap_or_default())
}

fn parse_list(s: Option<String>) -> Vec<String> {
    s.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Appends a log row (its `id` is ignored) and prunes old rows.
pub fn log_run(conn: &Connection, e: &RunLogEntry) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO autoupdate_runs
             (game, started_at, finished_at, forced, dry_run, outcome, reasons, actions, summary)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            e.game,
            e.started_at,
            e.finished_at,
            i64::from(e.forced),
            i64::from(e.dry_run),
            e.outcome.as_str(),
            json_list(&e.reasons),
            json_list(&e.actions),
            e.summary,
        ],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "DELETE FROM autoupdate_runs WHERE id <= (
             SELECT id FROM autoupdate_runs ORDER BY id DESC LIMIT 1 OFFSET ?1)",
        [LOG_KEEP],
    )?;
    Ok(id)
}

/// Newest log rows first.
pub fn last_runs(conn: &Connection, limit: usize) -> rusqlite::Result<Vec<RunLogEntry>> {
    let mut st = conn.prepare(
        "SELECT id, game, started_at, finished_at, forced, dry_run, outcome, reasons, actions, summary
         FROM autoupdate_runs ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = st.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |r| {
        Ok(RunLogEntry {
            id: r.get(0)?,
            game: r.get(1)?,
            started_at: r.get(2)?,
            finished_at: r.get(3)?,
            forced: r.get::<_, i64>(4)? != 0,
            dry_run: r.get::<_, i64>(5)? != 0,
            outcome: Outcome::parse(&r.get::<_, String>(6)?),
            reasons: parse_list(r.get(7)?),
            actions: parse_list(r.get(8)?),
            summary: r.get(9)?,
        })
    })?;
    rows.collect()
}
