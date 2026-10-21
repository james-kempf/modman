// SPDX-License-Identifier: GPL-3.0-or-later
//! Paths confined to a target root. Every write into the game folder or the
//! Proton prefix goes through [`confine`]: the parent folder is opened by an
//! `openat` walk from the root with `O_NOFOLLOW`, so a symlinked folder (for
//! example `dosdevices/z:` -> `/`) can never redirect a write outside the
//! root, even if it appears after the plan was resolved. Path-based helpers
//! then work on `/proc/self/fd/<dirfd>/<name>`, which names exactly the opened
//! folder.

use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use rustix::fs::{mkdirat, openat, Mode, OFlags, CWD};
use rustix::io::Errno;

/// Why a relative path component is refused, if it is.
pub(crate) fn bad_component(c: &str) -> Option<&'static str> {
    match c {
        "" => Some("empty path component"),
        "." | ".." => Some("'.' and '..' are not allowed"),
        c if c.contains('\0') => Some("NUL byte in name"),
        _ => None,
    }
}

/// Splits a `/`-separated relative path into checked components. Empty
/// components (doubled or trailing slashes) are skipped; a leading `/` is
/// refused.
pub(crate) fn components(rel: &str) -> Result<Vec<&str>, &'static str> {
    if rel.starts_with('/') {
        return Err("absolute paths are not allowed");
    }
    rel.split('/')
        .filter(|c| !c.is_empty())
        .map(|c| bad_component(c).map_or(Ok(c), Err))
        .collect()
}

/// A path inside the root whose parent folder is held open.
#[derive(Debug)]
pub(crate) struct Confined {
    _dir: OwnedFd,
    /// `/proc/self/fd/<dirfd>/<name>`; valid while `self` lives.
    pub path: PathBuf,
}

const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// Opens the parent of `rel` under `root` without following any symlink
/// below the root. With `create_parents`, missing folders are created.
/// A symlinked folder fails with `ELOOP`, a file in the way with `ENOTDIR`.
pub(crate) fn confine(root: &Path, rel: &str, create_parents: bool) -> io::Result<Confined> {
    let comps =
        components(rel).map_err(|m| io::Error::new(io::ErrorKind::InvalidInput, m.to_string()))?;
    let Some((name, parents)) = comps.split_last() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty path"));
    };
    // The root itself is trusted (it may be a symlinked library folder).
    let mut dir = openat(
        CWD,
        root,
        DIR_FLAGS.difference(OFlags::NOFOLLOW),
        Mode::empty(),
    )?;
    for c in parents {
        dir = match openat(&dir, *c, DIR_FLAGS, Mode::empty()) {
            Err(Errno::NOENT) if create_parents => {
                match mkdirat(&dir, *c, Mode::from_raw_mode(0o755)) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(e) => return Err(e.into()),
                }
                openat(&dir, *c, DIR_FLAGS, Mode::empty())?
            }
            r => r?,
        };
    }
    let path = PathBuf::from(format!("/proc/self/fd/{}/{name}", dir.as_raw_fd()));
    Ok(Confined { _dir: dir, path })
}

/// True for the errors [`confine`] reports for a symlinked or blocked folder.
pub(crate) fn is_escape(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::InvalidInput
        || matches!(
            e.raw_os_error(),
            Some(n) if n == Errno::LOOP.raw_os_error() || n == Errno::NOTDIR.raw_os_error()
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn components_reject_traversal_absolute_and_nul() {
        assert_eq!(components("Data//a/").unwrap(), ["Data", "a"]);
        for bad in ["../x", "a/../b", "./a", "a/.", "/etc", "a\0b"] {
            assert!(components(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn confine_refuses_symlinked_folder_pointing_outside() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(root.join("real")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();

        let e = confine(&root, "link/victim.txt", false).unwrap_err();
        assert!(is_escape(&e), "{e}");
        let e = confine(&root, "link/sub/victim.txt", true).unwrap_err();
        assert!(is_escape(&e), "{e}");
        assert!(!outside.join("sub").exists());

        let c = confine(&root, "real/new/f.txt", true).unwrap();
        fs::write(&c.path, "x").unwrap();
        assert_eq!(
            fs::read_to_string(root.join("real/new/f.txt")).unwrap(),
            "x"
        );
    }
}
