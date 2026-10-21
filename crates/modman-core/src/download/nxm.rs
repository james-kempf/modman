// SPDX-License-Identifier: GPL-3.0-or-later
//! The single nxm:// parser (CLI `modman nxm <url>`, GUI single-instance
//! argv, paste-link box). Input is untrusted: everything outside the two
//! documented forms (legacy `parse_nxm` L1517-1531) is rejected.
//!
//! - `nxm://<game>/mods/<mod>/files/<file>?key=<k>&expires=<t>[&user_id=<u>]`
//! - `nxm://<game>/collections/<slug>[/revisions/<n>]`

use url::Url;

use super::DownloadError;
use crate::nexus::NxmKey;

/// Longer input is rejected before parsing.
pub const MAX_NXM_LEN: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NxmModLink {
    /// Nexus game domain, lower-cased.
    pub game: String,
    pub mod_id: u64,
    pub file_id: u64,
    /// Present on links from "Slow download"; `Debug` is redacted.
    pub key: Option<NxmKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NxmLink {
    Mod(NxmModLink),
    Collection {
        game: String,
        slug: String,
        revision: Option<u64>,
    },
}

type R<T> = Result<T, DownloadError>;

fn bad(why: &'static str) -> DownloadError {
    DownloadError::InvalidNxm(why)
}

fn id(s: &str) -> R<u64> {
    if s.is_empty() || s.len() > 19 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad("ids must be decimal numbers"));
    }
    s.parse().map_err(|_| bad("id out of range"))
}

impl NxmLink {
    pub fn parse(input: &str) -> R<Self> {
        let input = input.trim();
        if input.len() > MAX_NXM_LEN {
            return Err(bad("link too long"));
        }
        if !input
            .get(..6)
            .is_some_and(|p| p.eq_ignore_ascii_case("nxm://"))
        {
            return Err(bad("scheme must be nxm://"));
        }
        let url = Url::parse(input).map_err(|_| bad("malformed URL"))?;
        if url.scheme() != "nxm" {
            return Err(bad("scheme must be nxm://"));
        }
        if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
            return Err(bad("unexpected user or port"));
        }
        if url.fragment().is_some() {
            return Err(bad("unexpected fragment"));
        }
        let game = url
            .host_str()
            .ok_or(bad("missing game"))?
            .to_ascii_lowercase();
        // A game domain is a single label: rejects foreign hosts such as
        // nxm://evil.example/… and IP literals.
        if game.is_empty()
            || game.len() > 64
            || !game
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            return Err(bad("game must be a Nexus game domain"));
        }
        let segs: Vec<&str> = url.path().trim_start_matches('/').split('/').collect();
        match segs.as_slice() {
            ["mods", m, "files", f] => {
                let key = parse_key(&url)?;
                Ok(Self::Mod(NxmModLink {
                    game,
                    mod_id: id(m)?,
                    file_id: id(f)?,
                    key,
                }))
            }
            ["collections", slug, rest @ ..] => {
                if url.query().is_some() {
                    return Err(bad("unexpected query on a collection link"));
                }
                let slug_ok = !slug.is_empty()
                    && slug.len() <= 64
                    && slug.bytes().all(|b| b.is_ascii_alphanumeric());
                if !slug_ok {
                    return Err(bad("collection slug must be alphanumeric"));
                }
                let revision = match rest {
                    [] => None,
                    ["revisions", n] => Some(id(n)?),
                    _ => return Err(bad("unexpected path after the collection slug")),
                };
                Ok(Self::Collection {
                    game,
                    slug: (*slug).to_owned(),
                    revision,
                })
            }
            _ => Err(bad("unknown link form")),
        }
    }

    pub fn game(&self) -> &str {
        match self {
            Self::Mod(m) => &m.game,
            Self::Collection { game, .. } => game,
        }
    }

    /// The link without secrets, for logs and the `downloads.url` column.
    pub fn redacted(&self) -> String {
        match self {
            Self::Mod(m) => format!("nxm://{}/mods/{}/files/{}", m.game, m.mod_id, m.file_id),
            Self::Collection {
                game,
                slug,
                revision,
            } => match revision {
                Some(r) => format!("nxm://{game}/collections/{slug}/revisions/{r}"),
                None => format!("nxm://{game}/collections/{slug}"),
            },
        }
    }
}

fn parse_key(url: &Url) -> R<Option<NxmKey>> {
    let (mut key, mut expires, mut user_id) = (None, None, None);
    for (k, v) in url.query_pairs() {
        let slot = match k.as_ref() {
            "key" => &mut key,
            "expires" => &mut expires,
            "user_id" => &mut user_id,
            _ => continue,
        };
        if slot.is_some() {
            return Err(bad("duplicate query parameter"));
        }
        *slot = Some(v.into_owned());
    }
    match (key, expires) {
        (None, None) if user_id.is_none() => Ok(None),
        (Some(k), Some(e)) => {
            let key_ok = !k.is_empty()
                && k.len() <= 256
                && k.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_=+/.~".contains(&b));
            if !key_ok {
                return Err(bad("invalid key"));
            }
            let expires = id(&e).map_err(|_| bad("expires must be a Unix time"))?;
            let user_id = user_id.as_deref().map(id).transpose()?;
            Ok(Some(NxmKey {
                key: k,
                expires: i64::try_from(expires).map_err(|_| bad("expires out of range"))?,
                user_id,
            }))
        }
        _ => Err(bad("key and expires must both be present")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_link_with_key() {
        let l = NxmLink::parse(
            "nxm://SkyrimSpecialEdition/mods/266/files/1000?key=abc_D-1&expires=1700000000&user_id=42",
        )
        .unwrap();
        let NxmLink::Mod(m) = &l else { panic!() };
        assert_eq!(m.game, "skyrimspecialedition");
        assert_eq!((m.mod_id, m.file_id), (266, 1000));
        let k = m.key.as_ref().unwrap();
        assert_eq!(
            (k.key.as_str(), k.expires, k.user_id),
            ("abc_D-1", 1_700_000_000, Some(42))
        );
        assert_eq!(
            l.redacted(),
            "nxm://skyrimspecialedition/mods/266/files/1000"
        );
        assert!(!format!("{l:?}").contains("abc_D-1"));
    }

    #[test]
    fn mod_link_without_key() {
        let l = NxmLink::parse("nxm://sse/mods/1/files/2").unwrap();
        assert!(matches!(l, NxmLink::Mod(NxmModLink { key: None, .. })));
    }

    #[test]
    fn collection_forms() {
        assert_eq!(
            NxmLink::parse("nxm://skyrimspecialedition/collections/qdurkx").unwrap(),
            NxmLink::Collection {
                game: "skyrimspecialedition".into(),
                slug: "qdurkx".into(),
                revision: None
            }
        );
        let l = NxmLink::parse("nxm://sse/collections/qdurkx/revisions/12").unwrap();
        assert_eq!(
            l,
            NxmLink::Collection {
                game: "sse".into(),
                slug: "qdurkx".into(),
                revision: Some(12)
            }
        );
        assert_eq!(l.redacted(), "nxm://sse/collections/qdurkx/revisions/12");
    }

    #[test]
    fn malformed_and_foreign_input_rejected() {
        for bad in [
            "",
            "https://www.nexusmods.com/sse/mods/1/files/2",
            "nxm:sse/mods/1/files/2",
            "nxm://evil.example.com/mods/1/files/2",
            "nxm://127.0.0.1/mods/1/files/2",
            "nxm://user@sse/mods/1/files/2",
            "nxm://sse:8080/mods/1/files/2",
            "nxm://sse/mods/x/files/2",
            "nxm://sse/mods/1/files/2/extra",
            "nxm://sse/mods/1/files/",
            "nxm://sse/mods/-1/files/2",
            "nxm://sse/mods/1/files/2?key=abc",
            "nxm://sse/mods/1/files/2?expires=5",
            "nxm://sse/mods/1/files/2?key=a%20b&expires=5",
            "nxm://sse/mods/1/files/2?key=a&key=b&expires=5",
            "nxm://sse/mods/1/files/2?key=a&expires=soon",
            "nxm://sse/mods/1/files/2#frag",
            "nxm://sse/mods/99999999999999999999/files/2",
            "nxm://sse/collections/../x",
            "nxm://sse/collections/a-b",
            "nxm://sse/collections/abc/revisions/x",
            "nxm://sse/collections/abc/other/1",
            "nxm://sse/collections/abc?key=1&expires=2",
            "nxm://sse/other/1",
        ] {
            assert!(NxmLink::parse(bad).is_err(), "accepted {bad:?}");
        }
        let long = format!(
            "nxm://sse/mods/1/files/2?key={}&expires=1",
            "a".repeat(3000)
        );
        assert!(NxmLink::parse(&long).is_err());
    }

    #[test]
    fn rejection_message_does_not_echo_key() {
        let e = NxmLink::parse("nxm://sse/mods/1/files/2?key=SECRETKEY").unwrap_err();
        assert!(!e.to_string().contains("SECRETKEY"));
    }
}
