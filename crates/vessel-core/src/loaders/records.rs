// SPDX-License-Identifier: GPL-3.0-or-later
//! Store accessors for `loader_installs`, `loader_files` and `loader_runs`
//! (migration 0007).

use rusqlite::{params, OptionalExtension, Row};

use super::types::{Occupied, Op, Owner};
use super::LoaderError;
use crate::store::{now, Store};

macro_rules! str_enum {
    ($(#[$m:meta])* $name:ident { $($v:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($v),+ }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $($name::$v => $s),+ }
            }

            pub fn parse(s: &str) -> Option<Self> {
                match s { $($s => Some($name::$v),)+ _ => None }
            }
        }
    };
}

str_enum!(
    /// `loader_installs.status`. The first three are "active".
    InstallStatus {
        Installed => "installed",
        Running => "running",
        Interrupted => "interrupted",
        Uninstalled => "uninstalled",
        Failed => "failed",
    }
);

impl InstallStatus {
    pub fn is_active(self) -> bool {
        matches!(
            self,
            InstallStatus::Installed | InstallStatus::Running | InstallStatus::Interrupted
        )
    }
}

str_enum!(
    /// `loader_installs.strategy`.
    Strategy {
        Installer => "installer",
        Deploy => "deploy",
    }
);

str_enum!(
    /// `loader_files.change`.
    FileChange {
        Created => "created",
        Modified => "modified",
        Deleted => "deleted",
    }
);

str_enum!(
    /// `loader_files.kind`.
    FileKind {
        File => "file",
        Link => "link",
    }
);

str_enum!(
    /// `loader_runs.phase`, in order.
    RunPhase {
        BackingUp => "backing_up",
        Spawned => "spawned",
        Diffing => "diffing",
        Done => "done",
    }
);

/// A new `loader_installs` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLoaderInstall {
    pub game_id: String,
    pub game_dir: String,
    pub loader_key: String,
    pub version: Option<String>,
    pub runtime: String,
    pub strategy: Strategy,
    pub status: InstallStatus,
    pub nexus_mod_id: Option<i64>,
    pub nexus_file_id: Option<i64>,
    pub origin: String,
    pub archive_sha256: Option<String>,
    pub archive_path: Option<String>,
    pub vanilla_sha256: Option<String>,
    pub game_build: Option<String>,
}

/// A `loader_installs` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderInstall {
    pub id: i64,
    /// `None` once the game was removed.
    pub game_id: Option<String>,
    pub game_dir: String,
    pub loader_key: String,
    pub version: Option<String>,
    pub runtime: String,
    pub strategy: String,
    pub status: String,
    pub nexus_mod_id: Option<i64>,
    pub nexus_file_id: Option<i64>,
    pub origin: String,
    pub archive_sha256: Option<String>,
    pub archive_path: Option<String>,
    pub vanilla_sha256: Option<String>,
    pub game_build: Option<String>,
    pub installed_at: i64,
    pub updated_at: i64,
    /// JSON [`LaunchChange`](super::types::LaunchChange) this install needs.
    pub launch: Option<String>,
}

const INSTALL_COLS: &str =
    "id, game_id, game_dir, loader_key, version, runtime, strategy, status, \
     nexus_mod_id, nexus_file_id, origin, archive_sha256, archive_path, vanilla_sha256, \
     game_build, installed_at, updated_at, launch";

fn install_row(r: &Row<'_>) -> rusqlite::Result<LoaderInstall> {
    Ok(LoaderInstall {
        id: r.get(0)?,
        game_id: r.get(1)?,
        game_dir: r.get(2)?,
        loader_key: r.get(3)?,
        version: r.get(4)?,
        runtime: r.get(5)?,
        strategy: r.get(6)?,
        status: r.get(7)?,
        nexus_mod_id: r.get(8)?,
        nexus_file_id: r.get(9)?,
        origin: r.get(10)?,
        archive_sha256: r.get(11)?,
        archive_path: r.get(12)?,
        vanilla_sha256: r.get(13)?,
        game_build: r.get(14)?,
        installed_at: r.get(15)?,
        updated_at: r.get(16)?,
        launch: r.get(17)?,
    })
}

/// A `loader_files` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderFile {
    pub path: String,
    pub change: FileChange,
    pub kind: FileKind,
    pub pre_sha256: Option<String>,
    pub post_sha256: Option<String>,
    pub backup_ref: Option<String>,
    pub out_of_scope: bool,
}

/// A new `loader_runs` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLoaderRun {
    pub install_id: i64,
    pub op: Op,
    pub consent_hash: String,
    pub pre_manifest_path: Option<String>,
    pub log_path: Option<String>,
}

/// A `loader_runs` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderRun {
    pub id: i64,
    pub install_id: i64,
    pub op: String,
    pub consent_hash: String,
    pub phase: String,
    pub pid: Option<i64>,
    pub pgid: Option<i64>,
    pub proc_start: Option<i64>,
    pub pre_manifest_path: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub exit_code: Option<i64>,
    pub timed_out: bool,
    pub log_path: Option<String>,
    /// Proton runs: `wineserver` binary and `WINEPREFIX` for recovery.
    pub wineserver: Option<String>,
    pub wineprefix: Option<String>,
}

const RUN_COLS: &str = "id, install_id, op, consent_hash, phase, pid, pgid, proc_start, \
     pre_manifest_path, started_at, ended_at, exit_code, timed_out, log_path, wineserver, \
     wineprefix";

fn run_row(r: &Row<'_>) -> rusqlite::Result<LoaderRun> {
    Ok(LoaderRun {
        id: r.get(0)?,
        install_id: r.get(1)?,
        op: r.get(2)?,
        consent_hash: r.get(3)?,
        phase: r.get(4)?,
        pid: r.get(5)?,
        pgid: r.get(6)?,
        proc_start: r.get(7)?,
        pre_manifest_path: r.get(8)?,
        started_at: r.get(9)?,
        ended_at: r.get(10)?,
        exit_code: r.get(11)?,
        timed_out: r.get(12)?,
        log_path: r.get(13)?,
        wineserver: r.get(14)?,
        wineprefix: r.get(15)?,
    })
}

const ACTIVE: &str = "status IN ('installed', 'running', 'interrupted')";

impl Store {
    /// Inserts an install. A second active row for the same game and loader
    /// fails on the partial unique index.
    pub fn insert_loader_install(&self, n: &NewLoaderInstall) -> Result<i64, LoaderError> {
        let t = now();
        self.conn().execute(
            "INSERT INTO loader_installs (game_id, game_dir, loader_key, version, runtime,
                 strategy, status, nexus_mod_id, nexus_file_id, origin, archive_sha256,
                 archive_path, vanilla_sha256, game_build, installed_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
            params![
                n.game_id,
                n.game_dir,
                n.loader_key,
                n.version,
                n.runtime,
                n.strategy.as_str(),
                n.status.as_str(),
                n.nexus_mod_id,
                n.nexus_file_id,
                n.origin,
                n.archive_sha256,
                n.archive_path,
                n.vanilla_sha256,
                n.game_build,
                t,
            ],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    pub fn loader_install(&self, id: i64) -> Result<Option<LoaderInstall>, LoaderError> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {INSTALL_COLS} FROM loader_installs WHERE id = ?1"),
                [id],
                install_row,
            )
            .optional()?)
    }

    /// The installed, running or interrupted install of `key` for a game.
    pub fn active_loader_install(
        &self,
        game_id: &str,
        key: &str,
    ) -> Result<Option<LoaderInstall>, LoaderError> {
        Ok(self
            .conn()
            .query_row(
                &format!(
                    "SELECT {INSTALL_COLS} FROM loader_installs
                     WHERE game_id = ?1 AND loader_key = ?2 AND {ACTIVE}"
                ),
                params![game_id, key],
                install_row,
            )
            .optional()?)
    }

    /// Every install row of a game, oldest first.
    pub fn loader_installs(&self, game_id: &str) -> Result<Vec<LoaderInstall>, LoaderError> {
        let mut stmt = self.conn().prepare(&format!(
            "SELECT {INSTALL_COLS} FROM loader_installs WHERE game_id = ?1 ORDER BY id"
        ))?;
        let rows = stmt.query_map([game_id], install_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn set_loader_install_status(
        &self,
        id: i64,
        status: InstallStatus,
    ) -> Result<(), LoaderError> {
        self.conn().execute(
            "UPDATE loader_installs SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, status.as_str(), now()],
        )?;
        Ok(())
    }

    /// Updates what an op changed about the install itself.
    #[allow(clippy::too_many_arguments)]
    pub fn set_loader_install_meta(
        &self,
        id: i64,
        version: Option<&str>,
        runtime: &str,
        archive_sha256: Option<&str>,
        archive_path: Option<&str>,
        vanilla_sha256: Option<&str>,
        game_build: Option<&str>,
    ) -> Result<(), LoaderError> {
        self.conn().execute(
            "UPDATE loader_installs SET version = ?2, runtime = ?3, archive_sha256 = ?4,
                 archive_path = ?5, vanilla_sha256 = ?6, game_build = ?7, updated_at = ?8
             WHERE id = ?1",
            params![
                id,
                version,
                runtime,
                archive_sha256,
                archive_path,
                vanilla_sha256,
                game_build,
                now()
            ],
        )?;
        Ok(())
    }

    pub fn set_loader_install_launch(
        &self,
        id: i64,
        launch: Option<&str>,
    ) -> Result<(), LoaderError> {
        self.conn().execute(
            "UPDATE loader_installs SET launch = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, launch, now()],
        )?;
        Ok(())
    }

    /// Launch JSON of every active install of a game that has one.
    pub fn active_loader_launches(&self, game_id: &str) -> Result<Vec<String>, LoaderError> {
        let mut stmt = self.conn().prepare(&format!(
            "SELECT launch FROM loader_installs
             WHERE game_id = ?1 AND {ACTIVE} AND launch IS NOT NULL ORDER BY id"
        ))?;
        let rows = stmt.query_map([game_id], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Replaces every `loader_files` row of an install, in one transaction.
    pub fn replace_loader_files(
        &mut self,
        install_id: i64,
        files: &[LoaderFile],
    ) -> Result<(), LoaderError> {
        let tx = self.conn_mut().transaction()?;
        tx.execute(
            "DELETE FROM loader_files WHERE install_id = ?1",
            [install_id],
        )?;
        for f in files {
            tx.execute(
                "INSERT INTO loader_files (install_id, path, change, kind, pre_sha256,
                     post_sha256, backup_ref, out_of_scope)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    install_id,
                    f.path,
                    f.change.as_str(),
                    f.kind.as_str(),
                    f.pre_sha256,
                    f.post_sha256,
                    f.backup_ref,
                    f.out_of_scope,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Records one changed file. A second row for the same path fails.
    pub fn insert_loader_file(&self, install_id: i64, f: &LoaderFile) -> Result<(), LoaderError> {
        self.conn().execute(
            "INSERT INTO loader_files (install_id, path, change, kind, pre_sha256,
                 post_sha256, backup_ref, out_of_scope)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                install_id,
                f.path,
                f.change.as_str(),
                f.kind.as_str(),
                f.pre_sha256,
                f.post_sha256,
                f.backup_ref,
                f.out_of_scope,
            ],
        )?;
        Ok(())
    }

    pub fn loader_files(&self, install_id: i64) -> Result<Vec<LoaderFile>, LoaderError> {
        let mut stmt = self.conn().prepare(
            "SELECT path, change, kind, pre_sha256, post_sha256, backup_ref, out_of_scope
             FROM loader_files WHERE install_id = ?1 ORDER BY path",
        )?;
        let rows = stmt.query_map([install_id], |r| {
            let bad = |i: usize, v: String| {
                rusqlite::Error::FromSqlConversionFailure(
                    i,
                    rusqlite::types::Type::Text,
                    format!("unexpected value '{v}'").into(),
                )
            };
            let change: String = r.get(1)?;
            let kind: String = r.get(2)?;
            Ok(LoaderFile {
                path: r.get(0)?,
                change: FileChange::parse(&change).ok_or_else(|| bad(1, change.clone()))?,
                kind: FileKind::parse(&kind).ok_or_else(|| bad(2, kind.clone()))?,
                pre_sha256: r.get(3)?,
                post_sha256: r.get(4)?,
                backup_ref: r.get(5)?,
                out_of_scope: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Backup blobs referenced by any loader file (GC roots).
    /// Backup blobs referenced by loader files or pending run backups
    /// (GC roots).
    pub fn loader_backup_refs(&self) -> Result<Vec<String>, LoaderError> {
        let mut stmt = self.conn().prepare(
            "SELECT backup_ref FROM loader_files WHERE backup_ref IS NOT NULL
             UNION SELECT backup_ref FROM loader_run_backups ORDER BY 1",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Records a backup taken for `run_id` (before the run spawns).
    pub fn add_loader_run_backup(&self, run_id: i64, backup_ref: &str) -> Result<(), LoaderError> {
        self.conn().execute(
            "INSERT OR IGNORE INTO loader_run_backups (run_id, backup_ref) VALUES (?1, ?2)",
            params![run_id, backup_ref],
        )?;
        Ok(())
    }

    /// Pending backups of a run.
    pub fn loader_run_backups(&self, run_id: i64) -> Result<Vec<String>, LoaderError> {
        let mut stmt = self.conn().prepare(
            "SELECT backup_ref FROM loader_run_backups WHERE run_id = ?1 ORDER BY backup_ref",
        )?;
        let rows = stmt.query_map([run_id], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Drops the run's pending backups: call once `loader_files` hold the
    /// refs, or the rollback has finished or was declined.
    pub fn clear_loader_run_backups(&self, run_id: i64) -> Result<(), LoaderError> {
        self.conn()
            .execute("DELETE FROM loader_run_backups WHERE run_id = ?1", [run_id])?;
        Ok(())
    }

    /// Starts a run in phase `backing_up`.
    pub fn insert_loader_run(&self, n: &NewLoaderRun) -> Result<i64, LoaderError> {
        self.conn().execute(
            "INSERT INTO loader_runs (install_id, op, consent_hash, phase, pre_manifest_path,
                 started_at, log_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                n.install_id,
                n.op.as_str(),
                n.consent_hash,
                RunPhase::BackingUp.as_str(),
                n.pre_manifest_path,
                now(),
                n.log_path,
            ],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    pub fn loader_run(&self, id: i64) -> Result<Option<LoaderRun>, LoaderError> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {RUN_COLS} FROM loader_runs WHERE id = ?1"),
                [id],
                run_row,
            )
            .optional()?)
    }

    pub fn set_loader_run_phase(&self, id: i64, phase: RunPhase) -> Result<(), LoaderError> {
        self.conn().execute(
            "UPDATE loader_runs SET phase = ?2 WHERE id = ?1",
            params![id, phase.as_str()],
        )?;
        Ok(())
    }

    /// Stores the spawned process identity, before waiting on it.
    pub fn set_loader_run_process(
        &self,
        id: i64,
        pid: i64,
        pgid: i64,
        proc_start: i64,
    ) -> Result<(), LoaderError> {
        self.conn().execute(
            "UPDATE loader_runs SET pid = ?2, pgid = ?3, proc_start = ?4 WHERE id = ?1",
            params![id, pid, pgid, proc_start],
        )?;
        Ok(())
    }

    /// Stores the Proton cleanup target of a run, before it spawns.
    pub fn set_loader_run_wine(
        &self,
        id: i64,
        wineserver: &str,
        wineprefix: &str,
    ) -> Result<(), LoaderError> {
        self.conn().execute(
            "UPDATE loader_runs SET wineserver = ?2, wineprefix = ?3 WHERE id = ?1",
            params![id, wineserver, wineprefix],
        )?;
        Ok(())
    }

    /// Ends a run: phase `done`, exit status recorded.
    pub fn finish_loader_run(
        &self,
        id: i64,
        exit_code: Option<i64>,
        timed_out: bool,
    ) -> Result<(), LoaderError> {
        self.conn().execute(
            "UPDATE loader_runs SET phase = 'done', ended_at = ?2, exit_code = ?3, timed_out = ?4
             WHERE id = ?1",
            params![id, now(), exit_code, timed_out],
        )?;
        Ok(())
    }

    /// Runs not in phase `done` (for startup recovery).
    pub fn unfinished_loader_runs(&self) -> Result<Vec<LoaderRun>, LoaderError> {
        let mut stmt = self.conn().prepare(&format!(
            "SELECT {RUN_COLS} FROM loader_runs WHERE phase <> 'done' ORDER BY id"
        ))?;
        let rows = stmt.query_map([], run_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Game-root paths a loader must not overwrite: files Vessel deployed
    /// into the game folder, and files tracked by active installs of other
    /// loaders (`except_key` is the loader being planned).
    pub fn occupied_paths(
        &self,
        game_id: &str,
        except_key: &str,
    ) -> Result<Vec<Occupied>, LoaderError> {
        let mut out = Vec::new();
        let mut stmt = self.conn().prepare(
            "SELECT rel_path FROM deployments WHERE game = ?1 AND target = 'game_dir'
             ORDER BY rel_path",
        )?;
        for p in stmt.query_map([game_id], |r| r.get::<_, String>(0))? {
            out.push(Occupied {
                path: p?,
                owner: Owner::Deployed,
            });
        }
        let mut stmt = self.conn().prepare(&format!(
            "SELECT f.path, i.loader_key FROM loader_files f
             JOIN loader_installs i ON i.id = f.install_id
             WHERE i.game_id = ?1 AND i.loader_key <> ?2 AND i.{ACTIVE}
               AND f.change <> 'deleted'
             ORDER BY f.path"
        ))?;
        let rows = stmt.query_map(params![game_id, except_key], |r| {
            Ok(Occupied {
                path: r.get(0)?,
                owner: Owner::Loader(r.get(1)?),
            })
        })?;
        for o in rows {
            out.push(o?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MIGRATIONS;
    use rusqlite::Connection;

    fn store_with_game() -> Store {
        let s = Store::open_in_memory().unwrap();
        s.conn()
            .execute(
                "INSERT INTO games (id, name, folder, added_at) VALUES ('valheim', 'Valheim', '/g', 0)",
                [],
            )
            .unwrap();
        s
    }

    fn new_install(key: &str) -> NewLoaderInstall {
        NewLoaderInstall {
            game_id: "valheim".into(),
            game_dir: "/g".into(),
            loader_key: key.into(),
            version: Some("5.4".into()),
            runtime: "proton".into(),
            strategy: Strategy::Deploy,
            status: InstallStatus::Installed,
            nexus_mod_id: None,
            nexus_file_id: None,
            origin: "local".into(),
            archive_sha256: None,
            archive_path: None,
            vanilla_sha256: None,
            game_build: None,
        }
    }

    fn file(path: &str) -> LoaderFile {
        LoaderFile {
            path: path.into(),
            change: FileChange::Created,
            kind: FileKind::File,
            pre_sha256: None,
            post_sha256: Some("ab".into()),
            backup_ref: Some("blob1".into()),
            out_of_scope: false,
        }
    }

    /// Catches: the partial unique index missing (two live installs of one
    /// loader) or covering finished rows (reinstall after uninstall blocked).
    #[test]
    fn one_active_install_per_game_and_loader() {
        let s = store_with_game();
        let id = s.insert_loader_install(&new_install("bepinex")).unwrap();
        assert!(s.insert_loader_install(&new_install("bepinex")).is_err());
        s.insert_loader_install(&new_install("other")).unwrap();
        s.set_loader_install_status(id, InstallStatus::Uninstalled)
            .unwrap();
        let again = s.insert_loader_install(&new_install("bepinex")).unwrap();
        let active = s
            .active_loader_install("valheim", "bepinex")
            .unwrap()
            .unwrap();
        assert_eq!(active.id, again);
        assert_eq!(s.loader_installs("valheim").unwrap().len(), 3);
    }

    /// Catches: duplicate file rows for one install (diff recorded twice).
    #[test]
    fn loader_files_primary_key_rejects_duplicates() {
        let s = store_with_game();
        let id = s.insert_loader_install(&new_install("bepinex")).unwrap();
        s.insert_loader_file(id, &file("winhttp.dll")).unwrap();
        assert!(s.insert_loader_file(id, &file("winhttp.dll")).is_err());
        assert_eq!(s.loader_files(id).unwrap(), [file("winhttp.dll")]);
        assert_eq!(s.loader_backup_refs().unwrap(), ["blob1"]);
    }

    /// Catches: removing a game cascading away rollback data.
    #[test]
    fn deleting_the_game_keeps_install_rows() {
        let s = store_with_game();
        let id = s.insert_loader_install(&new_install("bepinex")).unwrap();
        s.insert_loader_file(id, &file("winhttp.dll")).unwrap();
        s.conn()
            .execute("DELETE FROM games WHERE id = 'valheim'", [])
            .unwrap();
        let row = s.loader_install(id).unwrap().unwrap();
        assert_eq!((row.game_id, row.game_dir.as_str()), (None, "/g"));
        assert_eq!(s.loader_files(id).unwrap().len(), 1);
    }

    /// Catches: phase/process bookkeeping not persisting, which recovery
    /// relies on.
    #[test]
    fn run_lifecycle_is_persisted() {
        let s = store_with_game();
        let id = s.insert_loader_install(&new_install("bepinex")).unwrap();
        let run = s
            .insert_loader_run(&NewLoaderRun {
                install_id: id,
                op: Op::Install,
                consent_hash: "h".into(),
                pre_manifest_path: Some("/m".into()),
                log_path: None,
            })
            .unwrap();
        s.set_loader_run_phase(run, RunPhase::Spawned).unwrap();
        s.set_loader_run_process(run, 10, 10, 1234).unwrap();
        let open = s.unfinished_loader_runs().unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(
            (open[0].phase.as_str(), open[0].pgid, open[0].proc_start),
            ("spawned", Some(10), Some(1234))
        );
        s.finish_loader_run(run, Some(0), false).unwrap();
        assert!(s.unfinished_loader_runs().unwrap().is_empty());
        assert_eq!(s.loader_run(run).unwrap().unwrap().exit_code, Some(0));
    }

    /// Catches: own files reported as conflicts on upgrade, inactive
    /// installs still blocking, or deployed prefix files counted.
    #[test]
    fn occupied_paths_cover_deployed_and_other_active_loaders() {
        let s = store_with_game();
        s.conn()
            .execute_batch(
                "INSERT INTO deployments (game, target, rel_path, method, source, deployed_at)
                   VALUES ('valheim', 'game_dir', 'BepInEx/plugins/a.dll', 'copy', '/s', 0),
                          ('valheim', 'prefix_dir', 'x.ini', 'copy', '/s', 0);",
            )
            .unwrap();
        let other = s.insert_loader_install(&new_install("other")).unwrap();
        s.insert_loader_file(other, &file("other.dll")).unwrap();
        let own = s.insert_loader_install(&new_install("bepinex")).unwrap();
        s.insert_loader_file(own, &file("winhttp.dll")).unwrap();
        let occ = s.occupied_paths("valheim", "bepinex").unwrap();
        assert_eq!(
            occ,
            [
                Occupied {
                    path: "BepInEx/plugins/a.dll".into(),
                    owner: Owner::Deployed
                },
                Occupied {
                    path: "other.dll".into(),
                    owner: Owner::Loader("other".into())
                },
            ]
        );
        s.set_loader_install_status(other, InstallStatus::Uninstalled)
            .unwrap();
        assert_eq!(s.occupied_paths("valheim", "bepinex").unwrap().len(), 1);
    }

    /// Catches: 0007 failing on an existing v6 database, or losing games.
    #[test]
    fn migration_0007_applies_on_a_v6_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vessel.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE schema_migrations (
                     version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL);",
            )
            .unwrap();
            for m in MIGRATIONS.iter().filter(|m| m.version <= 6) {
                conn.execute_batch(m.sql).unwrap();
                conn.execute(
                    "INSERT INTO schema_migrations VALUES (?1, ?2, 0)",
                    params![m.version, m.name],
                )
                .unwrap();
            }
            conn.execute(
                "INSERT INTO games (id, name, added_at) VALUES ('valheim', 'Valheim', 0)",
                [],
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), 7);
        let (name, desired): (String, Option<String>) = s
            .conn()
            .query_row(
                "SELECT name, launch_options_desired FROM games WHERE id = 'valheim'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((name.as_str(), desired), ("Valheim", None));
        s.insert_loader_install(&new_install("bepinex")).unwrap();
    }
}
