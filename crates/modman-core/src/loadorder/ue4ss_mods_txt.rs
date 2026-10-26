// SPDX-License-Identifier: GPL-3.0-or-later
//! UE4SS `mods.txt`, owned by modman like plugins.txt (`[[shared_files]]`
//! with format `ue4ss_mods_txt`).
//!
//! - The file lives in the resolved UE4SS mods folder (current or legacy,
//!   see [`Ue4ssDef::resolve_mods_dir`]). Copies shipped by mods are never
//!   placed (the deploy plan drops them, so they never conflict); the
//!   highest-priority copy only seeds the file when the game has none.
//! - Deploy adds `Name : 1` for every staged UE4SS mod folder not listed
//!   yet, before the `Keybinds` entry. Every other byte stays as it was.
//! - Bookkeeping in `<game data>/shared/`: `<key>.orig` (the original
//!   bytes) and `<key>.json` (what modman wrote). Both are written and
//!   synced before the game file, so a crash at any point leaves enough to
//!   undo it.
//! - Purge: file unchanged since modman wrote it -> the original comes back
//!   byte for byte (or the file is removed when modman created it);
//!   otherwise only the lines modman added are removed.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::plugins_txt::{read_nofollow, write_bytes_atomic};
use super::LoadOrderError;
use crate::deploy::confine::{confine, is_escape, Confined};
use crate::deploy::{io_err, DeployError, PlanItem};
use crate::games::definitions::{SharedFileBase, SharedFileFormat, Ue4ssDef};
use crate::games::{builtin_definitions, find_definition, ue4ss_mods_dir, GameConfig};
use crate::staging::is_ue4ss_marker_path;

/// Bookkeeping folder inside the per-game data folder.
const STATE_DIR: &str = "shared";
const KEYBINDS: &str = "keybinds";

/// One modman-owned mods.txt of a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharedTarget {
    /// Mods folder, relative to the target root.
    pub dir: String,
    /// `<dir>/<name>`, relative to the target root.
    pub rel_path: String,
}

/// A mods.txt written by a deploy (recorded in the deploy journal).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Merged {
    pub rel_path: String,
    /// `None`: the game had no mods.txt.
    pub original_sha: Option<String>,
    pub written_sha: String,
    pub added: Vec<String>,
}

/// `<key>.json` in the bookkeeping folder.
#[derive(Debug, Serialize, Deserialize)]
struct State {
    rel_path: String,
    original_sha: Option<String>,
    written_sha: String,
    added: Vec<String>,
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `subdir` as a path prefix (empty for `.`).
fn base_prefix(game: &GameConfig) -> String {
    let s = game.subdir.trim_start_matches("./").trim_matches('/');
    if s.is_empty() || s == "." {
        String::new()
    } else {
        format!("{s}/")
    }
}

/// `[ue4ss]` and the mods.txt names of `game`'s built-in definition (the
/// same lookup staging uses).
fn definition(game: &GameConfig) -> Option<(Ue4ssDef, Vec<String>)> {
    let defs = builtin_definitions();
    let d = find_definition(&defs, game.definition.as_deref()?)?;
    let names = d
        .shared_files
        .iter()
        .filter(|f| {
            f.relative_to == SharedFileBase::Ue4ssModsDir
                && f.format == SharedFileFormat::Ue4ssModsTxt
        })
        .map(|f| f.name.clone())
        .collect();
    Some((d.ue4ss.clone()?, names))
}

/// Lower-cased target-root paths where a mod's mods.txt would land (current
/// and legacy folder): the plan merges these instead of placing them.
pub(crate) fn shared_paths(game: &GameConfig) -> HashSet<String> {
    let Some((u, names)) = definition(game) else {
        return HashSet::new();
    };
    let base = base_prefix(game);
    names
        .iter()
        .flat_map(|n| {
            [&u.mods_dir, &u.legacy_mods_dir].map(|d| format!("{base}{d}/{n}").to_lowercase())
        })
        .collect()
}

/// The mods.txt files of `game`, in the mods folder in use under `root`.
pub(crate) fn targets(game: &GameConfig, root: &Path) -> Vec<SharedTarget> {
    let (Some((_, names)), Some(mods_dir)) = (definition(game), ue4ss_mods_dir(game, Some(root)))
    else {
        return Vec::new();
    };
    let dir = format!("{}{mods_dir}", base_prefix(game));
    names
        .into_iter()
        .map(|n| SharedTarget {
            rel_path: format!("{dir}/{n}"),
            dir: dir.clone(),
        })
        .collect()
}

/// `(lower-cased name, value)` of a `Name : value` line; `None` for blank,
/// comment and other lines.
fn entry(line: &[u8]) -> Option<(String, String)> {
    let s = String::from_utf8_lossy(line);
    let s = s.trim();
    if s.starts_with(';') || s.starts_with('#') {
        return None;
    }
    let (n, v) = s.split_once(':')?;
    let n = n.trim();
    (!n.is_empty()).then(|| (n.to_lowercase(), v.trim().to_string()))
}

fn lines(raw: &[u8]) -> impl Iterator<Item = &[u8]> {
    raw.split_inclusive(|b| *b == b'\n')
}

/// Adds `Name : 1` for each of `names` not listed yet (case-insensitive),
/// before the last `Keybinds` entry (else at the end), with the file's line
/// ending. All other bytes are kept. Returns the new bytes and the names
/// added.
pub(crate) fn merge(base: &[u8], names: &[String]) -> (Vec<u8>, Vec<String>) {
    let listed: HashSet<String> = lines(base).filter_map(entry).map(|(n, _)| n).collect();
    let mut seen = HashSet::new();
    let added: Vec<String> = names
        .iter()
        .filter(|n| {
            let k = n.to_lowercase();
            !listed.contains(&k) && seen.insert(k)
        })
        .cloned()
        .collect();
    if added.is_empty() {
        return (base.to_vec(), added);
    }
    let eol: &[u8] = if base.windows(2).any(|w| w == b"\r\n") {
        b"\r\n"
    } else {
        b"\n"
    };
    let mut block = Vec::new();
    for n in &added {
        block.extend_from_slice(n.as_bytes());
        block.extend_from_slice(b" : 1");
        block.extend_from_slice(eol);
    }
    let all: Vec<&[u8]> = lines(base).collect();
    let keybinds = all
        .iter()
        .rposition(|l| entry(l).is_some_and(|(n, _)| n == KEYBINDS));
    let mut out = Vec::with_capacity(base.len() + block.len() + eol.len());
    match keybinds {
        Some(i) => {
            all[..i].iter().for_each(|l| out.extend_from_slice(l));
            out.extend_from_slice(&block);
            all[i..].iter().for_each(|l| out.extend_from_slice(l));
        }
        None => {
            out.extend_from_slice(base);
            if !base.is_empty() && !base.ends_with(b"\n") {
                out.extend_from_slice(eol);
            }
            out.extend_from_slice(&block);
        }
    }
    (out, added)
}

/// Removes the `Name : 1` lines modman added; a line the user changed (for
/// example to `: 0`) and everything else is kept byte for byte.
pub(crate) fn strip(raw: &[u8], added: &[String]) -> Vec<u8> {
    let ours: HashSet<String> = added.iter().map(|n| n.to_lowercase()).collect();
    lines(raw)
        .filter(|l| !entry(l).is_some_and(|(n, v)| v == "1" && ours.contains(&n)))
        .flatten()
        .copied()
        .collect()
}

/// UE4SS mod folders among the placed paths below `dir` (case-insensitive),
/// in plan order, each once, with their placed spelling; plus the placed
/// spelling of `dir` itself (the game folder may spell it differently).
/// A folder counts only when it holds a mod entry point (`Scripts/main.lua`
/// or `dlls/main.dll`, the staging detection rule); helper folders such as
/// UE4SS's own `shared/` are no mods.
fn mod_folders(dir: &str, placed: &[&str]) -> (Option<String>, Vec<String>) {
    let want: Vec<String> = dir.split('/').map(str::to_lowercase).collect();
    let n = want.len();
    let mut real = None;
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for rel in placed {
        let comps: Vec<&str> = rel.split('/').collect();
        // A folder needs a file below it: `<dir>/<Folder>/<file>`.
        let inside =
            comps.len() > n + 1 && comps.iter().zip(&want).all(|(c, d)| c.to_lowercase() == *d);
        if !inside {
            continue;
        }
        real.get_or_insert_with(|| comps[..n].join("/"));
        if is_ue4ss_marker_path(&comps[n + 1..].join("/")) && seen.insert(comps[n].to_lowercase()) {
            names.push(comps[n].to_string());
        }
    }
    (real, names)
}

fn to_deploy(e: LoadOrderError) -> DeployError {
    match e {
        LoadOrderError::Io { path, source } => DeployError::Io { path, source },
        LoadOrderError::Deploy(e) => *e,
        e => DeployError::PathConflict {
            rel: String::new(),
            reason: e.to_string(),
        },
    }
}

/// The confined mods.txt path; `None` when its folder does not exist.
fn open(root: &Path, rel: &str, create: bool) -> Result<Option<Confined>, DeployError> {
    match confine(root, rel, create) {
        Ok(c) => Ok(Some(c)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) if is_escape(&e) => Err(DeployError::PathConflict {
            rel: rel.to_string(),
            reason: format!("refusing to write through a symbolic link or non-folder ({e})"),
        }),
        Err(e) => Err(io_err(&root.join(rel))(e)),
    }
}

fn state_paths(game_data: &Path, rel: &str) -> (PathBuf, PathBuf) {
    let key = rel.replace('/', "__");
    let dir = game_data.join(STATE_DIR);
    (
        dir.join(format!("{key}.json")),
        dir.join(format!("{key}.orig")),
    )
}

fn write_synced(path: &Path, body: &[u8]) -> Result<(), DeployError> {
    let tmp = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp).map_err(io_err(&tmp))?;
    f.write_all(body).map_err(io_err(&tmp))?;
    f.sync_all().map_err(io_err(&tmp))?;
    fs::rename(&tmp, path).map_err(io_err(path))
}

fn remove_if_present(path: &Path) -> Result<(), DeployError> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_err(path)(e)),
        _ => Ok(()),
    }
}

/// Merges mods.txt for a deploy. `placed` are the target-root paths the
/// deploy placed (real spelling); `shipped` are the plan's mods.txt copies.
/// Undoes leftover bookkeeping first (normally none: purge ran before).
pub(crate) fn apply(
    game_data: &Path,
    game: &GameConfig,
    root: &Path,
    placed: &[&str],
    shipped: &[PlanItem],
) -> Result<Vec<Merged>, DeployError> {
    revert(game_data, root)?;
    let mut out = Vec::new();
    for t in targets(game, root) {
        let (real_dir, names) = mod_folders(&t.dir, placed);
        let seed = shipped
            .iter()
            .find(|i| i.rel_path.eq_ignore_ascii_case(&t.rel_path))
            .map(|i| &i.source);
        if names.is_empty() && seed.is_none() {
            continue;
        }
        // No mods folder: nothing was placed there, and creating one here
        // would escape the deploy's folder tracking.
        let rel_path = match real_dir {
            Some(d) => format!("{d}/{}", &t.rel_path[t.dir.len() + 1..]),
            None => t.rel_path.clone(),
        };
        let Some(c) = open(root, &rel_path, false)? else {
            continue;
        };
        let full = root.join(&rel_path);
        let current = read_nofollow(&c, &full).map_err(to_deploy)?;
        let base = match (&current, seed) {
            (Some(b), _) => b.clone(),
            (None, Some(s)) => fs::read(s).map_err(io_err(s))?,
            (None, None) => Vec::new(),
        };
        let (merged, added) = merge(&base, &names);
        if current.as_deref() == Some(merged.as_slice()) {
            continue;
        }
        let state = State {
            rel_path,
            original_sha: current.as_deref().map(sha),
            written_sha: sha(&merged),
            added,
        };
        // Write-ahead: the original and the sha about to be written are
        // durable before the game file changes.
        let (json, orig) = state_paths(game_data, &state.rel_path);
        let dir = game_data.join(STATE_DIR);
        fs::create_dir_all(&dir).map_err(io_err(&dir))?;
        if let Some(b) = &current {
            write_synced(&orig, b)?;
        }
        let body = serde_json::to_vec_pretty(&state).expect("state serializes");
        write_synced(&json, &body)?;
        write_bytes_atomic(&c, &full, &merged).map_err(to_deploy)?;
        out.push(Merged {
            rel_path: state.rel_path,
            original_sha: state.original_sha,
            written_sha: state.written_sha,
            added: state.added,
        });
    }
    Ok(out)
}

/// Undoes every recorded mods.txt write under `root` (purge, rollback).
/// Idempotent: bookkeeping is dropped only after the game file is handled.
pub(crate) fn revert(game_data: &Path, root: &Path) -> Result<(), DeployError> {
    let dir = game_data.join(STATE_DIR);
    let mut states: Vec<PathBuf> = match fs::read_dir(&dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_err(&dir)(e)),
    };
    states.sort();
    for json in states {
        let raw = fs::read(&json).map_err(io_err(&json))?;
        let state: State = serde_json::from_slice(&raw).map_err(|e| {
            io_err(&json)(io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
        })?;
        let (_, orig_path) = state_paths(game_data, &state.rel_path);
        let original = match state.original_sha {
            Some(_) => Some(fs::read(&orig_path).map_err(io_err(&orig_path))?),
            None => None,
        };
        revert_one(root, &state, original.as_deref())?;
        // The state goes first: a leftover original alone is inert.
        remove_if_present(&json)?;
        remove_if_present(&orig_path)?;
    }
    Ok(())
}

fn revert_one(root: &Path, state: &State, original: Option<&[u8]>) -> Result<(), DeployError> {
    let rel = state.rel_path.as_str();
    let full = root.join(rel);
    let Some(c) = open(root, rel, original.is_some())? else {
        return Ok(()); // folder gone and nothing to restore
    };
    let current = read_nofollow(&c, &full).map_err(to_deploy)?;
    let current_sha = current.as_deref().map(sha);
    if current_sha == state.original_sha {
        return Ok(());
    }
    let ours = current_sha.as_deref() == Some(state.written_sha.as_str());
    let body = match (&current, original) {
        // Unchanged since modman wrote it, or deleted: the original.
        (_, Some(o)) if ours || current.is_none() => Some(o.to_vec()),
        (None, None) => return Ok(()),
        (Some(_), None) if ours => None,
        // Changed by the user: only modman's lines go.
        (Some(cur), _) => {
            let kept = strip(cur, &state.added);
            if kept == *cur {
                return Ok(());
            }
            let empty = kept.iter().all(u8::is_ascii_whitespace);
            (!(empty && original.is_none())).then_some(kept)
        }
        (None, Some(_)) => unreachable!("handled above"),
    };
    match body {
        Some(b) => write_bytes_atomic(&c, &full, &b).map_err(to_deploy),
        None => remove_if_present(&c.path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    /// Catches: a merge that re-encodes the file (CRLF -> LF), drops the
    /// missing final newline, or appends after Keybinds.
    #[test]
    fn merge_inserts_before_keybinds_keeping_bytes() {
        let base = b"A : 1\r\nKeybinds : 1";
        let (out, added) = merge(base, &names(&["B", "a", "C"]));
        assert_eq!(added, ["B", "C"]);
        assert_eq!(out, b"A : 1\r\nB : 1\r\nC : 1\r\nKeybinds : 1");
        assert_eq!(strip(&out, &added), base);
    }

    /// Catches: appending onto a last line without a newline.
    #[test]
    fn merge_without_keybinds_appends_on_a_new_line() {
        let (out, _) = merge(b"; c\nA : 0", &names(&["B"]));
        assert_eq!(out, b"; c\nA : 0\nB : 1\n");
        assert_eq!(merge(b"", &names(&["B"])).0, b"B : 1\n");
    }

    /// Catches: strip removing a line the user changed or the user's own
    /// entries.
    #[test]
    fn strip_keeps_user_changed_lines() {
        let raw = b"B : 0\nUser : 1\nC : 1\n";
        assert_eq!(strip(raw, &names(&["b", "c"])), b"B : 0\nUser : 1\n");
    }

    /// Catches: files directly in the mods folder (mods.txt), other folders,
    /// or folders without a UE4SS entry point (UE4SS's own
    /// `shared/UEHelpers`, which would add `shared : 1`) counted as mods,
    /// and case duplicates listed twice.
    #[test]
    fn mod_folders_needs_a_file_below_the_folder() {
        let placed = [
            "Pal/ue4ss/Mods/A/Scripts/main.lua",
            "pal/UE4SS/mods/a/enabled.txt",
            "Pal/ue4ss/Mods/mods.txt",
            "Pal/ue4ss/Other/X/main.lua",
            "Pal/ue4ss/Mods/B/dlls/main.dll",
            "Pal/ue4ss/Mods/shared/UEHelpers/UEHelpers.lua",
            "Pal/ue4ss/Mods/C/scripts/MAIN.LUA",
            "Pal/ue4ss/Mods/D/Scripts/lib/main.lua",
        ];
        let (real, names) = mod_folders("Pal/UE4SS/Mods", &placed);
        assert_eq!(real.as_deref(), Some("Pal/ue4ss/Mods"));
        assert_eq!(
            names,
            ["A", "B", "C"],
            "shared/ and D (no entry point) are no mods"
        );
    }
}
