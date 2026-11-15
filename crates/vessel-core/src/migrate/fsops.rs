// SPDX-License-Identifier: GPL-3.0-or-later
//! Filesystem primitives: verified copies, tree manifests and the
//! rename-or-copy move with its `*.migrating` EXDEV fallback (R2).

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{Faults, MigrateError};

/// Suffix of an in-progress cross-device copy.
pub const MIGRATING_SUFFIX: &str = ".migrating";

pub fn migrating_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(MIGRATING_SUFFIX);
    PathBuf::from(s)
}

/// Exists without following a final symlink.
pub fn present(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

pub fn sha256_file(path: &Path) -> Result<String, MigrateError> {
    let mut f = File::open(path).map_err(|e| MigrateError::io(path, e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|e| MigrateError::io(path, e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn write_synced(path: &Path, body: &[u8]) -> Result<(), MigrateError> {
    let mut f = File::create(path).map_err(|e| MigrateError::io(path, e))?;
    f.write_all(body)
        .and_then(|()| f.sync_all())
        .map_err(|e| MigrateError::io(path, e))
}

/// Best effort: makes a rename in `dir` durable.
pub fn sync_dir(dir: &Path) {
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
}

pub fn remove_all(p: &Path) -> Result<(), MigrateError> {
    let r = match fs::symlink_metadata(p) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => Err(e),
        Ok(m) if m.is_dir() => fs::remove_dir_all(p),
        Ok(_) => fs::remove_file(p),
    };
    r.map_err(|e| MigrateError::io(p, e))
}

/// One entry of a tree manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Dir,
    File { size: u64, sha256: String },
    Link(PathBuf),
}

/// Every entry under `root` (or `root` itself when it is a file), keyed by
/// relative path. Equal manifests mean equal file count, sizes and hashes.
pub fn manifest(root: &Path) -> Result<BTreeMap<PathBuf, Entry>, MigrateError> {
    let mut out = BTreeMap::new();
    for e in walkdir::WalkDir::new(root).follow_links(false) {
        let e = e.map_err(|e| {
            let p = e.path().unwrap_or(root).to_path_buf();
            MigrateError::io(&p, e.into())
        })?;
        let rel = e
            .path()
            .strip_prefix(root)
            .unwrap_or(e.path())
            .to_path_buf();
        let ft = e.file_type();
        let entry = if ft.is_symlink() {
            Entry::Link(fs::read_link(e.path()).map_err(|x| MigrateError::io(e.path(), x))?)
        } else if ft.is_dir() {
            Entry::Dir
        } else {
            let size = e
                .metadata()
                .map_err(|x| MigrateError::io(e.path(), x.into()))?
                .len();
            Entry::File {
                size,
                sha256: sha256_file(e.path())?,
            }
        };
        out.insert(rel, entry);
    }
    Ok(out)
}

pub fn trees_equal(a: &Path, b: &Path) -> Result<bool, MigrateError> {
    Ok(manifest(a)? == manifest(b)?)
}

/// Copies `src` (file, symlink or tree) to `dst`, fsyncing every file.
/// `faults.partial_copy_after` stops after that many files (test seam).
pub fn copy_tree(src: &Path, dst: &Path, faults: &Faults) -> Result<(), MigrateError> {
    let mut files = 0usize;
    for e in walkdir::WalkDir::new(src).follow_links(false) {
        let e = e.map_err(|e| {
            let p = e.path().unwrap_or(src).to_path_buf();
            MigrateError::io(&p, e.into())
        })?;
        let rel = e.path().strip_prefix(src).unwrap_or(e.path());
        let out = if rel.as_os_str().is_empty() {
            dst.to_path_buf()
        } else {
            dst.join(rel)
        };
        let ft = e.file_type();
        if ft.is_symlink() {
            let target = fs::read_link(e.path()).map_err(|x| MigrateError::io(e.path(), x))?;
            std::os::unix::fs::symlink(target, &out).map_err(|x| MigrateError::io(&out, x))?;
        } else if ft.is_dir() {
            fs::create_dir_all(&out).map_err(|x| MigrateError::io(&out, x))?;
            if let Ok(m) = e.metadata() {
                let _ = fs::set_permissions(&out, m.permissions());
            }
        } else {
            if faults.partial_copy_after == Some(files) {
                return Err(MigrateError::InjectedCrash("partial copy".into()));
            }
            fs::copy(e.path(), &out).map_err(|x| MigrateError::io(&out, x))?;
            File::open(&out)
                .and_then(|f| f.sync_all())
                .map_err(|x| MigrateError::io(&out, x))?;
            files += 1;
        }
    }
    Ok(())
}

/// Copies one file via `<dst>.migrating`, checks its SHA-256 against the
/// source and renames it into place. Returns the hash.
pub fn copy_file_verified(src: &Path, dst: &Path) -> Result<String, MigrateError> {
    let tmp = migrating_path(dst);
    remove_all(&tmp)?;
    if let Some(dir) = dst.parent() {
        fs::create_dir_all(dir).map_err(|e| MigrateError::io(dir, e))?;
    }
    copy_tree(src, &tmp, &Faults::default())?;
    let want = sha256_file(src)?;
    if sha256_file(&tmp)? != want {
        return Err(MigrateError::Verify(src.to_path_buf()));
    }
    fs::rename(&tmp, dst).map_err(|e| MigrateError::io(dst, e))?;
    Ok(want)
}

fn is_exdev(e: &io::Error) -> bool {
    rustix::io::Errno::from_io_error(e) == Some(rustix::io::Errno::XDEV)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moved {
    /// Nothing at the source.
    Absent,
    /// Atomic rename.
    Renamed,
    /// Cross-device: copied, verified, renamed, source removed.
    Copied,
    /// Source and an unrelated, different target both exist; nothing moved.
    Conflict,
}

/// Moves `src` to `dst`. Only call when a `dst` that already exists can
/// only have come from an earlier run of this migration (marker present):
/// it then counts as done when it matches the source tree exactly.
pub fn move_entry(src: &Path, dst: &Path, faults: &Faults) -> Result<Moved, MigrateError> {
    let tmp = migrating_path(dst);
    // A leftover copy from an interrupted run is never trusted: redo it.
    remove_all(&tmp)?;
    if !present(src) {
        return Ok(Moved::Absent);
    }
    if present(dst) {
        if trees_equal(src, dst)? {
            remove_all(src)?;
            return Ok(Moved::Copied);
        }
        return Ok(Moved::Conflict);
    }
    if let Some(dir) = dst.parent() {
        fs::create_dir_all(dir).map_err(|e| MigrateError::io(dir, e))?;
    }
    if !faults.force_exdev {
        match fs::rename(src, dst) {
            Ok(()) => return Ok(Moved::Renamed),
            Err(e) if is_exdev(&e) => {}
            Err(e) => return Err(MigrateError::io(src, e)),
        }
    }
    copy_tree(src, &tmp, faults)?;
    if !trees_equal(src, &tmp)? {
        return Err(MigrateError::Verify(src.to_path_buf()));
    }
    fs::rename(&tmp, dst).map_err(|e| MigrateError::io(dst, e))?;
    if let Some(dir) = dst.parent() {
        sync_dir(dir);
    }
    remove_all(src)?;
    Ok(Moved::Copied)
}
