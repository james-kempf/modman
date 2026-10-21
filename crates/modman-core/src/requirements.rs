// SPDX-License-Identifier: GPL-3.0-or-later
//! M3: requirement report for `deps` (legacy `deps_py` L2949-3155,
//! `cmd_deps` L3488-3557): groups the cached Nexus requirements of the mods
//! in scope, detects the 13 external tools by file name, and applies the
//! per-game `deps ack` list.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use rusqlite::{params, Connection};

use crate::games::GameConfig;
use crate::mods::{list_mods, ModRecord, ModsError};
use crate::nexus::cached_requirements;
use crate::store::{self, Store};

/// A loader or script extender usually installed outside the mod manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tool {
    /// Ack key suffix: `tool:<key>`.
    pub key: &'static str,
    pub name: &'static str,
    /// Lower-cased file names only the real tool ships.
    pub files: &'static [&'static str],
}

/// The legacy tool table (L2978-2992), in match order.
pub const TOOLS: [Tool; 13] = [
    Tool {
        key: "ue4ss",
        name: "UE4SS",
        files: &["ue4ss.dll"],
    },
    Tool {
        key: "red4ext",
        name: "RED4ext",
        files: &["red4ext.dll"],
    },
    Tool {
        key: "cet",
        name: "Cyber Engine Tweaks",
        files: &["cyber_engine_tweaks.asi"],
    },
    Tool {
        key: "redscript",
        name: "redscript",
        files: &["scc.exe"],
    },
    Tool {
        key: "skse",
        name: "SKSE",
        files: &["skse64_loader.exe", "skse_loader.exe", "sksevr_loader.exe"],
    },
    Tool {
        key: "f4se",
        name: "F4SE",
        files: &["f4se_loader.exe"],
    },
    Tool {
        key: "sfse",
        name: "SFSE",
        files: &["sfse_loader.exe"],
    },
    Tool {
        key: "nvse",
        name: "xNVSE",
        files: &["nvse_loader.exe"],
    },
    Tool {
        key: "obse",
        name: "OBSE",
        files: &["obse_loader.exe", "obse64_loader.exe"],
    },
    Tool {
        key: "bepinex",
        name: "BepInEx",
        files: &["bepinex.core.dll", "bepinex.dll", "bepinex.preloader.dll"],
    },
    Tool {
        key: "melonloader",
        name: "MelonLoader",
        files: &["melonloader.dll"],
    },
    Tool {
        key: "smapi",
        name: "SMAPI",
        files: &["stardewmoddingapi.dll", "stardewmoddingapi.exe"],
    },
    Tool {
        key: "me2",
        name: "Mod Engine 2",
        files: &["modengine2_launcher.exe"],
    },
];

/// Regex `\w+` tokens of lower-cased text (for `\b...\b` matches).
fn words(t: &str) -> BTreeSet<&str> {
    t.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .collect()
}

/// Hand-written equivalents of the legacy tool regexes.
fn tool_matches(key: &str, t: &str, w: &BTreeSet<&str>) -> bool {
    let any_word = |list: &[&str]| list.iter().any(|x| w.contains(x));
    let any_sub = |list: &[&str]| list.iter().any(|x| t.contains(x));
    match key {
        "ue4ss" => any_sub(&["ue4ss", "unreal engine 4/5 scripting"]),
        "red4ext" => t.contains("red4ext"),
        "cet" => t.contains("cyber engine tweaks"),
        "redscript" => w.contains("redscript"),
        "skse" => any_word(&["skse", "skse64", "sksevr"]) || t.contains("skyrim script extender"),
        "f4se" => w.contains("f4se") || t.contains("fallout 4 script extender"),
        "sfse" => w.contains("sfse") || t.contains("starfield script extender"),
        "nvse" => any_word(&["nvse", "xnvse"]) || t.contains("new vegas script extender"),
        "obse" => any_word(&["obse", "obse64"]) || t.contains("oblivion script extender"),
        "bepinex" => t.contains("bepinex"),
        "melonloader" => any_sub(&["melonloader", "melon loader"]),
        "smapi" => {
            w.contains("smapi")
                || any_sub(&[
                    "stardewmoddingapi",
                    "stardew moddingapi",
                    "stardewmodding api",
                    "stardew modding api",
                ])
        }
        "me2" => any_sub(&["modengine2", "mod engine2", "modengine 2", "mod engine 2"]),
        _ => false,
    }
}

/// The tool a requirement name/URL refers to, if any.
pub fn tool_for(text: &str) -> Option<&'static Tool> {
    let t = text.to_lowercase();
    let w = words(&t);
    TOOLS.iter().find(|tool| tool_matches(tool.key, &t, &w))
}

/// Scheme, `www.` and fragments stripped; GitHub cut to owner/repo.
pub fn norm_url(u: &str) -> String {
    let mut u = u.trim().to_lowercase();
    if let Some(i) = u.find("://") {
        if u[..i].bytes().all(|b| b.is_ascii_lowercase()) && i > 0 {
            u = u[i + 3..].to_string();
        }
    }
    let u = u.strip_prefix("www.").unwrap_or(&u).to_string();
    if let Some(rest) = u.strip_prefix("github.com/") {
        let mut it = rest.splitn(3, '/');
        if let (Some(owner), Some(repo)) = (it.next(), it.next()) {
            let repo: String = repo
                .chars()
                .take_while(|c| !matches!(c, '?' | '#'))
                .collect();
            if !owner.is_empty() && !repo.is_empty() {
                return format!("github.com/{owner}/{repo}");
            }
        }
    }
    u.split('#')
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string()
}

/// `nexusmods.com/[games/]<domain>/mods/<id>` -> id when `<domain>` matches.
fn nexus_url_id(url: &str, domain: &str) -> Option<String> {
    let l = url.to_lowercase();
    let rest = &l[l.find("nexusmods.com/")? + "nexusmods.com/".len()..];
    let rest = rest.strip_prefix("games/").unwrap_or(rest);
    let (d, rest) = rest.split_once('/')?;
    if d != domain
        || d.is_empty()
        || !d
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return None;
    }
    let id: String = rest
        .strip_prefix("mods/")?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!id.is_empty()).then_some(id)
}

/// Row status, in report order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReqStatus {
    Missing,
    Disabled,
    External,
    Gone,
    Ok,
}

impl ReqStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Disabled => "disabled",
            Self::External => "external",
            Self::Gone => "gone",
            Self::Ok => "ok",
        }
    }
}

/// One requirement group (`deps --json` row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReqRow {
    pub status: ReqStatus,
    /// Ack key: `tool:<k>`, `nexus:<id>`, `nexus:<game>:<id>` or `url:<...>`.
    pub key: String,
    pub name: String,
    /// modman arguments that fix it, e.g. `install 123`.
    pub hint: String,
    pub urls: Vec<String>,
    /// Names of the mods (or collections) needing it.
    pub by: Vec<String>,
    pub detail: String,
    pub acked: bool,
}

/// A collection-level external resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraResource {
    pub name: String,
    pub url: String,
    pub by: String,
}

/// Which mods to analyse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    /// Row ids; `None` = all enabled mods.
    pub mods: Option<Vec<i64>>,
}

/// Pending change to a group: (key, Nexus id, URL, needed by).
type Edit = (String, Option<String>, Option<String>, Option<String>);

struct Group {
    key: String,
    name: String,
    nexus: Vec<String>,
    urls: Vec<String>,
    by: Vec<String>,
    tool: Option<&'static Tool>,
}

fn add_url(g: &mut Group, url: &str) {
    if url.is_empty() {
        return;
    }
    let k = norm_url(url);
    for u in g.urls.iter_mut() {
        if norm_url(u) == k {
            let archive = [".zip", ".7z", ".rar"]
                .iter()
                .any(|e| url.to_lowercase().ends_with(e));
            if url.len() > u.len() && !archive {
                *u = url.to_string();
            }
            return;
        }
    }
    g.urls.push(url.to_string());
}

/// Acked keys of a game.
pub fn acks(conn: &Connection, game: &str) -> Result<BTreeSet<String>, ModsError> {
    let mut stmt = conn.prepare("SELECT key FROM deps_ack WHERE game = ?1")?;
    let v = stmt
        .query_map([game], |r| r.get(0))?
        .collect::<Result<BTreeSet<String>, _>>()?;
    Ok(v)
}

pub fn ack(conn: &Connection, game: &str, key: &str) -> Result<(), ModsError> {
    conn.execute(
        "INSERT OR IGNORE INTO deps_ack (game, key, acked_at) VALUES (?1, ?2, ?3)",
        params![game, key, store::now()],
    )?;
    Ok(())
}

pub fn unack(conn: &Connection, game: &str, key: &str) -> Result<(), ModsError> {
    conn.execute(
        "DELETE FROM deps_ack WHERE game = ?1 AND key = ?2",
        params![game, key],
    )?;
    Ok(())
}

/// Collection `externalResources` for collections with a mod in scope.
fn collection_extras(
    conn: &Connection,
    game: &str,
    scope: &[i64],
) -> Result<Vec<ExtraResource>, ModsError> {
    let mut stmt = conn.prepare(
        "SELECT c.id, COALESCE(c.name, c.slug), c.external_resources FROM collections c
         WHERE c.game = ?1 AND c.external_resources IS NOT NULL",
    )?;
    let cols = stmt
        .query_map([game], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for (cid, cname, json) in cols {
        let mut m = conn.prepare("SELECT mod_id FROM collection_mods WHERE collection_id = ?1")?;
        let members = m
            .query_map([cid], |r| r.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if !members.iter().any(|id| scope.contains(id)) {
            continue;
        }
        let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&json)
        else {
            continue;
        };
        for it in items {
            let s = |k: &str| it.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let url = Some(s("resourceUrl"))
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| s("url"));
            out.push(ExtraResource {
                name: Some(s("name"))
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| "external resource".into()),
                url,
                by: format!("collection: {cname}"),
            });
        }
    }
    Ok(out)
}

/// Lower-cased file name -> group key, scanning `root` up to depth 8.
fn scan(
    root: &Path,
    wanted: &HashMap<String, String>,
    skip: &BTreeSet<String>,
    label: Option<&str>,
    found: &mut HashMap<String, String>,
) {
    if !root.is_dir() {
        return;
    }
    let targets: BTreeSet<&String> = wanted.values().collect();
    for e in walkdir::WalkDir::new(root)
        .max_depth(9)
        .into_iter()
        .filter_map(Result::ok)
    {
        if found.len() == targets.len() {
            return;
        }
        if !e.file_type().is_file() {
            continue;
        }
        let Some(k) = wanted.get(&e.file_name().to_string_lossy().to_lowercase()) else {
            continue;
        };
        let rel = e
            .path()
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if label.is_none() && skip.contains(&rel.to_lowercase()) {
            continue;
        }
        found
            .entry(k.clone())
            .or_insert_with(|| label.map_or(rel, str::to_string));
    }
}

/// Builds the requirement rows for `scope`, from the requirements cache only
/// (call `nexus::refresh_requirements` first to update it).
pub fn analyze(store: &Store, game: &GameConfig, scope: &Scope) -> Result<Vec<ReqRow>, ModsError> {
    let conn = store.conn();
    let mods = list_mods(conn, &game.domain)?;
    let in_scope: Vec<&ModRecord> = mods
        .iter()
        .filter(|m| match &scope.mods {
            Some(ids) => ids.contains(&m.id),
            None => m.enabled,
        })
        .collect();
    let mut enabled: HashMap<String, &ModRecord> = HashMap::new();
    let mut disabled: HashMap<String, &ModRecord> = HashMap::new();
    for m in &mods {
        if let Some(n) = m.nexus_mod_id {
            let map = if m.enabled {
                &mut enabled
            } else {
                &mut disabled
            };
            map.entry(n.to_string()).or_insert(m);
        }
    }
    let gid = game
        .nexus_game_id
        .map(|g| g.to_string())
        .unwrap_or_default();
    let domain = game.domain.as_str();
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut group = |key: String, name: &str, tool: Option<&'static Tool>| -> String {
        let g = groups.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            Group {
                key: key.clone(),
                name: name.to_string(),
                nexus: Vec::new(),
                urls: Vec::new(),
                by: Vec::new(),
                tool: None,
            }
        });
        if tool.is_some() {
            g.tool = tool;
        }
        key
    };
    let mut avail_cache: HashMap<String, bool> = HashMap::new();
    // (group key, Nexus id, URL, needed by)
    let mut edits: Vec<Edit> = Vec::new();
    for m in &in_scope {
        let Some(mid) = m.nexus_mod_id else { continue };
        let Some(reqs) = cached_requirements(store, domain, mid)?.and_then(|c| c.requirements)
        else {
            continue;
        };
        for r in reqs {
            let name = r
                .mod_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("unnamed requirement")
                .to_string();
            let ext = r.external_requirement;
            let rmod = r.mod_id.map(|v| v.to_string()).unwrap_or_default();
            let rgame = r.game_id.map(|v| v.to_string()).unwrap_or_default();
            let url = r.url.as_deref().unwrap_or("").trim().to_string();
            if !ext && rmod == mid.to_string() {
                continue;
            }
            let real_mod = !rmod.is_empty() && rmod != "0";
            let same_game = !ext && real_mod && (rgame.is_empty() || rgame == "0" || rgame == gid);
            let key = if let Some(t) = tool_for(&format!("{name} {url}")) {
                group(format!("tool:{}", t.key), t.name, Some(t))
            } else if same_game {
                group(format!("nexus:{rmod}"), &name, None)
            } else if !ext && real_mod {
                group(format!("nexus:{rgame}:{rmod}"), &name, None)
            } else {
                let n = norm_url(&url);
                group(
                    format!("url:{}", if n.is_empty() { name.to_lowercase() } else { n }),
                    &name,
                    None,
                )
            };
            let uid = if same_game {
                None
            } else {
                nexus_url_id(&url, domain)
            };
            let nexus = if same_game { Some(rmod.clone()) } else { uid };
            let url_add = nexus.is_none().then_some(url);
            edits.push((key, nexus, url_add, Some(m.name.clone())));
        }
    }
    let scope_ids: Vec<i64> = in_scope.iter().map(|m| m.id).collect();
    for e in collection_extras(conn, domain, &scope_ids)? {
        let key = if let Some(t) = tool_for(&format!("{} {}", e.name, e.url)) {
            group(format!("tool:{}", t.key), t.name, Some(t))
        } else {
            let n = norm_url(&e.url);
            group(
                format!(
                    "url:{}",
                    if n.is_empty() {
                        e.name.to_lowercase()
                    } else {
                        n
                    }
                ),
                &e.name,
                None,
            )
        };
        let uid = nexus_url_id(&e.url, domain);
        let url_add = uid.is_none().then(|| e.url.clone());
        edits.push((key, uid, url_add, Some(e.by.clone())));
    }
    for (key, nexus, url, by) in edits {
        let g = groups.get_mut(&key).expect("group created above");
        if let Some(n) = nexus {
            if !g.nexus.contains(&n) {
                g.nexus.push(n);
            }
        }
        if let Some(u) = url {
            add_url(g, &u);
        }
        if let Some(b) = by {
            if !g.by.contains(&b) {
                g.by.push(b);
            }
        }
    }

    // Tool files: game folder (minus files modman deployed), then enabled mods.
    let mut wanted: HashMap<String, String> = HashMap::new();
    for g in groups.values() {
        if let Some(t) = g.tool {
            for f in t.files {
                wanted.insert((*f).to_string(), g.key.clone());
            }
        }
    }
    let mut found: HashMap<String, String> = HashMap::new();
    if !wanted.is_empty() {
        let deployed: BTreeSet<String> = {
            let mut stmt = conn.prepare(
                "SELECT rel_path FROM deployments WHERE game = ?1 AND target = 'game_dir'",
            )?;
            let v = stmt
                .query_map([domain], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            v.into_iter().map(|s| s.to_lowercase()).collect()
        };
        if let Some(folder) = &game.folder {
            scan(folder, &wanted, &deployed, None, &mut found);
        }
        for m in mods.iter().filter(|m| m.enabled) {
            if let Some(dir) = &m.staging_dir {
                scan(
                    dir,
                    &wanted,
                    &BTreeSet::new(),
                    Some(&format!("provided by \"{}\"", m.name)),
                    &mut found,
                );
            }
        }
    }

    let acked = acks(conn, domain)?;
    let mut available = |id: &str| -> bool {
        *avail_cache.entry(id.to_string()).or_insert_with(|| {
            id.parse::<u64>()
                .ok()
                .and_then(|n| cached_requirements(store, domain, n).ok().flatten())
                .and_then(|c| c.available)
                != Some(false)
        })
    };
    let mut rows = Vec::new();
    for key in order {
        let g = &groups[&key];
        let have = g.nexus.iter().find_map(|i| enabled.get(i));
        let off = g.nexus.iter().find_map(|i| disabled.get(i));
        let mut hint = String::new();
        let mut detail = String::new();
        let status = if let Some(h) = have {
            detail = format!("installed: {}", h.name);
            ReqStatus::Ok
        } else if let Some(f) = found.get(&g.key) {
            detail = format!("found: {f}");
            ReqStatus::Ok
        } else if acked.contains(&g.key) {
            detail = "marked as installed".into();
            ReqStatus::Ok
        } else if let Some(o) = off {
            hint = format!("enable {}", o.uid);
            ReqStatus::Disabled
        } else {
            let live: Vec<&String> = g.nexus.iter().filter(|i| available(i)).collect();
            let gone: Vec<&String> = g.nexus.iter().filter(|i| !available(i)).collect();
            let st = if let Some(l) = live.first() {
                hint = format!("install {l}");
                ReqStatus::Missing
            } else if g.tool.is_some() {
                ReqStatus::Missing
            } else if !g.urls.is_empty() {
                ReqStatus::External
            } else {
                detail = "no longer on Nexus".into();
                ReqStatus::Gone
            };
            if let (Some(gid), true, Some(_)) = (gone.first(), live.is_empty(), g.tool) {
                detail = format!("its Nexus page (mod {gid}) is no longer available");
            }
            st
        };
        rows.push(ReqRow {
            status,
            key: g.key.clone(),
            name: g.name.clone(),
            hint,
            urls: g.urls.clone(),
            by: g.by.clone(),
            detail,
            acked: acked.contains(&g.key),
        });
    }
    rows.sort_by_key(|r| (r.status, r.name.to_lowercase()));
    Ok(rows)
}

/// `deps ack|unack <ref>` target: exact name (case-insensitive) or key wins,
/// else a unique name substring. `ack` considers missing/external rows,
/// `unack` only acked keys.
pub fn find_ack_target<'a>(
    rows: &'a [ReqRow],
    reference: &str,
    unack: bool,
) -> Result<&'a ReqRow, usize> {
    let q = reference.to_lowercase();
    let cands: Vec<&ReqRow> = rows
        .iter()
        .filter(|r| {
            if unack {
                r.acked
            } else {
                matches!(r.status, ReqStatus::External | ReqStatus::Missing)
            }
        })
        .collect();
    if let Some(r) = cands
        .iter()
        .find(|r| r.name.to_lowercase() == q || r.key == reference)
    {
        return Ok(r);
    }
    let subs: Vec<&&ReqRow> = cands
        .iter()
        .filter(|r| r.name.to_lowercase().contains(&q))
        .collect();
    match subs.as_slice() {
        [one] => Ok(one),
        many => Err(many.len()),
    }
}

/// Counts by status, for summaries.
pub fn counts(rows: &[ReqRow]) -> BTreeMap<ReqStatus, usize> {
    let mut m = BTreeMap::new();
    for r in rows {
        *m.entry(r.status).or_insert(0) += 1;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games;
    use crate::mods::tests::add;

    #[test]
    fn tool_patterns_match_legacy_regexes() {
        let k = |t: &str| tool_for(t).map(|t| t.key);
        assert_eq!(k("SKSE64"), Some("skse"));
        assert_eq!(k("Skyrim Script Extender (SKSE)"), Some("skse"));
        assert_eq!(k("skse64plugin"), None);
        assert_eq!(k("xNVSE https://github.com/xNVSE/NVSE"), Some("nvse"));
        assert_eq!(k("Melon Loader"), Some("melonloader"));
        assert_eq!(k("Stardew Modding API"), Some("smapi"));
        assert_eq!(k("ModEngine 2"), Some("me2"));
        assert_eq!(k("UE4SS for Palworld"), Some("ue4ss"));
        assert_eq!(k("redscripts"), None);
        assert_eq!(k("Address Library"), None);
        assert_eq!(TOOLS.len(), 13);
    }

    #[test]
    fn url_normalisation() {
        assert_eq!(
            norm_url("https://www.GitHub.com/a/b/releases/tag/v1"),
            "github.com/a/b"
        );
        assert_eq!(
            norm_url("http://skse.silverlock.org/#x"),
            "skse.silverlock.org"
        );
        assert_eq!(
            nexus_url_id(
                "https://www.nexusmods.com/games/sse/mods/12?tab=files",
                "sse"
            ),
            Some("12".into())
        );
        assert_eq!(
            nexus_url_id("https://www.nexusmods.com/other/mods/12", "sse"),
            None
        );
    }

    fn seed_reqs(s: &Store, game: &str, mod_id: i64, body: &str, available: Option<bool>) {
        s.conn()
            .execute(
                "INSERT INTO requirements_cache (game, nexus_mod_id, body, available, fetched_at, checked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![game, mod_id, body, available, store::now()],
            )
            .unwrap();
    }

    #[test]
    fn report_groups_tools_nexus_and_external_with_acks() {
        let s = Store::open_in_memory().unwrap();
        let mut g = GameConfig::custom("sse");
        g.nexus_game_id = Some(1704);
        games::save_game(s.conn(), &g).unwrap();
        add(&s, "sse", "UI-1-1", "Fancy UI", Some(1));
        add(&s, "sse", "Lib-2-2", "Some Lib", Some(2));
        s.conn()
            .execute("UPDATE mods SET enabled = 0 WHERE uid = 'Lib-2-2'", [])
            .unwrap();
        seed_reqs(
            &s,
            "sse",
            1,
            r#"[{"modId":0,"modName":"SKSE64","externalRequirement":true,"url":"https://skse.silverlock.org"},
                {"modId":2,"modName":"Some Lib","gameId":1704},
                {"modId":3,"modName":"Gone Lib","gameId":1704},
                {"modName":"Visual C++","externalRequirement":true,"url":"https://www.microsoft.com/vc"}]"#,
            Some(true),
        );
        seed_reqs(&s, "sse", 3, "[]", Some(false));
        let rows = analyze(&s, &g, &Scope::default()).unwrap();
        let st: Vec<(&str, ReqStatus)> = rows.iter().map(|r| (r.key.as_str(), r.status)).collect();
        assert_eq!(
            st,
            [
                ("tool:skse", ReqStatus::Missing),
                ("nexus:2", ReqStatus::Disabled),
                ("url:microsoft.com/vc", ReqStatus::External),
                ("nexus:3", ReqStatus::Gone),
            ]
        );
        assert_eq!(rows[1].hint, "enable Lib-2-2");
        assert_eq!(rows[0].by, ["Fancy UI"]);

        let target = find_ack_target(&rows, "skse", false).unwrap();
        ack(s.conn(), "sse", &target.key).unwrap();
        let rows = analyze(&s, &g, &Scope::default()).unwrap();
        let skse = rows.iter().find(|r| r.key == "tool:skse").unwrap();
        assert_eq!(
            (skse.status, skse.detail.as_str(), skse.acked),
            (ReqStatus::Ok, "marked as installed", true)
        );
        assert!(find_ack_target(&rows, "nothing", false).is_err());
        unack(s.conn(), "sse", "tool:skse").unwrap();
        assert!(acks(s.conn(), "sse").unwrap().is_empty());
    }

    #[test]
    fn tool_file_in_game_folder_counts_unless_deployed_by_modman() {
        let tmp = tempfile::tempdir().unwrap();
        let s = Store::open_in_memory().unwrap();
        let mut g = GameConfig::custom("sse");
        g.folder = Some(tmp.path().to_path_buf());
        games::save_game(s.conn(), &g).unwrap();
        add(&s, "sse", "UI-1-1", "Fancy UI", Some(1));
        seed_reqs(
            &s,
            "sse",
            1,
            r#"[{"modName":"SKSE64","externalRequirement":true,"url":"https://skse.silverlock.org"}]"#,
            Some(true),
        );
        std::fs::write(tmp.path().join("SKSE64_Loader.exe"), "x").unwrap();
        let rows = analyze(&s, &g, &Scope::default()).unwrap();
        assert_eq!(
            (rows[0].status, rows[0].detail.as_str()),
            (ReqStatus::Ok, "found: SKSE64_Loader.exe")
        );
        s.conn()
            .execute(
                "INSERT INTO deployments (game, target, rel_path, method, source, deployed_at)
                 VALUES ('sse', 'game_dir', 'SKSE64_Loader.exe', 'copy', '/s', 0)",
                [],
            )
            .unwrap();
        assert_eq!(
            analyze(&s, &g, &Scope::default()).unwrap()[0].status,
            ReqStatus::Missing
        );
    }
}
