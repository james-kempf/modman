// SPDX-License-Identifier: GPL-3.0-or-later
//! M2: per-mod layout normalisation (layout profiles) and staged file lists.
//!
//! [`normalize_layout`] turns an extracted archive into a folder that mirrors
//! the game's deploy root, following the legacy `normalize_layout` and
//! `layout_*` rules (legacy_spec.md §6): wrapper stripping, variant folders,
//! then per-profile placement. All moves go through one case-insensitive
//! merge, so `data/Textures` and `Data/textures` end up as one directory
//! (the first spelling placed wins).
//!
//! The input tree must already have passed [`crate::archive::validate_tree`]
//! (no symlinks or hard links). Per-library staging locations live in
//! `deploy.rs`; this module only shapes one mod's tree.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use sha2::{Digest, Sha256};

/// Errors from this module.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StagingError {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("unknown layout profile '{0}' (expected one of: {known})", known = crate::games::KNOWN_PROFILES.join(", "))]
    UnknownProfile(String),
}

/// A layout path (setting or computed) that would leave the staging folder.
fn unsafe_rel(out: &Path, rel: &str) -> StagingError {
    StagingError::Io {
        path: out.join(rel.replace('\0', "")),
        source: io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe relative path ('.', '..', absolute or NUL)",
        ),
    }
}

fn non_utf8(path: &Path) -> StagingError {
    StagingError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidData, "file name is not valid UTF-8"),
    }
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> StagingError + '_ {
    move |source| StagingError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Layout profile of a game definition (`profile` in the game TOML).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayoutProfile {
    Cyberpunk,
    Bethesda,
    Stardew,
    Bannerlord,
    Witcher3,
    Bepinex,
    Melonloader,
    Ue,
    Generic,
}

impl LayoutProfile {
    /// Same order as [`crate::games::KNOWN_PROFILES`].
    pub const ALL: [LayoutProfile; 9] = [
        LayoutProfile::Cyberpunk,
        LayoutProfile::Bethesda,
        LayoutProfile::Stardew,
        LayoutProfile::Bannerlord,
        LayoutProfile::Witcher3,
        LayoutProfile::Bepinex,
        LayoutProfile::Melonloader,
        LayoutProfile::Ue,
        LayoutProfile::Generic,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            LayoutProfile::Cyberpunk => "cyberpunk",
            LayoutProfile::Bethesda => "bethesda",
            LayoutProfile::Stardew => "stardew",
            LayoutProfile::Bannerlord => "bannerlord",
            LayoutProfile::Witcher3 => "witcher3",
            LayoutProfile::Bepinex => "bepinex",
            LayoutProfile::Melonloader => "melonloader",
            LayoutProfile::Ue => "ue",
            LayoutProfile::Generic => "generic",
        }
    }

    /// Profiles that never strip wrappers or look for variants (legacy L873, L895).
    fn keeps_structure(self) -> bool {
        matches!(
            self,
            LayoutProfile::Generic | LayoutProfile::Stardew | LayoutProfile::Bannerlord
        )
    }

    /// Lower-cased top-level folder names that mark a deploy root (legacy `profile_tops`).
    fn tops(self, paks_dir: &str) -> Vec<String> {
        let fixed: &[&str] = match self {
            LayoutProfile::Cyberpunk => {
                &["archive", "bin", "engine", "r6", "red4ext", "mods", "tools"]
            }
            LayoutProfile::Bethesda => &[
                "meshes",
                "textures",
                "scripts",
                "interface",
                "sound",
                "music",
                "strings",
                "video",
                "seq",
                "grass",
                "lodsettings",
                "shadersfx",
                "materials",
                "mcm",
                "skse",
                "f4se",
                "sfse",
                "nvse",
                "obse",
                "fose",
                "dialogueviews",
                "calientetools",
                "netscriptframework",
                "nemesis_engine",
                "platform",
                "distantlod",
                "lsdata",
                "fonts",
                "vis",
                "terrain",
                "facegen",
                "trees",
                "lightplacer",
                "actors",
                "sky",
                "lod",
                "shaders",
                "source",
                "pbrnifpatcher",
                "pbrtexturesets",
                "parallaxgen",
                "dyndolod",
                "splashes",
                "icons",
                "bookart",
                "menus",
                "characters",
                "creatures",
            ],
            LayoutProfile::Bepinex => &["bepinex", "doorstop_libs", "unstripped_corlib"],
            LayoutProfile::Melonloader => {
                &["mods", "plugins", "userdata", "userlibs", "melonloader"]
            }
            LayoutProfile::Witcher3 => &["mods", "dlc", "bin", "content"],
            LayoutProfile::Ue => {
                let first = paks_dir.split('/').next().unwrap_or("").to_lowercase();
                return [first, "engine".to_string()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            _ => &[],
        };
        fixed.iter().map(|s| s.to_string()).collect()
    }
}

impl FromStr for LayoutProfile {
    type Err = StagingError;

    /// Accepts exactly the names in [`crate::games::KNOWN_PROFILES`].
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LayoutProfile::ALL
            .into_iter()
            .find(|p| p.as_str() == s)
            .ok_or_else(|| StagingError::UnknownProfile(s.to_string()))
    }
}

impl fmt::Display for LayoutProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a staged mod's paths are relative to (legacy `MOD_ROOT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModRoot {
    /// The game's deploy directory (e.g. `Data` for Bethesda games).
    #[default]
    Deploy,
    /// The game's root folder (script extenders, engine fixes).
    Game,
}

impl ModRoot {
    /// Value stored in the mods table: `""` or `"game"`.
    pub fn as_str(self) -> &'static str {
        match self {
            ModRoot::Deploy => "",
            ModRoot::Game => "game",
        }
    }
}

/// Picks one variant folder; gets the sorted candidate names, returns an index.
pub type VariantChooser<'a> = &'a dyn Fn(&[String]) -> Option<usize>;

/// Inputs to [`normalize_layout`].
#[derive(Clone, Copy)]
pub struct LayoutOptions<'a> {
    pub profile: LayoutProfile,
    /// Display name; sanitised with [`sanitize_name`] for generated folders.
    pub mod_name: &'a str,
    /// Strip wrapper folders. Legacy passes `false` after a FOMOD install.
    pub strip_wrappers: bool,
    /// Look for variant folders. Legacy passes `false` after a FOMOD install.
    pub detect_variants: bool,
    /// Variant chosen earlier (reinstall, collections); matched by exact name.
    pub preset_variant: Option<&'a str>,
    /// Game's Paks folder relative to the deploy root (ue profile).
    pub paks_dir: Option<&'a str>,
    /// Interactive pick when there are several variants and no preset.
    pub choose_variant: Option<VariantChooser<'a>>,
}

impl<'a> LayoutOptions<'a> {
    pub fn new(profile: LayoutProfile, mod_name: &'a str) -> Self {
        LayoutOptions {
            profile,
            mod_name,
            strip_wrappers: true,
            detect_variants: true,
            preset_variant: None,
            paks_dir: None,
            choose_variant: None,
        }
    }
}

/// Result of [`normalize_layout`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayoutOutcome {
    pub root: ModRoot,
    /// Picked variant folder, when the archive had two or more.
    pub variant: Option<String>,
    /// All variant candidates (empty unless a pick was needed).
    pub variants: Vec<String>,
    /// User-facing notes (unrecognised folders, default variant pick).
    pub warnings: Vec<String>,
}

const DOC_EXTS: &[&str] = &[
    "txt", "md", "pdf", "jpg", "jpeg", "png", "gif", "webp", "bmp", "url", "htm", "html", "rtf",
    "doc", "docx", "nfo", "log",
];

fn ext_lower(name: &str) -> Option<String> {
    name.rsplit_once('.').map(|(_, e)| e.to_lowercase())
}

fn has_ext(name: &str, exts: &[&str]) -> bool {
    ext_lower(name).is_some_and(|e| exts.contains(&e.as_str()))
}

/// Documentation files that are dropped from a mod's root (legacy `DOC_RE`).
pub fn is_doc(name: &str) -> bool {
    has_ext(name, DOC_EXTS)
}

/// Folder-safe mod name (legacy `sanitize_name`): keeps `[A-Za-z0-9._-]`,
/// collapses `_` runs, trims `_.-` at both ends, max 60 chars, `mod` if empty.
pub fn sanitize_name(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    for c in name.chars() {
        let c = if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            c
        } else {
            '_'
        };
        if !(c == '_' && s.ends_with('_')) {
            s.push(c);
        }
    }
    let s: String = s
        .trim_matches(|c| matches!(c, '_' | '.' | '-'))
        .chars()
        .take(60)
        .collect();
    if s.is_empty() {
        "mod".into()
    } else {
        s
    }
}

/// One entry of a directory: name and whether it is a directory.
struct Ent {
    name: String,
    is_dir: bool,
}

/// Sorted entries of `dir` (byte order, like `sort -z` in the C locale).
fn entries(dir: &Path) -> Result<Vec<Ent>, StagingError> {
    let mut v = Vec::new();
    for e in fs::read_dir(dir).map_err(io_err(dir))? {
        let e = e.map_err(io_err(dir))?;
        let is_dir = e.file_type().map_err(io_err(&e.path()))?.is_dir();
        // A lossy name would point at a different (or no) file when used
        // as a path again, so such names are refused.
        let name = e
            .file_name()
            .into_string()
            .map_err(|_| non_utf8(&e.path()))?;
        v.push(Ent { name, is_dir });
    }
    v.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(v)
}

/// Is there a regular file directly in `dir` whose name matches case-insensitively?
fn has_file_ci(dir: &Path, name: &str) -> Result<bool, StagingError> {
    let want = name.to_lowercase();
    Ok(entries(dir)?
        .iter()
        .any(|e| !e.is_dir && e.name.to_lowercase() == want))
}

/// Recursive files under `dir` (sorted) matching `pred` on the file name.
fn find_files(dir: &Path, pred: impl Fn(&str) -> bool) -> Result<Vec<PathBuf>, StagingError> {
    let mut v = Vec::new();
    for e in walkdir::WalkDir::new(dir).min_depth(1).sort_by_file_name() {
        let e = e.map_err(|e| StagingError::Io {
            path: e.path().unwrap_or(dir).to_path_buf(),
            source: e.into(),
        })?;
        if e.file_type().is_file() && pred(&e.file_name().to_string_lossy()) {
            v.push(e.into_path());
        }
    }
    Ok(v)
}

/// Moves trees into `out`, resolving every path component case-insensitively
/// against what is already there (legacy `merge_into`, made case-insensitive).
struct Placer<'o> {
    out: &'o Path,
    /// Directory under `out` -> (lower-cased child name -> real child name).
    cache: HashMap<PathBuf, HashMap<String, String>>,
}

impl<'o> Placer<'o> {
    fn new(out: &'o Path) -> Self {
        Placer {
            out,
            cache: HashMap::new(),
        }
    }

    fn children(&mut self, dir: &Path) -> Result<&mut HashMap<String, String>, StagingError> {
        if !self.cache.contains_key(dir) {
            let mut m = HashMap::new();
            for e in entries(dir)? {
                m.entry(e.name.to_lowercase()).or_insert(e.name);
            }
            self.cache.insert(dir.to_path_buf(), m);
        }
        Ok(self.cache.get_mut(dir).expect("inserted above"))
    }

    /// Existing spelling of `name` in `dir`, else `name`; recorded as present.
    fn child(&mut self, dir: &Path, name: &str) -> Result<PathBuf, StagingError> {
        let real = self
            .children(dir)?
            .entry(name.to_lowercase())
            .or_insert_with(|| name.to_string())
            .clone();
        Ok(dir.join(real))
    }

    fn forget_tree(&mut self, path: &Path) {
        self.cache.retain(|k, _| !k.starts_with(path));
    }

    /// Moves `src` (file or directory) to `out/rel`, merging directories.
    /// `.`, `..` and NUL in `rel` are refused, so nothing lands outside `out`.
    fn place(&mut self, src: &Path, rel: &str) -> Result<(), StagingError> {
        let comps: Vec<&str> = rel.split('/').filter(|c| !c.is_empty()).collect();
        if comps
            .iter()
            .any(|c| matches!(*c, "." | "..") || c.contains('\0'))
        {
            return Err(unsafe_rel(self.out, rel));
        }
        let mut dir = self.out.to_path_buf();
        fs::create_dir_all(&dir).map_err(io_err(&dir))?;
        let Some((last, parents)) = comps.split_last() else {
            return Ok(());
        };
        for c in parents {
            dir = self.child(&dir, c)?;
            if !dir.is_dir() {
                fs::create_dir(&dir).map_err(io_err(&dir))?;
            }
        }
        let dst = self.child(&dir, last)?;
        self.merge(src, &dst)
    }

    fn merge(&mut self, src: &Path, dst: &Path) -> Result<(), StagingError> {
        let src_is_dir = fs::symlink_metadata(src).map_err(io_err(src))?.is_dir();
        let dst_meta = fs::symlink_metadata(dst).ok();
        if !src_is_dir {
            if dst_meta.as_ref().is_some_and(|m| m.is_dir()) {
                fs::remove_dir_all(dst).map_err(io_err(dst))?;
                self.forget_tree(dst);
            }
            return fs::rename(src, dst).map_err(io_err(src));
        }
        match dst_meta {
            Some(m) if m.is_dir() => {}
            Some(_) => {
                fs::remove_file(dst).map_err(io_err(dst))?;
                fs::create_dir(dst).map_err(io_err(dst))?;
            }
            None => fs::create_dir(dst).map_err(io_err(dst))?,
        }
        for e in entries(src)? {
            let child_dst = self.child(dst, &e.name)?;
            self.merge(&src.join(&e.name), &child_dst)?;
        }
        fs::remove_dir(src).map_err(io_err(src))
    }

    /// Legacy `place_all`: every entry of `src` to the same name in `out`;
    /// root-level doc files are skipped when `skip_docs`.
    fn place_all(&mut self, src: &Path, skip_docs: bool) -> Result<(), StagingError> {
        for e in entries(src)? {
            if skip_docs && !e.is_dir && is_doc(&e.name) {
                continue;
            }
            self.place(&src.join(&e.name), &e.name)?;
        }
        Ok(())
    }
}

struct Layout<'a, 'o> {
    profile: LayoutProfile,
    tops: Vec<String>,
    paks_dir: String,
    safe: String,
    placer: Placer<'o>,
    outcome: LayoutOutcome,
    opts: &'a LayoutOptions<'a>,
}

const CET_MODS: &str = "bin/x64/plugins/cyber_engine_tweaks/mods";

impl Layout<'_, '_> {
    fn is_top(&self, name: &str) -> bool {
        self.tops.contains(&name.to_lowercase())
    }

    /// Legacy `dir_has_top`: does `dir` already look like the deploy root?
    fn has_top(&self, dir: &Path) -> Result<bool, StagingError> {
        for e in entries(dir)? {
            let nl = e.name.to_lowercase();
            if e.is_dir {
                if self.tops.contains(&nl) {
                    return Ok(true);
                }
            } else if (self.profile == LayoutProfile::Bethesda
                && has_ext(&nl, &["esp", "esm", "esl", "bsa", "ba2"]))
                || (self.profile == LayoutProfile::Bepinex
                    && (nl == "winhttp.dll" || nl == "doorstop_config.ini"))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn strip_wrappers(&self, mut src: PathBuf) -> Result<PathBuf, StagingError> {
        let bethesda = self.profile == LayoutProfile::Bethesda;
        loop {
            let ents = entries(&src)?;
            let dirs: Vec<&Ent> = ents.iter().filter(|e| e.is_dir).collect();
            let files = ents
                .iter()
                .filter(|e| !e.is_dir && !is_doc(&e.name))
                .count();
            if dirs.len() != 1 || files != 0 {
                return Ok(src);
            }
            let d = &dirs[0].name;
            let dl = d.to_lowercase();
            let is_data = dl == "data" || dl == "data files";
            // Bethesda: a folder holding Data/ or an .exe is a wrapper even
            // when named like a deploy folder (e.g. "skse").
            if bethesda && !is_data {
                let inner = entries(&src.join(d))?;
                if inner.iter().any(|e| {
                    if e.is_dir {
                        e.name.eq_ignore_ascii_case("data")
                    } else {
                        has_ext(&e.name, &["exe"])
                    }
                }) {
                    src.push(d);
                    continue;
                }
            }
            if self.tops.contains(&dl) || dl == "fomod" {
                return Ok(src);
            }
            if bethesda && is_data {
                return Ok(src);
            }
            if self.profile == LayoutProfile::Witcher3
                && (dl.starts_with("mod") || dl.starts_with("dlc"))
            {
                return Ok(src);
            }
            src.push(d);
        }
    }

    fn pick_variant(&mut self, src: PathBuf) -> Result<PathBuf, StagingError> {
        if self.has_top(&src)? {
            return Ok(src);
        }
        let mut cands = Vec::new();
        for e in entries(&src)? {
            if e.is_dir && self.has_top(&src.join(&e.name))? {
                cands.push(e.name);
            }
        }
        if cands.len() < 2 {
            return Ok(src);
        }
        let preset = self
            .opts
            .preset_variant
            .and_then(|p| cands.iter().find(|c| c.as_str() == p).cloned());
        let chosen = preset.or_else(|| {
            self.opts
                .choose_variant
                .and_then(|f| f(&cands))
                .and_then(|i| cands.get(i).cloned())
        });
        let pick = match chosen {
            Some(p) => p,
            None => {
                let p = cands[0].clone();
                self.outcome.warnings.push(format!(
                    "{}: {} variants found, using '{p}' (reinstall interactively to choose)",
                    self.opts.mod_name,
                    cands.len()
                ));
                p
            }
        };
        self.outcome.variant = Some(pick.clone());
        self.outcome.variants = cands;
        Ok(src.join(pick))
    }

    fn cyberpunk(&mut self, src: &Path) -> Result<(), StagingError> {
        let safe = self.safe.clone();
        let structured = self.has_top(src)?;
        if has_file_ci(src, "init.lua")? && !structured {
            return self.placer.place(src, &format!("{CET_MODS}/{safe}"));
        }
        for e in entries(src)? {
            let p = src.join(&e.name);
            let n = &e.name;
            if e.is_dir {
                if self.is_top(n) {
                    self.placer.place(&p, n)?;
                } else if has_file_ci(&p, "init.lua")? {
                    self.placer.place(&p, &format!("{CET_MODS}/{n}"))?;
                } else if !find_files(&p, |f| has_ext(f, &["reds"]))?.is_empty() {
                    self.placer.place(&p, &format!("r6/scripts/{n}"))?;
                } else {
                    let archives = find_files(&p, |f| has_ext(f, &["archive", "xl"]))?;
                    if archives.is_empty() {
                        self.outcome
                            .warnings
                            .push(format!("Unrecognised folder '{n}' placed at the game root"));
                        self.placer.place(&p, n)?;
                    }
                    for f in archives {
                        let base = f.file_name().expect("file").to_string_lossy().into_owned();
                        self.placer.place(&f, &format!("archive/pc/mod/{base}"))?;
                    }
                }
            } else if structured {
                if !is_doc(n) {
                    self.placer.place(&p, n)?;
                }
            } else {
                let ext = ext_lower(n).unwrap_or_default();
                let rel = match ext.as_str() {
                    "archive" | "xl" => format!("archive/pc/mod/{n}"),
                    "reds" => format!("r6/scripts/{safe}/{n}"),
                    "yaml" | "yml" => format!("r6/tweaks/{safe}/{n}"),
                    "lua" => format!("{CET_MODS}/{safe}/{n}"),
                    "dll" => format!("red4ext/plugins/{safe}/{n}"),
                    _ if is_doc(n) => continue,
                    _ => n.clone(),
                };
                self.placer.place(&p, &rel)?;
            }
        }
        Ok(())
    }

    fn bethesda(&mut self, src: &Path) -> Result<(), StagingError> {
        let ents = entries(src)?;
        let data_dir = ents
            .iter()
            .rfind(|e| {
                let nl = e.name.to_lowercase();
                e.is_dir && (nl == "data" || nl == "data files")
            })
            .map(|e| e.name.clone());
        let mut root_mod = ents
            .iter()
            .any(|e| !e.is_dir && has_ext(&e.name, &["exe", "dll"]));
        if let Some(d) = &data_dir {
            let others = ents
                .iter()
                .any(|e| &e.name != d && (e.is_dir || !is_doc(&e.name)));
            root_mod |= others;
        }
        if root_mod {
            // Script extenders, engine fixes: relative to the game root.
            self.outcome.root = ModRoot::Game;
            return self.placer.place_all(src, true);
        }
        match data_dir {
            Some(d) => self.placer.place_all(&src.join(d), true),
            None => self.placer.place_all(src, true),
        }
    }

    /// stardew / bannerlord: each folder holding `marker` becomes a top-level folder.
    fn marker_layout(&mut self, src: &Path, marker: &str) -> Result<(), StagingError> {
        if has_file_ci(src, marker)? {
            let safe = self.safe.clone();
            return self.placer.place(src, &safe);
        }
        let want = marker.to_lowercase();
        let mut hits: Vec<(usize, PathBuf)> = Vec::new();
        for e in walkdir::WalkDir::new(src).min_depth(2).max_depth(5) {
            let e = e.map_err(|e| StagingError::Io {
                path: e.path().unwrap_or(src).to_path_buf(),
                source: e.into(),
            })?;
            if e.file_type().is_file() && e.file_name().to_string_lossy().to_lowercase() == want {
                hits.push((e.depth(), e.into_path()));
            }
        }
        // Shallowest first (legacy `sort -zn` on "<depth> <path>").
        hits.sort();
        let mut found = false;
        for (_, m) in hits {
            let d = m.parent().expect("depth >= 2").to_path_buf();
            if !d.is_dir() {
                continue; // moved along with an outer marker folder
            }
            let name = d
                .file_name()
                .expect("subdir")
                .to_string_lossy()
                .into_owned();
            self.placer.place(&d, &name)?;
            found = true;
        }
        if !found {
            self.placer.place_all(src, true)?;
        }
        Ok(())
    }

    fn bepinex(&mut self, src: &Path) -> Result<(), StagingError> {
        if self.has_top(src)? {
            return self.placer.place_all(src, true);
        }
        let safe = self.safe.clone();
        for e in entries(src)? {
            let p = src.join(&e.name);
            let n = &e.name;
            let rel = if e.is_dir {
                match n.to_lowercase().as_str() {
                    "plugins" | "config" | "patchers" | "core" => format!("BepInEx/{n}"),
                    _ => format!("BepInEx/plugins/{n}"),
                }
            } else if has_ext(n, &["cfg"]) {
                format!("BepInEx/config/{n}")
            } else if is_doc(n) {
                continue;
            } else {
                format!("BepInEx/plugins/{safe}/{n}")
            };
            self.placer.place(&p, &rel)?;
        }
        Ok(())
    }

    fn melonloader(&mut self, src: &Path) -> Result<(), StagingError> {
        if self.has_top(src)? {
            return self.placer.place_all(src, true);
        }
        for e in entries(src)? {
            let p = src.join(&e.name);
            if !e.is_dir && has_ext(&e.name, &["dll"]) {
                self.placer.place(&p, &format!("Mods/{}", e.name))?;
            } else if !e.is_dir && is_doc(&e.name) {
                continue;
            } else {
                self.placer.place(&p, &e.name)?;
            }
        }
        Ok(())
    }

    fn ue(&mut self, src: &Path) -> Result<(), StagingError> {
        if self.has_top(src)? {
            return self.placer.place_all(src, true);
        }
        let paks = find_files(src, |f| has_ext(f, &["pak", "utoc", "ucas", "sig"]))?;
        if paks.is_empty() {
            return self.placer.place_all(src, true);
        }
        let dir = self.paks_dir.clone();
        for f in paks {
            let base = f.file_name().expect("file").to_string_lossy().into_owned();
            self.placer.place(&f, &format!("{dir}/{base}"))?;
        }
        Ok(())
    }

    fn witcher3(&mut self, src: &Path) -> Result<(), StagingError> {
        let ents = entries(src)?;
        if ents
            .iter()
            .any(|e| e.is_dir && e.name.eq_ignore_ascii_case("content"))
        {
            let safe = &self.safe;
            let m = if safe.to_lowercase().starts_with("mod") {
                safe.clone()
            } else {
                format!("mod{safe}")
            };
            return self.placer.place(src, &format!("mods/{m}"));
        }
        for e in ents {
            let p = src.join(&e.name);
            let n = &e.name;
            let nl = n.to_lowercase();
            if e.is_dir {
                let rel = if self.is_top(n) {
                    n.clone()
                } else if nl.starts_with("mod") {
                    format!("mods/{n}")
                } else if nl.starts_with("dlc") {
                    format!("dlc/{n}")
                } else {
                    n.clone()
                };
                self.placer.place(&p, &rel)?;
            } else if !is_doc(n) {
                self.placer.place(&p, n)?;
            }
        }
        Ok(())
    }
}

/// Moves the extracted tree at `src` into `out` following the profile rules.
///
/// `src` is consumed (its files are moved). `out` is created if missing and
/// may already hold files (FOMOD output is merged the same way).
pub fn normalize_layout(
    src: &Path,
    out: &Path,
    opts: &LayoutOptions<'_>,
) -> Result<LayoutOutcome, StagingError> {
    let raw_paks = opts.paks_dir.unwrap_or("");
    if crate::deploy::check_rel_dir(raw_paks).is_err() {
        return Err(unsafe_rel(out, raw_paks));
    }
    let paks_dir = raw_paks.trim_matches('/').to_string();
    fs::create_dir_all(out).map_err(io_err(out))?;
    let mut l = Layout {
        profile: opts.profile,
        tops: opts.profile.tops(&paks_dir),
        paks_dir,
        safe: sanitize_name(opts.mod_name),
        placer: Placer::new(out),
        outcome: LayoutOutcome::default(),
        opts,
    };
    let mut src = src.to_path_buf();
    if !opts.profile.keeps_structure() {
        if opts.strip_wrappers {
            src = l.strip_wrappers(src)?;
        }
        if opts.detect_variants {
            src = l.pick_variant(src)?;
        }
    }
    match opts.profile {
        LayoutProfile::Cyberpunk => l.cyberpunk(&src)?,
        LayoutProfile::Bethesda => l.bethesda(&src)?,
        LayoutProfile::Stardew => l.marker_layout(&src, "manifest.json")?,
        LayoutProfile::Bannerlord => l.marker_layout(&src, "SubModule.xml")?,
        LayoutProfile::Bepinex => l.bepinex(&src)?,
        LayoutProfile::Melonloader => l.melonloader(&src)?,
        LayoutProfile::Ue => l.ue(&src)?,
        LayoutProfile::Witcher3 => l.witcher3(&src)?,
        LayoutProfile::Generic => l.placer.place_all(&src, false)?,
    }
    Ok(l.outcome)
}

/// One staged file, as stored in the `mod_files` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedFile {
    /// Relative to the staging dir, `/`-separated.
    pub path: String,
    pub size: u64,
    /// Lower-case hex SHA-256 of the content.
    pub sha256: String,
    /// Permission bits (`st_mode & 0o7777`).
    pub mode: u32,
}

/// Lists every regular file under `root` with size, SHA-256 and mode, sorted by path.
pub fn staged_files(root: &Path) -> Result<Vec<StagedFile>, StagingError> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    for e in walkdir::WalkDir::new(root)
        .min_depth(1)
        .follow_links(false)
        .sort_by_file_name()
    {
        let e = e.map_err(|e| StagingError::Io {
            path: e.path().unwrap_or(root).to_path_buf(),
            source: e.into(),
        })?;
        if !e.file_type().is_file() {
            continue;
        }
        let p = e.path();
        let mut f = fs::File::open(p).map_err(io_err(p))?;
        let meta = f.metadata().map_err(io_err(p))?;
        let mut h = Sha256::new();
        loop {
            let n = f.read(&mut buf).map_err(io_err(p))?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        let rel = p
            .strip_prefix(root)
            .expect("walkdir yields paths under its root")
            .components()
            .map(|c| c.as_os_str().to_str().ok_or_else(|| non_utf8(p)))
            .collect::<Result<Vec<_>, _>>()?
            .join("/");
        out.push(StagedFile {
            path: rel,
            size: meta.len(),
            sha256: h.finalize().iter().map(|b| format!("{b:02x}")).collect(),
            mode: meta.mode() & 0o7777,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use LayoutProfile as P;

    /// Creates files (content = their path); names ending in '/' are dirs.
    fn mk(root: &Path, paths: &[&str]) {
        for p in paths {
            let full = root.join(p);
            if p.ends_with('/') {
                fs::create_dir_all(&full).unwrap();
            } else {
                fs::create_dir_all(full.parent().unwrap()).unwrap();
                fs::write(&full, p.as_bytes()).unwrap();
            }
        }
    }

    /// Files under `root`, '/'-joined and sorted.
    fn files(root: &Path) -> Vec<String> {
        staged_files(root)
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect()
    }

    struct Run {
        _t: tempfile::TempDir,
        out: PathBuf,
        outcome: LayoutOutcome,
    }

    fn run_with(paths: &[&str], opts: LayoutOptions<'_>) -> Run {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        fs::create_dir_all(&src).unwrap();
        mk(&src, paths);
        let out = t.path().join("out");
        let outcome = normalize_layout(&src, &out, &opts).unwrap();
        Run {
            _t: t,
            out,
            outcome,
        }
    }

    fn run(profile: LayoutProfile, name: &str, paths: &[&str]) -> Run {
        run_with(paths, LayoutOptions::new(profile, name))
    }

    // ---- profile names ----

    #[test]
    fn layout_profile_from_str_accepts_exactly_known_profiles() {
        let names: Vec<&str> = P::ALL.iter().map(|p| p.as_str()).collect();
        assert_eq!(names, crate::games::KNOWN_PROFILES);
        for n in crate::games::KNOWN_PROFILES {
            let p: LayoutProfile = n.parse().unwrap();
            assert_eq!(p.to_string(), *n);
        }
        for bad in ["Bethesda", "skyrim", "", " generic", "unreal"] {
            assert!(matches!(
                bad.parse::<LayoutProfile>(),
                Err(StagingError::UnknownProfile(_))
            ));
        }
    }

    #[test]
    fn sanitize_name_matches_legacy() {
        assert_eq!(sanitize_name("SkyUI 5.2 (SE)"), "SkyUI_5.2_SE");
        assert_eq!(sanitize_name("--__a  b..--"), "a_b");
        assert_eq!(sanitize_name("日本語"), "mod");
        assert_eq!(sanitize_name(""), "mod");
        assert_eq!(sanitize_name(&"x".repeat(80)).len(), 60);
    }

    #[test]
    fn is_doc_is_case_insensitive() {
        assert!(is_doc("README.TXT"));
        assert!(is_doc("shot.JPEG"));
        assert!(!is_doc("plugin.esp"));
        assert!(!is_doc("txt"));
    }

    // ---- case-insensitive merge ----

    #[test]
    fn case_insensitive_merge_into_one_canonical_directory() {
        let r = run(
            P::Generic,
            "m",
            &[
                "data/Textures/a.dds",
                "Data/textures/b.dds",
                "data/Textures/sub/c.dds",
            ],
        );
        // "Data" sorts before "data", so its spelling is canonical.
        assert_eq!(
            files(&r.out),
            [
                "Data/textures/a.dds",
                "Data/textures/b.dds",
                "Data/textures/sub/c.dds"
            ]
        );
        let tops: Vec<_> = fs::read_dir(&r.out).unwrap().collect();
        assert_eq!(tops.len(), 1);
    }

    #[test]
    fn merge_into_existing_output_keeps_existing_spelling() {
        let t = tempfile::tempdir().unwrap();
        let out = t.path().join("out");
        mk(&out, &["Textures/old.dds"]);
        let src = t.path().join("src");
        mk(&src, &["textures/new.dds", "textures/OLD.dds"]);
        normalize_layout(&src, &out, &LayoutOptions::new(P::Generic, "m")).unwrap();
        assert_eq!(files(&out), ["Textures/new.dds", "Textures/old.dds"]);
        // Later file replaced the earlier one under the existing spelling.
        assert_eq!(
            fs::read(out.join("Textures/old.dds")).unwrap(),
            b"textures/OLD.dds"
        );
    }

    // ---- generic ----

    #[test]
    fn generic_keeps_structure_and_docs() {
        let r = run(P::Generic, "m", &["Wrapper/file.bin", "readme.txt"]);
        assert_eq!(files(&r.out), ["Wrapper/file.bin", "readme.txt"]);
        assert_eq!(r.outcome.root, ModRoot::Deploy);
    }

    // ---- bethesda ----

    #[test]
    fn bethesda_strips_wrapper_and_data_and_drops_docs() {
        let r = run(
            P::Bethesda,
            "Mod",
            &[
                "MyMod-1.0/Data/textures/x.dds",
                "MyMod-1.0/Data/mod.esp",
                "MyMod-1.0/readme.txt",
            ],
        );
        assert_eq!(files(&r.out), ["mod.esp", "textures/x.dds"]);
        assert_eq!(r.outcome.root, ModRoot::Deploy);
    }

    #[test]
    fn bethesda_exe_or_data_with_siblings_targets_game_root() {
        let r = run(
            P::Bethesda,
            "SKSE",
            &[
                "skse_1/skse64_loader.exe",
                "skse_1/Data/Scripts/a.pex",
                "skse_1/readme.txt",
            ],
        );
        assert_eq!(r.outcome.root, ModRoot::Game);
        assert_eq!(files(&r.out), ["Data/Scripts/a.pex", "skse64_loader.exe"]);

        // A wrapper named like a deploy folder ("skse") is still stripped
        // when it holds Data/ (legacy L883-886).
        let r = run(
            P::Bethesda,
            "x",
            &["SKSE/Data/SKSE/Plugins/p.dll", "SKSE/src/x.cpp"],
        );
        assert_eq!(r.outcome.root, ModRoot::Game);
        assert_eq!(files(&r.out), ["Data/SKSE/Plugins/p.dll", "src/x.cpp"]);
    }

    #[test]
    fn bethesda_loose_plugin_stays_at_deploy_root() {
        let r = run(
            P::Bethesda,
            "x",
            &["Cool Mod/cool.esp", "Cool Mod/meshes/a.nif"],
        );
        assert_eq!(files(&r.out), ["cool.esp", "meshes/a.nif"]);
        assert_eq!(r.outcome.root, ModRoot::Deploy);
    }

    #[test]
    fn wrapper_stripping_stops_at_fomod_and_deploy_folders() {
        let r = run(P::Bethesda, "x", &["textures/a.dds"]);
        assert_eq!(files(&r.out), ["textures/a.dds"]);
        let r = run(P::Cyberpunk, "x", &["fomod/ModuleConfig.xml"]);
        assert_eq!(files(&r.out), ["fomod/ModuleConfig.xml"]);
        // strip_wrappers=false (post-FOMOD) keeps the wrapper.
        let mut o = LayoutOptions::new(P::Bethesda, "x");
        o.strip_wrappers = false;
        let r = run_with(&["Wrap/Wrap2/a.esp"], o);
        assert_eq!(files(&r.out), ["Wrap/Wrap2/a.esp"]);
    }

    // ---- variants ----

    #[test]
    fn variant_folders_preset_chooser_and_default() {
        let paths = [
            "Mod/1k/textures/a.dds",
            "Mod/2k/textures/a.dds",
            "Mod/readme.txt",
        ];
        let r = run(P::Bethesda, "Mod", &paths);
        assert_eq!(r.outcome.variant.as_deref(), Some("1k"));
        assert_eq!(r.outcome.variants, ["1k", "2k"]);
        assert_eq!(r.outcome.warnings.len(), 1);
        assert_eq!(
            fs::read(r.out.join("textures/a.dds")).unwrap(),
            b"Mod/1k/textures/a.dds"
        );

        let mut o = LayoutOptions::new(P::Bethesda, "Mod");
        o.preset_variant = Some("2k");
        let r = run_with(&paths, o);
        assert_eq!(r.outcome.variant.as_deref(), Some("2k"));
        assert!(r.outcome.warnings.is_empty());
        assert_eq!(
            fs::read(r.out.join("textures/a.dds")).unwrap(),
            b"Mod/2k/textures/a.dds"
        );

        let pick_last = |c: &[String]| Some(c.len() - 1);
        let mut o = LayoutOptions::new(P::Bethesda, "Mod");
        o.choose_variant = Some(&pick_last);
        let r = run_with(&paths, o);
        assert_eq!(r.outcome.variant.as_deref(), Some("2k"));

        let mut o = LayoutOptions::new(P::Bethesda, "Mod");
        o.detect_variants = false;
        let r = run_with(&paths, o);
        assert_eq!(r.outcome.variant, None);
        assert_eq!(files(&r.out), ["1k/textures/a.dds", "2k/textures/a.dds"]);
    }

    // ---- cyberpunk ----

    #[test]
    fn cyberpunk_loose_files_by_extension() {
        let r = run(
            P::Cyberpunk,
            "My Mod",
            &[
                "a.archive",
                "b.xl",
                "c.reds",
                "d.yaml",
                "e.lua",
                "f.dll",
                "g.ini",
                "readme.md",
            ],
        );
        assert_eq!(
            files(&r.out),
            [
                "archive/pc/mod/a.archive",
                "archive/pc/mod/b.xl",
                "bin/x64/plugins/cyber_engine_tweaks/mods/My_Mod/e.lua",
                "g.ini",
                "r6/scripts/My_Mod/c.reds",
                "r6/tweaks/My_Mod/d.yaml",
                "red4ext/plugins/My_Mod/f.dll",
            ]
        );
    }

    #[test]
    fn cyberpunk_root_init_lua_goes_to_cet_mods() {
        let r = run(
            P::Cyberpunk,
            "Cet Thing",
            &["init.lua", "lib/x.lua", "readme.txt"],
        );
        assert_eq!(
            files(&r.out),
            [
                "bin/x64/plugins/cyber_engine_tweaks/mods/Cet_Thing/init.lua",
                "bin/x64/plugins/cyber_engine_tweaks/mods/Cet_Thing/lib/x.lua",
                "bin/x64/plugins/cyber_engine_tweaks/mods/Cet_Thing/readme.txt",
            ]
        );
    }

    #[test]
    fn cyberpunk_folders_by_content() {
        let r = run(
            P::Cyberpunk,
            "m",
            &[
                "archive/pc/mod/x.archive",
                "cetmod/init.lua",
                "scripts1/s.reds",
                "packed/deep/y.archive",
                "packed/deep/note.ini",
                "weird/thing.bin",
                "root.ini",
                "readme.txt",
            ],
        );
        assert_eq!(
            files(&r.out),
            [
                "archive/pc/mod/x.archive",
                "archive/pc/mod/y.archive",
                "bin/x64/plugins/cyber_engine_tweaks/mods/cetmod/init.lua",
                "r6/scripts/scripts1/s.reds",
                "root.ini",
                "weird/thing.bin",
            ]
        );
        assert_eq!(r.outcome.warnings.len(), 1);
        assert!(r.outcome.warnings[0].contains("weird"));
    }

    // ---- stardew / bannerlord ----

    #[test]
    fn stardew_manifest_at_root_and_nested() {
        let r = run(P::Stardew, "Cool Mod", &["manifest.json", "Cool.dll"]);
        assert_eq!(
            files(&r.out),
            ["Cool_Mod/Cool.dll", "Cool_Mod/manifest.json"]
        );

        let r = run(
            P::Stardew,
            "Pack",
            &[
                "Pack/[CP] A/manifest.json",
                "Pack/[CP] A/assets/x.png",
                "Pack/[CP] A/Inner/manifest.json",
                "Pack/B/MANIFEST.JSON",
                "Pack/readme.txt",
            ],
        );
        assert_eq!(
            files(&r.out),
            [
                "B/MANIFEST.JSON",
                "[CP] A/Inner/manifest.json",
                "[CP] A/assets/x.png",
                "[CP] A/manifest.json"
            ]
        );

        let r = run(P::Stardew, "x", &["Mods/thing.dll", "readme.txt"]);
        assert_eq!(files(&r.out), ["Mods/thing.dll"]);
    }

    #[test]
    fn bannerlord_submodule_marker() {
        let r = run(P::Bannerlord, "Calradia+", &["SubModule.xml", "bin/x.dll"]);
        assert_eq!(
            files(&r.out),
            ["Calradia/SubModule.xml", "Calradia/bin/x.dll"]
        );
        let r = run(
            P::Bannerlord,
            "x",
            &["Modules/MyMod/SubModule.xml", "Modules/MyMod/bin/a.dll"],
        );
        assert_eq!(files(&r.out), ["MyMod/SubModule.xml", "MyMod/bin/a.dll"]);
    }

    // ---- bepinex ----

    #[test]
    fn bepinex_places_into_bepinex_tree() {
        let r = run(
            P::Bepinex,
            "Thing",
            &[
                "Wrap/plugins/p.dll",
                "Wrap/Other/o.dll",
                "Wrap/thing.cfg",
                "Wrap/thing.dll",
                "Wrap/readme.md",
            ],
        );
        assert_eq!(
            files(&r.out),
            [
                "BepInEx/config/thing.cfg",
                "BepInEx/plugins/Other/o.dll",
                "BepInEx/plugins/Thing/thing.dll",
                "BepInEx/plugins/p.dll",
            ]
        );
        let r = run(
            P::Bepinex,
            "x",
            &["BepInEx/core/a.dll", "winhttp.dll", "doorstop_config.ini"],
        );
        assert_eq!(
            files(&r.out),
            ["BepInEx/core/a.dll", "doorstop_config.ini", "winhttp.dll"]
        );
    }

    // ---- melonloader ----

    #[test]
    fn melonloader_dlls_to_mods() {
        let r = run(
            P::Melonloader,
            "x",
            &["a.dll", "c.cfg", "readme.txt", "Sub/b.dll"],
        );
        assert_eq!(files(&r.out), ["Mods/a.dll", "Sub/b.dll", "c.cfg"]);
        // Already shaped like the game root (UserData is a deploy folder): as-is.
        let r = run(
            P::Melonloader,
            "x",
            &["a.dll", "UserData/c.cfg", "readme.txt"],
        );
        assert_eq!(files(&r.out), ["UserData/c.cfg", "a.dll"]);
    }

    // ---- ue ----

    #[test]
    fn paks_dir_with_parent_component_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        mk(&src, &["a_P.pak"]);
        let out = t.path().join("out");
        for bad in ["../escape", "/abs", "Pal/./x"] {
            let mut o = LayoutOptions::new(P::Ue, "x");
            o.paks_dir = Some(bad);
            assert!(normalize_layout(&src, &out, &o).is_err(), "{bad}");
        }
        assert!(!t.path().join("escape").exists());
        assert!(src.join("a_P.pak").exists(), "nothing moved");
    }

    #[test]
    fn non_utf8_names_are_refused_not_mangled() {
        use std::os::unix::ffi::OsStrExt;
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        fs::create_dir_all(&src).unwrap();
        let bad = std::ffi::OsStr::from_bytes(b"caf\xe9.esp");
        fs::write(src.join(bad), "x").unwrap();
        assert!(staged_files(&src).is_err());
        let out = t.path().join("out");
        let o = LayoutOptions::new(P::Generic, "x");
        assert!(normalize_layout(&src, &out, &o).is_err());
        assert!(
            src.join(bad).exists(),
            "left in place, not renamed to U+FFFD"
        );
    }

    #[test]
    fn placer_refuses_parent_components() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("f");
        fs::write(&f, "x").unwrap();
        let out = t.path().join("out");
        let mut p = Placer::new(&out);
        assert!(p.place(&f, "a/../../escape").is_err());
        assert!(!t.path().join("escape").exists());
        assert!(f.exists());
    }

    #[test]
    fn ue_flattens_paks_into_paks_dir() {
        let mut o = LayoutOptions::new(P::Ue, "x");
        o.paks_dir = Some("Pal/Content/Paks/~mods");
        let r = run_with(
            &[
                "Wrap/v1/a_P.pak",
                "Wrap/v1/a_P.utoc",
                "Wrap/v1/a_P.ucas",
                "Wrap/readme.txt",
            ],
            o,
        );
        assert_eq!(
            files(&r.out),
            [
                "Pal/Content/Paks/~mods/a_P.pak",
                "Pal/Content/Paks/~mods/a_P.ucas",
                "Pal/Content/Paks/~mods/a_P.utoc"
            ]
        );
        // Already shaped like the game root (first paks_dir component): as-is.
        let mut o = LayoutOptions::new(P::Ue, "x");
        o.paks_dir = Some("Pal/Content/Paks/~mods");
        let r = run_with(&["pal/Binaries/Win64/ue4ss/x.dll", "readme.txt"], o);
        assert_eq!(files(&r.out), ["pal/Binaries/Win64/ue4ss/x.dll"]);
    }

    // ---- witcher3 ----

    #[test]
    fn witcher3_content_folder_becomes_mods_entry() {
        let r = run(
            P::Witcher3,
            "Better Things",
            &["Wrapper/content/blob0.bundle", "Wrapper/readme.txt"],
        );
        assert_eq!(
            files(&r.out),
            [
                "mods/modBetter_Things/content/blob0.bundle",
                "mods/modBetter_Things/readme.txt"
            ]
        );
        let r = run(P::Witcher3, "modX", &["content/a.bundle"]);
        assert_eq!(files(&r.out), ["mods/modX/content/a.bundle"]);
    }

    #[test]
    fn witcher3_mod_and_dlc_folders() {
        let r = run(
            P::Witcher3,
            "x",
            &[
                "modFoo/content/a",
                "dlcBar/content/b",
                "bin/config/c.xml",
                "other/d",
                "e.ini",
                "f.txt",
            ],
        );
        assert_eq!(
            files(&r.out),
            [
                "bin/config/c.xml",
                "dlc/dlcBar/content/b",
                "e.ini",
                "mods/modFoo/content/a",
                "other/d"
            ]
        );
    }

    // ---- staged file list ----

    #[test]
    fn staged_files_lists_hash_size_and_mode() {
        let t = tempfile::tempdir().unwrap();
        mk(t.path(), &["b/x.dds", "a.esp", "empty/"]);
        let v = staged_files(t.path()).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].path, "a.esp");
        assert_eq!(v[0].size, 5);
        assert_eq!(
            v[0].sha256,
            "0d9869a63d685a81a1e07b17b5064459c9b47b264d1a98edf415c300e179e08d"
        );
        assert_eq!(v[0].sha256.len(), 64);
        assert_eq!(v[1].path, "b/x.dds");
        assert!(v[1].mode & 0o400 != 0);
    }
}
