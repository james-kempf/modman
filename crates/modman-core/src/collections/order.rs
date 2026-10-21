// SPDX-License-Identifier: GPL-3.0-or-later
//! Collection load order: Kahn topological sort over `modRules`
//! (legacy `collection_order_py` L2249-2315) and the contiguous-block move
//! (legacy `JQ_REORDER_BLOCK` L2318-2324).

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashMap};

use rusqlite::params;
use serde_json::Value;

use super::{Manifest, R};
use crate::mods::list_mods;
use crate::store::{Store, StoreError};

/// Result of [`rule_order`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ordered {
    /// Manifest indexes, lowest priority first.
    pub order: Vec<usize>,
    /// How many of them were part of a rule cycle (appended by rank).
    pub in_cycle: usize,
}

/// Match keys of a mod: (kind, value); md5, logical name and file
/// expression are lower-cased, tags are not.
fn keys(m: &Value) -> Vec<(&'static str, String)> {
    let s = m.get("source");
    let get = |k: &str| {
        s.and_then(|s| s.get(k))
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
    };
    let mut out = Vec::new();
    if let Some(t) = get("tag") {
        out.push(("tag", t.to_string()));
    }
    for (kind, field) in [
        ("md5", "md5"),
        ("lfn", "logicalFilename"),
        ("exp", "fileExpression"),
    ] {
        if let Some(v) = get(field) {
            out.push((kind, v.to_lowercase()));
        }
    }
    out
}

/// Orders the `present` manifest indexes (given in collection order) by
/// the `before`/`after` rules. Ties go by `(phase, position in present)`;
/// mods left in a cycle are appended in that rank order. Other rule types
/// are ignored.
pub fn rule_order(m: &Manifest, present: &[usize]) -> Ordered {
    let mut index: HashMap<(&'static str, String), usize> = HashMap::new();
    for &i in present {
        if let Some(mm) = m.mods.get(i) {
            for k in keys(&mm.raw) {
                index.entry(k).or_insert(i);
            }
        }
    }
    let lookup = |r: Option<&Value>| -> Option<usize> {
        let r = r?.as_object()?;
        [
            ("tag", "tag"),
            ("md5", "fileMD5"),
            ("lfn", "logicalFileName"),
            ("exp", "fileExpression"),
        ]
        .into_iter()
        .find_map(|(kind, field)| {
            let v = r.get(field)?;
            let v = match v {
                Value::String(s) if s.is_empty() => return None,
                Value::String(s) => s.clone(),
                Value::Null | Value::Bool(false) => return None,
                other => other.to_string(),
            };
            let v = if kind == "tag" { v } else { v.to_lowercase() };
            index.get(&(kind, v)).copied()
        })
    };
    let mut edges: HashMap<usize, BTreeSet<usize>> =
        present.iter().map(|&i| (i, BTreeSet::new())).collect();
    let mut indeg: HashMap<usize, usize> = present.iter().map(|&i| (i, 0)).collect();
    let mut before = |a: Option<usize>, b: Option<usize>| {
        let (Some(a), Some(b)) = (a, b) else { return };
        if a == b || !edges.get_mut(&a).expect("present").insert(b) {
            return;
        }
        *indeg.get_mut(&b).expect("present") += 1;
    };
    for r in m.mod_rules() {
        let s = lookup(r.get("source"));
        let t = lookup(r.get("reference"));
        match r.get("type").and_then(Value::as_str) {
            Some("before") => before(s, t),
            Some("after") => before(t, s),
            _ => {}
        }
    }
    let rank: HashMap<usize, (i64, usize)> = present
        .iter()
        .enumerate()
        .map(|(n, &i)| {
            let phase = m
                .mods
                .get(i)
                .and_then(|x| x.raw.get("phase"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            (i, (phase, n))
        })
        .collect();
    let mut heap: BinaryHeap<Reverse<((i64, usize), usize)>> = present
        .iter()
        .filter(|i| indeg[i] == 0)
        .map(|&i| Reverse((rank[&i], i)))
        .collect();
    let mut out = Vec::with_capacity(present.len());
    while let Some(Reverse((_, i))) = heap.pop() {
        out.push(i);
        for &j in &edges[&i] {
            let d = indeg.get_mut(&j).expect("present");
            *d -= 1;
            if *d == 0 {
                heap.push(Reverse((rank[&j], j)));
            }
        }
    }
    let mut in_cycle = 0;
    if out.len() < present.len() {
        let done: BTreeSet<usize> = out.iter().copied().collect();
        let mut rest: Vec<usize> = present
            .iter()
            .copied()
            .filter(|i| !done.contains(i))
            .collect();
        rest.sort_by_key(|i| rank[i]);
        in_cycle = rest.len();
        out.extend(rest);
    }
    Ordered {
        order: out,
        in_cycle,
    }
}

/// Moves `ids` (in this order) into one contiguous block that starts at the
/// lowest current position of any of them; unknown ids are skipped.
pub fn reorder_block(store: &mut Store, game: &str, ids: &[i64]) -> R<()> {
    let cur: Vec<i64> = list_mods(store.conn(), game)?
        .iter()
        .map(|m| m.id)
        .collect();
    let set: BTreeSet<i64> = ids.iter().copied().collect();
    let pos = cur
        .iter()
        .position(|id| set.contains(id))
        .unwrap_or(cur.len());
    let rest: Vec<i64> = cur.iter().copied().filter(|id| !set.contains(id)).collect();
    let mut seen = BTreeSet::new();
    let block = ids
        .iter()
        .copied()
        .filter(|id| cur.contains(id) && seen.insert(*id));
    let order: Vec<i64> = rest[..pos.min(rest.len())]
        .iter()
        .copied()
        .chain(block)
        .chain(rest[pos.min(rest.len())..].iter().copied())
        .collect();
    let tx = store.conn_mut().transaction().map_err(StoreError::from)?;
    for (p, id) in order.iter().enumerate() {
        tx.execute(
            "UPDATE mods SET priority = ?1 WHERE game = ?2 AND id = ?3",
            params![p as i64, game, id],
        )?;
    }
    tx.commit().map_err(StoreError::from)?;
    Ok(())
}
