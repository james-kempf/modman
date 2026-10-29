// SPDX-License-Identifier: GPL-3.0-or-later
//! SQLite side: WAL checkpoint of the old DB, library discovery, the
//! single-transaction path rewrite and the symlink retarget (R7).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::{Faults, MigrateError};

/// `(table, column)` pairs holding absolute paths as plain text.
const PATH_COLUMNS: &[(&str, &str)] = &[
    ("mods", "archive"),
    ("mods", "staging_dir"),
    ("backups", "store_path"),
    ("deployments", "source"),
    ("downloads", "dest_path"),
    ("downloads", "part_path"),
    ("collections", "dir"),
];

/// Exact-prefix replacements, without trailing slashes.
#[derive(Debug, Clone, Default)]
pub struct PrefixMap(pub Vec<(String, String)>);

impl PrefixMap {
    pub fn push(&mut self, old: &Path, new: &Path) {
        let s = |p: &Path| p.to_string_lossy().trim_end_matches('/').to_string();
        self.0.push((s(old), s(new)));
    }

    fn apply(pairs: impl Iterator<Item = (String, String)>, v: &str) -> Option<String> {
        for (from, to) in pairs {
            if v == from {
                return Some(to);
            }
            if let Some(rest) = v.strip_prefix(&from).filter(|r| r.starts_with('/')) {
                return Some(format!("{to}{rest}"));
            }
        }
        None
    }

    /// Old -> new, `None` when no prefix matches.
    pub fn map(&self, v: &str) -> Option<String> {
        Self::apply(self.0.iter().cloned(), v)
    }

    /// New -> old (for already rewritten rows).
    pub fn unmap(&self, v: &str) -> Option<String> {
        Self::apply(self.0.iter().map(|(a, b)| (b.clone(), a.clone())), v)
    }
}

fn open(path: &Path) -> Result<Connection, MigrateError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(conn)
}

/// Folds the old DB's WAL into the main file so a plain file copy is
/// complete. Fails when another connection blocks the checkpoint.
pub fn checkpoint(db: &Path) -> Result<(), MigrateError> {
    let conn = open(db)?;
    let busy: i64 = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
    if busy != 0 {
        return Err(MigrateError::Marker(format!(
            "{} is in use (WAL checkpoint blocked)",
            db.display()
        )));
    }
    conn.close().map_err(|(_, e)| e)?;
    Ok(())
}

fn has_table(conn: &Connection, t: &str) -> Result<bool, MigrateError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [t],
        |r| r.get(0),
    )?)
}

/// Steam libraries of the configured games, as staging_root sees them.
pub fn libraries(db: &Path) -> Result<Vec<PathBuf>, MigrateError> {
    if !db.is_file() {
        return Ok(Vec::new());
    }
    let conn = open(db)?;
    if !has_table(&conn, "games")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT folder, library FROM games")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<String>>(1)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (folder, library) = row?;
        let lib = library
            .map(PathBuf::from)
            .or_else(|| folder.and_then(|f| crate::deploy::library_of(Path::new(&f))));
        if let Some(lib) = lib.filter(|l| !out.contains(l)) {
            out.push(lib);
        }
    }
    Ok(out)
}

fn rewrite_json(v: &mut serde_json::Value, map: &PrefixMap) -> bool {
    match v {
        serde_json::Value::String(s) => match map.map(s) {
            Some(n) => {
                *s = n;
                true
            }
            None => false,
        },
        serde_json::Value::Array(a) => a
            .iter_mut()
            .fold(false, |acc, x| rewrite_json(x, map) | acc),
        serde_json::Value::Object(o) => o
            .values_mut()
            .fold(false, |acc, x| rewrite_json(x, map) | acc),
        _ => false,
    }
}

/// Rewrites every stored absolute path in ONE transaction; any error
/// (or `faults.fail_db_rewrite`) rolls everything back. Returns the number
/// of changed values. Idempotent: rewritten values match no old prefix.
pub fn rewrite(db: &Path, map: &PrefixMap, faults: &Faults) -> Result<usize, MigrateError> {
    let mut conn = open(db)?;
    let tx = conn.transaction()?;
    let mut changed = 0;
    let mut update = |table: &str, col: &str, json: bool, filter: &str| {
        if !has_table(&tx, table)? {
            return Ok::<_, MigrateError>(());
        }
        let rows: Vec<(i64, String)> = {
            let mut stmt = tx.prepare(&format!(
                "SELECT rowid, {col} FROM {table} WHERE {col} IS NOT NULL {filter}"
            ))?;
            let r = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            r.collect::<Result<_, _>>()?
        };
        for (id, v) in rows {
            let new = if json {
                // Journal details are JSON: rewrite parsed string values,
                // never the serialized text.
                serde_json::from_str::<serde_json::Value>(&v)
                    .ok()
                    .and_then(|mut j| rewrite_json(&mut j, map).then(|| j.to_string()))
            } else {
                map.map(&v)
            };
            if let Some(new) = new {
                tx.execute(
                    &format!("UPDATE {table} SET {col} = ?1 WHERE rowid = ?2"),
                    rusqlite::params![new, id],
                )?;
                changed += 1;
            }
        }
        Ok(())
    };
    for (t, c) in PATH_COLUMNS {
        update(t, c, false, "")?;
    }
    update("deploy_journal_steps", "detail", true, "")?;
    update(
        "settings",
        "value",
        false,
        &format!("AND key = '{}'", crate::config::ConfigKey::DataDir.name()),
    )?;
    if faults.fail_db_rewrite {
        return Err(MigrateError::InjectedCrash("db rewrite".into()));
    }
    tx.commit()?;
    Ok(changed)
}

/// Outcome of [`retarget`].
#[derive(Debug, Default)]
pub struct Retarget {
    pub retargeted: usize,
    /// Game-side links that no longer point at the old staged file.
    pub skipped: Vec<PathBuf>,
}

/// Points symlink deployments whose `source` was rewritten at the new
/// staged path, atomically (temp link + rename). A link the user changed
/// is left alone and reported.
pub fn retarget(db: &Path, map: &PrefixMap) -> Result<Retarget, MigrateError> {
    let mut out = Retarget::default();
    if !db.is_file() {
        return Ok(out);
    }
    let conn = open(db)?;
    if !has_table(&conn, "deployments")? {
        return Ok(out);
    }
    let mut stmt = conn.prepare(
        "SELECT d.target, d.rel_path, d.source, g.folder, g.prefix
         FROM deployments d JOIN games g ON g.domain = d.game
         WHERE d.method = 'symlink' ORDER BY d.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;
    for row in rows {
        let (target, rel, source, folder, prefix) = row?;
        let Some(old) = map.unmap(&source) else {
            continue; // not rewritten (e.g. offline library)
        };
        let root = if target == "prefix_dir" {
            prefix
        } else {
            folder
        };
        let Some(root) = root else { continue };
        let link = Path::new(&root).join(&rel);
        match std::fs::read_link(&link) {
            Ok(t) if t == Path::new(&source) => {}
            Ok(t) if t == Path::new(&old) => {
                replace_link(&link, Path::new(&source))?;
                out.retargeted += 1;
            }
            _ => out.skipped.push(link),
        }
    }
    Ok(out)
}

fn replace_link(link: &Path, target: &Path) -> Result<(), MigrateError> {
    let name = link.file_name().unwrap_or_default().to_string_lossy();
    let tmp = link.with_file_name(format!(".{name}.vessel-tmp"));
    super::fsops::remove_all(&tmp)?;
    std::os::unix::fs::symlink(target, &tmp).map_err(|e| MigrateError::io(&tmp, e))?;
    std::fs::rename(&tmp, link).map_err(|e| MigrateError::io(link, e))
}
