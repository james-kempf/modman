// SPDX-License-Identifier: GPL-3.0-or-later
//! Collection manifest fetch against a wiremock server: GraphQL revision
//! (adult flag, revision pin), archive via the API download link, API
//! fallback, md5 search. No request leaves 127.0.0.1.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use vessel_core::collections::{fetch_manifest, fetch_revision, revision_dir, CollectionError};
use vessel_core::download::Fetcher;
use vessel_core::nexus::{ApiKey, ApiKeyAuth, NexusClient, NexusConfig, NexusError, RetryPolicy};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "CollectionKey0123456789";

fn client(server: &MockServer) -> NexusClient {
    let mut c = NexusConfig::with_base(&server.uri()).unwrap();
    c.retry = RetryPolicy {
        attempts: 1,
        rate_limit_step: Duration::from_millis(1),
        server_step: Duration::from_millis(1),
    };
    let auth = Arc::new(ApiKeyAuth::new(ApiKey::new(KEY).unwrap()));
    NexusClient::new(c, Some(auth), None).unwrap()
}

fn rev_body(number: u64, link: Option<&str>) -> Value {
    json!({"data": {"collectionRevision": {
        "revisionNumber": number, "revisionStatus": "published", "downloadLink": link,
        "totalSize": "1234", "modCount": 1,
        "collection": {"name": "Test Col", "slug": "abc123", "summary": "s",
                       "latestPublishedRevision": {"revisionNumber": 9},
                       "user": {"name": "Curator"}, "game": {"domainName": "testgame", "name": "Test"}},
        "modFiles": [{"fileId": 11, "optional": false,
                      "file": {"modId": 1, "name": "f", "version": "1.0", "sizeInBytes": "10",
                               "mod": {"name": "Api Mod", "author": "A"}}}],
        "externalResources": []
    }}})
}

fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (n, d) in files {
        w.start_file(*n, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(d).unwrap();
    }
    w.finish().unwrap().into_inner()
}

#[tokio::test]
async fn fetch_revision_pins_revision_and_sends_adult_setting() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/graphql"))
        .and(body_string_contains("\"revision\":4"))
        .and(body_string_contains("\"viewAdultContent\":true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(rev_body(4, None)))
        .expect(1)
        .mount(&server)
        .await;
    let rev = fetch_revision(&client(&server), "testgame", "abc123", Some(4), true)
        .await
        .unwrap();
    assert_eq!((rev.number, rev.latest), (4, Some(9)));
    assert_eq!(
        (rev.name.as_str(), rev.author.as_str()),
        ("Test Col", "Curator")
    );
    assert_eq!(rev.total_size, 1234);
}

#[tokio::test]
async fn fetch_revision_reports_unknown_collections() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/graphql"))
        .and(body_string_contains("\"viewAdultContent\":false"))
        .and(body_string_contains("\"revision\":null"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": {"collectionRevision": null}})),
        )
        .mount(&server)
        .await;
    let e = fetch_revision(&client(&server), "testgame", "nope1", None, false)
        .await
        .unwrap_err();
    assert!(matches!(e, CollectionError::NotFound(_)), "{e:?}");
    assert!(matches!(
        fetch_revision(&client(&server), "../x", "abc", None, false).await,
        Err(CollectionError::BadRef(_))
    ));
}

#[tokio::test]
async fn fetch_manifest_takes_collection_json_from_the_archive() {
    let server = MockServer::start().await;
    let cj = br#"{"info":{"name":"Test Col"},"mods":[{"name":"Bundled","source":{"type":"bundle"}}],"modRules":[{"type":"before"}]}"#;
    let archive = zip(&[("collection.json", cj), ("bundled/Bundled-1.zip", b"data")]);
    Mock::given(method("GET"))
        .and(path("/v1/collections/abc123/revisions/4/download_link"))
        .and(header("apikey", KEY))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "download_links": [{"name": "CDN", "short_name": "cdn", "URI": format!("{}/cdn/col.7z?sig=1", server.uri())}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/cdn/col.7z"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server);
    let raw = rev_body(4, Some("/v1/collections/abc123/revisions/4/download_link"));
    Mock::given(method("POST"))
        .and(path("/v2/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(raw))
        .mount(&server)
        .await;
    let rev = fetch_revision(&c, "testgame", "abc123", Some(4), false)
        .await
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let dir = revision_dir(tmp.path(), "testgame", "abc123", 4);
    let f = fetch_manifest(&c, &Fetcher::new().unwrap(), &rev, &dir)
        .await
        .unwrap();
    assert!(!f.from_api, "{:?}", f.archive_error);
    assert_eq!(f.manifest.mods[0].name, "Bundled");
    assert_eq!(f.manifest.mod_rules().len(), 1);
    assert!(dir.join("revision.json").is_file());
    assert!(dir.join("content/bundled/Bundled-1.zip").is_file());
    // A second run reuses collection.json (the mocks expect one call each).
    let again = fetch_manifest(&c, &Fetcher::new().unwrap(), &rev, &dir)
        .await
        .unwrap();
    assert_eq!(again.manifest, f.manifest);
}

#[tokio::test]
async fn fetch_manifest_falls_back_to_the_api_mod_list() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/collections/abc123/revisions/4/download_link"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"message": "no"})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v2/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(rev_body(
            4,
            Some("/v1/collections/abc123/revisions/4/download_link"),
        )))
        .mount(&server)
        .await;
    let c = client(&server);
    let rev = fetch_revision(&c, "testgame", "abc123", None, false)
        .await
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let f = fetch_manifest(&c, &Fetcher::new().unwrap(), &rev, tmp.path())
        .await
        .unwrap();
    assert!(f.from_api);
    assert!(f.archive_error.unwrap().contains("403"));
    let m = &f.manifest.mods[0];
    assert_eq!(
        (m.name.as_str(), m.mod_id, m.file_id),
        ("Api Mod", Some(1), Some(11))
    );
    assert!(f.manifest.mod_rules().is_empty());
}

#[tokio::test]
async fn collection_archive_link_must_stay_on_the_api_origin() {
    let server = MockServer::start().await;
    let c = client(&server);
    for link in ["https://evil.example/v1/x", "//evil.example/x"] {
        assert!(
            matches!(
                c.collection_archive_uri(link).await,
                Err(NexusError::BadUrl(_))
            ),
            "{link}"
        );
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn md5_search_identifies_files_and_treats_404_as_no_match() {
    let server = MockServer::start().await;
    let hit = "0123456789abcdef0123456789abcdef";
    Mock::given(method("GET"))
        .and(path(format!("/v1/games/testgame/mods/md5_search/{hit}.json")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"mod": {"mod_id": 5, "name": "Found"}, "file_details": {"file_id": 50, "version": "2.0", "md5": hit}}
        ])))
        .mount(&server)
        .await;
    let c = client(&server);
    let m = c.md5_search("testgame", hit).await.unwrap();
    assert_eq!((m[0].mod_id, m[0].file_id), (5, 50));
    assert_eq!(m[0].mod_name.as_deref(), Some("Found"));
    let none = c
        .md5_search("testgame", "ffffffffffffffffffffffffffffffff")
        .await
        .unwrap();
    assert!(none.is_empty());
    assert!(c.md5_search("testgame", "../../x").await.is_err());
}
