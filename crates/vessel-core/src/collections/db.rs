// SPDX-License-Identifier: GPL-3.0-or-later
//! Installed collections: records, membership (legacy `mods[].collections`),
//! update "dropped" split, removal (legacy L2653-2785) and the stored rules
//! for the plugin stage.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use super::{match_installed, CollectionError, Manifest, R};
use crate::mods::{list_mods, remove_mods, ModRecord};
use crate::store::{self, Store, StoreError};

/// One installed collection.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CollectionRecord {
    pub id: i64,
    pub game: String,
    pub slug: String,
    pub name: Option<String>,
    pub author: Option<String>,
    pub revision: u64,
    /// Folder with revision.json, collection.json and the extracted archive.
    pub dir: Option<PathBuf>,
    /// Names of the mods that failed in the last install.
    pub failed: Vec<String>,
    pub installed_at: i64,
    /// Member row ids by collection position.
    pub mods: Vec<i64>,
}

/// [`list`] row.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CollectionSummary {
    pub record: CollectionRecord,
    pub enabled: usize,
}

fn members(conn: &Connection, cid: i64) -> rusqlite::Result<Vec<i64>> {
    let mut st = conn.prepare(
        "SELECT mod_id FROM collection_mods WHERE collection_id = ?1 ORDER BY position, mod_id",
    )?;
    let v = st
        .query_map([cid], |r| r.get(0))?
        .collect::<Result<Vec<i64>, _>>()?;
    Ok(v)
}

const COLS: &str = "id, game, slug, name, author, revision, dir, failed, installed_at";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CollectionRecord> {
    let failed: Option<String> = r.get(7)?;
    Ok(CollectionRecord {
        id: r.get(0)?,
        game: r.get(1)?,
        slug: r.get(2)?,
        name: r.get(3)?,
        author: r.get(4)?,
        revision: u64::try_from(r.get::<_, i64>(5)?).unwrap_or(0),
        dir: r.get::<_, Option<String>>(6)?.map(PathBuf::from),
        failed: failed
            .and_then(|f| serde_json::from_str(&f).ok())
            .unwrap_or_default(),
        installed_at: r.get(8)?,
        mods: Vec::new(),
    })
}

/// The collection `slug` of `game`, if installed.
pub fn get(conn: &Connection, game: &str, slug: &str) -> R<Option<CollectionRecord>> {
    let rec = conn
        .query_row(
            &format!("SELECT {COLS} FROM collections WHERE game = ?1 AND slug = ?2"),
            params![game, slug],
            row,
        )
        .optional()?;
    match rec {
        Some(mut r) => {
            r.mods = members(conn, r.id)?;
            Ok(Some(r))
        }
        None => Ok(None),
    }
}

/// Installed collections of `game` with mod and enabled counts (legacy
/// `collection_list` L2653-2670).
pub fn list(conn: &Connection, game: &str) -> R<Vec<CollectionSummary>> {
    let mut st = conn.prepare(&format!(
        "SELECT {COLS} FROM collections WHERE game = ?1 ORDER BY installed_at, slug"
    ))?;
    let recs = st.query_map([game], row)?.collect::<Result<Vec<_>, _>>()?;
    let mods = list_mods(conn, game)?;
    recs.into_iter()
        .map(|mut r| {
            r.mods = members(conn, r.id)?;
            let enabled = mods
                .iter()
                .filter(|m| m.enabled && r.mods.contains(&m.id))
                .count();
            Ok(CollectionSummary { record: r, enabled })
        })
        .collect()
}

/// What [`record`] stores.
pub struct NewRecord<'a> {
    pub slug: &'a str,
    pub name: &'a str,
    pub author: &'a str,
    pub revision: u64,
    pub dir: &'a Path,
    /// collection.json.
    pub manifest: &'a Value,
    /// `modRules`.
    pub rules: &'a Value,
    /// `externalResources` of the revision.
    pub external: &'a Value,
    pub failed: &'a [String],
}

/// Inserts or replaces the collection row and adds `ordered` as members at
/// positions 0.. (earlier members stay, after them, like the legacy
/// `mods[].collections` union). Returns the collection id.
pub fn record(store: &mut Store, game: &str, rec: &NewRecord<'_>, ordered: &[i64]) -> R<i64> {
    let tx = store.conn_mut().transaction().map_err(StoreError::from)?;
    let failed = serde_json::to_string(rec.failed).unwrap_or_else(|_| "[]".into());
    tx.execute(
        "INSERT INTO collections (game, slug, name, author, revision, dir, manifest, rules,
            external_resources, failed, installed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT (game, slug) DO UPDATE SET name = excluded.name, author = excluded.author,
            revision = excluded.revision, dir = excluded.dir, manifest = excluded.manifest,
            rules = excluded.rules, external_resources = excluded.external_resources,
            failed = excluded.failed, installed_at = excluded.installed_at",
        params![
            game,
            rec.slug,
            rec.name,
            rec.author,
            i64::try_from(rec.revision).unwrap_or(i64::MAX),
            rec.dir.to_string_lossy(),
            rec.manifest.to_string(),
            rec.rules.to_string(),
            rec.external.to_string(),
            failed,
            store::now()
        ],
    )?;
    let cid: i64 = tx.query_row(
        "SELECT id FROM collections WHERE game = ?1 AND slug = ?2",
        params![game, rec.slug],
        |r| r.get(0),
    )?;
    let old = members(&tx, cid)?;
    let mut seen = BTreeSet::new();
    let all: Vec<i64> = ordered
        .iter()
        .chain(old.iter())
        .copied()
        .filter(|id| seen.insert(*id))
        .collect();
    tx.execute(
        "DELETE FROM collection_mods WHERE collection_id = ?1",
        [cid],
    )?;
    for (p, id) in all.iter().enumerate() {
        tx.execute(
            "INSERT INTO collection_mods (collection_id, mod_id, position)
             SELECT ?1, id, ?3 FROM mods WHERE id = ?2 AND game = ?4",
            params![cid, id, p as i64, game],
        )?;
    }
    tx.commit().map_err(StoreError::from)?;
    Ok(cid)
}

/// Removes `ids` from collection `slug` (the mods stay installed).
pub fn detach(store: &Store, game: &str, slug: &str, ids: &[i64]) -> R<()> {
    for id in ids {
        store.conn().execute(
            "DELETE FROM collection_mods WHERE mod_id = ?1 AND collection_id =
                (SELECT id FROM collections WHERE game = ?2 AND slug = ?3)",
            params![id, game, slug],
        )?;
    }
    Ok(())
}

/// Number of collections `mod_id` belongs to besides `slug`.
fn other_collections(conn: &Connection, game: &str, slug: &str, mod_id: i64) -> R<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM collection_mods cm JOIN collections c ON c.id = cm.collection_id
         WHERE cm.mod_id = ?1 AND c.game = ?2 AND c.slug <> ?3",
        params![mod_id, game, slug],
        |r| r.get(0),
    )?)
}

/// Mods an update no longer includes, split by membership.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dropped {
    /// Only in this collection: uninstall candidates.
    pub exclusive: Vec<ModRecord>,
    /// Also in another collection: only lose the membership.
    pub shared: Vec<ModRecord>,
}

/// Members from before an update (`old`) that the new revision dropped:
/// not part of this install (`kept`) and matching no entry of the new
/// manifest (by md5 or modId-fileId), so a deselected optional mod is not
/// treated as dropped (legacy L2730-2741 compared record ids only).
pub fn dropped_members(
    conn: &Connection,
    game: &str,
    slug: &str,
    old: &[i64],
    kept: &[i64],
    manifest: &Manifest,
) -> R<Dropped> {
    let mods = list_mods(conn, game)?;
    let mut out = Dropped::default();
    for m in mods
        .iter()
        .filter(|m| old.contains(&m.id) && !kept.contains(&m.id))
    {
        let one = std::slice::from_ref(m);
        if manifest
            .mods
            .iter()
            .any(|mm| match_installed(mm, one).is_some())
        {
            continue;
        }
        if other_collections(conn, game, slug, m.id)? == 0 {
            out.exclusive.push(m.clone());
        } else {
            out.shared.push(m.clone());
        }
    }
    Ok(out)
}

/// What [`remove`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Removed {
    /// uids of the uninstalled mods.
    pub uninstalled: Vec<String>,
    /// Members kept because another collection has them (or `keep_mods`).
    pub kept: usize,
}

/// Members of `slug` split into (only in this collection, also elsewhere).
pub fn exclusive_members(
    conn: &Connection,
    game: &str,
    slug: &str,
) -> R<(Vec<ModRecord>, Vec<ModRecord>)> {
    let rec = get(conn, game, slug)?.ok_or_else(|| CollectionError::NotInstalled(slug.into()))?;
    let mods = list_mods(conn, game)?;
    let (mut ex, mut sh) = (Vec::new(), Vec::new());
    for m in mods.into_iter().filter(|m| rec.mods.contains(&m.id)) {
        if other_collections(conn, game, slug, m.id)? == 0 {
            ex.push(m);
        } else {
            sh.push(m);
        }
    }
    Ok((ex, sh))
}

/// Deletes the collection record. Mods only in this collection are
/// uninstalled (archives kept) unless `keep_mods`; shared mods stay
/// (legacy `collection_remove` L2760-2785).
pub fn remove(store: &mut Store, game: &str, slug: &str, keep_mods: bool) -> R<Removed> {
    let (ex, sh) = exclusive_members(store.conn(), game, slug)?;
    let mut out = Removed {
        kept: sh.len(),
        ..Default::default()
    };
    if keep_mods {
        out.kept += ex.len();
    } else if !ex.is_empty() {
        let ids: Vec<i64> = ex.iter().map(|m| m.id).collect();
        remove_mods(store, game, &ids, None)?;
        out.uninstalled = ex.into_iter().map(|m| m.uid).collect();
    }
    store.conn().execute(
        "DELETE FROM collections WHERE game = ?1 AND slug = ?2",
        params![game, slug],
    )?;
    crate::mods::mark_changed(store, game)?;
    Ok(out)
}

/// A plugin load-order rule from a collection (Vortex `pluginRules`):
/// `plugin` loads after every name in `after`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRule {
    pub collection: String,
    pub plugin: String,
    pub after: Vec<String>,
}

/// Plugin rules of every installed collection of `game`, from the stored
/// collection.json (`pluginRules.plugins[] {name, after[]}`). Stored only;
/// the plugin stage (M6) merges them into plugins.txt.
pub fn plugin_rules(conn: &Connection, game: &str) -> R<Vec<PluginRule>> {
    let mut st = conn.prepare(
        "SELECT slug, manifest FROM collections WHERE game = ?1 AND manifest IS NOT NULL
         ORDER BY installed_at, slug",
    )?;
    let rows = st
        .query_map([game], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for (slug, text) in rows {
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(items) = v.pointer("/pluginRules/plugins").and_then(Value::as_array) else {
            continue;
        };
        for it in items {
            let Some(plugin) = it.get("name").and_then(Value::as_str) else {
                continue;
            };
            let after = it
                .get("after")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            out.push(PluginRule {
                collection: slug.clone(),
                plugin: plugin.to_string(),
                after,
            });
        }
    }
    Ok(out)
}

/// Stored `modRules` of collection `slug` (JSON array).
pub fn mod_rules(conn: &Connection, game: &str, slug: &str) -> R<Value> {
    let v: Option<Option<String>> = conn
        .query_row(
            "SELECT rules FROM collections WHERE game = ?1 AND slug = ?2",
            params![game, slug],
            |r| r.get(0),
        )
        .optional()?;
    Ok(v.flatten()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Array(Vec::new())))
}
