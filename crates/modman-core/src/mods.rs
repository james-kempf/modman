// SPDX-License-Identifier: GPL-3.0-or-later
//! M3: installed mods of a game (legacy `mods.json`, L520-592, L3397-3704).
//!
//! The `mods` table holds one row per installed mod; `priority` is the load
//! order (0 = lowest, last wins). Every function keeps priorities dense
//! (0..n-1) after it changes the order.

mod install;
mod profiles;

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::archive::ArchiveError;
use crate::staging::StagingError;
use crate::store::{self, Store, StoreError};

pub use install::{find_fomod, install_archive, InstallMeta, InstallOutcome};
pub use profiles::{
    delete_profile, list_profiles, load_profile, save_profile, valid_profile_name, ProfileLoad,
    ProfileSummary,
};

/// Errors from mod management. Messages are user-facing.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ModsError {
    #[error("no installed mod matches '{0}'; see: modman list")]
    NotFound(String),
    #[error("'{reference}' matches {} mods; use #position or the full id:{}", matches.len(),
        matches.iter().map(|(p, id)| format!("\n    #{p}  {id}")).collect::<String>())]
    Ambiguous {
        reference: String,
        /// (1-based position, uid) of every match.
        matches: Vec<(usize, String)>,
    },
    #[error(
        "invalid order '{0}': use #N, first, last, up [n], down [n], before <mod> or after <mod>"
    )]
    InvalidOrder(String),
    #[error("a mod cannot be ordered relative to itself")]
    SelfRelative,
    #[error("no mods belong to collection '{0}'")]
    NoCollectionMods(String),
    #[error("profile names may contain only letters, digits, '.', '_' and '-': '{0}'")]
    InvalidProfileName(String),
    #[error("no profile named '{0}'")]
    ProfileNotFound(String),
    #[error("no installable files in {0}")]
    NoInstallableFiles(String),
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    #[error(transparent)]
    Staging(#[from] StagingError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Nexus(#[from] crate::nexus::NexusError),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl From<rusqlite::Error> for ModsError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(StoreError::Sqlite(e))
    }
}

pub(crate) fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> ModsError + '_ {
    move |source| ModsError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// One installed mod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModRecord {
    /// Row id (`mods.id`).
    pub id: i64,
    pub game: String,
    /// User-facing id, e.g. `SkyUI-12604-35407`.
    pub uid: String,
    pub name: String,
    pub version: Option<String>,
    pub nexus_mod_id: Option<u64>,
    pub nexus_file_id: Option<u64>,
    pub md5: Option<String>,
    pub source: String,
    pub archive: Option<PathBuf>,
    pub author: Option<String>,
    pub category: Option<String>,
    pub update_policy: Option<String>,
    /// `"game"` = paths relative to the game root, `""` = deploy folder.
    pub root: String,
    pub variant: Option<String>,
    pub staging_dir: Option<PathBuf>,
    pub enabled: bool,
    pub priority: i64,
    pub file_count: i64,
    pub size: i64,
    pub installed_at: i64,
    /// 1-based position in the load order.
    pub position: usize,
}

const MOD_COLUMNS: &str =
    "id, game, uid, name, version, nexus_mod_id, nexus_file_id, md5, source, \
     archive, author, category, update_policy, root, variant, staging_dir, enabled, priority, \
     file_count, size, installed_at";

fn opt_path(v: Option<String>) -> Option<PathBuf> {
    v.filter(|s| !s.is_empty()).map(PathBuf::from)
}

fn opt_u64(v: Option<i64>) -> Option<u64> {
    v.and_then(|n| u64::try_from(n).ok())
}

fn row_to_mod(r: &Row<'_>) -> rusqlite::Result<ModRecord> {
    Ok(ModRecord {
        id: r.get(0)?,
        game: r.get(1)?,
        uid: r.get(2)?,
        name: r.get(3)?,
        version: r.get::<_, Option<String>>(4)?.filter(|s| !s.is_empty()),
        nexus_mod_id: opt_u64(r.get(5)?),
        nexus_file_id: opt_u64(r.get(6)?),
        md5: r.get::<_, Option<String>>(7)?.filter(|s| !s.is_empty()),
        source: r.get(8)?,
        archive: opt_path(r.get(9)?),
        author: r.get::<_, Option<String>>(10)?.filter(|s| !s.is_empty()),
        category: r.get::<_, Option<String>>(11)?.filter(|s| !s.is_empty()),
        update_policy: r.get::<_, Option<String>>(12)?.filter(|s| !s.is_empty()),
        root: r.get(13)?,
        variant: r.get::<_, Option<String>>(14)?.filter(|s| !s.is_empty()),
        staging_dir: opt_path(r.get(15)?),
        enabled: r.get(16)?,
        priority: r.get(17)?,
        file_count: r.get(18)?,
        size: r.get(19)?,
        installed_at: r.get(20)?,
        position: 0,
    })
}

/// All mods of `game` in load order (lowest priority first).
pub fn list_mods(conn: &Connection, game: &str) -> Result<Vec<ModRecord>, ModsError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MOD_COLUMNS} FROM mods WHERE game = ?1 ORDER BY priority, id"
    ))?;
    let mut v = stmt
        .query_map([game], row_to_mod)?
        .collect::<Result<Vec<_>, _>>()?;
    for (i, m) in v.iter_mut().enumerate() {
        m.position = i + 1;
    }
    Ok(v)
}

/// One mod by row id.
pub fn get_mod(conn: &Connection, game: &str, id: i64) -> Result<Option<ModRecord>, ModsError> {
    Ok(list_mods(conn, game)?.into_iter().find(|m| m.id == id))
}

/// Collection slugs each mod belongs to (row id -> slugs).
pub fn collections_of(conn: &Connection, mod_id: i64) -> Result<Vec<String>, ModsError> {
    let mut stmt = conn.prepare(
        "SELECT c.slug FROM collection_mods cm JOIN collections c ON c.id = cm.collection_id
         WHERE cm.mod_id = ?1 ORDER BY c.slug",
    )?;
    let v = stmt
        .query_map([mod_id], |r| r.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(v)
}

/// Row ids of the mods in collection `slug` of `game`.
pub fn collection_mod_ids(
    conn: &Connection,
    game: &str,
    slug: &str,
) -> Result<Vec<i64>, ModsError> {
    let mut stmt = conn.prepare(
        "SELECT cm.mod_id FROM collection_mods cm JOIN collections c ON c.id = cm.collection_id
         JOIN mods m ON m.id = cm.mod_id
         WHERE c.game = ?1 AND c.slug = ?2 ORDER BY m.priority, m.id",
    )?;
    let v = stmt
        .query_map(params![game, slug], |r| r.get(0))?
        .collect::<Result<Vec<i64>, _>>()?;
    Ok(v)
}

/// Resolves a mod reference (legacy `JQ_RESOLVE`, L544-572). Tiers, first
/// non-empty wins: `#N` position, exact id, exact name (case-insensitive),
/// Nexus mod id (digits), substring of name or id (case-insensitive). More
/// than one match in the winning tier is an error listing all of them.
pub fn resolve_in(mods: &[ModRecord], reference: &str) -> Result<ModRecord, ModsError> {
    let q = reference.to_lowercase();
    let by_pos: Vec<&ModRecord> = reference
        .strip_prefix('#')
        .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|n| n.parse::<usize>().ok())
        .filter(|&n| n >= 1)
        .and_then(|n| mods.get(n - 1))
        .into_iter()
        .collect();
    let tiers: [Vec<&ModRecord>; 5] = [
        by_pos,
        mods.iter().filter(|m| m.uid == reference).collect(),
        mods.iter().filter(|m| m.name.to_lowercase() == q).collect(),
        if !reference.is_empty() && reference.bytes().all(|b| b.is_ascii_digit()) {
            mods.iter()
                .filter(|m| m.nexus_mod_id.is_some_and(|n| n.to_string() == reference))
                .collect()
        } else {
            Vec::new()
        },
        mods.iter()
            .filter(|m| m.name.to_lowercase().contains(&q) || m.uid.to_lowercase().contains(&q))
            .collect(),
    ];
    let hit = tiers
        .into_iter()
        .find(|t| !t.is_empty())
        .ok_or_else(|| ModsError::NotFound(reference.to_string()))?;
    match hit.as_slice() {
        [one] => Ok((*one).clone()),
        many => Err(ModsError::Ambiguous {
            reference: reference.to_string(),
            matches: many.iter().map(|m| (m.position, m.uid.clone())).collect(),
        }),
    }
}

/// [`resolve_in`] over the mods of `game`.
pub fn resolve(conn: &Connection, game: &str, reference: &str) -> Result<ModRecord, ModsError> {
    resolve_in(&list_mods(conn, game)?, reference)
}

fn changed_key(game: &str) -> String {
    format!("mods_changed_at.{game}")
}

/// Records that the mod set or order of `game` changed (legacy: mods.json mtime).
pub fn mark_changed(store: &Store, game: &str) -> Result<(), ModsError> {
    store.set_setting(&changed_key(game), &store::now().to_string())?;
    Ok(())
}

/// Mods changed after the last deploy (legacy `mods.json -nt deployed.stamp`).
/// `last_deployed_at` is `None` when nothing is deployed.
pub fn changed_since(
    store: &Store,
    game: &str,
    last_deployed_at: Option<i64>,
) -> Result<bool, ModsError> {
    let changed = store
        .setting(&changed_key(game))?
        .and_then(|v| v.parse::<i64>().ok());
    Ok(match (changed, last_deployed_at) {
        (Some(c), Some(d)) => c > d,
        (Some(_), None) => true,
        (None, _) => false,
    })
}

/// Sets `enabled` on the given rows.
pub fn set_enabled(
    store: &mut Store,
    game: &str,
    ids: &[i64],
    enabled: bool,
) -> Result<(), ModsError> {
    let tx = store.conn_mut().transaction().map_err(StoreError::from)?;
    for id in ids {
        tx.execute(
            "UPDATE mods SET enabled = ?1 WHERE game = ?2 AND id = ?3",
            params![enabled, game, id],
        )?;
    }
    tx.commit().map_err(StoreError::from)?;
    mark_changed(store, game)
}

/// Writes priorities 0..n-1 in the given row order.
fn write_order(store: &mut Store, game: &str, order: &[i64]) -> Result<(), ModsError> {
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

/// Where to move a mod (legacy `cmd_order`, L3670-3704).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderSpec {
    /// `first|bottom|lowest`: position #1, lowest priority.
    First,
    /// `last|top|highest`.
    Last,
    /// Raise priority by n.
    Up(usize),
    /// Lower priority by n.
    Down(usize),
    Before(String),
    After(String),
    /// `#N` or `N` (1-based).
    Position(usize),
}

impl OrderSpec {
    /// Parses `spec [arg]`.
    pub fn parse(spec: &str, arg: Option<&str>) -> Result<Self, ModsError> {
        let bad = || ModsError::InvalidOrder(spec.to_string());
        let count = |a: Option<&str>| -> Result<usize, ModsError> {
            match a {
                None => Ok(1),
                Some(s) => s
                    .parse()
                    .map_err(|_| ModsError::InvalidOrder(format!("{spec} {s}"))),
            }
        };
        Ok(match spec {
            "first" | "bottom" | "lowest" => Self::First,
            "last" | "top" | "highest" => Self::Last,
            "up" => Self::Up(count(arg)?),
            "down" => Self::Down(count(arg)?),
            "before" | "after" => {
                let other = arg
                    .filter(|a| !a.is_empty())
                    .ok_or_else(|| ModsError::InvalidOrder(format!("{spec} needs another mod")))?
                    .to_string();
                if spec == "before" {
                    Self::Before(other)
                } else {
                    Self::After(other)
                }
            }
            s => {
                let n = s.strip_prefix('#').unwrap_or(s);
                if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(bad());
                }
                Self::Position(n.parse().map_err(|_| bad())?)
            }
        })
    }
}

/// Result of [`move_mod`]: 1-based positions and the mod count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moved {
    pub from: usize,
    pub to: usize,
    pub of: usize,
}

/// Moves `reference` per `spec`; the target position is clamped.
pub fn move_mod(
    store: &mut Store,
    game: &str,
    reference: &str,
    spec: &OrderSpec,
) -> Result<(ModRecord, Moved), ModsError> {
    let mods = list_mods(store.conn(), game)?;
    let me = resolve_in(&mods, reference)?;
    let n = mods.len() as i64;
    let cur = (me.position - 1) as i64;
    let rest: Vec<&ModRecord> = mods.iter().filter(|m| m.id != me.id).collect();
    let index_in_rest = |r: &str| -> Result<i64, ModsError> {
        let other = resolve_in(&mods, r)?;
        if other.id == me.id {
            return Err(ModsError::SelfRelative);
        }
        Ok(rest.iter().position(|m| m.id == other.id).unwrap_or(0) as i64)
    };
    let new = match spec {
        OrderSpec::First => 0,
        OrderSpec::Last => n - 1,
        OrderSpec::Up(k) => cur + *k as i64,
        OrderSpec::Down(k) => cur - *k as i64,
        OrderSpec::Before(r) => index_in_rest(r)?,
        OrderSpec::After(r) => index_in_rest(r)? + 1,
        OrderSpec::Position(p) => *p as i64 - 1,
    }
    .clamp(0, (n - 1).max(0)) as usize;
    let mut order: Vec<i64> = rest.iter().map(|m| m.id).collect();
    order.insert(new.min(order.len()), me.id);
    write_order(store, game, &order)?;
    mark_changed(store, game)?;
    Ok((
        me,
        Moved {
            from: cur as usize + 1,
            to: new + 1,
            of: mods.len(),
        },
    ))
}

/// What [`remove_mods`] deleted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Removed {
    pub mods: usize,
    pub archives: Vec<PathBuf>,
}

/// Uninstalls mods (legacy `uninstall_ids`, L3593-3609): deletes the staging
/// folders and rows (collection membership goes with them). With
/// `delete_archive`, an archive inside `downloads_dir` that no other mod uses
/// is deleted too.
pub fn remove_mods(
    store: &mut Store,
    game: &str,
    ids: &[i64],
    delete_archive: Option<&Path>,
) -> Result<Removed, ModsError> {
    let all = list_mods(store.conn(), game)?;
    let doomed: Vec<&ModRecord> = all.iter().filter(|m| ids.contains(&m.id)).collect();
    let mut out = Removed::default();
    for m in &doomed {
        if let Some(dir) = &m.staging_dir {
            match std::fs::remove_dir_all(dir) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io_err(dir)(e)),
                _ => {}
            }
        }
        let (Some(dl), Some(arch)) = (delete_archive, &m.archive) else {
            continue;
        };
        let shared = all
            .iter()
            .any(|o| !ids.contains(&o.id) && o.archive.as_ref() == Some(arch));
        if arch.starts_with(dl) && arch.is_file() && !shared && !out.archives.contains(arch) {
            std::fs::remove_file(arch).map_err(io_err(arch))?;
            out.archives.push(arch.clone());
        }
    }
    let tx = store.conn_mut().transaction().map_err(StoreError::from)?;
    for m in &doomed {
        tx.execute("DELETE FROM mods WHERE id = ?1", [m.id])?;
    }
    tx.commit().map_err(StoreError::from)?;
    out.mods = doomed.len();
    let order: Vec<i64> = all
        .iter()
        .filter(|m| !ids.contains(&m.id))
        .map(|m| m.id)
        .collect();
    write_order(store, game, &order)?;
    mark_changed(store, game)?;
    Ok(out)
}

/// `<data-dir>/<domain>/downloads`.
pub fn downloads_dir(data_dir: &Path, game: &str) -> PathBuf {
    data_dir.join(game).join("downloads")
}

/// FOMOD choices recorded for a mod (Vortex format JSON), if any.
pub fn fomod_choices(conn: &Connection, mod_id: i64) -> Result<Option<String>, ModsError> {
    Ok(conn
        .query_row(
            "SELECT choices FROM fomod_choices WHERE mod_id = ?1",
            [mod_id],
            |r| r.get(0),
        )
        .optional()?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::games::{self, GameConfig};

    pub(crate) fn store_with_game(game: &str) -> Store {
        let s = Store::open_in_memory().unwrap();
        games::save_game(s.conn(), &GameConfig::custom(game)).unwrap();
        s
    }

    pub(crate) fn add(s: &Store, game: &str, uid: &str, name: &str, nexus: Option<i64>) -> i64 {
        let prio: i64 = s
            .conn()
            .query_row(
                "SELECT COALESCE(MAX(priority) + 1, 0) FROM mods WHERE game = ?1",
                [game],
                |r| r.get(0),
            )
            .unwrap();
        s.conn()
            .execute(
                "INSERT INTO mods (game, uid, name, nexus_mod_id, priority, installed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0)",
                params![game, uid, name, nexus, prio],
            )
            .unwrap();
        s.conn().last_insert_rowid()
    }

    fn uids(s: &Store) -> Vec<String> {
        list_mods(s.conn(), "g")
            .unwrap()
            .into_iter()
            .map(|m| m.uid)
            .collect()
    }

    fn sample() -> Store {
        let s = store_with_game("g");
        add(&s, "g", "SkyUI-12604-1", "SkyUI", Some(12604));
        add(&s, "g", "SkyUI-Patch-99-2", "SkyUI Patch", Some(99));
        add(&s, "g", "Ordinary", "Ordinary Mod", None);
        add(&s, "g", "12604", "Numeric Name", None);
        s
    }

    #[test]
    fn resolves_position_tier_first() {
        let s = sample();
        assert_eq!(resolve(s.conn(), "g", "#3").unwrap().uid, "Ordinary");
        assert!(matches!(
            resolve(s.conn(), "g", "#9"),
            Err(ModsError::NotFound(_))
        ));
        assert!(matches!(
            resolve(s.conn(), "g", "#0"),
            Err(ModsError::NotFound(_))
        ));
    }

    #[test]
    fn exact_id_beats_nexus_id_and_substring() {
        // "12604" is both a uid and a Nexus id; the id tier wins.
        let s = sample();
        assert_eq!(
            resolve(s.conn(), "g", "12604").unwrap().name,
            "Numeric Name"
        );
        assert_eq!(
            resolve(s.conn(), "g", "SkyUI-12604-1").unwrap().name,
            "SkyUI"
        );
    }

    #[test]
    fn exact_name_is_case_insensitive_and_beats_substring() {
        // "skyui" is a substring of two mods but the exact name of one.
        let s = sample();
        assert_eq!(
            resolve(s.conn(), "g", "skyui").unwrap().uid,
            "SkyUI-12604-1"
        );
    }

    #[test]
    fn nexus_mod_id_tier() {
        let s = sample();
        assert_eq!(
            resolve(s.conn(), "g", "99").unwrap().uid,
            "SkyUI-Patch-99-2"
        );
    }

    #[test]
    fn unique_substring_resolves() {
        let s = sample();
        assert_eq!(resolve(s.conn(), "g", "ORDIN").unwrap().uid, "Ordinary");
    }

    #[test]
    fn ambiguous_substring_lists_every_match() {
        let s = sample();
        let e = resolve(s.conn(), "g", "sky").unwrap_err();
        let ModsError::Ambiguous { matches, .. } = &e else {
            panic!("{e:?}")
        };
        assert_eq!(
            matches,
            &[
                (1, "SkyUI-12604-1".to_string()),
                (2, "SkyUI-Patch-99-2".to_string())
            ]
        );
        let msg = e.to_string();
        assert!(
            msg.contains("#1  SkyUI-12604-1") && msg.contains("#2  SkyUI-Patch-99-2"),
            "{msg}"
        );
        assert!(matches!(
            resolve(s.conn(), "g", "nothing"),
            Err(ModsError::NotFound(_))
        ));
    }

    #[test]
    fn order_specs_parse() {
        let p = |s: &str, a: Option<&str>| OrderSpec::parse(s, a).unwrap();
        assert_eq!(p("bottom", None), OrderSpec::First);
        assert_eq!(p("highest", None), OrderSpec::Last);
        assert_eq!(p("up", None), OrderSpec::Up(1));
        assert_eq!(p("down", Some("3")), OrderSpec::Down(3));
        assert_eq!(p("#4", None), OrderSpec::Position(4));
        assert_eq!(p("2", None), OrderSpec::Position(2));
        assert_eq!(p("after", Some("x")), OrderSpec::After("x".into()));
        assert!(OrderSpec::parse("before", None).is_err());
        assert!(OrderSpec::parse("sideways", None).is_err());
        assert!(OrderSpec::parse("#x", None).is_err());
    }

    #[test]
    fn move_mod_follows_legacy_positions_and_clamps() {
        let mut s = store_with_game("g");
        for u in ["a", "b", "c", "d"] {
            add(&s, "g", u, u, None);
        }
        let mv = |s: &mut Store, r: &str, spec: OrderSpec| move_mod(s, "g", r, &spec).unwrap().1;
        assert_eq!(
            mv(&mut s, "a", OrderSpec::Last),
            Moved {
                from: 1,
                to: 4,
                of: 4
            }
        );
        assert_eq!(uids(&s), ["b", "c", "d", "a"]);
        mv(&mut s, "a", OrderSpec::First);
        assert_eq!(uids(&s), ["a", "b", "c", "d"]);
        mv(&mut s, "a", OrderSpec::Up(2));
        assert_eq!(uids(&s), ["b", "c", "a", "d"]);
        mv(&mut s, "a", OrderSpec::Down(9));
        assert_eq!(uids(&s), ["a", "b", "c", "d"]);
        mv(&mut s, "a", OrderSpec::After("c".into()));
        assert_eq!(uids(&s), ["b", "c", "a", "d"]);
        mv(&mut s, "d", OrderSpec::Before("b".into()));
        assert_eq!(uids(&s), ["d", "b", "c", "a"]);
        mv(&mut s, "a", OrderSpec::Position(2));
        assert_eq!(uids(&s), ["d", "a", "b", "c"]);
        assert!(matches!(
            move_mod(&mut s, "g", "a", &OrderSpec::Before("a".into())),
            Err(ModsError::SelfRelative)
        ));
        let prios: Vec<i64> = list_mods(s.conn(), "g")
            .unwrap()
            .iter()
            .map(|m| m.priority)
            .collect();
        assert_eq!(prios, [0, 1, 2, 3]);
    }

    #[test]
    fn enable_disable_and_change_tracking() {
        let mut s = store_with_game("g");
        let a = add(&s, "g", "a", "a", None);
        assert!(!changed_since(&s, "g", None).unwrap());
        set_enabled(&mut s, "g", &[a], false).unwrap();
        assert!(!list_mods(s.conn(), "g").unwrap()[0].enabled);
        assert!(changed_since(&s, "g", None).unwrap());
        assert!(!changed_since(&s, "g", Some(i64::MAX)).unwrap());
    }

    #[test]
    fn remove_deletes_staging_and_only_unshared_archives_in_downloads() {
        let tmp = tempfile::tempdir().unwrap();
        let dl = tmp.path().join("downloads");
        std::fs::create_dir_all(&dl).unwrap();
        let shared = dl.join("shared.zip");
        let own = dl.join("own.zip");
        let outside = tmp.path().join("outside.zip");
        for f in [&shared, &own, &outside] {
            std::fs::write(f, "x").unwrap();
        }
        let mut s = store_with_game("g");
        let mk = |uid: &str, arch: &Path| {
            let id = add(&s, "g", uid, uid, None);
            let st = tmp.path().join("staging").join(uid);
            std::fs::create_dir_all(&st).unwrap();
            s.conn()
                .execute(
                    "UPDATE mods SET archive = ?1, staging_dir = ?2 WHERE id = ?3",
                    params![arch.to_string_lossy(), st.to_string_lossy(), id],
                )
                .unwrap();
            id
        };
        let a = mk("a", &shared);
        let _b = mk("b", &shared);
        let c = mk("c", &own);
        let d = mk("d", &outside);
        let r = remove_mods(&mut s, "g", &[a, c, d], Some(&dl)).unwrap();
        assert_eq!(r.mods, 3);
        assert_eq!(r.archives, vec![own.clone()]);
        assert!(shared.exists() && outside.exists() && !own.exists());
        assert!(!tmp.path().join("staging/a").exists());
        assert!(tmp.path().join("staging/b").exists());
        assert_eq!(uids(&s), ["b"]);
        assert_eq!(list_mods(s.conn(), "g").unwrap()[0].priority, 0);
    }
}
