// SPDX-License-Identifier: GPL-3.0-or-later
//! GraphQL v2: raw queries, search, batched mod lookups, adult-content flag.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::client::check_domain;
use super::{NexusClient, NexusError};

/// Requirement fields for [`NexusClient::mod_batch`] (legacy L2886).
pub const REQUIREMENT_FIELDS: &str = "modRequirements { nexusRequirements { nodes { modId modName gameId externalRequirement url notes } } }";

const SEARCH_QUERY: &str = "query($d: String!, $q: String!) {
  mods(filter: {gameDomainName: [{value: $d, op: EQUALS}], nameStemmed: [{value: $q, op: MATCHES}]},
       sort: [{relevance: {direction: DESC}}], count: 25) {
    totalCount nodes { modId name author version summary downloads endorsements thumbnailUrl category updatedAt } } }";

const BROWSE_MODS_QUERY: &str = "query($f: ModsFilter, $s: [ModsSort!], $o: Int, $c: Int) {
  mods(filter: $f, sort: $s, offset: $o, count: $c) {
    totalCount nodes { modId name author version summary downloads endorsements thumbnailUrl category updatedAt } } }";

const BROWSE_COLLECTIONS_QUERY: &str =
    "query($f: CollectionsSearchFilter, $s: [CollectionsSearchSort!], $o: Int, $c: Int) {
  collectionsV2(filter: $f, sort: $s, offset: $o, count: $c) {
    totalCount nodes { slug name summary endorsements totalDownloads
      tileImage { thumbnailUrl(size: med) } user { name } category { name }
      latestPublishedRevision { revisionNumber modCount totalSize } } } }";

const MOD_DESCRIPTION_QUERY: &str = "query($ids: [CompositeDomainWithIdInput!]!) {
  legacyModsByDomain(ids: $ids) { nodes { modId description } } }";

const COLLECTION_DESCRIPTION_QUERY: &str =
    "query($slug: String, $domainName: String, $viewAdultContent: Boolean) {
  collection(slug: $slug, domainName: $domainName, viewAdultContent: $viewAdultContent) { description } }";

/// A non-blank string at `pointer`.
fn description_at(data: &Value, pointer: &str) -> Option<String> {
    data.pointer(pointer)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
}

/// The description of `mod_id` in a `legacyModsByDomain` response (the
/// node with that id; Nexus BBCode). `None` when missing or blank.
fn parse_mod_description(data: &Value, mod_id: u64) -> Option<String> {
    data.pointer("/legacyModsByDomain/nodes")?
        .as_array()?
        .iter()
        .find(|n| n.get("modId").and_then(Value::as_u64) == Some(mod_id))
        .and_then(|n| description_at(n, "/description"))
}

/// The description in a `collection` response (Markdown). `None` when
/// missing or blank.
fn parse_collection_description(data: &Value) -> Option<String> {
    description_at(data, "/collection/description")
}

/// Order of a browse listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowseSort {
    #[default]
    Endorsements,
    Downloads,
    Newest,
}

impl BrowseSort {
    /// The GraphQL sort field (the same name for mods and collections).
    fn field(self) -> &'static str {
        match self {
            BrowseSort::Endorsements => "endorsements",
            BrowseSort::Downloads => "downloads",
            BrowseSort::Newest => "createdAt",
        }
    }
}

/// One page of a browse listing: mods or collections of a game.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BrowseQuery<'a> {
    /// Name words; empty = everything.
    pub text: &'a str,
    /// Only what was created at or after this Unix time.
    pub since: Option<i64>,
    pub sort: BrowseSort,
    pub offset: u64,
    pub count: u64,
    /// Include adult content (the `adult` setting).
    pub adult: bool,
}

/// `text` reduced to lower-case words: the filters reach a Lucene query
/// unescaped, so its operator characters (and AND/OR/NOT) would change or
/// break the search. Matching ignores case.
fn search_words(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '.')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `YYYY-MM-DD` (UTC) of a Unix time: collections filter dates by day
/// (a time with colons breaks their query), mods by Unix seconds.
pub fn utc_date(unix: i64) -> String {
    // Civil-from-days (Howard Hinnant), valid for any day after 0000-03-01.
    let z = unix.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// A collection in a browse listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrowseCollection {
    pub slug: String,
    pub name: String,
    pub summary: Option<String>,
    pub author: Option<String>,
    pub category: Option<String>,
    pub endorsements: Option<u64>,
    pub downloads: Option<u64>,
    /// Latest published revision.
    pub revision: Option<u64>,
    pub mod_count: Option<u64>,
    /// Bytes of the latest revision.
    pub total_size: Option<u64>,
    /// Tile picture (medium thumbnail).
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrowseCollections {
    pub total: u64,
    pub collections: Vec<BrowseCollection>,
}

/// Accepts ids sent as numbers or strings.
pub(crate) fn lenient_u64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Ok(match Option::<Value>::deserialize(d)? {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    })
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMod {
    #[serde(default, deserialize_with = "lenient_u64")]
    pub mod_id: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub downloads: Option<u64>,
    #[serde(default)]
    pub endorsements: Option<u64>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SearchResults {
    pub total: u64,
    pub mods: Vec<SearchMod>,
}

fn graphql_errors(resp: &Value) -> Option<NexusError> {
    let errs = resp.get("errors")?.as_array()?;
    if errs.is_empty() {
        return None;
    }
    let messages = errs
        .iter()
        .map(|e| {
            e.get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
                .to_owned()
        })
        .collect();
    let paths = errs
        .iter()
        .filter_map(|e| {
            e.pointer("/path/0").map(|p| match p {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
        })
        .collect();
    Some(NexusError::GraphQl { messages, paths })
}

impl NexusClient {
    /// Runs a query and returns `.data`. A non-empty `errors` is a failure
    /// (legacy L494-503).
    pub async fn graphql(&self, query: &str, variables: Value) -> Result<Value, NexusError> {
        let variables = if variables.is_null() {
            json!({})
        } else {
            variables
        };
        let body = self
            .post_graphql(&json!({ "query": query, "variables": variables }))
            .await?;
        let resp: Value = serde_json::from_str(&body)
            .map_err(|e| NexusError::Decode(format!("GraphQL response: {e}")))?;
        if let Some(e) = graphql_errors(&resp) {
            return Err(e);
        }
        Ok(resp.get("data").cloned().unwrap_or(Value::Null))
    }

    /// Collection queries are the only calls that send the `adult` setting,
    /// as `$viewAdultContent` (legacy L2176, L2681).
    pub async fn collection_graphql(
        &self,
        query: &str,
        mut variables: Value,
        view_adult: bool,
    ) -> Result<Value, NexusError> {
        if !variables.is_object() {
            variables = json!({});
        }
        variables["viewAdultContent"] = Value::Bool(view_adult);
        self.graphql(query, variables).await
    }

    /// Name search, 25 results by relevance (legacy L3815-3846). Never sends
    /// the adult flag.
    pub async fn search(&self, game: &str, text: &str) -> Result<SearchResults, NexusError> {
        let g = check_domain(game)?;
        let data = self
            .graphql(SEARCH_QUERY, json!({ "d": g, "q": text }))
            .await?;
        let mods = data.get("mods").cloned().unwrap_or(Value::Null);
        let total = mods.get("totalCount").and_then(Value::as_u64).unwrap_or(0);
        let nodes = mods.get("nodes").cloned().unwrap_or_else(|| json!([]));
        let mods: Vec<SearchMod> = serde_json::from_value(nodes)
            .map_err(|e| NexusError::Decode(format!("search results: {e}")))?;
        Ok(SearchResults { total, mods })
    }

    /// A page of `game`'s mods (see [`BrowseQuery`]). Never sends the adult
    /// flag; adult mods are filtered out unless `q.adult`.
    pub async fn browse_mods(
        &self,
        game: &str,
        q: &BrowseQuery<'_>,
    ) -> Result<SearchResults, NexusError> {
        let g = check_domain(game)?;
        let mut f = json!({ "gameDomainName": [{ "value": g, "op": "EQUALS" }] });
        let words = search_words(q.text);
        if !words.is_empty() {
            f["nameStemmed"] = json!([{ "value": words, "op": "MATCHES" }]);
        }
        if let Some(t) = q.since {
            f["createdAt"] = json!([{ "value": t.to_string(), "op": "GTE" }]);
        }
        if !q.adult {
            f["adultContent"] = json!([{ "value": false }]);
        }
        let data = self.graphql(BROWSE_MODS_QUERY, browse_vars(f, q)).await?;
        let mods = data.get("mods").cloned().unwrap_or(Value::Null);
        let total = mods.get("totalCount").and_then(Value::as_u64).unwrap_or(0);
        let nodes = mods.get("nodes").cloned().unwrap_or_else(|| json!([]));
        let mods: Vec<SearchMod> = serde_json::from_value(nodes)
            .map_err(|e| NexusError::Decode(format!("mod listing: {e}")))?;
        Ok(SearchResults { total, mods })
    }

    /// A page of `game`'s collections (see [`BrowseQuery`]).
    pub async fn browse_collections(
        &self,
        game: &str,
        q: &BrowseQuery<'_>,
    ) -> Result<BrowseCollections, NexusError> {
        let g = check_domain(game)?;
        let mut f = json!({ "gameDomain": [{ "value": g, "op": "EQUALS" }] });
        let words = search_words(q.text);
        if !words.is_empty() {
            f["generalSearch"] = json!([{ "value": words, "op": "WILDCARD" }]);
        }
        if let Some(t) = q.since {
            f["createdAt"] = json!([{ "value": utc_date(t), "op": "GTE" }]);
        }
        if !q.adult {
            f["adultContent"] = json!([{ "value": false }]);
        }
        let data = self
            .graphql(BROWSE_COLLECTIONS_QUERY, browse_vars(f, q))
            .await?;
        let page = data.get("collectionsV2").cloned().unwrap_or(Value::Null);
        let total = page.get("totalCount").and_then(Value::as_u64).unwrap_or(0);
        let nodes = page
            .get("nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(BrowseCollections {
            total,
            collections: nodes.iter().filter_map(browse_collection).collect(),
        })
    }

    /// A mod's long description (Nexus BBCode); `None` when it has none.
    pub async fn mod_description(
        &self,
        game: &str,
        mod_id: u64,
    ) -> Result<Option<String>, NexusError> {
        let g = check_domain(game)?;
        let data = self
            .graphql(
                MOD_DESCRIPTION_QUERY,
                json!({ "ids": [{ "gameDomain": g, "modId": mod_id }] }),
            )
            .await?;
        Ok(parse_mod_description(&data, mod_id))
    }

    /// A collection's description (Markdown); `None` when it has none.
    /// `view_adult` is the `adult` setting (else adult collections fail).
    pub async fn collection_description(
        &self,
        game: &str,
        slug: &str,
        view_adult: bool,
    ) -> Result<Option<String>, NexusError> {
        let g = check_domain(game)?;
        let data = self
            .collection_graphql(
                COLLECTION_DESCRIPTION_QUERY,
                json!({ "slug": slug, "domainName": g }),
                view_adult,
            )
            .await?;
        Ok(parse_collection_description(&data))
    }

    /// Looks up `fields` for several mods in one query (aliases `m<id>`).
    /// Nexus nulls the whole result when one mod is hidden or deleted, so
    /// the failing aliases are recorded as `None` and the rest asked again,
    /// up to 3 tries (legacy `gql_mod_batch` L2891-2911).
    pub async fn mod_batch(
        &self,
        game_id: u64,
        mod_ids: &[u64],
        fields: &str,
    ) -> Result<BTreeMap<u64, Option<Value>>, NexusError> {
        let mut out = BTreeMap::new();
        let mut todo: Vec<u64> = mod_ids.to_vec();
        todo.sort_unstable();
        todo.dedup();
        let mut tries = 0;
        while !todo.is_empty() && tries < 3 {
            tries += 1;
            let mut q = String::from("query {");
            for m in &todo {
                q.push_str(&format!(
                    " m{m}: mod(modId: \"{m}\", gameId: \"{game_id}\") {{ {fields} }}"
                ));
            }
            q.push_str(" }");
            match self.graphql(&q, json!({})).await {
                Ok(data) => {
                    for m in &todo {
                        let v = data.get(format!("m{m}")).cloned().filter(|v| !v.is_null());
                        out.insert(*m, v);
                    }
                    return Ok(out);
                }
                Err(NexusError::GraphQl { messages, paths }) => {
                    let bad: Vec<u64> = paths
                        .iter()
                        .filter_map(|p| p.strip_prefix('m')?.parse().ok())
                        .filter(|m| todo.contains(m))
                        .collect();
                    if bad.is_empty() {
                        return Err(NexusError::GraphQl { messages, paths });
                    }
                    for m in &bad {
                        out.insert(*m, None);
                    }
                    todo.retain(|m| !bad.contains(m));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }
}

/// Variables of a browse query: filter, sort, page.
fn browse_vars(filter: Value, q: &BrowseQuery<'_>) -> Value {
    json!({
        "f": filter,
        "s": [{ q.sort.field(): { "direction": "DESC" } }],
        "o": q.offset,
        "c": q.count,
    })
}

/// One `collectionsV2` node; `None` without a slug or name.
fn browse_collection(n: &Value) -> Option<BrowseCollection> {
    let text = |p: &str| n.pointer(p).and_then(Value::as_str).map(str::to_owned);
    let num = |p: &str| {
        n.pointer(p).and_then(|v| match v {
            Value::Number(x) => x.as_u64(),
            Value::String(s) => s.parse().ok(),
            _ => None,
        })
    };
    Some(BrowseCollection {
        slug: text("/slug")?,
        name: text("/name")?,
        summary: text("/summary"),
        author: text("/user/name"),
        category: text("/category/name"),
        endorsements: num("/endorsements"),
        downloads: num("/totalDownloads"),
        revision: num("/latestPublishedRevision/revisionNumber"),
        mod_count: num("/latestPublishedRevision/modCount"),
        total_size: num("/latestPublishedRevision/totalSize"),
        image_url: text("/tileImage/thumbnailUrl"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_date_formats_days() {
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(1_790_640_000), "2026-09-29");
        assert_eq!(utc_date(951_782_400), "2000-02-29");
        assert_eq!(utc_date(1_790_726_399), "2026-09-29");
    }

    // Fails when the description is read from the wrong path, from another mod's node, or when a blank
    // description is passed on instead of falling back (None).
    #[test]
    fn mod_description_parses_the_matching_node() {
        let data = json!({ "legacyModsByDomain": { "nodes": [
            { "modId": 7, "description": "other" },
            { "modId": 266, "description": "[b]USSEP[/b]\n<br />x" }
        ] } });
        assert_eq!(
            parse_mod_description(&data, 266).as_deref(),
            Some("[b]USSEP[/b]\n<br />x")
        );
        assert_eq!(parse_mod_description(&data, 1), None);
        let blank =
            json!({ "legacyModsByDomain": { "nodes": [{ "modId": 1, "description": "  " }] } });
        assert_eq!(parse_mod_description(&blank, 1), None);
        assert_eq!(parse_mod_description(&json!({}), 1), None);
        let null =
            json!({ "legacyModsByDomain": { "nodes": [{ "modId": 1, "description": null }] } });
        assert_eq!(parse_mod_description(&null, 1), None);
    }

    // Fails when the collection description path changes or blank/null descriptions are not None.
    #[test]
    fn collection_description_parses_or_is_none() {
        let data = json!({ "collection": { "description": "### Hi\n\n**bold**" } });
        assert_eq!(
            parse_collection_description(&data).as_deref(),
            Some("### Hi\n\n**bold**")
        );
        assert_eq!(
            parse_collection_description(&json!({ "collection": { "description": "" } })),
            None
        );
        assert_eq!(
            parse_collection_description(&json!({ "collection": null })),
            None
        );
    }

    #[test]
    fn search_words_drop_query_operators() {
        assert_eq!(search_words("  SkyUI: (v5) AND \"x\" "), "skyui v5 and x");
        assert_eq!(search_words("Lin's 1.0"), "lin's 1.0");
        assert_eq!(search_words("*:*"), "");
    }
}
