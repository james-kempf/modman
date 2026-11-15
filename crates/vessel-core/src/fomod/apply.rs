// SPDX-License-Identifier: GPL-3.0-or-later
//! Copies planned files out of the extracted tree (legacy `fomod_py`
//! `find_ci`/`safe_rel`/placement, L976-1004, L1286-1330), confined to it.

use std::path::{Path, PathBuf};

use super::plan::InstallPlan;
use super::FomodError;

/// What [`apply_plan`] did besides placing files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyReport {
    /// Files placed (folder entries count every file inside).
    pub placed: usize,
    /// Sources not found in the archive (or refused: `..`, symlinks).
    pub missing: Vec<String>,
    /// Unsafe destinations that were skipped.
    pub refused: Vec<String>,
}

fn parts(p: &str) -> Vec<&str> {
    p.split(['/', '\\'])
        .filter(|x| !x.is_empty() && *x != ".")
        .collect()
}

/// Case-insensitive lookup of `rel` (either separator) under `base`. `..`
/// is refused, and leading separators or drive-less absolute paths stay
/// relative to `base`. With `follow_symlinks == false` any symlink on the
/// way is refused, so the result cannot leave `base`.
pub(crate) fn lookup(base: &Path, rel: &str, follow_symlinks: bool) -> Option<PathBuf> {
    let mut cur = base.to_path_buf();
    for p in parts(rel) {
        if p == ".." {
            return None;
        }
        let cand = cur.join(p);
        let found = if cand.symlink_metadata().is_ok() {
            cand
        } else {
            let low = p.to_lowercase();
            let hit = std::fs::read_dir(&cur)
                .ok()?
                .filter_map(Result::ok)
                .find(|e| e.file_name().to_string_lossy().to_lowercase() == low)?;
            hit.path()
        };
        if !follow_symlinks && found.symlink_metadata().ok()?.file_type().is_symlink() {
            return None;
        }
        cur = found;
    }
    Some(cur)
}

/// Destination made safe-relative; `None` when it contains `..`.
fn safe_rel(p: &str) -> Option<String> {
    let v = parts(p);
    (!v.contains(&"..")).then(|| v.join("/"))
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> FomodError + '_ {
    move |source| FomodError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn place_file(src: &Path, dst: &Path) -> Result<(), FomodError> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }
    if let Ok(md) = dst.symlink_metadata() {
        if md.is_dir() {
            std::fs::remove_dir_all(dst).map_err(io(dst))?;
        } else {
            std::fs::remove_file(dst).map_err(io(dst))?;
        }
    }
    if std::fs::hard_link(src, dst).is_err() {
        std::fs::copy(src, dst).map_err(io(dst))?;
    }
    Ok(())
}

fn place_tree(src: &Path, dst: &Path) -> Result<usize, FomodError> {
    let mut n = 0;
    std::fs::create_dir_all(dst).map_err(io(dst))?;
    for e in walkdir::WalkDir::new(src).follow_links(false).min_depth(1) {
        let e = e.map_err(|e| FomodError::Io {
            path: src.to_path_buf(),
            source: e.into(),
        })?;
        let rel = e.path().strip_prefix(src).expect("walk stays under src");
        let target = dst.join(rel);
        let ft = e.file_type();
        if ft.is_dir() {
            std::fs::create_dir_all(&target).map_err(io(&target))?;
        } else if ft.is_file() {
            place_file(e.path(), &target)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Places `plan` from `root` (the folder holding `fomod/`) into `out`. Later
/// entries overwrite earlier ones. Missing sources and unsafe destinations
/// are reported and skipped, like legacy.
pub fn apply_plan(root: &Path, plan: &InstallPlan, out: &Path) -> Result<ApplyReport, FomodError> {
    std::fs::create_dir_all(out).map_err(io(out))?;
    let mut rep = ApplyReport::default();
    for f in &plan.files {
        let Some(s) = lookup(root, &f.source, false).filter(|s| s.exists()) else {
            rep.missing.push(f.source.clone());
            continue;
        };
        let dst = f.destination.as_deref();
        if s.is_dir() {
            let Some(d) = safe_rel(dst.unwrap_or(&f.source)) else {
                rep.refused.push(dst.unwrap_or(&f.source).to_string());
                continue;
            };
            rep.placed += place_tree(
                &s,
                &if d.is_empty() {
                    out.to_path_buf()
                } else {
                    out.join(d)
                },
            )?;
            continue;
        }
        let base = s
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let d = match dst {
            None => safe_rel(&f.source),
            Some(d) if d.trim().is_empty() => Some(base.clone()),
            Some(d) if d.ends_with(['/', '\\']) => safe_rel(d).map(|r| {
                if r.is_empty() {
                    base.clone()
                } else {
                    format!("{r}/{base}")
                }
            }),
            Some(d) => safe_rel(d),
        };
        let Some(d) = d else {
            rep.refused.push(dst.unwrap_or(&f.source).to_string());
            continue;
        };
        place_file(&s, &out.join(if d.is_empty() { base } else { d }))?;
        rep.placed += 1;
    }
    Ok(rep)
}
