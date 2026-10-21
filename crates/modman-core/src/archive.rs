// SPDX-License-Identifier: GPL-3.0-or-later
//! M3: zip, 7z, rar (and tar) extraction with hardened entry handling.
//!
//! Archives are untrusted. Every backend feeds one writer ([`Sink`]) that only
//! creates regular files and directories at sanitised relative paths. Entries
//! with `..`, absolute paths, symlinks, hardlinks or special files fail the
//! whole extraction (they are not silently skipped), the destination is
//! emptied on failure, and nothing is ever written outside it.
//!
//! zip and 7z are read in pure Rust (`zip`, `sevenz-rust2`). rar and tar are
//! read through the system libarchive, which `compress-tools` links behind the
//! default-on cargo feature `rar` (spikes.md §2). compress-tools does not
//! expose an entry's hardlink target, so the few libarchive calls needed are
//! declared here directly.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// Errors from this module.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ArchiveError {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("unrecognised archive format: {}", .0.display())]
    UnknownFormat(PathBuf),
    /// The extraction library for this format is not part of this build.
    #[error("{format} archives need {library}, which this build of modman does not include (cargo feature `rar`)")]
    MissingBackend {
        format: &'static str,
        library: &'static str,
    },
    #[error("unsafe path in archive entry {entry:?}: {reason}")]
    UnsafePath { entry: String, reason: &'static str },
    #[error("archive entry {entry:?} is a {kind}; links and special files are not allowed")]
    ForbiddenEntry { entry: String, kind: &'static str },
    #[error("{} is password-protected; encrypted archives are not supported", .0.display())]
    Encrypted(PathBuf),
    #[error("cannot read {}: {message}", archive.display())]
    Corrupt { archive: PathBuf, message: String },
    #[error("extraction destination {} exists and is not an empty directory", .0.display())]
    DestinationNotEmpty(PathBuf),
    /// A tree produced by extraction (or copied from a folder) is unsafe to stage.
    #[error("unsafe extracted tree at {}: {reason}", path.display())]
    UnsafeTree { path: PathBuf, reason: &'static str },
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> ArchiveError + '_ {
    move |source| ArchiveError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Supported archive formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    SevenZip,
    Rar,
    /// tar, optionally gzip/bzip2/xz/zstd compressed.
    Tar,
}

impl ArchiveKind {
    pub fn name(self) -> &'static str {
        match self {
            ArchiveKind::Zip => "zip",
            ArchiveKind::SevenZip => "7z",
            ArchiveKind::Rar => "rar",
            ArchiveKind::Tar => "tar",
        }
    }

    fn needs_libarchive(self) -> bool {
        matches!(self, ArchiveKind::Rar | ArchiveKind::Tar)
    }
}

/// Detects the archive kind from the file name, falling back to magic bytes
/// (legacy `archive_kind`).
pub fn detect_kind(path: &Path) -> Result<ArchiveKind, ArchiveError> {
    let lower = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    const TAR_EXTS: &[&str] = &[
        ".tar", ".tar.gz", ".tgz", ".tar.bz2", ".tbz2", ".tar.xz", ".txz", ".tar.zst",
    ];
    if lower.ends_with(".zip") {
        return Ok(ArchiveKind::Zip);
    }
    if lower.ends_with(".7z") {
        return Ok(ArchiveKind::SevenZip);
    }
    if lower.ends_with(".rar") {
        return Ok(ArchiveKind::Rar);
    }
    if TAR_EXTS.iter().any(|e| lower.ends_with(e)) {
        return Ok(ArchiveKind::Tar);
    }
    let mut head = Vec::with_capacity(512);
    File::open(path)
        .and_then(|f| f.take(512).read_to_end(&mut head))
        .map_err(io_err(path))?;
    kind_from_magic(&head).ok_or_else(|| ArchiveError::UnknownFormat(path.to_path_buf()))
}

fn kind_from_magic(head: &[u8]) -> Option<ArchiveKind> {
    let starts = |m: &[u8]| head.starts_with(m);
    if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") {
        Some(ArchiveKind::Zip)
    } else if starts(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        Some(ArchiveKind::SevenZip)
    } else if starts(b"Rar!\x1a\x07") {
        Some(ArchiveKind::Rar)
    } else if starts(&[0x1f, 0x8b])
        || starts(b"BZh")
        || starts(&[0xfd, b'7', b'z', b'X', b'Z', 0])
        || starts(&[0x28, 0xb5, 0x2f, 0xfd])
        || head.get(257..262) == Some(b"ustar")
    {
        Some(ArchiveKind::Tar)
    } else {
        None
    }
}

/// Result of a successful extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    /// `None` when the source was a directory (bundled collection folder).
    pub kind: Option<ArchiveKind>,
    pub files: u64,
    pub bytes: u64,
}

/// Whether this build can read rar/tar (libarchive via feature `rar`).
const LIBARCHIVE: bool = cfg!(feature = "rar");

/// Extracts `src` (an archive, or a directory to copy) into `dest`.
///
/// `dest` must be missing or an empty directory. On any error `dest` is left
/// empty. The finished tree is checked with [`validate_tree`].
pub fn extract(src: &Path, dest: &Path) -> Result<Extracted, ArchiveError> {
    extract_with(src, dest, LIBARCHIVE)
}

fn extract_with(src: &Path, dest: &Path, libarchive: bool) -> Result<Extracted, ArchiveError> {
    let is_dir = fs::metadata(src).map_err(io_err(src))?.is_dir();
    let kind = if is_dir {
        None
    } else {
        Some(detect_kind(src)?)
    };
    if let Some(k) = kind.filter(|k| k.needs_libarchive() && !libarchive) {
        return Err(ArchiveError::MissingBackend {
            format: k.name(),
            library: "libarchive",
        });
    }
    prepare_dest(dest)?;
    let mut sink = Sink {
        dest,
        files: 0,
        bytes: 0,
    };
    let result = match kind {
        None => copy_dir(src, &mut sink),
        Some(ArchiveKind::Zip) => extract_zip(src, &mut sink),
        Some(ArchiveKind::SevenZip) => extract_7z(src, &mut sink),
        #[cfg(feature = "rar")]
        Some(k @ (ArchiveKind::Rar | ArchiveKind::Tar)) => libarchive::extract(src, k, &mut sink),
        #[cfg(not(feature = "rar"))]
        Some(k @ (ArchiveKind::Rar | ArchiveKind::Tar)) => Err(ArchiveError::MissingBackend {
            format: k.name(),
            library: "libarchive",
        }),
    }
    .and_then(|()| validate_tree(dest));
    match result {
        Ok(()) => Ok(Extracted {
            kind,
            files: sink.files,
            bytes: sink.bytes,
        }),
        Err(e) => {
            // Best effort: the caller discards the work dir anyway.
            let _ = clear_dir(dest);
            Err(e)
        }
    }
}

fn prepare_dest(dest: &Path) -> Result<(), ArchiveError> {
    match fs::symlink_metadata(dest) {
        Ok(m) if m.is_dir() => {
            if fs::read_dir(dest).map_err(io_err(dest))?.next().is_some() {
                return Err(ArchiveError::DestinationNotEmpty(dest.to_path_buf()));
            }
            Ok(())
        }
        Ok(_) => Err(ArchiveError::DestinationNotEmpty(dest.to_path_buf())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(dest).map_err(io_err(dest))
        }
        Err(e) => Err(io_err(dest)(e)),
    }
}

fn clear_dir(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// Turns an archive entry name into a safe relative path.
///
/// `\` counts as a separator (Windows-authored archives). Rejects absolute
/// paths, drive prefixes, `..` components and NUL bytes. Returns `None` for
/// names that resolve to the archive root (`./`).
pub fn sanitize_entry_path(raw: &str) -> Result<Option<PathBuf>, ArchiveError> {
    let unsafe_path = |reason| ArchiveError::UnsafePath {
        entry: raw.to_string(),
        reason,
    };
    if raw.contains('\0') {
        return Err(unsafe_path("NUL byte in name"));
    }
    let name = raw.replace('\\', "/");
    let bytes = name.as_bytes();
    if name.starts_with('/') {
        return Err(unsafe_path("absolute path"));
    }
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(unsafe_path("drive-letter path"));
    }
    let mut out = PathBuf::new();
    for comp in name.split('/') {
        match comp {
            "" | "." => {}
            ".." => return Err(unsafe_path("parent directory component '..'")),
            c => out.push(c),
        }
    }
    Ok((!out.as_os_str().is_empty()).then_some(out))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryType {
    File,
    Dir,
    Symlink,
    /// Only libarchive formats (rar, tar) carry hard-link entries.
    #[cfg_attr(not(feature = "rar"), allow(dead_code))]
    Hardlink,
    Special,
}

const S_IFMT: u32 = 0o170_000;
const S_IFREG: u32 = 0o100_000;
const S_IFDIR: u32 = 0o040_000;
const S_IFLNK: u32 = 0o120_000;

/// Classifies a unix mode; a zero file-type field defers to `is_dir`.
fn type_from_mode(mode: u32, is_dir: bool) -> EntryType {
    match mode & S_IFMT {
        0 if is_dir => EntryType::Dir,
        0 | S_IFREG => EntryType::File,
        S_IFDIR => EntryType::Dir,
        S_IFLNK => EntryType::Symlink,
        _ => EntryType::Special,
    }
}

/// Checks type and name of one entry. `Ok(None)` means "nothing to write".
fn check_entry(name: &str, ty: EntryType) -> Result<Option<PathBuf>, ArchiveError> {
    let forbidden = |kind| ArchiveError::ForbiddenEntry {
        entry: name.to_string(),
        kind,
    };
    match ty {
        EntryType::Symlink => return Err(forbidden("symbolic link")),
        EntryType::Hardlink => return Err(forbidden("hard link")),
        EntryType::Special => return Err(forbidden("special file")),
        EntryType::File | EntryType::Dir => {}
    }
    let rel = sanitize_entry_path(name)?;
    if rel.is_none() && ty == EntryType::File {
        return Err(ArchiveError::UnsafePath {
            entry: name.to_string(),
            reason: "file entry without a name",
        });
    }
    Ok(rel)
}

/// The only code that writes extracted data. `dest` starts empty and no
/// links are ever created inside it, so joined paths cannot leave it;
/// `O_NOFOLLOW` is defence in depth.
struct Sink<'a> {
    dest: &'a Path,
    files: u64,
    bytes: u64,
}

impl Sink<'_> {
    fn dir(&mut self, rel: &Path) -> Result<(), ArchiveError> {
        let p = self.dest.join(rel);
        fs::create_dir_all(&p).map_err(io_err(&p))
    }

    fn file(&mut self, rel: &Path, data: &mut dyn Read, exec: bool) -> Result<(), ArchiveError> {
        let p = self.dest.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(if exec { 0o755 } else { 0o644 })
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(&p)
            .map_err(io_err(&p))?;
        let n = io::copy(data, &mut f).map_err(io_err(&p))?;
        self.files += 1;
        self.bytes += n;
        Ok(())
    }
}

fn extract_zip(src: &Path, sink: &mut Sink) -> Result<(), ArchiveError> {
    use zip::result::ZipError;
    let corrupt = |e: ZipError| match e {
        ZipError::UnsupportedArchive(m) if m == ZipError::PASSWORD_REQUIRED => {
            ArchiveError::Encrypted(src.to_path_buf())
        }
        ZipError::InvalidPassword => ArchiveError::Encrypted(src.to_path_buf()),
        e => ArchiveError::Corrupt {
            archive: src.to_path_buf(),
            message: e.to_string(),
        },
    };
    let file = File::open(src).map_err(io_err(src))?;
    let mut zip = zip::ZipArchive::new(file).map_err(corrupt)?;

    // Pass 1 (headers only): reject the archive before anything is written.
    let mut plan = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let e = zip.by_index_raw(i).map_err(corrupt)?;
        if e.encrypted() {
            return Err(ArchiveError::Encrypted(src.to_path_buf()));
        }
        let name = e.name().map_err(corrupt)?.into_owned();
        let mode = e.unix_mode().unwrap_or(0);
        let ty = type_from_mode(mode, e.is_dir());
        let ty = if ty == EntryType::File && e.is_dir() {
            EntryType::Dir
        } else {
            ty
        };
        plan.push((check_entry(&name, ty)?, ty, mode & 0o111 != 0));
    }
    // Pass 2: write.
    for (i, (rel, ty, exec)) in plan.into_iter().enumerate() {
        let Some(rel) = rel else { continue };
        if ty == EntryType::Dir {
            sink.dir(&rel)?;
        } else {
            let mut e = zip.by_index(i).map_err(corrupt)?;
            sink.file(&rel, &mut e, exec)?;
        }
    }
    Ok(())
}

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
/// p7zip/7-Zip: the high 16 bits hold a unix mode.
const FILE_ATTRIBUTE_UNIX_EXTENSION: u32 = 0x8000;

fn sevenz_entry_type(e: &sevenz_rust2::ArchiveEntry) -> (EntryType, bool) {
    if e.has_windows_attributes {
        let a = e.windows_attributes;
        if a & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return (EntryType::Symlink, false);
        }
        if a & FILE_ATTRIBUTE_UNIX_EXTENSION != 0 {
            let mode = a >> 16;
            return (type_from_mode(mode, e.is_directory), mode & 0o111 != 0);
        }
    }
    let ty = if e.is_directory {
        EntryType::Dir
    } else {
        EntryType::File
    };
    (ty, false)
}

fn extract_7z(src: &Path, sink: &mut Sink) -> Result<(), ArchiveError> {
    use sevenz_rust2::{ArchiveReader, Error as SzError, Password};
    let map = |e: SzError| match e {
        SzError::PasswordRequired | SzError::MaybeBadPassword(_) => {
            ArchiveError::Encrypted(src.to_path_buf())
        }
        e => ArchiveError::Corrupt {
            archive: src.to_path_buf(),
            message: e.to_string(),
        },
    };
    let mut reader = ArchiveReader::open(src, Password::empty()).map_err(map)?;
    // Pass 1 (headers only): reject the archive before anything is written.
    for e in &reader.archive().files {
        if !e.is_anti_item {
            check_entry(&e.name, sevenz_entry_type(e).0)?;
        }
    }
    let mut failure: Option<ArchiveError> = None;
    let res = reader.for_each_entries(|e, data| {
        if e.is_anti_item {
            return Ok(true);
        }
        let (ty, exec) = sevenz_entry_type(e);
        let step = check_entry(&e.name, ty).and_then(|rel| match rel {
            None => Ok(()),
            Some(rel) if ty == EntryType::Dir => sink.dir(&rel),
            Some(rel) => sink.file(&rel, data, exec),
        });
        match step {
            Ok(()) => Ok(true),
            Err(err) => {
                failure = Some(err);
                Ok(false)
            }
        }
    });
    if let Some(err) = failure {
        return Err(err);
    }
    res.map_err(map)
}

/// Copies a directory (bundled collection folder) through the same sink.
fn copy_dir(src: &Path, sink: &mut Sink) -> Result<(), ArchiveError> {
    for entry in walkdir::WalkDir::new(src)
        .min_depth(1)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.map_err(|e| {
            let p = e.path().unwrap_or(src).to_path_buf();
            ArchiveError::Io {
                path: p,
                source: e.into(),
            }
        })?;
        let rel = entry
            .path()
            .strip_prefix(src)
            .expect("walkdir yields paths under its root");
        let name = rel.to_string_lossy();
        let ft = entry.file_type();
        let ty = if ft.is_symlink() {
            EntryType::Symlink
        } else if ft.is_dir() {
            EntryType::Dir
        } else if ft.is_file() {
            EntryType::File
        } else {
            EntryType::Special
        };
        check_entry(&name, ty)?;
        if ty == EntryType::Dir {
            sink.dir(rel)?;
        } else {
            let mut f = File::open(entry.path()).map_err(io_err(entry.path()))?;
            let exec = f.metadata().map(|m| m.mode() & 0o111 != 0).unwrap_or(false);
            sink.file(rel, &mut f, exec)?;
        }
    }
    Ok(())
}

/// Checks a tree before it is staged: no symlinks, no hard links, no special
/// files, nothing that resolves outside `root`. Use it on any tree that was
/// not written by [`extract`] (it also runs at the end of `extract`).
pub fn validate_tree(root: &Path) -> Result<(), ArchiveError> {
    let unsafe_tree = |path: &Path, reason| ArchiveError::UnsafeTree {
        path: path.to_path_buf(),
        reason,
    };
    let meta = fs::symlink_metadata(root).map_err(io_err(root))?;
    if !meta.is_dir() {
        return Err(unsafe_tree(root, "root is not a directory"));
    }
    for entry in walkdir::WalkDir::new(root).min_depth(1).follow_links(false) {
        let entry = entry.map_err(|e| {
            let p = e.path().unwrap_or(root).to_path_buf();
            ArchiveError::Io {
                path: p,
                source: e.into(),
            }
        })?;
        let path = entry.path();
        let ft = entry.file_type();
        if ft.is_symlink() {
            return Err(unsafe_tree(path, "symbolic link"));
        }
        if !path.starts_with(root)
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(unsafe_tree(path, "path escapes the extraction root"));
        }
        if ft.is_file() {
            let m = entry.metadata().map_err(|e| ArchiveError::Io {
                path: path.to_path_buf(),
                source: e.into(),
            })?;
            if m.nlink() > 1 {
                return Err(unsafe_tree(path, "hard link"));
            }
        } else if !ft.is_dir() {
            return Err(unsafe_tree(path, "special file"));
        }
    }
    Ok(())
}

/// Minimal libarchive binding for rar and tar.
#[cfg(feature = "rar")]
mod libarchive {
    use super::{check_entry, ArchiveError, ArchiveKind, EntryType, Sink};
    use std::ffi::{c_char, c_int, c_void, CStr, CString};
    use std::io::{self, Read};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    // compress-tools' build script locates and links the system libarchive;
    // this import keeps that dependency tied to the `rar` feature.
    use compress_tools as _;

    #[repr(C)]
    struct RawArchive {
        _p: [u8; 0],
    }
    #[repr(C)]
    struct RawEntry {
        _p: [u8; 0],
    }

    const ARCHIVE_EOF: c_int = 1;
    const ARCHIVE_OK: c_int = 0;
    const ARCHIVE_WARN: c_int = -20;

    #[link(name = "archive")]
    extern "C" {
        fn archive_read_new() -> *mut RawArchive;
        fn archive_read_free(a: *mut RawArchive) -> c_int;
        fn archive_read_support_filter_all(a: *mut RawArchive) -> c_int;
        fn archive_read_support_format_rar(a: *mut RawArchive) -> c_int;
        fn archive_read_support_format_rar5(a: *mut RawArchive) -> c_int;
        fn archive_read_support_format_tar(a: *mut RawArchive) -> c_int;
        fn archive_read_open_filename(
            a: *mut RawArchive,
            name: *const c_char,
            block: usize,
        ) -> c_int;
        fn archive_read_next_header(a: *mut RawArchive, e: *mut *mut RawEntry) -> c_int;
        fn archive_read_data(a: *mut RawArchive, buf: *mut c_void, len: usize) -> isize;
        fn archive_error_string(a: *mut RawArchive) -> *const c_char;
        fn archive_entry_pathname(e: *mut RawEntry) -> *const c_char;
        fn archive_entry_pathname_utf8(e: *mut RawEntry) -> *const c_char;
        fn archive_entry_hardlink(e: *mut RawEntry) -> *const c_char;
        fn archive_entry_filetype(e: *mut RawEntry) -> u32;
        fn archive_entry_mode(e: *mut RawEntry) -> u32;
        fn archive_entry_is_encrypted(e: *mut RawEntry) -> c_int;
    }

    /// Owns an `archive*` read handle.
    struct Handle(*mut RawArchive);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the pointer came from archive_read_new and is freed once.
            unsafe { archive_read_free(self.0) };
        }
    }

    impl Handle {
        fn error(&self) -> String {
            // SAFETY: the handle is live; libarchive returns NULL or a C string it owns.
            unsafe {
                let p = archive_error_string(self.0);
                if p.is_null() {
                    "unknown libarchive error".into()
                } else {
                    CStr::from_ptr(p).to_string_lossy().into_owned()
                }
            }
        }
    }

    /// Streams the current entry's data.
    struct Data<'a>(&'a Handle);

    impl Read for Data<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            // SAFETY: buf is valid for buf.len() bytes; the handle is live.
            let n = unsafe { archive_read_data(self.0 .0, buf.as_mut_ptr().cast(), buf.len()) };
            if n < 0 {
                Err(io::Error::other(self.0.error()))
            } else {
                Ok(n as usize)
            }
        }
    }

    fn cstr_opt(p: *const c_char) -> Option<String> {
        // SAFETY: p is NULL or a NUL-terminated string owned by the entry.
        (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }

    pub(super) fn extract(
        src: &Path,
        kind: ArchiveKind,
        sink: &mut Sink,
    ) -> Result<(), ArchiveError> {
        let corrupt = |message: String| ArchiveError::Corrupt {
            archive: src.to_path_buf(),
            message,
        };
        let cpath = CString::new(src.as_os_str().as_bytes())
            .map_err(|_| corrupt("path contains a NUL byte".into()))?;
        // SAFETY: archive_read_new has no preconditions; NULL is checked.
        let raw = unsafe { archive_read_new() };
        if raw.is_null() {
            return Err(corrupt("libarchive: out of memory".into()));
        }
        let h = Handle(raw);
        // SAFETY: h.0 is a fresh read handle; cpath outlives the call.
        let rc = unsafe {
            archive_read_support_filter_all(h.0);
            match kind {
                ArchiveKind::Rar => {
                    archive_read_support_format_rar(h.0);
                    archive_read_support_format_rar5(h.0);
                }
                _ => {
                    archive_read_support_format_tar(h.0);
                }
            }
            archive_read_open_filename(h.0, cpath.as_ptr(), 64 * 1024)
        };
        if rc != ARCHIVE_OK {
            return Err(corrupt(h.error()));
        }
        loop {
            let mut e: *mut RawEntry = std::ptr::null_mut();
            // SAFETY: h is open; e receives an entry owned by h.
            let rc = unsafe { archive_read_next_header(h.0, &mut e) };
            if rc == ARCHIVE_EOF {
                return Ok(());
            }
            if rc != ARCHIVE_OK && rc != ARCHIVE_WARN {
                return Err(corrupt(h.error()));
            }
            // SAFETY: e is valid until the next archive_read_next_header call.
            let (name, hardlink, filetype, mode, encrypted) = unsafe {
                (
                    cstr_opt(archive_entry_pathname_utf8(e))
                        .or_else(|| cstr_opt(archive_entry_pathname(e)))
                        .unwrap_or_default(),
                    !archive_entry_hardlink(e).is_null(),
                    archive_entry_filetype(e),
                    archive_entry_mode(e),
                    archive_entry_is_encrypted(e) != 0,
                )
            };
            let ty = if hardlink {
                EntryType::Hardlink
            } else if filetype == 0 {
                EntryType::Special
            } else {
                super::type_from_mode(filetype, false)
            };
            let rel = check_entry(&name, ty)?;
            if encrypted {
                return Err(ArchiveError::Encrypted(src.to_path_buf()));
            }
            let Some(rel) = rel else { continue };
            if ty == EntryType::Dir {
                sink.dir(&rel)?;
            } else {
                sink.file(&rel, &mut Data(&h), mode & 0o111 != 0)
                    .map_err(|err| match err {
                        // A read error is a broken archive, not a disk problem.
                        ArchiveError::Io { source, .. }
                            if source.kind() == io::ErrorKind::Other =>
                        {
                            corrupt(source.to_string())
                        }
                        other => other,
                    })?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::symlink;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/archive")
            .join(name)
    }

    /// Lists every path under `root` as '/'-joined relative strings.
    fn listing(root: &Path) -> Vec<String> {
        let mut v: Vec<String> = walkdir::WalkDir::new(root)
            .min_depth(1)
            .into_iter()
            .map(|e| {
                let e = e.unwrap();
                let rel = e
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                if e.file_type().is_dir() {
                    format!("{rel}/")
                } else {
                    rel
                }
            })
            .collect();
        v.sort();
        v
    }

    /// Temp layout: `<tmp>/out` is the destination, `<tmp>` is "outside".
    fn scratch() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let out = t.path().join("out");
        (t, out)
    }

    fn outside_untouched(t: &tempfile::TempDir) {
        let names: Vec<String> = fs::read_dir(t.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != "out" && !n.ends_with(".zip") && !n.ends_with(".7z"))
            .collect();
        assert!(names.is_empty(), "written outside dest: {names:?}");
    }

    enum ZipEntry<'a> {
        File(&'a str, &'a [u8]),
        Dir(&'a str),
        Symlink(&'a str, &'a str),
        RawMode(&'a str, u32),
    }

    fn make_zip(path: &Path, entries: &[ZipEntry]) {
        use zip::write::SimpleFileOptions;
        let mut w = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = SimpleFileOptions::default();
        let mut pending_mode = None;
        for e in entries {
            match e {
                ZipEntry::File(n, d) => {
                    w.start_file(*n, opts).unwrap();
                    w.write_all(d).unwrap();
                }
                ZipEntry::Dir(n) => {
                    w.add_directory(*n, opts).unwrap();
                }
                ZipEntry::Symlink(n, t) => {
                    w.add_symlink(*n, *t, opts).unwrap();
                }
                ZipEntry::RawMode(n, mode) => {
                    w.start_file(*n, opts).unwrap();
                    w.write_all(b"x").unwrap();
                    pending_mode = Some(*mode);
                }
            }
        }
        w.finish().unwrap();
        // The writer masks unix_permissions to permission bits, so a raw file
        // type (FIFO, device) is patched into the last central-directory
        // record's external attributes.
        if let Some(mode) = pending_mode {
            let mut bytes = fs::read(path).unwrap();
            let cd = bytes.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
            bytes[cd + 5] = 3; // version made by: unix
            bytes[cd + 38..cd + 42].copy_from_slice(&(mode << 16).to_le_bytes());
            fs::write(path, bytes).unwrap();
        }
    }

    fn make_7z(path: &Path, entries: &[(&str, &[u8], Option<u32>)]) {
        let mut w = sevenz_rust2::ArchiveWriter::create(path).unwrap();
        for (name, data, attrs) in entries {
            let mut e = sevenz_rust2::ArchiveEntry::new_file(name);
            if let Some(a) = attrs {
                e.has_windows_attributes = true;
                e.windows_attributes = *a;
            }
            w.push_archive_entry(e, Some(*data)).unwrap();
        }
        w.finish().unwrap();
    }

    // ---- kind detection ----

    #[test]
    fn detect_kind_uses_extension_then_magic() {
        assert_eq!(
            detect_kind(&fixture("normal.7z")).unwrap(),
            ArchiveKind::SevenZip
        );
        assert_eq!(
            detect_kind(&fixture("rar5_normal.rar")).unwrap(),
            ArchiveKind::Rar
        );
        assert_eq!(
            detect_kind(&fixture("tar_normal.tar.gz")).unwrap(),
            ArchiveKind::Tar
        );
        let t = tempfile::tempdir().unwrap();
        for (src, kind) in [
            ("normal.7z", ArchiveKind::SevenZip),
            ("rar5_normal.rar", ArchiveKind::Rar),
            ("tar_normal.tar.gz", ArchiveKind::Tar),
        ] {
            let p = t.path().join(format!("download-{}", kind.name()));
            fs::copy(fixture(src), &p).unwrap();
            assert_eq!(detect_kind(&p).unwrap(), kind, "magic for {src}");
        }
        let z = t.path().join("noext");
        make_zip(&z, &[ZipEntry::File("a.txt", b"a")]);
        assert_eq!(detect_kind(&z).unwrap(), ArchiveKind::Zip);
        let junk = t.path().join("junk.bin");
        fs::write(&junk, b"not an archive").unwrap();
        assert!(matches!(
            detect_kind(&junk),
            Err(ArchiveError::UnknownFormat(_))
        ));
    }

    // ---- happy paths ----

    #[test]
    fn extracts_zip_fixture() {
        let (t, out) = scratch();
        let z = t.path().join("m.zip");
        make_zip(
            &z,
            &[
                ZipEntry::Dir("Mod/"),
                ZipEntry::File("Mod/textures/a.dds", b"tex"),
                ZipEntry::File("Mod\\meshes\\b.nif", b"nif"),
                ZipEntry::File("./readme.txt", b"hi"),
            ],
        );
        let r = extract(&z, &out).unwrap();
        assert_eq!(r.kind, Some(ArchiveKind::Zip));
        assert_eq!(r.files, 3);
        assert_eq!(
            listing(&out),
            [
                "Mod/",
                "Mod/meshes/",
                "Mod/meshes/b.nif",
                "Mod/textures/",
                "Mod/textures/a.dds",
                "readme.txt"
            ]
        );
        assert_eq!(fs::read(out.join("Mod/textures/a.dds")).unwrap(), b"tex");
    }

    #[test]
    fn extracts_7z_fixture() {
        let (_t, out) = scratch();
        let r = extract(&fixture("normal.7z"), &out).unwrap();
        assert_eq!(r.kind, Some(ArchiveKind::SevenZip));
        assert_eq!(
            listing(&out),
            [
                "Mod/",
                "Mod/readme.txt",
                "Mod/textures/",
                "Mod/textures/a.dds"
            ]
        );
        assert_eq!(fs::read(out.join("Mod/textures/a.dds")).unwrap(), b"tex\n");
    }

    #[cfg(feature = "rar")]
    #[test]
    fn extracts_rar_fixture() {
        let (_t, out) = scratch();
        let r = extract(&fixture("rar5_normal.rar"), &out).unwrap();
        assert_eq!(r.kind, Some(ArchiveKind::Rar));
        assert_eq!(listing(&out), ["helloworld.txt"]);
        assert_eq!(
            fs::read(out.join("helloworld.txt")).unwrap(),
            b"hello libarchive test suite!\n"
        );
    }

    #[cfg(feature = "rar")]
    #[test]
    fn extracts_tar_fixture() {
        let (_t, out) = scratch();
        extract(&fixture("tar_normal.tar.gz"), &out).unwrap();
        assert_eq!(listing(&out), ["Mod/", "Mod/Data/", "Mod/Data/a.esp"]);
    }

    #[test]
    fn copies_a_directory_source() {
        let (t, out) = scratch();
        let src = t.path().join("bundle");
        fs::create_dir_all(src.join("Data")).unwrap();
        fs::write(src.join("Data/a.esp"), b"x").unwrap();
        let r = extract(&src, &out).unwrap();
        assert_eq!(r.kind, None);
        assert_eq!(listing(&out), ["Data/", "Data/a.esp"]);
    }

    #[test]
    fn refuses_non_empty_destination() {
        let (t, out) = scratch();
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("keep"), b"k").unwrap();
        let z = t.path().join("m.zip");
        make_zip(&z, &[ZipEntry::File("a", b"a")]);
        assert!(matches!(
            extract(&z, &out),
            Err(ArchiveError::DestinationNotEmpty(_))
        ));
        assert_eq!(fs::read(out.join("keep")).unwrap(), b"k");
    }

    // ---- traversal and absolute paths ----

    #[test]
    fn sanitize_entry_path_rules() {
        assert_eq!(
            sanitize_entry_path("a\\b/./c").unwrap().unwrap(),
            Path::new("a/b/c")
        );
        assert_eq!(sanitize_entry_path("./").unwrap(), None);
        for bad in [
            "../x",
            "a/../../x",
            "a\\..\\x",
            "/etc/passwd",
            "\\\\srv\\x",
            "C:\\x",
            "c:x",
            "a\0b",
        ] {
            assert!(
                matches!(
                    sanitize_entry_path(bad),
                    Err(ArchiveError::UnsafePath { .. })
                ),
                "{bad:?} accepted"
            );
        }
    }

    #[test]
    fn zip_rejects_dotdot_and_absolute_entries() {
        for bad in [
            "../evil.txt",
            "ok/../../evil.txt",
            "..\\evil.txt",
            "/tmp/evil.txt",
        ] {
            let (t, out) = scratch();
            let z = t.path().join("bad.zip");
            make_zip(
                &z,
                &[
                    ZipEntry::File("good.txt", b"g"),
                    ZipEntry::File(bad, b"evil"),
                ],
            );
            let err = extract(&z, &out).unwrap_err();
            assert!(
                matches!(err, ArchiveError::UnsafePath { .. }),
                "{bad}: {err}"
            );
            assert!(listing(&out).is_empty(), "{bad}: partial output left");
            outside_untouched(&t);
        }
    }

    #[test]
    fn sevenz_rejects_dotdot_and_absolute_entries() {
        for bad in [
            "../evil.txt",
            "a/../../evil.txt",
            "/tmp/evil.txt",
            "C:\\evil.txt",
        ] {
            let (t, out) = scratch();
            let a = t.path().join("bad.7z");
            make_7z(&a, &[("good.txt", b"g", None), (bad, b"evil", None)]);
            let err = extract(&a, &out).unwrap_err();
            assert!(
                matches!(err, ArchiveError::UnsafePath { .. }),
                "{bad}: {err}"
            );
            assert!(listing(&out).is_empty());
            outside_untouched(&t);
        }
    }

    #[cfg(feature = "rar")]
    #[test]
    fn rar_rejects_dotdot_and_absolute_entries() {
        for f in ["rar5_traversal.rar", "rar5_absolute.rar"] {
            let (t, out) = scratch();
            let err = extract(&fixture(f), &out).unwrap_err();
            assert!(matches!(err, ArchiveError::UnsafePath { .. }), "{f}: {err}");
            assert!(listing(&out).is_empty());
            assert!(!t.path().join("loworld.txt").exists());
            outside_untouched(&t);
        }
    }

    // ---- links and special files ----

    #[test]
    fn zip_rejects_symlink_and_special_entries() {
        let (t, out) = scratch();
        let z = t.path().join("link.zip");
        make_zip(
            &z,
            &[
                ZipEntry::File("a.txt", b"a"),
                ZipEntry::Symlink("link", "../../etc/passwd"),
            ],
        );
        let err = extract(&z, &out).unwrap_err();
        assert!(
            matches!(&err, ArchiveError::ForbiddenEntry { kind, .. } if *kind == "symbolic link"),
            "{err}"
        );
        assert!(listing(&out).is_empty());
        outside_untouched(&t);

        // A FIFO mode (zip cannot carry hard links; other non-regular types
        // are rejected as special files).
        let (t, out) = scratch();
        let z = t.path().join("fifo.zip");
        make_zip(&z, &[ZipEntry::RawMode("fifo", 0o010_644)]);
        let err = extract(&z, &out).unwrap_err();
        assert!(matches!(err, ArchiveError::ForbiddenEntry { .. }), "{err}");
        outside_untouched(&t);
    }

    #[test]
    fn sevenz_rejects_symlink_entries() {
        // Real p7zip `7z a -snl` output.
        let (t, out) = scratch();
        let err = extract(&fixture("symlink.7z"), &out).unwrap_err();
        assert!(
            matches!(&err, ArchiveError::ForbiddenEntry { kind, .. } if *kind == "symbolic link"),
            "{err}"
        );
        assert!(listing(&out).is_empty());
        outside_untouched(&t);

        // Windows reparse point (symlink/junction) attribute.
        let (t, out) = scratch();
        let a = t.path().join("reparse.7z");
        make_7z(&a, &[("junction", b"", Some(FILE_ATTRIBUTE_REPARSE_POINT))]);
        assert!(matches!(
            extract(&a, &out),
            Err(ArchiveError::ForbiddenEntry { .. })
        ));
        outside_untouched(&t);
    }

    #[test]
    fn sevenz_hardlinks_extract_as_independent_files() {
        // The 7z format has no hard-link entry type: `7z a -snh` stores both
        // names as regular files. Prove extraction never creates a link.
        let (_t, out) = scratch();
        extract(&fixture("hardlink.7z"), &out).unwrap();
        for n in ["Mod/readme.txt", "Mod/hard.txt"] {
            let m = fs::symlink_metadata(out.join(n)).unwrap();
            assert!(m.is_file());
            assert_eq!(m.nlink(), 1, "{n}");
        }
    }

    #[cfg(feature = "rar")]
    #[test]
    fn rar_rejects_symlink_entries() {
        for f in ["rar5_symlink.rar", "rar4_symlink.rar"] {
            let (t, out) = scratch();
            let err = extract(&fixture(f), &out).unwrap_err();
            assert!(
                matches!(&err, ArchiveError::ForbiddenEntry { kind, .. } if *kind == "symbolic link"),
                "{f}: {err}"
            );
            assert!(listing(&out).is_empty(), "{f}: partial output left");
            outside_untouched(&t);
        }
    }

    #[cfg(feature = "rar")]
    #[test]
    fn rar_rejects_hardlink_entries() {
        let (t, out) = scratch();
        let err = extract(&fixture("rar5_hardlink.rar"), &out).unwrap_err();
        assert!(
            matches!(&err, ArchiveError::ForbiddenEntry { kind, .. } if *kind == "hard link"),
            "{err}"
        );
        assert!(listing(&out).is_empty());
        outside_untouched(&t);
    }

    #[cfg(feature = "rar")]
    #[test]
    fn tar_rejects_hardlink_entries() {
        let (_t, out) = scratch();
        let err = extract(&fixture("tar_hardlink.tar.gz"), &out).unwrap_err();
        assert!(matches!(&err, ArchiveError::ForbiddenEntry { kind, .. } if *kind == "hard link"));
    }

    #[test]
    fn directory_source_with_symlink_is_rejected() {
        let (t, out) = scratch();
        let src = t.path().join("bundle");
        fs::create_dir_all(&src).unwrap();
        symlink("/etc", src.join("etc")).unwrap();
        assert!(matches!(
            extract(&src, &out),
            Err(ArchiveError::ForbiddenEntry { .. })
        ));
        assert!(listing(&out).is_empty());
    }

    // ---- post-extraction validator ----

    #[test]
    fn validate_tree_rejects_symlinks_escapes_and_hardlinks() {
        let t = tempfile::tempdir().unwrap();
        let outside = t.path().join("secret");
        fs::write(&outside, b"s").unwrap();

        let tree = |name: &str| {
            let r = t.path().join(name);
            fs::create_dir_all(r.join("Data/textures")).unwrap();
            fs::write(r.join("Data/textures/a.dds"), b"a").unwrap();
            r
        };

        let clean = tree("clean");
        validate_tree(&clean).unwrap();

        let escaping = tree("escaping");
        symlink("../../secret", escaping.join("Data/escape")).unwrap();
        let err = validate_tree(&escaping).unwrap_err();
        assert!(matches!(err, ArchiveError::UnsafeTree { .. }), "{err}");

        let inner = tree("inner");
        symlink("textures/a.dds", inner.join("Data/alias.dds")).unwrap();
        assert!(matches!(
            validate_tree(&inner),
            Err(ArchiveError::UnsafeTree { .. })
        ));

        let dirlink = tree("dirlink");
        symlink(t.path(), dirlink.join("up")).unwrap();
        assert!(matches!(
            validate_tree(&dirlink),
            Err(ArchiveError::UnsafeTree { .. })
        ));

        let hard = tree("hard");
        fs::hard_link(&outside, hard.join("Data/secret")).unwrap();
        let err = validate_tree(&hard).unwrap_err();
        assert!(
            matches!(&err, ArchiveError::UnsafeTree { reason, .. } if *reason == "hard link"),
            "{err}"
        );

        let root_link = t.path().join("rootlink");
        symlink(&clean, &root_link).unwrap();
        assert!(matches!(
            validate_tree(&root_link),
            Err(ArchiveError::UnsafeTree { .. })
        ));
        assert_eq!(fs::read(&outside).unwrap(), b"s");
    }

    // ---- missing backend ----

    #[test]
    fn missing_libarchive_is_a_typed_error_naming_it() {
        for f in ["rar5_normal.rar", "tar_normal.tar.gz"] {
            let (_t, out) = scratch();
            let err = extract_with(&fixture(f), &out, false).unwrap_err();
            match &err {
                ArchiveError::MissingBackend { library, .. } => assert_eq!(*library, "libarchive"),
                other => panic!("{f}: {other}"),
            }
            assert!(err.to_string().contains("libarchive"));
            assert!(!out.exists(), "dest must not be created");
        }
        // zip and 7z never need it.
        let (_t, out) = scratch();
        extract_with(&fixture("normal.7z"), &out, false).unwrap();
    }

    #[test]
    fn encrypted_zip_is_reported() {
        let (t, out) = scratch();
        let z = t.path().join("enc.zip");
        // ZipCrypto flag (bit 0) set on an otherwise plain entry.
        make_zip(&z, &[ZipEntry::File("a.txt", b"a")]);
        let mut bytes = fs::read(&z).unwrap();
        let cd = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        bytes[cd + 8] |= 1;
        bytes[6] |= 1;
        fs::write(&z, bytes).unwrap();
        assert!(matches!(extract(&z, &out), Err(ArchiveError::Encrypted(_))));
    }
}
