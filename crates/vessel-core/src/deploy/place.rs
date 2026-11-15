// SPDX-License-Identifier: GPL-3.0-or-later
//! Placing one file with the method chain, and recognising our placements.

use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use super::{Faults, Method};

/// Hex SHA-256 of a byte string.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Streams `r` into a SHA-256 hasher (and `out`, when given).
pub(crate) fn hash_copy(mut r: impl Read, mut out: Option<&mut File>) -> io::Result<String> {
    use std::io::Write;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        if let Some(o) = out.as_deref_mut() {
            o.write_all(&buf[..n])?;
        }
    }
    Ok(hex(&h.finalize()))
}

pub(crate) fn sha256_file(path: &Path) -> io::Result<String> {
    hash_copy(File::open(path)?, None)
}

pub(crate) fn mtime_ns(m: &fs::Metadata) -> i64 {
    m.mtime() * 1_000_000_000 + m.mtime_nsec()
}

pub(crate) fn system_time(ns: i64) -> SystemTime {
    if ns >= 0 {
        UNIX_EPOCH + Duration::from_nanos(ns as u64)
    } else {
        UNIX_EPOCH - Duration::from_nanos(ns.unsigned_abs())
    }
}

/// Sibling temp name used while placing or restoring `target`.
pub(crate) fn tmp_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!(".{name}.vessel-tmp"))
}

/// Gives `f` the permissions and mtime of `src`. Works through the open
/// handle, so read-only modes are applied last without reopening.
fn copy_attrs(f: &File, src: &fs::Metadata) -> io::Result<()> {
    f.set_modified(system_time(mtime_ns(src)))?;
    f.set_permissions(fs::Permissions::from_mode(src.mode() & 0o7777))
}

fn try_method(m: Method, src: &Path, src_meta: &fs::Metadata, tmp: &Path) -> io::Result<()> {
    match m {
        Method::Hardlink => fs::hard_link(src, tmp),
        Method::Symlink => std::os::unix::fs::symlink(src, tmp),
        Method::Reflink => {
            reflink_copy::reflink(src, tmp)?;
            let f = fs::OpenOptions::new().write(true).open(tmp)?;
            copy_attrs(&f, src_meta)
        }
        Method::Copy => {
            // create_new: never write through a symlink planted at the temp name.
            let mut out = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(tmp)?;
            io::copy(&mut File::open(src)?, &mut out)?;
            copy_attrs(&out, src_meta)
        }
    }
}

/// Places `src` at `target` with the first method in `chain` that works,
/// replacing whatever is there atomically (temp file + rename). Hardlink and
/// reflink are skipped when source and target folder differ in `st_dev`.
/// `shown` is the user-facing spelling of `target` (logs, fault injection).
pub(crate) fn place(
    src: &Path,
    target: &Path,
    shown: &Path,
    chain: &[Method],
    faults: &Faults,
) -> io::Result<Method> {
    let src_meta = fs::metadata(src)?;
    let dir = target.parent().unwrap_or(Path::new("/"));
    let same_dev = fs::metadata(dir)?.dev() == src_meta.dev();
    let tmp = tmp_path(target);
    let mut last = io::Error::other("no deploy method available");
    for &m in chain {
        if m.same_device_only() && !same_dev {
            continue;
        }
        if faults.deny.as_ref().is_some_and(|deny| deny(m, shown)) {
            last = io::Error::other(format!("{} denied (fault injection)", m.as_str()));
            continue;
        }
        let _ = fs::remove_file(&tmp);
        match try_method(m, src, &src_meta, &tmp).and_then(|()| fs::rename(&tmp, target)) {
            Ok(()) => return Ok(m),
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                tracing::debug!(method = m.as_str(), path = %shown.display(), error = %e, "deploy method failed");
                last = e;
            }
        }
    }
    Err(last)
}

/// What was recorded about a placement (all optional for unrecorded ones).
pub(crate) struct Placed<'a> {
    pub method: Option<Method>,
    pub source: &'a Path,
    pub inode: Option<i64>,
    pub size: Option<i64>,
    pub mtime_ns: Option<i64>,
}

fn same_content(path: &Path, source: &Path) -> bool {
    match (sha256_file(path), sha256_file(source)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// True when `meta` (lstat of `abs`) is still the file we placed, using the
/// legacy per-method checks (L1685-1688): hardlink = inode, copy = size and
/// mtime, symlink = link target. Without a record (crash before the record
/// was written), any method's placement of `source` counts.
pub(crate) fn is_ours(p: &Placed<'_>, abs: &Path, meta: &fs::Metadata) -> bool {
    let link_ok = || meta.is_symlink() && fs::read_link(abs).is_ok_and(|l| l == p.source);
    let src_ino = || fs::metadata(p.source).ok().map(|s| (s.dev(), s.ino()));
    match p.method {
        Some(Method::Symlink) => link_ok(),
        Some(Method::Hardlink) => {
            meta.is_file()
                && match p.inode {
                    Some(ino) => meta.ino() as i64 == ino,
                    None => src_ino() == Some((meta.dev(), meta.ino())),
                }
        }
        Some(Method::Copy | Method::Reflink) => {
            meta.is_file()
                && match (p.size, p.mtime_ns) {
                    (Some(s), Some(t)) => meta.size() as i64 == s && mtime_ns(meta) == t,
                    _ => same_content(abs, p.source),
                }
        }
        None => {
            link_ok()
                || (meta.is_file()
                    && (src_ino() == Some((meta.dev(), meta.ino())) || same_content(abs, p.source)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_of_sha256_matches_known_vector() {
        assert_eq!(
            hash_copy(&b"abc"[..], None).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn place_replaces_existing_file_atomically() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        fs::write(&src, "new").unwrap();
        fs::write(&dst, "old").unwrap();
        let m = place(&src, &dst, &dst, &[Method::Hardlink], &Faults::default()).unwrap();
        assert_eq!(m, Method::Hardlink);
        assert_eq!(fs::read_to_string(&dst).unwrap(), "new");
        assert!(!tmp_path(&dst).exists());
    }
}
