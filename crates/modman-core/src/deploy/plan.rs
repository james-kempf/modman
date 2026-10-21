// SPDX-License-Identifier: GPL-3.0-or-later
//! Deploy plan: one winner per path (legacy `build_plan`, L1586-1608) and
//! case-insensitive resolution against the game folder (`scan_game_folder` /
//! `resolve_dir`, L1611-1642).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::{io_err, DeployError};
use crate::games::{GameConfig, Target};

/// A mod taking part in a plan.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ModRef {
    pub id: i64,
    pub uid: String,
    pub name: String,
    pub priority: i64,
}

/// An enabled mod left out of the plan.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SkippedMod {
    pub uid: String,
    pub reason: String,
}

/// One target path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanItem {
    /// Relative to the target root, `/`-separated, first provider's spelling.
    pub rel_path: String,
    /// Winning mod (index into [`Plan::mods`]).
    pub winner: usize,
    /// Absolute staged file of the winner.
    pub source: PathBuf,
    /// Path of the winner's file inside its staging dir.
    pub staged_path: String,
    /// Every mod providing the path, lowest priority first.
    pub providers: Vec<usize>,
}

/// A path provided by more than one mod.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Conflict {
    pub rel_path: String,
    pub winner: ModRef,
    /// Overridden mods, lowest priority first.
    pub losers: Vec<ModRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub target: Target,
    /// Contributing mods, in priority order.
    pub mods: Vec<ModRef>,
    /// In order of first appearance.
    pub items: Vec<PlanItem>,
    pub skipped: Vec<SkippedMod>,
}

impl Plan {
    pub fn conflicts(&self) -> Vec<Conflict> {
        self.items
            .iter()
            .filter(|i| i.providers.len() > 1)
            .map(|i| Conflict {
                rel_path: i.rel_path.clone(),
                winner: self.mods[i.winner].clone(),
                losers: i
                    .providers
                    .iter()
                    .filter(|p| **p != i.winner)
                    .map(|p| self.mods[*p].clone())
                    .collect(),
            })
            .collect()
    }
}

/// `subdir` as a path prefix: empty for `.`, else `<subdir>/`
/// (legacy `deploy_base_rel`, L380-385).
fn base_prefix(subdir: &str) -> String {
    let s = subdir.trim_start_matches("./").trim_matches('/');
    if s.is_empty() || s == "." {
        String::new()
    } else {
        format!("{s}/")
    }
}

/// Builds the plan from the enabled mods of `game`, lowest priority first.
/// Staging trees are read as plain directories; non-regular files and
/// non-UTF-8 names are ignored.
pub fn build_plan(conn: &Connection, game: &GameConfig) -> Result<Plan, DeployError> {
    let mut stmt = conn.prepare(
        "SELECT id, uid, name, priority, root, staging_dir FROM mods
         WHERE game = ?1 AND enabled = 1 ORDER BY priority, id",
    )?;
    let rows = stmt
        .query_map([&game.domain], |r| {
            Ok((
                ModRef {
                    id: r.get(0)?,
                    uid: r.get(1)?,
                    name: r.get(2)?,
                    priority: r.get(3)?,
                },
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let base = base_prefix(&game.subdir);
    let mut plan = Plan {
        target: game.target,
        mods: Vec::new(),
        items: Vec::new(),
        skipped: Vec::new(),
    };
    let mut index: HashMap<String, usize> = HashMap::new();
    for (m, root, staging) in rows {
        let Some(dir) = staging.filter(|s| !s.is_empty()).map(PathBuf::from) else {
            plan.skipped.push(SkippedMod {
                uid: m.uid,
                reason: "no staging folder recorded".into(),
            });
            continue;
        };
        if !dir.is_dir() {
            plan.skipped.push(SkippedMod {
                uid: m.uid,
                reason: format!("staged files missing ({})", dir.display()),
            });
            continue;
        }
        let prefix = if root == "game" { "" } else { base.as_str() };
        let mi = plan.mods.len();
        plan.mods.push(m);
        for e in walkdir::WalkDir::new(&dir).min_depth(1).sort_by_file_name() {
            let e = e.map_err(|e| DeployError::Io {
                path: e.path().map_or_else(|| dir.clone(), Path::to_path_buf),
                source: e.into(),
            })?;
            if !e.file_type().is_file() {
                continue;
            }
            let Some(staged) = e
                .path()
                .strip_prefix(&dir)
                .ok()
                .and_then(Path::to_str)
                .map(str::to_string)
            else {
                tracing::warn!(path = %e.path().display(), "skipping non-UTF-8 staged path");
                continue;
            };
            let rel = format!("{prefix}{staged}");
            let key = rel.to_lowercase();
            match index.get(&key) {
                Some(&i) => {
                    let item = &mut plan.items[i];
                    if item.providers.last() != Some(&mi) {
                        item.providers.push(mi);
                    }
                    item.winner = mi;
                    item.source = e.path().to_path_buf();
                    item.staged_path = staged;
                }
                None => {
                    index.insert(key, plan.items.len());
                    plan.items.push(PlanItem {
                        rel_path: rel,
                        winner: mi,
                        source: e.path().to_path_buf(),
                        staged_path: staged,
                        providers: vec![mi],
                    });
                }
            }
        }
    }
    Ok(plan)
}

/// Conflicts of the current enabled mods of `game`.
pub fn conflicts(conn: &Connection, game: &GameConfig) -> Result<Vec<Conflict>, DeployError> {
    Ok(build_plan(conn, game)?.conflicts())
}

/// Kind of a directory entry, from `lstat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Dir,
    /// A symlink; `to_dir` when it resolves to a folder.
    Link {
        to_dir: bool,
    },
}

/// A directory entry: real name and kind.
type Listing = HashMap<String, (String, Kind)>;

/// Maps plan paths onto existing spellings in the target, case-insensitively.
/// Read-only; folders that do not exist are reported in [`Self::new_dirs`].
///
/// Symlinked folders are followed only when they resolve inside the root;
/// the returned path then uses the resolved spelling, so it contains no
/// symlinked folder (placement re-checks this with an `O_NOFOLLOW` walk).
/// A symlinked folder leading outside the root is a [`DeployError::PathConflict`].
pub(crate) struct CaseResolver {
    root: PathBuf,
    /// Canonical root, computed on first use.
    canon_root: Option<PathBuf>,
    listings: HashMap<String, Listing>,
    new_lookup: HashMap<String, String>,
    /// Folders to create (real spelling), parents first.
    pub new_dirs: Vec<String>,
}

fn join(a: &str, b: &str) -> String {
    if a.is_empty() {
        b.to_string()
    } else {
        format!("{a}/{b}")
    }
}

impl CaseResolver {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            canon_root: None,
            listings: HashMap::new(),
            new_lookup: HashMap::new(),
            new_dirs: Vec::new(),
        }
    }

    fn lookup(&mut self, dir: &str, name: &str) -> Result<Option<(String, Kind)>, DeployError> {
        if !self.listings.contains_key(dir) {
            let abs = self.root.join(dir);
            let mut map = Listing::new();
            for e in std::fs::read_dir(&abs).map_err(io_err(&abs))? {
                let e = e.map_err(io_err(&abs))?;
                let Some(n) = e.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                let ft = e.file_type().map_err(io_err(&e.path()))?;
                let kind = if ft.is_symlink() {
                    Kind::Link {
                        to_dir: std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir()),
                    }
                } else if ft.is_dir() {
                    Kind::Dir
                } else {
                    Kind::File
                };
                // An exact spelling beats a case-insensitive one.
                map.insert(n.clone(), (n.clone(), kind));
                map.entry(format!("\0{}", n.to_lowercase()))
                    .or_insert((n, kind));
            }
            self.listings.insert(dir.to_string(), map);
        }
        let map = &self.listings[dir];
        Ok(map
            .get(name)
            .or_else(|| map.get(&format!("\0{}", name.to_lowercase())))
            .cloned())
    }

    /// Root-relative real path of the folder symlink `link` (root-relative),
    /// or `None` when it resolves outside the root (or not to a folder).
    fn follow_inside(&mut self, link: &str) -> Result<Option<String>, DeployError> {
        if self.canon_root.is_none() {
            let c = self.root.canonicalize().map_err(io_err(&self.root))?;
            self.canon_root = Some(c);
        }
        let canon_root = self.canon_root.as_deref().expect("set above");
        let Ok(target) = self.root.join(link).canonicalize() else {
            return Ok(None);
        };
        Ok(target
            .strip_prefix(canon_root)
            .ok()
            .filter(|_| target.is_dir())
            .and_then(Path::to_str)
            .map(str::to_string))
    }

    /// Real relative path for `rel` and whether something exists there.
    pub fn resolve(&mut self, rel: &str) -> Result<(String, bool), DeployError> {
        let conflict = |reason: &str| DeployError::PathConflict {
            rel: rel.to_string(),
            reason: reason.to_string(),
        };
        let comps = super::confine::components(rel).map_err(conflict)?;
        let mut real = String::new();
        let mut fresh = false;
        for (i, c) in comps.iter().enumerate() {
            let last = i + 1 == comps.len();
            if !fresh {
                if let Some((name, kind)) = self.lookup(&real, c)? {
                    let here = join(&real, &name);
                    match kind {
                        Kind::Dir | Kind::Link { to_dir: true } if last => {
                            return Err(conflict("a folder exists at that path"))
                        }
                        _ if last => return Ok((here, true)),
                        Kind::Dir => real = here,
                        Kind::Link { to_dir: true } => {
                            real = self.follow_inside(&here)?.ok_or_else(|| {
                                conflict(&format!(
                                    "{here} is a symbolic link to a folder outside the target folder"
                                ))
                            })?;
                        }
                        _ => return Err(conflict(&format!("{here} is a file"))),
                    }
                    continue;
                }
            }
            if last {
                return Ok((join(&real, c), false));
            }
            let key = join(&real, c).to_lowercase();
            real = match self.new_lookup.get(&key) {
                Some(r) => r.clone(),
                None => {
                    let r = join(&real, c);
                    self.new_lookup.insert(key, r.clone());
                    self.new_dirs.push(r.clone());
                    r
                }
            };
            fresh = true;
        }
        Err(conflict("empty path"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_prefix_matches_legacy() {
        assert_eq!(base_prefix("."), "");
        assert_eq!(base_prefix(""), "");
        assert_eq!(base_prefix("Data"), "Data/");
        assert_eq!(base_prefix("./Data/"), "Data/");
    }

    #[test]
    fn resolver_reuses_existing_spelling_and_records_new_dirs_once() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("Data/Meshes")).unwrap();
        std::fs::write(tmp.path().join("Data/A.esp"), "").unwrap();
        let mut r = CaseResolver::new(tmp.path());
        assert_eq!(
            r.resolve("data/a.ESP").unwrap(),
            ("Data/A.esp".into(), true)
        );
        assert_eq!(
            r.resolve("DATA/meshes/x/y.nif").unwrap(),
            ("Data/Meshes/x/y.nif".into(), false)
        );
        assert_eq!(
            r.resolve("data/MESHES/X/z.nif").unwrap(),
            ("Data/Meshes/x/z.nif".into(), false)
        );
        assert_eq!(r.new_dirs, ["Data/Meshes/x"]);
        assert!(matches!(
            r.resolve("data/a.esp/sub.txt"),
            Err(DeployError::PathConflict { .. })
        ));
    }
}
