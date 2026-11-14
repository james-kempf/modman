// SPDX-License-Identifier: GPL-3.0-or-later
//! Lossless restore of uninstalled mods: the store side. A
//! [`RestoreSnapshot`] copies the whole `mods` row and every row of each
//! child table that references it, column by column (read with `SELECT *`,
//! so a column added by a later migration is captured without code
//! changes). [`apply`] writes a snapshot back over a freshly reinstalled row.
//!
//! Tables with a foreign key to `mods` (all migrations, checked by a test):
//! - `mod_files` (CASCADE): snapshotted; the reinstall rebuilds it from the
//!   staged files and [`apply`] verifies the rebuilt rows match.
//! - `fomod_choices` (CASCADE): snapshotted and written back.
//! - `collection_mods` (CASCADE): snapshotted and written back; a collection
//!   that no longer exists is skipped with a warning.
//! - `deployments.mod_id` (SET NULL): not needed; deploy state is rebuilt
//!   by the next deploy from the restored rows.

use std::path::{Path, PathBuf};

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, Transaction};

use super::{list_mods, ModRecord, ModsError};
use crate::store::{Store, StoreError};

/// Child tables copied with a snapshot, each keyed by its `mod_id` column.
pub const CHILD_TABLES: &[&str] = &["mod_files", "fomod_choices", "collection_mods"];

/// Tables with a foreign key to `mods` that a snapshot leaves out, and why.
pub const NOT_SNAPSHOTTED: &[(&str, &str)] = &[(
    "deployments",
    "ON DELETE SET NULL; rebuilt by the next deploy from the restored rows",
)];

/// Columns of `mods` that [`apply`] does not copy back: `id` is the new row
/// (children are remapped to it), `priority` comes from the clamped
/// position, `staging_dir` is where the reinstall put the files (equal
/// unless the staging root moved since the uninstall).
const NOT_COPIED: &[&str] = &["id", "priority", "staging_dir"];

/// One row as (column, value) pairs, in table order.
pub type RowValues = Vec<(String, Value)>;

/// Everything needed to put one uninstalled mod back exactly as it was.
#[derive(Debug, Clone, PartialEq)]
pub struct RestoreSnapshot {
    pub record: ModRecord,
    /// Every column of the `mods` row.
    pub row: RowValues,
    /// Rows of each [`CHILD_TABLES`] entry, in that order.
    pub children: Vec<(&'static str, Vec<RowValues>)>,
    /// (collection id, slug) of each membership, for warnings.
    pub collections: Vec<(i64, String)>,
}

impl RestoreSnapshot {
    pub fn child_rows(&self, table: &str) -> &[RowValues] {
        self.children
            .iter()
            .find(|(t, _)| *t == table)
            .map_or(&[], |(_, r)| r.as_slice())
    }

    /// Recorded FOMOD choices (JSON), if any.
    pub fn fomod_choices(&self) -> Option<String> {
        self.child_rows("fomod_choices").first().and_then(|r| {
            r.iter()
                .find(|(c, _)| c == "choices")
                .and_then(|(_, v)| match v {
                    Value::Text(s) => Some(s.clone()),
                    _ => None,
                })
        })
    }
}

fn select_rows(conn: &Connection, sql: &str, id: i64) -> rusqlite::Result<Vec<RowValues>> {
    let mut st = conn.prepare(sql)?;
    let names: Vec<String> = st.column_names().into_iter().map(str::to_string).collect();
    let rows = st.query_map([id], |r| {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| Ok((n.clone(), r.get::<_, Value>(i)?)))
            .collect::<rusqlite::Result<RowValues>>()
    })?;
    rows.collect()
}

/// Copies `m`'s row and child rows. Call before the rows are deleted.
pub fn capture(conn: &Connection, m: &ModRecord) -> Result<RestoreSnapshot, ModsError> {
    let row = select_rows(conn, "SELECT * FROM mods WHERE id = ?1", m.id)?
        .into_iter()
        .next()
        .ok_or_else(|| ModsError::NotFound(m.uid.clone()))?;
    let mut children = Vec::new();
    for t in CHILD_TABLES {
        let sql = format!("SELECT * FROM \"{t}\" WHERE mod_id = ?1 ORDER BY rowid");
        children.push((*t, select_rows(conn, &sql, m.id)?));
    }
    let mut st = conn.prepare(
        "SELECT c.id, c.slug FROM collection_mods cm JOIN collections c ON c.id = cm.collection_id
         WHERE cm.mod_id = ?1",
    )?;
    let collections = st
        .query_map([m.id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(RestoreSnapshot {
        record: m.clone(),
        row,
        children,
        collections,
    })
}

/// Why `snap` cannot go back without touching another mod, if so: its uid,
/// its name (case-insensitive) or its install folder is taken. `dest` is
/// where the reinstall would stage it.
pub fn conflict(
    conn: &Connection,
    game: &str,
    snap: &RestoreSnapshot,
    dest: &Path,
) -> Result<Option<String>, ModsError> {
    let m = &snap.record;
    let name = m.name.to_lowercase();
    let folders: Vec<&Path> = std::iter::once(dest)
        .chain(m.staging_dir.as_deref())
        .collect();
    for o in list_mods(conn, game)? {
        if o.uid == m.uid {
            return Ok(Some(format!("the id {} is used by {}", m.uid, o.name)));
        }
        if o.name.to_lowercase() == name {
            return Ok(Some(format!("another mod is named {}", o.name)));
        }
        if o.staging_dir
            .as_deref()
            .is_some_and(|d| folders.contains(&d))
        {
            return Ok(Some(format!("{} uses its install folder", o.name)));
        }
    }
    if dest.exists() {
        return Ok(Some(format!(
            "its install folder {} exists",
            dest.display()
        )));
    }
    Ok(None)
}

/// A mod's `mod_files` rows without `mod_id`, sorted by path.
fn file_keys(rows: &[RowValues]) -> Vec<RowValues> {
    let path = |r: &RowValues| match r.iter().find(|(c, _)| c == "path") {
        Some((_, Value::Text(p))) => p.clone(),
        _ => String::new(),
    };
    let mut v: Vec<RowValues> = rows
        .iter()
        .map(|r| r.iter().filter(|(c, _)| c != "mod_id").cloned().collect())
        .collect();
    v.sort_by_key(path);
    v
}

fn insert_row(
    tx: &Transaction<'_>,
    table: &str,
    row: &RowValues,
    mod_id: i64,
) -> rusqlite::Result<()> {
    let cols: Vec<String> = row.iter().map(|(c, _)| format!("\"{c}\"")).collect();
    let marks = vec!["?"; cols.len()].join(", ");
    let vals = row.iter().map(|(c, v)| {
        if c == "mod_id" {
            Value::Integer(mod_id)
        } else {
            v.clone()
        }
    });
    tx.execute(
        &format!(
            "INSERT INTO \"{table}\" ({}) VALUES ({marks})",
            cols.join(", ")
        ),
        params_from_iter(vals),
    )?;
    Ok(())
}

/// Writes `snap` over the reinstalled row `id` of `game` in one
/// transaction: every `mods` column (except [`NOT_COPIED`]), the FOMOD
/// choices, the collection memberships and the position, clamped to
/// 1..=count. The rebuilt `mod_files` must match the snapshot (else the
/// archive changed and nothing is written). Returns warnings.
pub fn apply(
    store: &mut Store,
    game: &str,
    id: i64,
    snap: &RestoreSnapshot,
) -> Result<Vec<String>, ModsError> {
    let mut warnings = Vec::new();
    let tx = store.conn_mut().transaction().map_err(StoreError::from)?;
    let now = select_rows(
        &tx,
        "SELECT * FROM mod_files WHERE mod_id = ?1 ORDER BY rowid",
        id,
    )?;
    if file_keys(&now) != file_keys(snap.child_rows("mod_files")) {
        return Err(ModsError::RestoreMismatch(snap.record.name.clone()));
    }
    let copied: Vec<&(String, Value)> = snap
        .row
        .iter()
        .filter(|(c, _)| !NOT_COPIED.contains(&c.as_str()))
        .collect();
    let set: Vec<String> = copied
        .iter()
        .enumerate()
        .map(|(i, (c, _))| format!("\"{c}\" = ?{}", i + 1))
        .collect();
    let vals = copied
        .iter()
        .map(|(_, v)| v.clone())
        .chain(std::iter::once(Value::Integer(id)));
    tx.execute(
        &format!(
            "UPDATE mods SET {} WHERE id = ?{}",
            set.join(", "),
            copied.len() + 1
        ),
        params_from_iter(vals),
    )?;
    tx.execute("DELETE FROM fomod_choices WHERE mod_id = ?1", [id])?;
    for r in snap.child_rows("fomod_choices") {
        insert_row(&tx, "fomod_choices", r, id)?;
    }
    for r in snap.child_rows("collection_mods") {
        let cid = r.iter().find(|(c, _)| c == "collection_id").map(|(_, v)| v);
        let alive: bool = match cid {
            Some(Value::Integer(c)) => tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM collections WHERE id = ?1 AND game = ?2)",
                params![c, game],
                |r| r.get(0),
            )?,
            _ => false,
        };
        if alive {
            insert_row(&tx, "collection_mods", r, id)?;
        } else {
            let slug = match cid {
                Some(Value::Integer(c)) => snap
                    .collections
                    .iter()
                    .find(|(i, _)| i == c)
                    .map(|(_, s)| s.clone()),
                _ => None,
            };
            warnings.push(format!(
                "{}: collection {} no longer exists; membership skipped",
                snap.record.name,
                slug.unwrap_or_else(|| "?".into())
            ));
        }
    }
    let mut order: Vec<i64> = {
        let mut st = tx.prepare("SELECT id FROM mods WHERE game = ?1 ORDER BY priority, id")?;
        let v = st
            .query_map([game], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        v.into_iter().filter(|&m| m != id).collect()
    };
    let at = snap.record.position.clamp(1, order.len() + 1) - 1;
    order.insert(at, id);
    for (p, m) in order.iter().enumerate() {
        tx.execute(
            "UPDATE mods SET priority = ?1 WHERE id = ?2",
            params![p as i64, m],
        )?;
    }
    tx.commit().map_err(StoreError::from)?;
    super::mark_changed(store, game)?;
    Ok(warnings)
}

/// Compensating rollback of a reinstall: removes row `id` (its child rows
/// cascade) and its staging folder through the uninstall path.
pub fn rollback(store: &mut Store, game: &str, id: i64) -> Result<(), ModsError> {
    super::remove_mods(store, game, &[id], None).map(|_| ())
}

/// The folder a reinstall of `uid` stages into.
pub fn dest_for(staging_root: &Path, uid: &str) -> PathBuf {
    staging_root.join(uid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::tests::{add, store_with_game};
    use std::collections::BTreeSet;

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut st = conn
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .unwrap();
        st.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    // Fails when a migration adds a table with a foreign key to mods that
    // the snapshot neither copies nor lists as not needed (data loss on
    // restore).
    #[test]
    fn every_table_referencing_mods_is_snapshotted_or_listed() {
        let s = store_with_game("g");
        let conn = s.conn();
        let mut st = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap();
        let tables: Vec<String> = st
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut refs = BTreeSet::new();
        for t in &tables {
            let mut fk = conn
                .prepare(&format!(
                    "SELECT \"table\" FROM pragma_foreign_key_list('{t}')"
                ))
                .unwrap();
            let targets: Vec<String> = fk
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            if targets.iter().any(|x| x == "mods") {
                refs.insert(t.clone());
            }
        }
        let covered: BTreeSet<String> = CHILD_TABLES
            .iter()
            .chain(NOT_SNAPSHOTTED.iter().map(|(t, _)| t))
            .map(|t| t.to_string())
            .collect();
        assert_eq!(refs, covered);
    }

    // Fails when the snapshot is built from a hand-kept field list that
    // misses a column of mods or of a child table.
    #[test]
    fn snapshot_has_every_column_of_the_tables() {
        let s = store_with_game("g");
        let id = add(&s, "g", "a", "A", Some(1));
        s.conn()
            .execute_batch(&format!(
                "INSERT INTO collections (id, game, slug, revision, installed_at)
                     VALUES (7, 'g', 'c', 1, 0);
                 INSERT INTO collection_mods VALUES (7, {id}, 3);
                 INSERT INTO fomod_choices VALUES ({id}, '{{}}', 'ask', 5);
                 INSERT INTO mod_files VALUES ({id}, 'x.esp', 1, 'h', 420);"
            ))
            .unwrap();
        let m = list_mods(s.conn(), "g").unwrap().remove(0);
        let snap = capture(s.conn(), &m).unwrap();
        let names = |r: &RowValues| r.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
        assert_eq!(names(&snap.row), columns(s.conn(), "mods"));
        for t in CHILD_TABLES {
            let rows = snap.child_rows(t);
            assert_eq!(rows.len(), 1, "{t}");
            assert_eq!(names(&rows[0]), columns(s.conn(), t), "{t}");
        }
        assert_eq!(snap.collections, vec![(7, "c".to_string())]);
    }
}
