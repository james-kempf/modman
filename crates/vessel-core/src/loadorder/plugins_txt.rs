// SPDX-License-Identifier: GPL-3.0-or-later
//! plugins.txt in the Proton prefix (legacy_spec §10; `plugins_update`
//! L2053-2101, `plugins_remove_managed` L2103-2120).
//!
//! The file is reached through [`confine`]: the folders below the prefix are
//! opened without following symlinks, the file itself is read with
//! `O_NOFOLLOW`, and it is replaced by a temp file + rename, so a planted
//! symlink can never redirect the write. Lines are handled as bytes, so names
//! in the game's ANSI code page survive a rewrite.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use rustix::fs::{openat, Mode, OFlags, CWD};
use rustix::io::Errno;
use serde::Serialize;

use super::loot::{cached_masterlist, loot_game_type, sort_with_loot};
use super::rules::apply_rules;
use super::{io_err, LoadOrderError};
use crate::collections::{plugin_rules, PluginRule};
use crate::deploy::confine::{components, confine, is_escape, Confined};
use crate::games::{GameConfig, PluginsFormat};

/// Header lines written by [`update_plugins`] (legacy L2091-2092).
const HEADER: [&str; 2] = [
    "# This file is used by the game to keep track of your downloaded content.",
    "# Please do not modify this file. (entries marked by vessel are removed on purge)",
];

/// vessel's bookkeeping files for one game (legacy `GDATA`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginsFiles {
    /// Per-game data folder, `<data-dir>/<domain>`.
    pub game_data: PathBuf,
}

impl PluginsFiles {
    pub fn new(game_data: impl Into<PathBuf>) -> Self {
        Self {
            game_data: game_data.into(),
        }
    }

    /// Names vessel appended on the last deploy, one per line.
    pub fn managed_path(&self) -> PathBuf {
        self.game_data.join("plugins.managed")
    }

    /// The user's plugins.txt as it was before vessel first wrote it.
    pub fn orig_path(&self) -> PathBuf {
        self.game_data.join("plugins.txt.orig")
    }

    /// Marker: plugins.txt did not exist before vessel first wrote it.
    pub fn absent_path(&self) -> PathBuf {
        self.game_data.join("plugins.txt.absent")
    }

    /// Records the pre-deploy state once: a byte copy of the file, or the
    /// absent marker. Later deploys see vessel's own file and must not
    /// record it as the original.
    fn record_original(&self, raw: Option<&[u8]>) -> Result<(), LoadOrderError> {
        let (orig, absent) = (self.orig_path(), self.absent_path());
        if orig.exists() || absent.exists() {
            return Ok(());
        }
        fs::create_dir_all(&self.game_data).map_err(io_err(&self.game_data))?;
        match raw {
            Some(raw) => fs::write(&orig, raw).map_err(io_err(&orig)),
            None => fs::write(&absent, b"").map_err(io_err(&absent)),
        }
    }

    /// Drops all bookkeeping after a purge. The managed list goes first: a
    /// leftover backup makes the next purge redo the (idempotent) restore,
    /// while a leftover managed list without the backup would rewrite the
    /// restored file.
    fn clear(&self) -> Result<(), LoadOrderError> {
        for p in [self.managed_path(), self.absent_path(), self.orig_path()] {
            match fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_err(&p)(e)),
            }
        }
        Ok(())
    }

    fn read_managed(&self) -> Result<Vec<String>, LoadOrderError> {
        let p = self.managed_path();
        match fs::read(&p) {
            Ok(b) => Ok(String::from_utf8_lossy(&b)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(io_err(&p)(e)),
        }
    }

    fn write_managed(&self, names: &[String]) -> Result<(), LoadOrderError> {
        let p = self.managed_path();
        fs::create_dir_all(&self.game_data).map_err(io_err(&self.game_data))?;
        let mut body = String::new();
        for n in names {
            body.push_str(n);
            body.push('\n');
        }
        let tmp = p.with_extension("managed.tmp");
        let mut f = fs::File::create(&tmp).map_err(io_err(&tmp))?;
        f.write_all(body.as_bytes()).map_err(io_err(&tmp))?;
        f.sync_all().map_err(io_err(&tmp))?;
        fs::rename(&tmp, &p).map_err(io_err(&p))
    }
}

/// Why the plugin stage did nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "path", rename_all = "snake_case")]
pub enum SkipReason {
    /// Not a bethesda-profile game with a plugins format.
    NotBethesda,
    /// No Proton prefix / AppData folder known and no `plugins-file` set.
    NoPluginsFile,
    /// The plugins.txt folder does not exist yet (launch the game once).
    FolderMissing(PathBuf),
}

/// How the appended plugins were ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SortMethod {
    /// libloot, with the cached masterlist when `masterlist` is true.
    Loot { masterlist: bool },
    /// Mod priority (masters first) plus collection rules; LOOT was not
    /// available or failed (see the warnings).
    Rules,
}

/// Result of a plugins.txt update or cleanup.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PluginsReport {
    pub path: PathBuf,
    /// Plugins found in enabled mods.
    pub provided: usize,
    /// Names appended (now managed), in load order.
    pub added: Vec<String>,
    /// Previously managed names taken out.
    pub removed: Vec<String>,
    /// The file was rewritten.
    pub written: bool,
    pub sorted_by: Option<SortMethod>,
    pub warnings: Vec<String>,
}

/// Outcome of the plugin stage of deploy or purge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum PluginStage {
    Skipped(SkipReason),
    Updated(PluginsReport),
    /// The stage failed; deploy/purge itself succeeded.
    Failed(String),
}

/// One entry of plugins.txt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginLine {
    pub name: String,
    pub active: bool,
    /// Added by vessel on the last deploy.
    pub managed: bool,
}

/// `.esm`, `.esl` or `.esp` (any case).
pub fn is_plugin_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".esm") || l.ends_with(".esl") || l.ends_with(".esp")
}

fn is_master_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".esm") || l.ends_with(".esl")
}

/// The plugin stage applies: bethesda profile with a plugins format
/// (legacy `plugins_file`, L2048).
pub fn plugins_applies(game: &GameConfig) -> bool {
    game.profile == "bethesda" && game.plugins_format.is_some()
}

/// Where plugins.txt is: (trusted root, relative path below it, full path).
fn locate(game: &GameConfig) -> Option<(PathBuf, String, PathBuf)> {
    if let Some(p) = &game.plugins_file {
        let root = p.parent()?.to_path_buf();
        let name = p.file_name()?.to_str()?.to_string();
        return Some((root, name, p.clone()));
    }
    let full = game.default_plugins_file()?;
    let folder = game.appdata_folder.as_deref()?;
    components(folder).ok()?;
    let rel = format!("drive_c/users/steamuser/AppData/Local/{folder}/plugins.txt");
    Some((game.prefix.clone()?, rel, full))
}

enum Opened {
    Ready(Confined),
    FolderMissing,
}

fn open_dir(root: &Path, rel: &str, full: &Path) -> Result<Opened, LoadOrderError> {
    match confine(root, rel, false) {
        Ok(c) => Ok(Opened::Ready(c)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Opened::FolderMissing),
        Err(e) if is_escape(&e) => Err(LoadOrderError::Symlink(full.to_path_buf())),
        Err(e) => Err(io_err(full)(e)),
    }
}

/// Reads the file without following a symlink at its name.
pub(super) fn read_nofollow(c: &Confined, full: &Path) -> Result<Option<Vec<u8>>, LoadOrderError> {
    let fd = match openat(
        CWD,
        &c.path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => return Ok(None),
        Err(Errno::LOOP) => return Err(LoadOrderError::Symlink(full.to_path_buf())),
        Err(e) => return Err(io_err(full)(e.into())),
    };
    let mut buf = Vec::new();
    fs::File::from(fd)
        .read_to_end(&mut buf)
        .map_err(io_err(full))?;
    Ok(Some(buf))
}

/// Replaces the file atomically: `plugins.txt.vessel.tmp` + rename (legacy
/// L2090-2098). The temp file is created with `O_EXCL | O_NOFOLLOW`.
fn write_atomic(c: &Confined, full: &Path, lines: &[Vec<u8>]) -> Result<(), LoadOrderError> {
    let mut body = Vec::new();
    for l in lines {
        body.extend_from_slice(l);
        body.extend_from_slice(b"\r\n");
    }
    write_bytes_atomic(c, full, &body)
}

/// [`write_atomic`] for exact bytes (restoring the pre-deploy original).
pub(super) fn write_bytes_atomic(
    c: &Confined,
    full: &Path,
    body: &[u8],
) -> Result<(), LoadOrderError> {
    let name = c
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = c.path.with_file_name(format!("{name}.vessel.tmp"));
    let _ = fs::remove_file(&tmp);
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .custom_flags(libc_nofollow())
        .open(&tmp)
        .map_err(io_err(full))?;
    f.write_all(body).map_err(io_err(full))?;
    f.sync_all().map_err(io_err(full))?;
    drop(f);
    fs::rename(&tmp, &c.path).map_err(io_err(full))
}

fn libc_nofollow() -> i32 {
    OFlags::NOFOLLOW.bits() as i32
}

/// Lines without the trailing CR; the empty piece after a final newline is
/// dropped.
fn split_lines(raw: &[u8]) -> Vec<Vec<u8>> {
    if raw.is_empty() {
        return Vec::new();
    }
    let mut v: Vec<Vec<u8>> = raw
        .split(|b| *b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec())
        .collect();
    if raw.ends_with(b"\n") {
        v.pop();
    }
    v
}

/// Case-insensitive key of a line's plugin name (`*` stripped).
fn line_key(line: &[u8]) -> String {
    let n = line.strip_prefix(b"*").unwrap_or(line);
    String::from_utf8_lossy(n).to_lowercase()
}

fn is_entry(line: &[u8]) -> bool {
    !line.is_empty() && !line.starts_with(b"#")
}

/// Deploy folder name for `root = game` mods (legacy `deploy_base_rel`).
fn base_dir(game: &GameConfig) -> String {
    game.subdir
        .trim_start_matches("./")
        .trim_matches('/')
        .to_string()
}

/// Plugins at the top level of each enabled mod's staged deploy folder, in
/// mod priority order, de-duplicated case-insensitively (legacy L2064-2074).
fn collect_ours(conn: &Connection, game: &GameConfig) -> Result<Vec<String>, LoadOrderError> {
    let mut stmt = conn.prepare(
        "SELECT root, staging_dir FROM mods WHERE game = ?1 AND enabled = 1
         ORDER BY priority, id",
    )?;
    let rows = stmt
        .query_map([&game.domain], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let base = base_dir(game);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (root, staging) in rows {
        let Some(dir) = staging.filter(|s| !s.is_empty()).map(PathBuf::from) else {
            continue;
        };
        let pdir = if root == "game" && !base.is_empty() && base != "." {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            let found = rd.flatten().find(|e| {
                e.file_type().is_ok_and(|t| t.is_dir())
                    && e.file_name().to_string_lossy().eq_ignore_ascii_case(&base)
            });
            match found {
                Some(e) => e.path(),
                None => continue,
            }
        } else {
            dir
        };
        let Ok(rd) = fs::read_dir(&pdir) else {
            continue;
        };
        let mut names: Vec<String> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| is_plugin_name(n))
            .collect();
        names.sort();
        for n in names {
            if seen.insert(n.to_lowercase()) {
                out.push(n);
            }
        }
    }
    Ok(out)
}

/// Orders the appended plugins: collection rules, then LOOT when the game
/// type is supported and the plugins can be read from the game folder.
fn sort_added(
    game: &GameConfig,
    files: &PluginsFiles,
    local_dir: &Path,
    added: Vec<String>,
    rules: &[PluginRule],
    warnings: &mut Vec<String>,
) -> (Vec<String>, SortMethod) {
    let pre = apply_rules(&added, rules);
    let (Some(gt), Some(folder)) = (loot_game_type(game), game.folder.as_deref()) else {
        return (pre, SortMethod::Rules);
    };
    if pre.len() < 2 {
        return (pre, SortMethod::Loot { masterlist: false });
    }
    let ml = cached_masterlist(&files.game_data);
    let ml_ref = ml.as_ref().map(|(m, p)| (m.as_path(), p.as_deref()));
    match sort_with_loot(gt, folder, local_dir, &pre, ml_ref, rules) {
        Ok(v) => (
            v,
            SortMethod::Loot {
                masterlist: ml.is_some(),
            },
        ),
        Err(e) => {
            warnings.push(format!("LOOT sort failed, kept mod order: {e}"));
            (pre, SortMethod::Rules)
        }
    }
}

/// After deploy: drops vessel's previous entries, appends the plugins of
/// enabled mods not listed yet (masters first, then LOOT/rules order) and
/// records them in `plugins.managed`. User lines are kept verbatim; comment
/// lines are replaced by vessel's header (legacy L2081-2099). The original
/// is copied to `plugins.txt.orig` once (or `plugins.txt.absent` marks that
/// there was none).
///
/// Crash safety: `plugins.managed` lists old and new entries before the
/// rename and only the new ones after, so an interruption never turns a
/// vessel entry into a "user" entry.
pub fn update_plugins(
    conn: &Connection,
    game: &GameConfig,
    files: &PluginsFiles,
) -> Result<PluginStage, LoadOrderError> {
    if !plugins_applies(game) {
        return Ok(PluginStage::Skipped(SkipReason::NotBethesda));
    }
    let Some((root, rel, full)) = locate(game) else {
        return Ok(PluginStage::Skipped(SkipReason::NoPluginsFile));
    };
    let c = match open_dir(&root, &rel, &full)? {
        Opened::Ready(c) => c,
        Opened::FolderMissing => {
            return Ok(PluginStage::Skipped(SkipReason::FolderMissing(
                full.parent().unwrap_or(&full).to_path_buf(),
            )))
        }
    };
    let mut report = PluginsReport {
        path: full.clone(),
        ..Default::default()
    };
    let old_managed = files.read_managed()?;
    let managed_keys: HashSet<String> = old_managed.iter().map(|n| n.to_lowercase()).collect();
    let raw = read_nofollow(&c, &full)?;
    let ours = collect_ours(conn, game)?;
    report.provided = ours.len();

    let mut existing = Vec::new();
    for line in split_lines(raw.as_deref().unwrap_or_default()) {
        if !is_entry(&line) {
            continue;
        }
        if managed_keys.contains(&line_key(&line)) {
            report.removed.push(
                String::from_utf8_lossy(line.strip_prefix(b"*").unwrap_or(&line)).into_owned(),
            );
        } else {
            existing.push(line);
        }
    }
    if ours.is_empty() && report.removed.is_empty() {
        if !old_managed.is_empty() {
            files.write_managed(&[])?;
        }
        return Ok(PluginStage::Updated(report));
    }
    let have: HashSet<String> = existing.iter().map(|l| line_key(l)).collect();
    let fresh = |n: &&String| !have.contains(&n.to_lowercase());
    let mut added: Vec<String> = ours
        .iter()
        .filter(|n| is_master_name(n))
        .filter(fresh)
        .cloned()
        .collect();
    added.extend(
        ours.iter()
            .filter(|n| !is_master_name(n))
            .filter(fresh)
            .cloned(),
    );
    if !added.is_empty() {
        let rules = plugin_rules(conn, &game.domain).unwrap_or_else(|e| {
            report
                .warnings
                .push(format!("collection plugin rules not read: {e}"));
            Vec::new()
        });
        let local_dir = full.parent().unwrap_or(&full).to_path_buf();
        let (sorted, how) =
            sort_added(game, files, &local_dir, added, &rules, &mut report.warnings);
        added = sorted;
        report.sorted_by = Some(how);
    }

    files.record_original(raw.as_deref())?;
    let mut pending = old_managed.clone();
    pending.extend(added.iter().cloned());
    files.write_managed(&pending)?;

    let star = game.plugins_format == Some(PluginsFormat::Star);
    let mut lines: Vec<Vec<u8>> = HEADER.iter().map(|h| h.as_bytes().to_vec()).collect();
    lines.extend(existing);
    lines.extend(added.iter().map(|n| {
        if star {
            format!("*{n}").into_bytes()
        } else {
            n.clone().into_bytes()
        }
    }));
    write_atomic(&c, &full, &lines)?;
    files.write_managed(&added)?;
    report.added = added;
    report.written = true;
    Ok(PluginStage::Updated(report))
}

/// Entry lines (comments and blank lines skipped), bytes as written.
fn entries(raw: &[u8]) -> Vec<Vec<u8>> {
    split_lines(raw)
        .into_iter()
        .filter(|l| is_entry(l))
        .collect()
}

/// On purge: removes every line whose name is in `plugins.managed` (any
/// `*`; legacy L2103-2120). When the remaining entries equal those before
/// the first deploy, the file is put back byte for byte: the backup is
/// restored, or the file deleted when there was none. Otherwise the user
/// changed it meanwhile and the rewrite (other lines kept) stays. The
/// bookkeeping files are removed afterwards.
pub fn remove_managed(
    game: &GameConfig,
    files: &PluginsFiles,
) -> Result<PluginStage, LoadOrderError> {
    let managed = files.read_managed()?;
    let orig = match fs::read(files.orig_path()) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(io_err(&files.orig_path())(e)),
    };
    let absent = orig.is_none() && files.absent_path().exists();
    if managed.is_empty() && orig.is_none() && !absent {
        let stage = if plugins_applies(game) {
            PluginStage::Updated(PluginsReport {
                path: locate(game).map(|l| l.2).unwrap_or_default(),
                ..Default::default()
            })
        } else {
            PluginStage::Skipped(SkipReason::NotBethesda)
        };
        return Ok(stage);
    }
    let skip = |files: &PluginsFiles, r: SkipReason| -> Result<PluginStage, LoadOrderError> {
        files.write_managed(&[])?;
        Ok(PluginStage::Skipped(r))
    };
    if !plugins_applies(game) {
        return skip(files, SkipReason::NotBethesda);
    }
    let Some((root, rel, full)) = locate(game) else {
        return skip(files, SkipReason::NoPluginsFile);
    };
    let c = match open_dir(&root, &rel, &full)? {
        Opened::Ready(c) => c,
        Opened::FolderMissing => {
            return skip(
                files,
                SkipReason::FolderMissing(full.parent().unwrap_or(&full).to_path_buf()),
            )
        }
    };
    let keys: HashSet<String> = managed.iter().map(|n| n.to_lowercase()).collect();
    let mut report = PluginsReport {
        path: full.clone(),
        ..Default::default()
    };
    if let Some(raw) = read_nofollow(&c, &full)? {
        let mut kept = Vec::new();
        for line in split_lines(&raw) {
            if !line.is_empty() && keys.contains(&line_key(&line)) {
                report.removed.push(
                    String::from_utf8_lossy(line.strip_prefix(b"*").unwrap_or(&line)).into_owned(),
                );
            } else {
                kept.push(line);
            }
        }
        let left: Vec<Vec<u8>> = kept.iter().filter(|l| is_entry(l)).cloned().collect();
        match &orig {
            Some(o) if left == entries(o) => {
                if raw != *o {
                    write_bytes_atomic(&c, &full, o)?;
                    report.written = true;
                }
            }
            None if absent && left.is_empty() => {
                // unlink never follows a symlink at the final name.
                fs::remove_file(&c.path).map_err(io_err(&full))?;
                report.written = true;
            }
            _ => {
                write_atomic(&c, &full, &kept)?;
                report.written = true;
            }
        }
    }
    files.clear()?;
    Ok(PluginStage::Updated(report))
}

/// Entries of plugins.txt (comments and blank lines skipped), with their
/// active and managed state. `None` when the stage does not apply or the
/// file does not exist.
pub fn read_plugins_txt(
    game: &GameConfig,
    files: &PluginsFiles,
) -> Result<Option<Vec<PluginLine>>, LoadOrderError> {
    if !plugins_applies(game) {
        return Ok(None);
    }
    let Some((root, rel, full)) = locate(game) else {
        return Ok(None);
    };
    let Opened::Ready(c) = open_dir(&root, &rel, &full)? else {
        return Ok(None);
    };
    let Some(raw) = read_nofollow(&c, &full)? else {
        return Ok(None);
    };
    let managed: HashSet<String> = files
        .read_managed()?
        .iter()
        .map(|n| n.to_lowercase())
        .collect();
    let star = game.plugins_format == Some(PluginsFormat::Star);
    Ok(Some(
        split_lines(&raw)
            .into_iter()
            .filter(|l| is_entry(l))
            .map(|l| {
                let active = !star || l.starts_with(b"*");
                let name = String::from_utf8_lossy(l.strip_prefix(b"*").unwrap_or(&l)).into_owned();
                PluginLine {
                    managed: managed.contains(&name.to_lowercase()),
                    name,
                    active,
                }
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Catches: CRLF residue in names or a phantom line after the final
    /// newline.
    #[test]
    fn split_lines_strips_cr_and_final_newline() {
        assert_eq!(
            split_lines(b"a\r\n*b\r\n\r\nc"),
            vec![b"a".to_vec(), b"*b".to_vec(), b"".to_vec(), b"c".to_vec()]
        );
        assert_eq!(split_lines(b"a\n"), vec![b"a".to_vec()]);
        assert!(split_lines(b"").is_empty());
    }

    /// Catches: star prefix or case defeating the presence test.
    #[test]
    fn line_key_ignores_star_and_case() {
        assert_eq!(line_key(b"*SkyUI_SE.ESP"), "skyui_se.esp");
        assert_eq!(line_key(b"skyui_se.esp"), "skyui_se.esp");
    }

    /// Catches: an AppData folder with `..` escaping the prefix.
    #[test]
    fn locate_refuses_traversal_in_appdata_folder() {
        let mut g = GameConfig::custom("x");
        g.profile = "bethesda".into();
        g.plugins_format = Some(PluginsFormat::Star);
        g.prefix = Some("/p".into());
        g.appdata_folder = Some("../../etc".into());
        assert!(locate(&g).is_none());
        g.appdata_folder = Some("Skyrim Special Edition".into());
        let (root, rel, _) = locate(&g).unwrap();
        assert_eq!(root, PathBuf::from("/p"));
        assert_eq!(
            rel,
            "drive_c/users/steamuser/AppData/Local/Skyrim Special Edition/plugins.txt"
        );
    }
}
