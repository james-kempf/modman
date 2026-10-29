// SPDX-License-Identifier: GPL-3.0-or-later
//! Nexus client against a wiremock server. No request leaves 127.0.0.1.

use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use vessel_core::nexus::{
    cached_requirements, check_updates, refresh_requirements, ApiKey, ApiKeyAuth, AuthProvider,
    Backend, KeyStorage, KeyringOpener, NexusClient, NexusConfig, NexusError, NxmKey, ProbeOutcome,
    RateLimit, RetryPolicy, SharedStore, CREDENTIALS_FILE,
};
use vessel_core::paths::Paths;
use vessel_core::store::{self, Store};
use wiremock::matchers::{
    body_string_contains, header, method, path, query_param, query_param_is_missing,
};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const KEY: &str = "SuperSecretApiKey0123456789";

fn config(server: &MockServer) -> NexusConfig {
    let mut c = NexusConfig::with_base(&server.uri()).unwrap();
    c.retry = RetryPolicy {
        attempts: 4,
        rate_limit_step: Duration::from_millis(5),
        server_step: Duration::from_millis(5),
    };
    c.timeout = Duration::from_secs(10);
    c
}

fn auth() -> Arc<dyn AuthProvider> {
    Arc::new(ApiKeyAuth::new(ApiKey::new(KEY).unwrap()))
}

fn client(server: &MockServer) -> NexusClient {
    NexusClient::new(config(server), Some(auth()), None).unwrap()
}

fn shared() -> SharedStore {
    Arc::new(Mutex::new(Store::open_in_memory().unwrap()))
}

fn validate_body(premium: bool) -> Value {
    json!({"user_id": 7, "key": KEY, "name": "Tester", "is_premium": premium,
           "is_supporter": false, "email": "t@example.invalid"})
}

// --- key validation ---------------------------------------------------------

#[tokio::test]
async fn validate_key_success_reports_premium_and_sends_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/users/validate.json"))
        .and(header("apikey", KEY))
        .and(header("Application-Name", "vessel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(validate_body(true)))
        .expect(1)
        .mount(&server)
        .await;
    let acct = client(&server).validate().await.unwrap();
    assert_eq!(acct.name, "Tester");
    assert_eq!(acct.user_id, 7);
    assert!(acct.is_premium);
    // The echoed key is not kept anywhere in the account.
    assert!(!format!("{acct:?}").contains(KEY));
}

#[tokio::test]
async fn validate_key_401_is_unauthorized_without_retry() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/users/validate.json"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"message": "Please provide a valid API Key"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server).validate().await.unwrap_err();
    assert!(matches!(err, NexusError::Unauthorized), "{err:?}");
}

#[tokio::test]
async fn validate_without_key_is_not_logged_in_and_sends_nothing() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/users/validate.json"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let c = NexusClient::new(config(&server), None, None).unwrap();
    assert!(matches!(c.validate().await, Err(NexusError::NotLoggedIn)));
}

// --- download links ---------------------------------------------------------

fn links() -> Value {
    json!([
        {"name": "Nexus CDN", "short_name": "Nexus CDN", "URI": "https://cdn.example/a/SkyUI 5.2.7z?md5=x"},
        {"name": "Amsterdam", "short_name": "Amsterdam", "URI": "https://ams.example/a/SkyUI.7z"}
    ])
}

const LINK_PATH: &str = "/v1/games/skyrimspecialedition/mods/12604/files/35407/download_link.json";

#[tokio::test]
async fn premium_download_link_is_direct_without_key_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(LINK_PATH))
        .and(query_param_is_missing("key"))
        .and(query_param_is_missing("expires"))
        .respond_with(ResponseTemplate::new(200).set_body_json(links()))
        .expect(2)
        .mount(&server)
        .await;
    let c = client(&server);
    let uri = c
        .download_uri("skyrimspecialedition", 12604, 35407, true, None, None)
        .await
        .unwrap();
    assert_eq!(uri, "https://cdn.example/a/SkyUI%205.2.7z?md5=x");
    let uri = c
        .download_uri(
            "skyrimspecialedition",
            12604,
            35407,
            true,
            None,
            Some("Amsterdam"),
        )
        .await
        .unwrap();
    assert_eq!(uri, "https://ams.example/a/SkyUI.7z");
}

#[tokio::test]
async fn free_account_without_nxm_key_fails_before_any_request() {
    let server = MockServer::start().await;
    Mock::given(path(LINK_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(links()))
        .expect(0)
        .mount(&server)
        .await;
    let err = client(&server)
        .download_uri("skyrimspecialedition", 12604, 35407, false, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, NexusError::NxmKeyRequired), "{err:?}");
}

#[tokio::test]
async fn free_account_403_maps_to_nxm_key_required() {
    let server = MockServer::start().await;
    Mock::given(path(LINK_PATH))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"message": "You don't have permission"})),
        )
        .mount(&server)
        .await;
    let err = client(&server)
        .download_links("skyrimspecialedition", 12604, 35407, None)
        .await
        .unwrap_err();
    assert!(matches!(err, NexusError::NxmKeyRequired), "{err:?}");
}

#[tokio::test]
async fn nxm_key_and_expires_exchanged_for_link() {
    let server = MockServer::start().await;
    let expires = store::now() + 3600;
    Mock::given(method("GET"))
        .and(path(LINK_PATH))
        .and(query_param("key", "nxmKEY_abc"))
        .and(query_param("expires", expires.to_string()))
        .respond_with(ResponseTemplate::new(200).set_body_json(links()))
        .expect(1)
        .mount(&server)
        .await;
    let k = NxmKey {
        key: "nxmKEY_abc".into(),
        expires,
        user_id: Some(7),
    };
    let uri = client(&server)
        .download_uri("skyrimspecialedition", 12604, 35407, false, Some(&k), None)
        .await
        .unwrap();
    assert!(uri.starts_with("https://cdn.example/"));
}

#[tokio::test]
async fn expired_nxm_key_rejected_without_request() {
    let server = MockServer::start().await;
    Mock::given(path(LINK_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(links()))
        .expect(0)
        .mount(&server)
        .await;
    let k = NxmKey {
        key: "k".into(),
        expires: store::now() - 1,
        user_id: None,
    };
    let err = client(&server)
        .download_links("skyrimspecialedition", 12604, 35407, Some(&k))
        .await
        .unwrap_err();
    assert!(matches!(err, NexusError::NxmKeyExpired));
}

// --- rate limits ------------------------------------------------------------

fn rl_response(hourly: u32, daily: u32) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("x-rl-hourly-limit", "100")
        .insert_header("x-rl-hourly-remaining", hourly.to_string().as_str())
        .insert_header("x-rl-hourly-reset", "2026-10-20T13:00:00+00:00")
        .insert_header("x-rl-daily-limit", "2500")
        .insert_header("x-rl-daily-remaining", daily.to_string().as_str())
        .set_body_json(validate_body(false))
}

#[tokio::test]
async fn exhausted_rate_limit_throttles_and_persists_across_clients() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/users/validate.json"))
        .respond_with(rl_response(0, 0))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    let db = shared();
    let first = NexusClient::new(config(&server), Some(auth()), Some(db.clone())).unwrap();
    first.validate().await.unwrap();
    let rl = first.rate_limit().unwrap();
    assert_eq!(
        (rl.hourly_remaining, rl.daily_remaining),
        (Some(0), Some(0))
    );
    drop(first);

    // A new client over the same store refuses locally: the mock expects
    // exactly one request in total.
    let second = NexusClient::new(config(&server), Some(auth()), Some(db.clone())).unwrap();
    assert_eq!(second.rate_limit(), Some(rl.clone()));
    let err = second.validate().await.unwrap_err();
    assert!(
        matches!(&err, NexusError::RateLimited { hourly, daily, .. } if hourly == "0" && daily == "0"),
        "{err:?}"
    );
    assert_eq!(RateLimit::load(&db.lock().unwrap()).unwrap(), Some(rl));
}

#[tokio::test]
async fn low_hourly_budget_is_typed_error_and_stops_update_checks() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/games/sse/mods/updated.json"))
        .and(query_param("period", "1m"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-rl-hourly-remaining", "19")
                .insert_header("x-rl-daily-remaining", "0")
                .set_body_json(
                    json!([{"mod_id": 1, "latest_file_update": 5, "latest_mod_activity": 5}]),
                ),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/v1/games/sse/mods/1/files.json"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"files": [], "file_updates": []})),
        )
        .expect(0)
        .mount(&server)
        .await;
    let db = shared();
    let c = NexusClient::new(config(&server), Some(auth()), Some(db.clone())).unwrap();
    let out = check_updates(&c, &db, "sse", &[1, 2], false).await.unwrap();
    assert!(out.stopped_early);
    assert_eq!(out.fetched, 0);
    assert!(matches!(
        c.ensure_budget(20),
        Err(NexusError::RateLimitLow {
            remaining: 19,
            reserve: 20
        })
    ));
}

#[tokio::test]
async fn http_429_backs_off_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/users/validate.json"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(2)
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path("/v1/users/validate.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(validate_body(false)))
        .expect(1)
        .mount(&server)
        .await;
    assert!(!client(&server).validate().await.unwrap().is_premium);
}

#[tokio::test]
async fn http_429_after_four_attempts_is_rate_limited() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/users/validate.json"))
        .respond_with(ResponseTemplate::new(429))
        .expect(4)
        .mount(&server)
        .await;
    let err = client(&server).validate().await.unwrap_err();
    assert!(matches!(err, NexusError::RateLimited { .. }), "{err:?}");
}

#[tokio::test]
async fn server_errors_retried_up_to_four_attempts() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/games/sse/mods/5.json"))
        .respond_with(ResponseTemplate::new(503))
        .expect(4)
        .mount(&server)
        .await;
    let err = client(&server).mod_info("sse", 5).await.unwrap_err();
    assert!(
        matches!(err, NexusError::Http { status: 503, .. }),
        "{err:?}"
    );
}

// --- REST info and updates ----------------------------------------------------

#[tokio::test]
async fn mod_info_files_and_not_found() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/games/sse/mods/266.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "mod_id": 266, "name": "SKSE", "version": "2.2", "available": true,
            "contains_adult_content": false, "unknown_field": 1})))
        .mount(&server)
        .await;
    Mock::given(path("/v1/games/sse/mods/266/files.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "files": [{"file_id": 10, "name": "Main", "version": "2.2", "category_name": "MAIN",
                       "file_name": "skse.7z", "size_in_bytes": 1000, "uploaded_timestamp": 1}],
            "file_updates": [{"old_file_id": 9, "new_file_id": 10, "uploaded_timestamp": 1}]})))
        .mount(&server)
        .await;
    let c = client(&server);
    let m = c.mod_info("sse", 266).await.unwrap();
    assert_eq!(m.name.as_deref(), Some("SKSE"));
    let f = c.mod_files("sse", 266).await.unwrap();
    assert_eq!(f.files[0].file_id, 10);
    assert_eq!(f.file_updates[0].new_file_id, 10);
    assert!(matches!(
        c.mod_info("sse", 1).await,
        Err(NexusError::NotFound(_))
    ));
    assert!(matches!(
        c.mod_info("../x", 1).await,
        Err(NexusError::BadUrl(_))
    ));
}

#[tokio::test]
async fn update_check_uses_cache_and_updated_list() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/games/sse/mods/updated.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path("/v1/games/sse/mods/1/files.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "files": [{"file_id": 1, "name": "a", "category_name": "OLD_VERSION"},
                      {"file_id": 2, "name": "b", "category_name": "MAIN"}],
            "file_updates": [{"old_file_id": 1, "new_file_id": 2}]})))
        .expect(1)
        .mount(&server)
        .await;
    let db = shared();
    let c = NexusClient::new(config(&server), Some(auth()), Some(db.clone())).unwrap();
    let first = check_updates(&c, &db, "sse", &[1], false).await.unwrap();
    assert_eq!(first.fetched, 1);
    let second = check_updates(&c, &db, "sse", &[1], false).await.unwrap();
    assert_eq!(second.fetched, 0, "fresh cache must not be re-fetched");
    let files = &second.files[&1];
    assert_eq!(
        vessel_core::nexus::latest_file(files, 1).unwrap().file_id,
        2
    );
}

// --- GraphQL ------------------------------------------------------------------

#[tokio::test]
async fn search_sends_no_adult_flag_and_collection_query_does() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/graphql"))
        .and(body_string_contains("nameStemmed"))
        .respond_with(move |req: &Request| {
            let body: Value = serde_json::from_slice(&req.body).unwrap();
            assert!(body["variables"].get("viewAdultContent").is_none());
            assert_eq!(body["variables"]["d"], "sse");
            ResponseTemplate::new(200).set_body_json(json!({"data": {"mods": {
                "totalCount": 1, "nodes": [{"modId": 266, "name": "SKSE", "downloads": 5}]}}}))
        })
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v2/graphql"))
        .and(body_string_contains("collectionRevision"))
        .respond_with(move |req: &Request| {
            let body: Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["variables"]["viewAdultContent"], true);
            assert_eq!(body["variables"]["slug"], "abc");
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"collectionRevision": {"revisionNumber": 3}}}))
        })
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server);
    let r = c.search("sse", "skse").await.unwrap();
    assert_eq!(r.total, 1);
    assert_eq!(r.mods[0].mod_id, Some(266));
    let d = c
        .collection_graphql(
            "query($slug: String) { collectionRevision(slug: $slug) { revisionNumber } }",
            json!({"slug": "abc"}),
            true,
        )
        .await
        .unwrap();
    assert_eq!(d["collectionRevision"]["revisionNumber"], 3);
}

#[tokio::test]
async fn graphql_errors_are_typed() {
    let server = MockServer::start().await;
    Mock::given(path("/v2/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": null, "errors": [{"message": "boom", "path": ["x"]}]})))
        .mount(&server)
        .await;
    let err = client(&server)
        .graphql("query { x }", Value::Null)
        .await
        .unwrap_err();
    match err {
        NexusError::GraphQl { messages, paths } => {
            assert_eq!(messages, ["boom"]);
            assert_eq!(paths, ["x"]);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn mod_batch_records_hidden_mods_and_retries_rest() {
    let server = MockServer::start().await;
    // First query has both aliases and fails on m2 (hidden mod).
    Mock::given(path("/v2/graphql"))
        .and(body_string_contains("m2:"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": null, "errors": [{"message": "not found", "path": ["m2"]}]})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/v2/graphql"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": {"m1": {"modId": 1}}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let out = client(&server)
        .mod_batch(1704, &[2, 1, 1], "modId")
        .await
        .unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[&1], Some(json!({"modId": 1})));
    assert_eq!(out[&2], None);
}

#[tokio::test]
async fn requirements_cached_with_ttl_and_availability_checked() {
    let server = MockServer::start().await;
    Mock::given(path("/v2/graphql"))
        .and(body_string_contains("modRequirements"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": {"m10": {
            "modRequirements": {"nexusRequirements": {"nodes": [
                {"modId": "20", "modName": "Dep", "gameId": "1704", "externalRequirement": false,
                 "url": "", "notes": ""}]}}}}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/v2/graphql"))
        .and(body_string_contains("m20:"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"m20": null}})))
        .expect(1)
        .mount(&server)
        .await;
    let db = shared();
    let c = client(&server);
    refresh_requirements(&c, &db, "sse", 1704, &[10], false)
        .await
        .unwrap();
    // Within the TTL nothing is asked again (mock expectations are exact).
    refresh_requirements(&c, &db, "sse", 1704, &[10], false)
        .await
        .unwrap();
    let s = db.lock().unwrap();
    let r = cached_requirements(&s, "sse", 10).unwrap().unwrap();
    assert_eq!(r.requirements.unwrap()[0].mod_id, Some(20));
    assert_eq!(r.available, Some(true));
    let dep = cached_requirements(&s, "sse", 20).unwrap().unwrap();
    assert_eq!(dep.available, Some(false));
    assert_eq!(dep.requirements, None);
}

// --- key storage ----------------------------------------------------------------

fn unavailable_opener() -> KeyringOpener {
    Box::new(|| {
        Err(keyring_core::Error::PlatformFailure(
            "org.freedesktop.secrets was not provided by any .service files".into(),
        ))
    })
}

fn mode(p: &std::path::Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn keyring_unavailable_falls_back_to_0600_file_in_0700_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::for_home(tmp.path());
    let ks = KeyStorage::open(&paths, unavailable_opener(), Duration::from_secs(3)).unwrap();
    assert_eq!(ks.backend(), Backend::File);
    assert!(ks.unavailable_reason().is_some());
    let key = ApiKey::new(KEY).unwrap();
    assert_eq!(ks.save(&key).unwrap(), Backend::File);
    let file = paths.config_dir.join(CREDENTIALS_FILE);
    assert_eq!(ks.file_path(), file);
    assert_eq!(mode(&file), 0o600);
    assert_eq!(mode(&paths.config_dir), 0o700);
    assert_eq!(ks.load(None).unwrap(), Some(key.clone()));
    // The env var wins and is never written.
    let env = ks.load(Some("EnvKey")).unwrap().unwrap();
    assert_eq!(env.expose(), "EnvKey");
    assert_eq!(ks.load(None).unwrap(), Some(key));
    ks.clear().unwrap();
    assert!(!file.exists());
    assert_eq!(ks.load(None).unwrap(), None);
}

#[test]
fn existing_open_config_dir_is_tightened_to_0700() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::for_home(tmp.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::set_permissions(&paths.config_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let ks = KeyStorage::new(&paths, ProbeOutcome::Unavailable("test".into()));
    ks.save(&ApiKey::new(KEY).unwrap()).unwrap();
    assert_eq!(mode(&paths.config_dir), 0o700);
}

#[test]
fn keyring_probe_timeout_triggers_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::for_home(tmp.path());
    // Simulates Secret Service activation hanging (spikes.md §3).
    let hung: KeyringOpener = Box::new(|| {
        std::thread::sleep(Duration::from_secs(30));
        unreachable!("probe must have given up")
    });
    let t = Instant::now();
    let ks = KeyStorage::open(&paths, hung, Duration::from_millis(200)).unwrap();
    assert!(t.elapsed() < Duration::from_secs(5));
    assert_eq!(ks.backend(), Backend::File);
    assert!(ks.unavailable_reason().unwrap().contains("did not answer"));
}

#[test]
fn keyring_available_is_used_and_removes_file_copy() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::for_home(tmp.path());
    // Earlier file-backend login.
    KeyStorage::new(&paths, ProbeOutcome::Unavailable("x".into()))
        .save(&ApiKey::new("OldFileKey").unwrap())
        .unwrap();
    let mock = keyring_core::mock::Store::new().unwrap();
    let opener: KeyringOpener = Box::new(move || Ok(mock));
    let ks = KeyStorage::open(&paths, opener, Duration::from_secs(3)).unwrap();
    // The file copy keeps winning until the next login.
    assert_eq!(ks.backend(), Backend::File);
    assert_eq!(ks.load(None).unwrap().unwrap().expose(), "OldFileKey");
    let key = ApiKey::new(KEY).unwrap();
    assert_eq!(ks.save(&key).unwrap(), Backend::Keyring);
    assert!(!paths.config_dir.join(CREDENTIALS_FILE).exists());
    assert_eq!(ks.backend(), Backend::Keyring);
    assert_eq!(ks.load(None).unwrap(), Some(key));
}

#[test]
fn keyring_write_failure_falls_back_to_file() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::for_home(tmp.path());
    let mock = keyring_core::mock::Store::new().unwrap();
    use keyring_core::api::CredentialStoreApi;
    let entry = mock.build("vessel", "nexus-api-key", None).unwrap();
    let twin = mock.build("vessel", "nexus-api-key", None).unwrap();
    let cred: &keyring_core::mock::Cred = twin.as_any().downcast_ref().unwrap();
    cred.set_error(keyring_core::Error::NoStorageAccess("locked".into()));
    let ks = KeyStorage::new(&paths, ProbeOutcome::Available(entry));
    assert_eq!(ks.save(&ApiKey::new(KEY).unwrap()).unwrap(), Backend::File);
    assert_eq!(mode(&paths.config_dir.join(CREDENTIALS_FILE)), 0o600);
}

#[test]
fn group_readable_credentials_file_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::for_home(tmp.path());
    let ks = KeyStorage::new(&paths, ProbeOutcome::Unavailable("x".into()));
    ks.save(&ApiKey::new(KEY).unwrap()).unwrap();
    std::fs::set_permissions(ks.file_path(), std::fs::Permissions::from_mode(0o640)).unwrap();
    let err = ks.load(None).unwrap_err();
    assert!(
        matches!(err, NexusError::InsecureCredentials { mode: 0o640, .. }),
        "{err:?}"
    );
    assert!(!err.to_string().contains(KEY));
}

#[test]
fn symlinked_credentials_file_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::for_home(tmp.path());
    let ks = KeyStorage::new(&paths, ProbeOutcome::Unavailable("x".into()));
    ks.save(&ApiKey::new(KEY).unwrap()).unwrap();
    // A planted link to a 0600 file someone else controls the content of.
    let elsewhere = tmp.path().join("planted.toml");
    std::fs::rename(ks.file_path(), &elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, ks.file_path()).unwrap();
    let err = ks.load(None).unwrap_err();
    assert!(err.to_string().contains("symbolic link"), "{err}");
    assert!(!err.to_string().contains(KEY));
}

// --- redirects ------------------------------------------------------------

#[tokio::test]
async fn api_key_is_not_forwarded_on_cross_origin_redirect() {
    let api = MockServer::start().await;
    let other = MockServer::start().await;
    Mock::given(path("/v1/users/validate.json"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/steal", other.uri()).as_str()),
        )
        .mount(&api)
        .await;
    Mock::given(path("/steal"))
        .respond_with(ResponseTemplate::new(200).set_body_json(validate_body(true)))
        .mount(&other)
        .await;
    let err = client(&api).validate().await.unwrap_err();
    assert!(
        matches!(err, NexusError::Http { status: 302, .. }),
        "{err:?}"
    );
    let seen = other.received_requests().await.unwrap();
    assert!(
        seen.iter().all(|r| !r.headers.contains_key("apikey")),
        "apikey leaked to another host"
    );
    assert!(seen.is_empty());
}

#[tokio::test]
async fn same_origin_redirect_is_followed() {
    let api = MockServer::start().await;
    Mock::given(path("/v1/users/validate.json"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/v1/moved.json"))
        .mount(&api)
        .await;
    Mock::given(path("/v1/moved.json"))
        .and(header("apikey", KEY))
        .respond_with(ResponseTemplate::new(200).set_body_json(validate_body(false)))
        .expect(1)
        .mount(&api)
        .await;
    assert_eq!(client(&api).validate().await.unwrap().user_id, 7);
}
