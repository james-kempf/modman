// SPDX-License-Identifier: GPL-3.0-or-later
//! Content-addressed backup store for game files replaced by a deploy.
//! Blobs live at `<root>/<sha[..2]>/<sha>`. A symlink is stored as its link
//! target; the `backups.mode` column keeps the full `st_mode` (type bits too).

use std::fs::{self, File};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::place::{hash_copy, mtime_ns, sha256_file, system_time, tmp_path};

const S_IFMT: u32 = 0o170000;
const S_IFLNK: u32 = 0o120000;

/// A held (or just written) backup.
#[derive(Debug, Clone)]
pub(crate) struct Blob {
    pub sha256: String,
    pub path: PathBuf,
    pub size: i64,
    pub mode: u32,
    pub mtime_ns: i64,
}

fn fsync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

pub(crate) struct BlobStore<'a> {
    pub root: &'a Path,
}

impl BlobStore<'_> {
    pub fn blob_path(&self, sha: &str) -> PathBuf {
        self.root.join(&sha[..2]).join(sha)
    }

    /// Copies the file (or symlink) at `path` into the store; durable on
    /// return (file and folder fsynced).
    pub fn store(&self, path: &Path) -> io::Result<Blob> {
        let meta = fs::symlink_metadata(path)?;
        fs::create_dir_all(self.root)?;
        let tmp = self.root.join(format!(".incoming-{}", std::process::id()));
        let mut out = File::create(&tmp)?;
        let sha = if meta.is_symlink() {
            let target = fs::read_link(path)?;
            hash_copy(target.as_os_str().as_bytes(), Some(&mut out))?
        } else {
            hash_copy(File::open(path)?, Some(&mut out))?
        };
        out.sync_all()?;
        drop(out);
        let blob = self.blob_path(&sha);
        let dir = blob.parent().expect("blob has a parent");
        fs::create_dir_all(dir)?;
        if blob.exists() {
            fs::remove_file(&tmp)?;
        } else {
            fs::rename(&tmp, &blob)?;
            fsync_dir(dir)?;
            fsync_dir(self.root)?;
        }
        Ok(Blob {
            sha256: sha,
            path: blob,
            size: meta.size() as i64,
            mode: meta.mode(),
            mtime_ns: mtime_ns(&meta),
        })
    }

    /// Writes the backup back to `target` (content, permissions, mtime, or
    /// the symlink), atomically and durably.
    pub fn restore(&self, b: &Blob, target: &Path) -> io::Result<()> {
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = tmp_path(target);
        let _ = fs::remove_file(&tmp);
        if b.mode & S_IFMT == S_IFLNK {
            let link = fs::read(&b.path)?;
            std::os::unix::fs::symlink(std::ffi::OsStr::from_bytes(&link), &tmp)?;
        } else {
            let mut out = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            io::copy(&mut File::open(&b.path)?, &mut out)?;
            out.set_modified(system_time(b.mtime_ns))?;
            out.set_permissions(fs::Permissions::from_mode(b.mode & 0o7777))?;
            out.sync_all()?;
        }
        fs::rename(&tmp, target)?;
        if let Some(dir) = target.parent() {
            fsync_dir(dir)?;
        }
        Ok(())
    }

    /// True when `target` (lstat `meta`) holds exactly the backed-up original.
    pub fn matches(&self, b: &Blob, target: &Path, meta: &fs::Metadata) -> bool {
        if b.mode & S_IFMT == S_IFLNK {
            return meta.is_symlink()
                && match (fs::read_link(target), fs::read(&b.path)) {
                    (Ok(l), Ok(want)) => l.as_os_str().as_bytes() == want.as_slice(),
                    _ => false,
                };
        }
        meta.is_file()
            && meta.size() as i64 == b.size
            && sha256_file(target).is_ok_and(|s| s == b.sha256)
    }

    /// Copies a blob to `dest` (used to keep a retired backup in overwrite/).
    pub fn copy_out(&self, b: &Blob, dest: &Path) -> io::Result<()> {
        if let Some(dir) = dest.parent() {
            fs::create_dir_all(dir)?;
        }
        let _ = fs::remove_file(dest);
        self.restore(b, dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_dedups_and_restore_roundtrips_mode_mtime_and_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("store");
        let s = BlobStore { root: &root };
        let a = tmp.path().join("a");
        fs::write(&a, "same").unwrap();
        fs::set_permissions(&a, fs::Permissions::from_mode(0o751)).unwrap();
        let b = tmp.path().join("b");
        fs::write(&b, "same").unwrap();
        let (ba, bb) = (s.store(&a).unwrap(), s.store(&b).unwrap());
        assert_eq!(ba.path, bb.path);
        fs::remove_file(&a).unwrap();
        s.restore(&ba, &a).unwrap();
        let m = fs::metadata(&a).unwrap();
        assert_eq!(m.mode() & 0o7777, 0o751);
        assert_eq!(mtime_ns(&m), ba.mtime_ns);
        assert!(s.matches(&ba, &a, &fs::symlink_metadata(&a).unwrap()));

        let l = tmp.path().join("l");
        std::os::unix::fs::symlink("a", &l).unwrap();
        let bl = s.store(&l).unwrap();
        fs::remove_file(&l).unwrap();
        s.restore(&bl, &l).unwrap();
        assert_eq!(fs::read_link(&l).unwrap(), Path::new("a"));
    }
}
