// SPDX-License-Identifier: GPL-3.0-or-later
//! M5: Nexus collections (legacy `collection_*` L2125-2808, legacy_spec §11).
//!
//! Step-wise API so the CLI and the GUI can drive their own prompts and
//! progress: [`fetch_revision`] (GraphQL, revision pin, adult flag) and
//! [`fetch_manifest`] (collection archive or API fallback), then optional
//! selection ([`parse_selection`], [`select`]), dedupe against installed
//! mods ([`plan`]), archive sources ([`find_bundled`], [`direct_file_name`]),
//! per-mod [`ManifestMod::install_meta`] (FOMOD choices in `collection`
//! mode) and [`finish`] (load order from `modRules`, membership, record).
//! Removal and the update "dropped mods" split live in [`db`].

pub mod db;
pub mod order;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::archive::{self, ArchiveError};
use crate::download::{DownloadError, FetchRequest, Fetcher};
use crate::fomod::{FomodChoices, FomodMode};
use crate::mods::{InstallMeta, ModRecord, ModsError};
use crate::nexus::{NexusClient, NexusError};
use crate::store::{Store, StoreError};

pub use db::{
    detach, dropped_members, exclusive_members, get, list, mod_rules, plugin_rules, record, remove,
    CollectionRecord, CollectionSummary, Dropped, NewRecord, PluginRule, Removed,
};
pub use order::{reorder_block, rule_order, Ordered};

/// Errors from this module. Messages are user-facing.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CollectionError {
    #[error("cannot parse collection reference '{0}'")]
    BadRef(String),
    #[error("collection '{0}' not found or not published")]
    NotFound(String),
    #[error("collection '{0}' is not installed")]
    NotInstalled(String),
    #[error("collection.json is invalid: {0}")]
    InvalidManifest(String),
    #[error(transparent)]
    Nexus(#[from] NexusError),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    #[error(transparent)]
    Mods(#[from] ModsError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl From<rusqlite::Error> for CollectionError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(StoreError::Sqlite(e))
    }
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> CollectionError + '_ {
    move |source| CollectionError::Io {
        path: path.to_path_buf(),
        source,
    }
}

type R<T> = Result<T, CollectionError>;

// ── references ──────────────────────────────────────────────

/// A parsed collection reference (legacy `parse_collection_ref` L2125-2139).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionRef {
    /// Game domain; `None` for a bare slug (use the active game).
    pub game: Option<String>,
    pub slug: String,
    pub revision: Option<u64>,
}

fn is_slug(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn is_domain(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// `<game>/collections/<slug>[/revisions/<n>]` at the start of `rest`.
fn parse_tail(game: &str, rest: &str) -> Option<CollectionRef> {
    let game = game.to_ascii_lowercase();
    let rest = rest.strip_prefix("collections/")?;
    let end = |c: char| matches!(c, '/' | '?' | '#');
    let slug: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    let after = &rest[slug.len()..];
    if !is_slug(&slug) || !is_domain(&game) || after.chars().next().is_some_and(|c| !end(c)) {
        return None;
    }
    let revision = after
        .strip_prefix("/revisions/")
        .map(|r| {
            r.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .filter(|d| !d.is_empty() && d.len() <= 19)
        .and_then(|d| d.parse().ok());
    Some(CollectionRef {
        game: Some(game),
        slug,
        revision,
    })
}

/// Accepts `nxm://<game>/collections/<slug>[/revisions/<n>]`,
/// `…nexusmods.com/games/<game>/collections/<slug>[…]`,
/// `…nexusmods.com/<game>/collections/<slug>[…]` and a bare slug of 3-12
/// letters or digits.
pub fn parse_ref(s: &str) -> Option<CollectionRef> {
    let s = s.trim();
    if let Some(rest) = s
        .get(..6)
        .filter(|p| p.eq_ignore_ascii_case("nxm://"))
        .map(|_| &s[6..])
    {
        let (game, tail) = rest.split_once('/')?;
        return parse_tail(game, tail);
    }
    if let Some(i) = s.to_ascii_lowercase().find("nexusmods.com/") {
        let rest = &s[i + "nexusmods.com/".len()..];
        let rest = rest.strip_prefix("games/").unwrap_or(rest);
        let (game, tail) = rest.split_once('/')?;
        return parse_tail(game, tail);
    }
    ((3..=12).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| {
        CollectionRef {
            game: None,
            slug: s.to_string(),
            revision: None,
        }
    })
}

// ── GraphQL revision ────────────────────────────────────────

/// Legacy `GQL_COLLECTION` (L2163-2170).
pub const REVISION_QUERY: &str = "query Rev($slug: String, $domainName: String, $revision: Int, $viewAdultContent: Boolean) {
  collectionRevision(slug: $slug, domainName: $domainName, revision: $revision, viewAdultContent: $viewAdultContent) {
    revisionNumber revisionStatus downloadLink totalSize assetsSizeBytes modCount createdAt
    collection { name summary slug latestPublishedRevision { revisionNumber } user { name } game { domainName name } }
    modFiles { fileId optional file { modId name version sizeInBytes mod { name author } } }
    externalResources { name resourceType resourceUrl }
  }
}";

/// Numbers that the API sends as numbers or strings.
fn num(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn text(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// One `collectionRevision` as returned by GraphQL.
#[derive(Debug, Clone, PartialEq)]
pub struct Revision {
    /// Game domain the query named.
    pub game: String,
    pub slug: String,
    pub number: u64,
    pub status: String,
    pub name: String,
    pub author: String,
    pub summary: String,
    pub game_name: String,
    /// Latest published revision, if reported.
    pub latest: Option<u64>,
    pub total_size: u64,
    pub mod_count: u64,
    pub optional_count: usize,
    /// API-relative path of the collection archive link.
    pub download_link: Option<String>,
    pub raw: Value,
}

impl Revision {
    fn from_value(game: &str, slug: &str, raw: Value) -> R<Self> {
        let number = num(raw.get("revisionNumber"))
            .ok_or_else(|| CollectionError::NotFound(slug.to_string()))?;
        let c = raw.get("collection").cloned().unwrap_or(Value::Null);
        let optional_count = raw
            .get("modFiles")
            .and_then(Value::as_array)
            .map_or(0, |a| {
                a.iter()
                    .filter(|m| m.get("optional").and_then(Value::as_bool) == Some(true))
                    .count()
            });
        Ok(Self {
            game: game.to_string(),
            slug: slug.to_string(),
            number,
            status: text(raw.get("revisionStatus")).unwrap_or_default(),
            name: text(c.get("name")).unwrap_or_else(|| slug.to_string()),
            author: text(c.pointer("/user/name")).unwrap_or_default(),
            summary: text(c.get("summary")).unwrap_or_default(),
            game_name: text(c.pointer("/game/name")).unwrap_or_default(),
            latest: num(c.pointer("/latestPublishedRevision/revisionNumber")),
            total_size: num(raw.get("totalSize")).unwrap_or(0),
            mod_count: num(raw.get("modCount")).unwrap_or(0),
            optional_count,
            download_link: text(raw.get("downloadLink")),
            raw,
        })
    }

    /// `externalResources` (collection-level requirements).
    pub fn external_resources(&self) -> Value {
        self.raw
            .get("externalResources")
            .filter(|v| v.is_array())
            .cloned()
            .unwrap_or_else(|| json!([]))
    }
}

/// Queries a revision (`None` = latest). `view_adult` is the `adult`
/// setting; it is the only call that sends it (legacy L2176, L2681; the
/// legacy `collection update` forced `false`, ambiguity A6: here the setting
/// applies to every collection query).
pub async fn fetch_revision(
    client: &NexusClient,
    game: &str,
    slug: &str,
    revision: Option<u64>,
    view_adult: bool,
) -> R<Revision> {
    if !is_domain(game) || !is_slug(slug) {
        return Err(CollectionError::BadRef(format!("{game}/{slug}")));
    }
    let vars = json!({ "slug": slug, "domainName": game, "revision": revision });
    let data = client
        .collection_graphql(REVISION_QUERY, vars, view_adult)
        .await?;
    match data.get("collectionRevision") {
        Some(v) if v.is_object() => Revision::from_value(game, slug, v.clone()),
        _ => Err(CollectionError::NotFound(slug.to_string())),
    }
}

/// `<data-dir>/<game>/collections/<slug>/rev<N>`.
pub fn revision_dir(data_dir: &Path, game: &str, slug: &str, revision: u64) -> PathBuf {
    data_dir
        .join(game)
        .join("collections")
        .join(slug)
        .join(format!("rev{revision}"))
}

// ── manifest (collection.json) ──────────────────────────────

/// One `mods[]` entry of collection.json.
#[derive(Debug, Clone, PartialEq)]
pub struct ManifestMod {
    /// Index in `mods[]`.
    pub index: usize,
    pub name: String,
    pub version: Option<String>,
    pub optional: bool,
    /// `source.type`: nexus (default), bundle, direct, browse, manual, ...
    pub source_type: String,
    pub mod_id: Option<u64>,
    pub file_id: Option<u64>,
    /// Lower-case.
    pub md5: Option<String>,
    pub file_size: u64,
    pub raw: Value,
}

impl ManifestMod {
    fn from_value(index: usize, raw: Value) -> Self {
        let src = raw.get("source").cloned().unwrap_or(Value::Null);
        let mod_id = num(src.get("modId"));
        let name = text(raw.get("name"))
            .or_else(|| text(src.get("logicalFilename")))
            .unwrap_or_else(|| format!("mod {}", mod_id.map_or(String::new(), |m| m.to_string())));
        Self {
            index,
            name,
            version: text(raw.get("version")),
            optional: raw.get("optional").and_then(Value::as_bool) == Some(true),
            source_type: text(src.get("type")).unwrap_or_else(|| "nexus".into()),
            mod_id,
            file_id: num(src.get("fileId")),
            md5: text(src.get("md5")).map(|m| m.to_ascii_lowercase()),
            file_size: num(src.get("fileSize")).unwrap_or(0),
            raw,
        }
    }

    fn src(&self, key: &str) -> Option<&str> {
        self.raw
            .get("source")?
            .get(key)?
            .as_str()
            .filter(|s| !s.is_empty())
    }

    /// `source.url` (direct / browse / manual).
    pub fn url(&self) -> Option<&str> {
        self.src("url")
    }

    /// Curator's download instructions (browse / manual).
    pub fn instructions(&self) -> Option<&str> {
        self.src("instructions").or_else(|| {
            self.raw
                .get("instructions")?
                .as_str()
                .filter(|s| !s.is_empty())
        })
    }

    /// Recorded FOMOD choices (Vortex format), if any.
    pub fn choices(&self) -> Option<FomodChoices> {
        self.raw
            .get("choices")
            .filter(|v| !v.is_null())
            .and_then(FomodChoices::from_value)
    }

    /// Install metadata (legacy L2491-2496). `fomod_mode` is the
    /// `--fomod` value: `collection` replays [`Self::choices`] and never asks.
    pub fn install_meta(&self, fomod_mode: FomodMode) -> InstallMeta {
        let s = |v: Option<&Value>| text(v);
        InstallMeta {
            name: Some(self.name.clone()),
            version: self.version.clone(),
            nexus_mod_id: self.mod_id,
            nexus_file_id: self.file_id,
            md5: self.md5.clone(),
            source: Some(self.source_type.clone()),
            author: s(self.raw.get("author")),
            category: s(self.raw.pointer("/details/category")),
            update_policy: self.src("updatePolicy").map(str::to_owned),
            fomod_mode,
            fomod_choices: self.choices(),
            ..Default::default()
        }
    }
}

/// A parsed collection.json.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub mods: Vec<ManifestMod>,
    pub raw: Value,
}

impl Manifest {
    pub fn from_value(raw: Value) -> R<Self> {
        let mods = raw
            .get("mods")
            .and_then(Value::as_array)
            .ok_or_else(|| CollectionError::InvalidManifest("mods is not an array".into()))?
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, m)| ManifestMod::from_value(i, m))
            .collect();
        Ok(Self { mods, raw })
    }

    pub fn load(path: &Path) -> R<Self> {
        let text = std::fs::read_to_string(path).map_err(io_err(path))?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|e| CollectionError::InvalidManifest(format!("{}: {e}", path.display())))?;
        Self::from_value(v)
    }

    /// `modRules` (load-order rules; only before/after are applied).
    pub fn mod_rules(&self) -> &[Value] {
        self.raw
            .get("modRules")
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    }

    /// `info.installInstructions`.
    pub fn install_instructions(&self) -> Option<&str> {
        self.raw
            .pointer("/info/installInstructions")?
            .as_str()
            .filter(|s| !s.trim().is_empty())
    }

    /// Minimal manifest from the API `modFiles` (no choices, no rules;
    /// legacy L2213-2223).
    pub fn from_revision(rev: &Revision) -> Self {
        let d = rev.game.as_str();
        let mods: Vec<Value> = rev
            .raw
            .get("modFiles")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|f| {
                        let file = f.get("file").cloned().unwrap_or(Value::Null);
                        json!({
                            "name": text(file.pointer("/mod/name")).or_else(|| text(file.get("name"))),
                            "version": file.get("version"),
                            "optional": f.get("optional").and_then(Value::as_bool).unwrap_or(false),
                            "domainName": d,
                            "author": text(file.pointer("/mod/author")).unwrap_or_default(),
                            "source": {
                                "type": "nexus",
                                "modId": num(file.get("modId")),
                                "fileId": num(f.get("fileId")),
                                "fileSize": num(file.get("sizeInBytes")).unwrap_or(0),
                            }
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let raw = json!({
            "info": { "name": rev.name, "author": rev.author, "domainName": d },
            "mods": mods,
            "modRules": [],
        });
        Self::from_value(raw).expect("mods is an array")
    }
}

/// [`fetch_manifest`] result.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchedManifest {
    pub manifest: Manifest,
    pub dir: PathBuf,
    /// Built from the API mod list because the archive was unavailable
    /// (no FOMOD choices, no load-order rules).
    pub from_api: bool,
    /// Why the archive could not be used, when `from_api`.
    pub archive_error: Option<String>,
}

/// First `collection.json` (case-insensitive) at depth 1-2 under `root`.
fn find_collection_json(root: &Path) -> Option<PathBuf> {
    walkdir::WalkDir::new(root)
        .min_depth(1)
        .max_depth(2)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("collection.json")
        })
        .min_by_key(|e| e.depth())
        .map(|e| e.into_path())
}

async fn fetch_archive(
    client: &NexusClient,
    fetcher: &Fetcher,
    link: &str,
    dir: &Path,
) -> Result<(), String> {
    let uri = client
        .collection_archive_uri(link)
        .await
        .map_err(|e| e.to_string())?;
    let dest = dir.join("collection.archive");
    fetcher
        .fetch(FetchRequest {
            url: &uri,
            dest: &dest,
            md5: None,
            sha256: None,
            events: None,
        })
        .await
        .map_err(|e| e.to_string())?;
    let content = dir.join("content");
    let cj = dir.join("collection.json");
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        if content.exists() {
            std::fs::remove_dir_all(&content).map_err(|e| e.to_string())?;
        }
        archive::extract(&dest, &content).map_err(|e| e.to_string())?;
        let found = find_collection_json(&content)
            .ok_or_else(|| "the collection archive has no collection.json".to_string())?;
        std::fs::copy(&found, &cj).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Stores `revision.json` in `dir` and makes sure `collection.json` exists:
/// kept from an earlier run, else taken from the collection archive, else
/// built from the API mod list (legacy `fetch_collection` L2174-2226).
pub async fn fetch_manifest(
    client: &NexusClient,
    fetcher: &Fetcher,
    rev: &Revision,
    dir: &Path,
) -> R<FetchedManifest> {
    std::fs::create_dir_all(dir).map_err(io_err(dir))?;
    let rj = dir.join("revision.json");
    std::fs::write(&rj, format!("{}\n", rev.raw)).map_err(io_err(&rj))?;
    let cj = dir.join("collection.json");
    let present = |p: &Path| std::fs::metadata(p).is_ok_and(|m| m.len() > 0);
    let mut archive_error = None;
    if !present(&cj) {
        archive_error = Some(match &rev.download_link {
            Some(link) => match fetch_archive(client, fetcher, link, dir).await {
                Ok(()) => String::new(),
                Err(e) => e,
            },
            None => "the revision has no download link".into(),
        })
        .filter(|e| !e.is_empty());
    }
    if present(&cj) {
        return Ok(FetchedManifest {
            manifest: Manifest::load(&cj)?,
            dir: dir.to_path_buf(),
            from_api: false,
            archive_error: None,
        });
    }
    let manifest = Manifest::from_revision(rev);
    let text = serde_json::to_string_pretty(&manifest.raw)
        .map_err(|e| CollectionError::InvalidManifest(e.to_string()))?;
    std::fs::write(&cj, text).map_err(io_err(&cj))?;
    Ok(FetchedManifest {
        manifest,
        dir: dir.to_path_buf(),
        from_api: true,
        archive_error,
    })
}

// ── optional mods ───────────────────────────────────────────

/// `--optional` (legacy L2385-2410).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OptionalMode {
    All,
    None,
    /// Prompt; `none` when non-interactive.
    #[default]
    Ask,
}

impl std::str::FromStr for OptionalMode {
    type Err = CollectionError;
    fn from_str(s: &str) -> R<Self> {
        match s {
            "all" => Ok(Self::All),
            "none" => Ok(Self::None),
            "ask" | "" => Ok(Self::Ask),
            other => Err(CollectionError::BadRef(format!(
                "--optional must be all, none or ask, not '{other}'"
            ))),
        }
    }
}

/// `all`, `none` or `1,3-5` (1-based) -> sorted 0-based indexes below
/// `max`; `None` on a malformed token (legacy `parse_selection` L2229-2247).
/// Out-of-range numbers are ignored.
pub fn parse_selection(text: &str, max: usize) -> Option<Vec<usize>> {
    let t = text.trim().to_lowercase().replace(',', " ");
    match t.trim() {
        "all" | "a" | "*" => return Some((0..max).collect()),
        "none" | "n" | "0" | "" => return Some(Vec::new()),
        _ => {}
    }
    let mut out = std::collections::BTreeSet::new();
    let n = |s: &str| -> Option<usize> {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok())?
    };
    for tok in t.split_whitespace() {
        let (a, b) = match tok.split_once('-') {
            Some((a, b)) => (n(a)?, n(b)?),
            None => (n(tok)?, n(tok)?),
        };
        out.extend((a.max(1)..=b.min(max)).map(|i| i - 1));
    }
    Some(out.into_iter().collect())
}

/// Indexes of the optional mods, in manifest order.
pub fn optional_indexes(m: &Manifest) -> Vec<usize> {
    m.mods
        .iter()
        .filter(|x| x.optional)
        .map(|x| x.index)
        .collect()
}

/// Required mods plus the picked optional ones (`picks` index into
/// [`optional_indexes`]), sorted.
pub fn select(m: &Manifest, picks: &[usize]) -> Vec<usize> {
    let opt = optional_indexes(m);
    let mut v: Vec<usize> = m
        .mods
        .iter()
        .filter(|x| !x.optional)
        .map(|x| x.index)
        .chain(picks.iter().filter_map(|&p| opt.get(p).copied()))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

// ── plan ────────────────────────────────────────────────────

/// Installed mod matching a manifest entry: md5 first, then
/// modId-fileId (legacy L2413-2425; later rows win like the legacy maps).
pub fn match_installed(mm: &ManifestMod, installed: &[ModRecord]) -> Option<i64> {
    let by_md5 = mm.md5.as_deref().and_then(|h| {
        installed
            .iter()
            .rev()
            .find(|r| r.md5.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(h)))
    });
    let by_file = || {
        let m = mm.mod_id?;
        installed
            .iter()
            .rev()
            .find(|r| r.nexus_mod_id == Some(m) && r.nexus_file_id == mm.file_id)
    };
    by_md5.or_else(by_file).map(|r| r.id)
}

/// What an install has to do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub selected: Vec<usize>,
    /// Manifest index -> installed row id (already present).
    pub have: BTreeMap<usize, i64>,
    /// Manifest indexes to acquire and install.
    pub todo: Vec<usize>,
    /// Bytes of the Nexus files to download (manifest `fileSize`).
    pub download_bytes: u64,
}

pub fn plan(m: &Manifest, selected: &[usize], installed: &[ModRecord]) -> Plan {
    let mut p = Plan {
        selected: selected.to_vec(),
        ..Default::default()
    };
    for &i in selected {
        let Some(mm) = m.mods.get(i) else { continue };
        match match_installed(mm, installed) {
            Some(id) => {
                p.have.insert(i, id);
            }
            None => {
                p.todo.push(i);
                if mm.source_type == "nexus" {
                    p.download_bytes += mm.file_size;
                }
            }
        }
    }
    p
}

// ── archive sources ─────────────────────────────────────────

/// A bundled file in `<dir>/content/bundled` whose name starts with
/// `source.fileExpression`, `source.logicalFilename` or the mod name
/// (case-insensitive; legacy `collection_bundled` L2612-2623).
pub fn find_bundled(dir: &Path, mm: &ManifestMod) -> Option<PathBuf> {
    let bdir = dir.join("content").join("bundled");
    let mut entries: Vec<(String, PathBuf)> = std::fs::read_dir(&bdir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| (e.file_name().to_string_lossy().to_lowercase(), e.path()))
        .collect();
    entries.sort();
    [
        mm.src("fileExpression"),
        mm.src("logicalFilename"),
        Some(mm.name.as_str()),
    ]
    .into_iter()
    .flatten()
    .map(str::to_lowercase)
    .filter(|w| !w.is_empty())
    .find_map(|want| {
        entries
            .iter()
            .find(|(n, _)| n.starts_with(&want))
            .map(|(_, p)| p.clone())
    })
}

/// Local file name for a direct download (legacy `collection_direct`
/// L2630-2631): the URL's last segment, else `<name>.archive`.
pub fn direct_file_name(url: &str, name: &str) -> String {
    let last = url
        .split(['?', '#'])
        .next()
        .and_then(|p| p.rsplit('/').next())
        .unwrap_or("");
    crate::download::file_name_from_uri(&format!("x/{last}"))
        .unwrap_or_else(|| format!("{}.archive", crate::staging::sanitize_name(name)))
}

/// Free bytes on the filesystem holding `path` (disk-space warning,
/// legacy L2437-2440).
pub fn free_bytes(path: &Path) -> Option<u64> {
    let mut p = path;
    while !p.exists() {
        p = p.parent()?;
    }
    let s = rustix::fs::statvfs(p).ok()?;
    Some(s.f_bavail.saturating_mul(s.f_frsize))
}

// ── finish ──────────────────────────────────────────────────

/// Input of [`finish`].
pub struct Finish<'a> {
    pub game: &'a str,
    pub revision: &'a Revision,
    pub manifest: &'a Manifest,
    pub dir: &'a Path,
    pub selected: &'a [usize],
    /// Manifest index -> row id of every present mod (had + installed).
    pub ids: &'a BTreeMap<usize, i64>,
    /// Names of the mods that failed.
    pub failed: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// Collection mods in their new load order.
    pub ordered: Vec<i64>,
    /// Mods appended because of a rule cycle.
    pub in_cycle: usize,
    pub rules: usize,
}

/// Applies the load order of the collection's `modRules` as one block at
/// the lowest position of its mods (legacy L2505-2522), then records the
/// collection and its membership (legacy L2524-2533). Rules are stored for
/// the plugin stage (T11).
pub fn finish(store: &mut Store, f: &Finish<'_>) -> R<Finished> {
    let present: Vec<usize> = f
        .selected
        .iter()
        .copied()
        .filter(|i| f.ids.contains_key(i))
        .collect();
    let ord = rule_order(f.manifest, &present);
    let mut ordered: Vec<i64> = Vec::new();
    for i in &ord.order {
        if let Some(id) = f.ids.get(i) {
            if !ordered.contains(id) {
                ordered.push(*id);
            }
        }
    }
    if !ordered.is_empty() {
        reorder_block(store, f.game, &ordered)?;
    }
    record(
        store,
        f.game,
        &NewRecord {
            slug: &f.revision.slug,
            name: &f.revision.name,
            author: &f.revision.author,
            revision: f.revision.number,
            dir: f.dir,
            manifest: &f.manifest.raw,
            rules: &Value::Array(f.manifest.mod_rules().to_vec()),
            external: &f.revision.external_resources(),
            failed: f.failed,
        },
        &ordered,
    )?;
    crate::mods::mark_changed(store, f.game)?;
    Ok(Finished {
        ordered,
        in_cycle: ord.in_cycle,
        rules: f.manifest.mod_rules().len(),
    })
}

#[cfg(test)]
mod tests;
