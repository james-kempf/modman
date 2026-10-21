// SPDX-License-Identifier: GPL-3.0-or-later
//! nxm:// intake shared by `modman nxm <url>`, the GUI single-instance
//! callback and the GUI paste-link box (spikes.md §5): every URL is parsed
//! by [`NxmLink::parse`] before anything is queued, then the queued row is
//! processed like legacy `nxm_handle` (L4276-4305).

use serde::Serialize;

use super::install::{install_nexus, NexusInstall};
use super::nexus as nx;
use super::{chain, games_hint, AppError, Cx, R};
use crate::download::{self, DownloadStatus, NxmLink};
use crate::games;
use crate::store::Store;

/// The URL without its query (which holds `key` and `expires`).
pub fn redact_url(url: &str) -> &str {
    url.split('?').next().unwrap_or("")
}

/// A parsed and queued link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    pub link: NxmLink,
    /// `downloads` row id.
    pub row: i64,
}

/// Validates `url` with the core parser and queues it. Malformed or foreign
/// links are rejected before any row is written.
pub fn accept(store: &Store, url: &str) -> R<Accepted> {
    let link = parse(url)?;
    tracing::info!("nxm link received: {}", link.redacted());
    let row = download::enqueue_nxm(store, &link)?;
    Ok(Accepted { link, row })
}

/// Longest part of a rejected link echoed back.
const ECHO_CHARS: usize = 200;

/// A rejected link for a message: no query, control characters escaped,
/// at most [`ECHO_CHARS`] characters.
fn echo(url: &str) -> String {
    let url = redact_url(url.trim());
    let mut out: String = url
        .chars()
        .take(ECHO_CHARS)
        .flat_map(|c| -> Box<dyn Iterator<Item = char>> {
            if c.is_control() {
                Box::new(c.escape_default())
            } else {
                Box::new(std::iter::once(c))
            }
        })
        .collect();
    if url.chars().nth(ECHO_CHARS).is_some() {
        out.push('…');
    }
    out
}

/// [`NxmLink::parse`] with a message that never echoes the query.
pub fn parse(url: &str) -> R<NxmLink> {
    NxmLink::parse(url)
        .map_err(|e| AppError::Msg(format!("Unrecognised nxm link {}: {e}", echo(url))))
}

/// What processing a link did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Processed {
    pub game: String,
    /// `mod` or `collection`.
    pub kind: String,
    /// Mod id or collection slug.
    pub what: String,
    /// Installed (`false`: declined, cancelled or download only).
    pub installed: bool,
    pub installed_mod: Option<super::install::InstalledMod>,
    pub collection: Option<super::collection::CollectionResult>,
}

/// Installs what a queued link names and closes its row.
pub fn process(cx: &mut Cx<'_>, accepted: &Accepted) -> R<Processed> {
    let row = accepted.row;
    match &accepted.link {
        NxmLink::Collection {
            game,
            slug,
            revision,
        } => {
            let r = super::collection::install_link(cx, game, slug, *revision, row)?;
            Ok(Processed {
                game: game.clone(),
                kind: "collection".into(),
                what: slug.clone(),
                installed: !r.cancelled && !r.download_only,
                installed_mod: None,
                collection: Some(r),
            })
        }
        NxmLink::Mod(m) => {
            let res = (|| -> R<Processed> {
                let game = games::require_game(cx.store.conn(), &m.game).map_err(games_hint)?;
                let nexus = nx::require(cx.paths, cx.store)?;
                let done = install_nexus(
                    cx,
                    &nexus,
                    &game,
                    NexusInstall {
                        mod_id: m.mod_id,
                        file_id: Some(m.file_id),
                        key: m.key.clone(),
                        row: Some(row),
                        ..Default::default()
                    },
                )?;
                if done.is_some() {
                    super::deploy::after_change(cx, &game)?;
                }
                Ok(Processed {
                    game: m.game.clone(),
                    kind: "mod".into(),
                    what: m.mod_id.to_string(),
                    installed: done.is_some(),
                    installed_mod: done.as_ref().map(Into::into),
                    collection: None,
                })
            })();
            if let Err(e) = &res {
                let still_open = download::get(cx.store, row).is_ok_and(|d| {
                    !matches!(d.status, DownloadStatus::Done | DownloadStatus::Failed)
                });
                if still_open {
                    let _ = download::mark_failed(cx.store, row, &chain(e));
                }
            }
            res
        }
    }
}

/// `queued` rows, oldest first (drained by the GUI at start).
pub fn queued(store: &Store) -> R<Vec<download::Download>> {
    Ok(download::list(store, Some(DownloadStatus::Queued))?)
}

/// The link a queued nxm/collection row stands for (`None` for other rows).
pub fn link_of(d: &download::Download) -> Option<Accepted> {
    let game = d.game.clone()?;
    let link = match (&d.collection_slug, d.nexus_mod_id, d.nexus_file_id) {
        (Some(slug), _, _) => NxmLink::Collection {
            game,
            slug: slug.clone(),
            revision: d.collection_revision,
        },
        (None, Some(mod_id), Some(file_id)) if d.source == download::DownloadSource::Nxm => {
            NxmLink::Mod(download::NxmModLink {
                game,
                mod_id,
                file_id,
                key: d.nxm.clone(),
            })
        }
        _ => return None,
    };
    Some(Accepted { link, row: d.id })
}

/// Queued links to process, oldest first.
pub fn pending(store: &Store) -> R<Vec<Accepted>> {
    Ok(queued(store)?.iter().filter_map(link_of).collect())
}

/// The row is still waiting (not already being processed).
pub fn is_queued(store: &Store, row: i64) -> bool {
    download::get(store, row).is_ok_and(|d| d.status == DownloadStatus::Queued)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(store: &Store) -> usize {
        download::list(store, None).unwrap().len()
    }

    #[test]
    fn malformed_and_foreign_links_are_rejected_without_a_row() {
        let store = Store::open_in_memory().unwrap();
        for bad in [
            "https://www.nexusmods.com/skyrimspecialedition/mods/266",
            "nxm://evil.example/mods/1/files/2",
            "nxm://skyrimspecialedition/mods/x/files/2?key=SECRET",
            "nxm://skyrimspecialedition/../../etc",
            "",
        ] {
            let e = accept(&store, bad).unwrap_err().to_string();
            assert!(!e.contains("SECRET"), "{e}");
        }
        assert_eq!(rows(&store), 0);
    }

    #[test]
    fn rejections_echo_a_short_escaped_link() {
        let long = format!("nxm://x/{}", "a".repeat(5000));
        let e = parse(&long).unwrap_err().to_string();
        assert!(e.len() < 400 && e.contains('…'), "{e}");
        let e = parse("nxm://x/\u{1b}[31mred\nnext\u{7}?key=SECRET")
            .unwrap_err()
            .to_string();
        assert!(!e.chars().any(|c| c.is_control()), "{e:?}");
        assert!(e.contains("\\u{1b}[31mred\\nnext"), "{e}");
        assert!(!e.contains("SECRET"));
    }

    #[test]
    fn a_valid_link_is_queued_once() {
        let store = Store::open_in_memory().unwrap();
        let url = "nxm://skyrimspecialedition/mods/266/files/1000?key=k&expires=9999999999";
        let a = accept(&store, url).unwrap();
        let b = accept(&store, url).unwrap();
        assert_eq!(a.row, b.row);
        assert_eq!(queued(&store).unwrap().len(), 1);
        let p = pending(&store).unwrap();
        assert_eq!(p, vec![a.clone()]);
        assert!(is_queued(&store, a.row));
        let c = accept(&store, "nxm://stardewvalley/collections/abc/revisions/3").unwrap();
        assert_eq!(pending(&store).unwrap()[1], c);
    }
}
