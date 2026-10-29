// SPDX-License-Identifier: GPL-3.0-or-later
//! SQLite store: connection setup (WAL), embedded migrations, settings and the
//! deploy lock.
//!
//! Migrations are plain SQL files embedded at build time. Each runs in its own
//! transaction and is recorded in `schema_migrations`. A database written by a
//! newer binary (unknown version) is refused.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

/// Errors from the store.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "database schema version {found} is newer than this Vessel supports ({supported}); upgrade Vessel"
    )]
    SchemaTooNew { found: i64, supported: i64 },
    #[error("migration {version} ({name}) failed: {source}")]
    Migration {
        version: i64,
        name: &'static str,
        source: rusqlite::Error,
    },
    #[error("another Vessel process is deploying or purging (lock {0} is held)")]
    LockBusy(PathBuf),
}

/// One embedded migration.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

/// All migrations, in order. Versions must be strictly increasing.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "core",
        sql: include_str!("../migrations/0001_core.sql"),
    },
    Migration {
        version: 2,
        name: "deploy",
        sql: include_str!("../migrations/0002_deploy.sql"),
    },
    Migration {
        version: 3,
        name: "nexus",
        sql: include_str!("../migrations/0003_nexus.sql"),
    },
    Migration {
        version: 4,
        name: "collections",
        sql: include_str!("../migrations/0004_collections.sql"),
    },
    Migration {
        version: 5,
        name: "autoupdate",
        sql: include_str!("../migrations/0005_autoupdate.sql"),
    },
];

/// Highest schema version this binary knows.
pub fn latest_version() -> i64 {
    MIGRATIONS.last().map_or(0, |m| m.version)
}

/// Current Unix time in seconds.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// An open, migrated database.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating if needed) the database at `path`, enables WAL and
    /// applies pending migrations.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|source| StoreError::Io {
                path: dir.to_path_buf(),
                source,
            })?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    /// In-memory database (tests, previews). No WAL.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self, StoreError> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Deploy/purge journals must survive power loss.
        conn.pragma_update(None, "synchronous", "FULL")?;
        migrate(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Highest applied migration version.
    pub fn schema_version(&self) -> Result<i64, StoreError> {
        applied_version(&self.conn)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    }
}

fn applied_version(conn: &Connection) -> Result<i64, StoreError> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?)
}

fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
             version    INTEGER PRIMARY KEY,
             name       TEXT NOT NULL,
             applied_at INTEGER NOT NULL
         );",
    )?;
    let current = applied_version(conn)?;
    let supported = latest_version();
    if current > supported {
        return Err(StoreError::SchemaTooNew {
            found: current,
            supported,
        });
    }
    for m in MIGRATIONS.iter().filter(|m| m.version > current) {
        let wrap = |source| StoreError::Migration {
            version: m.version,
            name: m.name,
            source,
        };
        let tx = conn.transaction().map_err(wrap)?;
        tx.execute_batch(m.sql).map_err(wrap)?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
            params![m.version, m.name, now()],
        )
        .map_err(wrap)?;
        tx.commit().map_err(wrap)?;
        tracing::debug!(version = m.version, name = m.name, "applied migration");
    }
    Ok(())
}

/// Exclusive advisory lock (flock) held for the duration of a deploy or purge,
/// so a CLI deploy cannot race a GUI deploy. Released on drop.
#[derive(Debug)]
pub struct DeployLock {
    _file: File,
    path: PathBuf,
}

impl DeployLock {
    /// Takes the lock without blocking; `LockBusy` if another holder exists.
    pub fn try_acquire(path: &Path) -> Result<Self, StoreError> {
        let io_err = |source| StoreError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io_err)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .map_err(io_err)?;
        match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => Ok(Self {
                _file: file,
                path: path.to_path_buf(),
            }),
            Err(e) if e == rustix::io::Errno::WOULDBLOCK => {
                Err(StoreError::LockBusy(path.to_path_buf()))
            }
            Err(e) => Err(io_err(e.into())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every table the plan requires (tech stack §5 plus T03 additions).
    const REQUIRED_TABLES: &[&str] = &[
        "schema_migrations",
        "settings",
        "games",
        "profiles",
        "profile_mods",
        "mods",
        "mod_files",
        "deployments",
        "deployment_dirs",
        "deploy_journal",
        "deploy_journal_steps",
        "backups",
        "downloads",
        "collections",
        "collection_mods",
        "updates_cache",
        "requirements_cache",
        "ratelimit",
        "fomod_choices",
        "deps_ack",
        "autoupdate_state",
        "autoupdate_runs",
    ];

    fn tables(store: &Store) -> Vec<String> {
        let mut stmt = store
            .conn()
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn migrations_create_every_required_table_on_empty_db() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("vessel.db")).unwrap();
        let have = tables(&store);
        for t in REQUIRED_TABLES {
            assert!(have.iter().any(|h| h == t), "missing table {t}");
        }
        assert_eq!(store.schema_version().unwrap(), latest_version());
    }

    #[test]
    fn reopen_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vessel.db");
        drop(Store::open(&path).unwrap());
        let store = Store::open(&path).unwrap();
        let rows: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, MIGRATIONS.len() as i64);
    }

    #[test]
    fn migration_versions_strictly_increase() {
        for w in MIGRATIONS.windows(2) {
            assert!(w[0].version < w[1].version);
        }
    }

    #[test]
    fn file_db_uses_wal_and_foreign_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("vessel.db")).unwrap();
        let mode: String = store
            .conn()
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        let err = store.conn().execute(
            "INSERT INTO mods (game, uid, name, priority, installed_at) VALUES ('nogame', 'x', 'x', 0, 0)",
            [],
        );
        assert!(err.is_err(), "foreign key to games must be enforced");
    }

    #[test]
    fn refuses_database_from_newer_binary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vessel.db");
        {
            let store = Store::open(&path).unwrap();
            store
                .conn()
                .execute(
                    "INSERT INTO schema_migrations VALUES (?1, 'future', 0)",
                    [latest_version() + 1],
                )
                .unwrap();
        }
        match Store::open(&path) {
            Err(StoreError::SchemaTooNew { found, supported }) => {
                assert_eq!(found, latest_version() + 1);
                assert_eq!(supported, latest_version());
            }
            other => panic!("expected SchemaTooNew, got {:?}", other.err()),
        }
    }

    #[test]
    fn settings_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.setting("k").unwrap(), None);
        store.set_setting("k", "1").unwrap();
        store.set_setting("k", "2").unwrap();
        assert_eq!(store.setting("k").unwrap().as_deref(), Some("2"));
        store.delete_setting("k").unwrap();
        assert_eq!(store.setting("k").unwrap(), None);
    }

    #[test]
    fn deploy_lock_is_exclusive_until_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locks/deploy.lock");
        let held = DeployLock::try_acquire(&path).unwrap();
        assert!(matches!(
            DeployLock::try_acquire(&path),
            Err(StoreError::LockBusy(_))
        ));
        drop(held);
        DeployLock::try_acquire(&path).unwrap();
    }
}
