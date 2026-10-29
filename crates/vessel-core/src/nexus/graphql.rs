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
