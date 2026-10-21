// SPDX-License-Identifier: GPL-3.0-or-later
//! Saved profiles: load order + enabled state (legacy `cmd_profile`,
//! L3852-3894). Entries are keyed by mod uid so they survive reinstalls.

use rusqlite::{params, OptionalExtension};

use super::{list_mods, mark_changed, write_order, ModsError};
use crate::store::{self, Store, StoreError};

/// Legacy name rule `^[A-Za-z0-9._-]+$`.
pub fn valid_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSummary {
    pub name: String,
    pub mods: usize,
    pub enabled: usize,
    pub saved_at: i64,
}

fn profile_id(store: &Store, game: &str, name: &str) -> Result<Option<i64>, ModsError> {
    Ok(store
        .conn()
        .query_row(
            "SELECT id FROM profiles WHERE game = ?1 AND name = ?2",
            params![game, name],
            |r| r.get(0),
        )
        .optional()?)
}

/// Profiles of `game`, by name.
pub fn list_profiles(store: &Store, game: &str) -> Result<Vec<ProfileSummary>, ModsError> {
    let mut stmt = store.conn().prepare(
        "SELECT p.name, p.saved_at, COUNT(pm.position), COALESCE(SUM(pm.enabled), 0)
         FROM profiles p LEFT JOIN profile_mods pm ON pm.profile_id = p.id
         WHERE p.game = ?1 GROUP BY p.id ORDER BY p.name",
    )?;
    let v = stmt
        .query_map([game], |r| {
            Ok(ProfileSummary {
                name: r.get(0)?,
                saved_at: r.get(1)?,
                mods: r.get::<_, i64>(2)? as usize,
                enabled: r.get::<_, i64>(3)? as usize,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

/// Saves (or overwrites) a snapshot of the current order and enabled state.
/// Returns the number of mods saved.
pub fn save_profile(store: &mut Store, game: &str, name: &str) -> Result<usize, ModsError> {
    if !valid_profile_name(name) {
        return Err(ModsError::InvalidProfileName(name.to_string()));
    }
    let mods = list_mods(store.conn(), game)?;
    let tx = store.conn_mut().transaction().map_err(StoreError::from)?;
    tx.execute(
        "DELETE FROM profiles WHERE game = ?1 AND name = ?2",
        params![game, name],
    )?;
    tx.execute(
        "INSERT INTO profiles (game, name, saved_at) VALUES (?1, ?2, ?3)",
        params![game, name, store::now()],
    )?;
    let pid = tx.last_insert_rowid();
    for (i, m) in mods.iter().enumerate() {
        tx.execute(
            "INSERT INTO profile_mods (profile_id, position, mod_uid, enabled) VALUES (?1, ?2, ?3, ?4)",
            params![pid, i as i64, m.uid, m.enabled],
        )?;
    }
    tx.commit().map_err(StoreError::from)?;
    Ok(mods.len())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileLoad {
    /// Profile entries that are no longer installed.
    pub missing: usize,
    /// Installed mods not in the profile (appended, disabled).
    pub added_disabled: usize,
}

/// Applies a profile: its mods in its order with its enabled state, then
/// installed mods it does not list, appended disabled.
pub fn load_profile(store: &mut Store, game: &str, name: &str) -> Result<ProfileLoad, ModsError> {
    let pid = profile_id(store, game, name)?
        .ok_or_else(|| ModsError::ProfileNotFound(name.to_string()))?;
    let entries: Vec<(String, bool)> = {
        let mut stmt = store.conn().prepare(
            "SELECT mod_uid, enabled FROM profile_mods WHERE profile_id = ?1 ORDER BY position",
        )?;
        let v = stmt
            .query_map([pid], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        v
    };
    let mods = list_mods(store.conn(), game)?;
    let mut order = Vec::with_capacity(mods.len());
    let mut enabled = Vec::with_capacity(mods.len());
    let mut missing = 0;
    for (uid, en) in &entries {
        match mods.iter().find(|m| &m.uid == uid) {
            Some(m) if !order.contains(&m.id) => {
                order.push(m.id);
                enabled.push((m.id, *en));
            }
            Some(_) => {}
            None => missing += 1,
        }
    }
    let mut added_disabled = 0;
    for m in &mods {
        if !order.contains(&m.id) {
            order.push(m.id);
            enabled.push((m.id, false));
            added_disabled += 1;
        }
    }
    write_order(store, game, &order)?;
    let tx = store.conn_mut().transaction().map_err(StoreError::from)?;
    for (id, en) in enabled {
        tx.execute(
            "UPDATE mods SET enabled = ?1 WHERE id = ?2",
            params![en, id],
        )?;
    }
    tx.commit().map_err(StoreError::from)?;
    mark_changed(store, game)?;
    Ok(ProfileLoad {
        missing,
        added_disabled,
    })
}

pub fn delete_profile(store: &Store, game: &str, name: &str) -> Result<(), ModsError> {
    let n = store.conn().execute(
        "DELETE FROM profiles WHERE game = ?1 AND name = ?2",
        params![game, name],
    )?;
    if n == 0 {
        return Err(ModsError::ProfileNotFound(name.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{add, store_with_game};
    use super::*;
    use crate::mods::{remove_mods, set_enabled, OrderSpec};

    fn state(s: &Store) -> Vec<(String, bool)> {
        list_mods(s.conn(), "g")
            .unwrap()
            .into_iter()
            .map(|m| (m.uid, m.enabled))
            .collect()
    }

    #[test]
    fn save_load_roundtrip_restores_order_and_enabled_state() {
        let mut s = store_with_game("g");
        let a = add(&s, "g", "a", "a", None);
        add(&s, "g", "b", "b", None);
        add(&s, "g", "c", "c", None);
        set_enabled(&mut s, "g", &[a], false).unwrap();
        assert_eq!(save_profile(&mut s, "g", "p1").unwrap(), 3);
        let saved = state(&s);

        crate::mods::move_mod(&mut s, "g", "a", &OrderSpec::Last).unwrap();
        let all: Vec<i64> = list_mods(s.conn(), "g")
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect();
        set_enabled(&mut s, "g", &all, true).unwrap();
        add(&s, "g", "d", "d", None);
        let c = list_mods(s.conn(), "g")
            .unwrap()
            .into_iter()
            .find(|m| m.uid == "c")
            .unwrap();
        remove_mods(&mut s, "g", &[c.id], None).unwrap();

        let r = load_profile(&mut s, "g", "p1").unwrap();
        assert_eq!(
            r,
            ProfileLoad {
                missing: 1,
                added_disabled: 1
            }
        );
        let mut want: Vec<(String, bool)> = saved.into_iter().filter(|(u, _)| u != "c").collect();
        want.push(("d".into(), false));
        assert_eq!(state(&s), want);

        let l = list_profiles(&s, "g").unwrap();
        assert_eq!((l[0].name.as_str(), l[0].mods, l[0].enabled), ("p1", 3, 2));
        delete_profile(&s, "g", "p1").unwrap();
        assert!(matches!(
            load_profile(&mut s, "g", "p1"),
            Err(ModsError::ProfileNotFound(_))
        ));
    }

    #[test]
    fn profile_names_are_validated() {
        let mut s = store_with_game("g");
        for bad in ["", "a b", "../x", "x/y"] {
            assert!(matches!(
                save_profile(&mut s, "g", bad),
                Err(ModsError::InvalidProfileName(_))
            ));
        }
        assert!(valid_profile_name("My_Profile-1.0"));
    }
}
