// SPDX-License-Identifier: GPL-3.0-or-later
//! Download queue in the `downloads` table, and nxm:// intake. A row is
//! committed before any network I/O so a killed process leaves a resumable
//! row. URLs are stored without `key`/`expires`/`user_id` parameters.

use rusqlite::{params, OptionalExtension, Row};
use url::Url;

use super::{DownloadError, Fetched, NxmLink};
use crate::nexus::NxmKey;
use crate::store::{self, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadSource {
    Nxm,
    Nexus,
    Direct,
    Collection,
    Manual,
}

impl DownloadSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nxm => "nxm",
            Self::Nexus => "nexus",
            Self::Direct => "direct",
            Self::Collection => "collection",
            Self::Manual => "manual",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "nxm" => Self::Nxm,
            "nexus" => Self::Nexus,
            "direct" => Self::Direct,
            "collection" => Self::Collection,
            _ => Self::Manual,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Paused,
    Done,
    Failed,
    Cancelled,
}

impl DownloadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "downloading" => Self::Downloading,
            "paused" => Self::Paused,
            "done" => Self::Done,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Queued,
        }
    }
}

/// Fields for [`enqueue`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDownload {
    pub game: Option<String>,
    pub source: DownloadSource,
    pub url: Option<String>,
    pub nexus_mod_id: Option<u64>,
    pub nexus_file_id: Option<u64>,
    pub collection_slug: Option<String>,
    pub collection_revision: Option<u64>,
    pub nxm: Option<NxmKey>,
    pub file_name: Option<String>,
    pub dest_path: Option<String>,
    pub size: Option<u64>,
    pub md5: Option<String>,
}

impl NewDownload {
    pub fn new(source: DownloadSource) -> Self {
        Self {
            game: None,
            source,
            url: None,
            nexus_mod_id: None,
            nexus_file_id: None,
            collection_slug: None,
            collection_revision: None,
            nxm: None,
            file_name: None,
            dest_path: None,
            size: None,
            md5: None,
        }
    }
}

/// One `downloads` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub id: i64,
    pub game: Option<String>,
    pub source: DownloadSource,
    pub url: Option<String>,
    pub nexus_mod_id: Option<u64>,
    pub nexus_file_id: Option<u64>,
    pub collection_slug: Option<String>,
    pub collection_revision: Option<u64>,
    pub nxm: Option<NxmKey>,
    pub file_name: Option<String>,
    pub dest_path: Option<String>,
    pub part_path: Option<String>,
    pub size: Option<u64>,
    pub bytes_done: u64,
    pub md5: Option<String>,
    pub sha256: Option<String>,
    pub status: DownloadStatus,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

const COLUMNS: &str = "id, game, source, url, nexus_mod_id, nexus_file_id, collection_slug,
    collection_revision, nxm_key, nxm_expires, nxm_user_id, file_name, dest_path, part_path,
    size, bytes_done, md5, sha256, status, error, created_at, updated_at";

fn u(v: Option<i64>) -> Option<u64> {
    v.and_then(|n| u64::try_from(n).ok())
}

fn i(v: Option<u64>) -> Option<i64> {
    v.and_then(|n| i64::try_from(n).ok())
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<Download> {
    let key: Option<String> = r.get(8)?;
    let expires: Option<i64> = r.get(9)?;
    let nxm = match (key, expires) {
        (Some(key), Some(expires)) => Some(NxmKey {
            key,
            expires,
            user_id: u(r.get(10)?),
        }),
        _ => None,
    };
    Ok(Download {
        id: r.get(0)?,
        game: r.get(1)?,
        source: DownloadSource::parse(&r.get::<_, String>(2)?),
        url: r.get(3)?,
        nexus_mod_id: u(r.get(4)?),
        nexus_file_id: u(r.get(5)?),
        collection_slug: r.get(6)?,
        collection_revision: u(r.get(7)?),
        nxm,
        file_name: r.get(11)?,
        dest_path: r.get(12)?,
        part_path: r.get(13)?,
        size: u(r.get(14)?),
        bytes_done: u(r.get(15)?).unwrap_or(0),
        md5: r.get(16)?,
        sha256: r.get(17)?,
        status: DownloadStatus::parse(&r.get::<_, String>(18)?),
        error: r.get(19)?,
        created_at: r.get(20)?,
        updated_at: r.get(21)?,
    })
}

/// Drops secret query parameters before a URL is stored.
fn strip_secrets(url: &str) -> String {
    let Ok(mut parsed) = Url::parse(url) else {
        // Not a URL we can edit: keep only the part before the query.
        return url.split('?').next().unwrap_or_default().to_owned();
    };
    let kept: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| !matches!(k.as_ref(), "key" | "expires" | "user_id"))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        parsed.set_query(None);
    } else {
        parsed.query_pairs_mut().clear().extend_pairs(kept);
    }
    parsed.to_string()
}

/// Inserts a queued row and returns its id.
pub fn enqueue(store: &Store, d: &NewDownload) -> Result<i64, DownloadError> {
    let now = store::now();
    store.conn().execute(
        "INSERT INTO downloads (game, source, url, nexus_mod_id, nexus_file_id, collection_slug,
            collection_revision, nxm_key, nxm_expires, nxm_user_id, file_name, dest_path, part_path,
            size, md5, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 'queued', ?16, ?16)",
        params![
            d.game,
            d.source.as_str(),
            d.url.as_deref().map(strip_secrets),
            i(d.nexus_mod_id),
            i(d.nexus_file_id),
            d.collection_slug,
            i(d.collection_revision),
            d.nxm.as_ref().map(|k| k.key.as_str()),
            d.nxm.as_ref().map(|k| k.expires),
            d.nxm.as_ref().and_then(|k| i(k.user_id)),
            d.file_name,
            d.dest_path,
            d.dest_path.as_ref().map(|p| format!("{p}.part")),
            i(d.size),
            d.md5,
            now
        ],
    )?;
    Ok(store.conn().last_insert_rowid())
}

/// Queues a parsed nxm:// link. A link for a file that already has an
/// unfinished row refreshes that row's key (and re-queues it if it had
/// failed or was paused) instead of adding a duplicate.
pub fn enqueue_nxm(store: &Store, link: &NxmLink) -> Result<i64, DownloadError> {
    let now = store::now();
    match link {
        NxmLink::Mod(m) => {
            let existing: Option<i64> = store
                .conn()
                .query_row(
                    "SELECT id FROM downloads WHERE game = ?1 AND nexus_mod_id = ?2
                       AND nexus_file_id = ?3
                       AND status IN ('queued', 'downloading', 'paused', 'failed')
                     ORDER BY id DESC LIMIT 1",
                    params![m.game, i(Some(m.mod_id)), i(Some(m.file_id))],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = existing {
                if let Some(k) = &m.key {
                    store.conn().execute(
                        "UPDATE downloads SET nxm_key = ?1, nxm_expires = ?2, nxm_user_id = ?3,
                            status = CASE WHEN status IN ('failed', 'paused') THEN 'queued' ELSE status END,
                            error = NULL, updated_at = ?4
                         WHERE id = ?5",
                        params![k.key, k.expires, i(k.user_id), now, id],
                    )?;
                }
                return Ok(id);
            }
            let mut d = NewDownload::new(DownloadSource::Nxm);
            d.game = Some(m.game.clone());
            d.url = Some(link.redacted());
            d.nexus_mod_id = Some(m.mod_id);
            d.nexus_file_id = Some(m.file_id);
            d.nxm = m.key.clone();
            enqueue(store, &d)
        }
        NxmLink::Collection {
            game,
            slug,
            revision,
        } => {
            let existing: Option<i64> = store
                .conn()
                .query_row(
                    "SELECT id FROM downloads WHERE game = ?1 AND collection_slug = ?2
                       AND collection_revision IS ?3 AND status IN ('queued', 'downloading', 'paused')
                     ORDER BY id DESC LIMIT 1",
                    params![game, slug, i(*revision)],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = existing {
                return Ok(id);
            }
            let mut d = NewDownload::new(DownloadSource::Collection);
            d.game = Some(game.clone());
            d.url = Some(link.redacted());
            d.collection_slug = Some(slug.clone());
            d.collection_revision = *revision;
            enqueue(store, &d)
        }
    }
}

pub fn get(store: &Store, id: i64) -> Result<Download, DownloadError> {
    store
        .conn()
        .query_row(
            &format!("SELECT {COLUMNS} FROM downloads WHERE id = ?1"),
            [id],
            from_row,
        )
        .optional()?
        .ok_or(DownloadError::NotFound(id))
}

/// Rows in queue order, optionally filtered by status.
pub fn list(store: &Store, status: Option<DownloadStatus>) -> Result<Vec<Download>, DownloadError> {
    let sql =
        format!("SELECT {COLUMNS} FROM downloads WHERE ?1 IS NULL OR status = ?1 ORDER BY id");
    let mut stmt = store.conn().prepare(&sql)?;
    let rows = stmt
        .query_map([status.map(DownloadStatus::as_str)], from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn touch(
    store: &Store,
    id: i64,
    sql: &str,
    p: &[&dyn rusqlite::ToSql],
) -> Result<(), DownloadError> {
    let n = store.conn().execute(sql, p)?;
    if n == 0 {
        return Err(DownloadError::NotFound(id));
    }
    Ok(())
}

pub fn set_status(store: &Store, id: i64, status: DownloadStatus) -> Result<(), DownloadError> {
    touch(
        store,
        id,
        "UPDATE downloads SET status = ?1, updated_at = ?2 WHERE id = ?3",
        &[&status.as_str(), &store::now(), &id],
    )
}

pub fn update_progress(
    store: &Store,
    id: i64,
    bytes_done: u64,
    size: Option<u64>,
) -> Result<(), DownloadError> {
    touch(
        store,
        id,
        "UPDATE downloads SET bytes_done = ?1, size = COALESCE(?2, size), status = 'downloading',
            updated_at = ?3 WHERE id = ?4",
        &[&i(Some(bytes_done)), &i(size), &store::now(), &id],
    )
}

/// Marks the row done and clears the nxm key (single-use).
pub fn mark_done(store: &Store, id: i64, f: &Fetched) -> Result<(), DownloadError> {
    let dest = f.path.to_string_lossy().into_owned();
    let name = f.path.file_name().map(|n| n.to_string_lossy().into_owned());
    touch(
        store,
        id,
        "UPDATE downloads SET status = 'done', dest_path = ?1, part_path = NULL,
            file_name = COALESCE(?2, file_name), size = ?3, bytes_done = ?3, md5 = ?4,
            sha256 = ?5, nxm_key = NULL, nxm_expires = NULL, error = NULL, updated_at = ?6
         WHERE id = ?7",
        &[
            &dest,
            &name,
            &i(Some(f.size)),
            &f.md5,
            &f.sha256,
            &store::now(),
            &id,
        ],
    )
}

pub fn mark_failed(store: &Store, id: i64, error: &str) -> Result<(), DownloadError> {
    touch(
        store,
        id,
        "UPDATE downloads SET status = 'failed', error = ?1, updated_at = ?2 WHERE id = ?3",
        &[&error, &store::now(), &id],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_stripped_from_stored_url() {
        assert_eq!(
            strip_secrets("https://cdn.example/f.7z?md5=x&expires=1&key=SECRET"),
            "https://cdn.example/f.7z?md5=x"
        );
        assert_eq!(
            strip_secrets("nxm://sse/mods/1/files/2?key=SECRET&expires=9"),
            "nxm://sse/mods/1/files/2"
        );
        assert_eq!(strip_secrets("not a url?key=S"), "not a url");
    }
}
