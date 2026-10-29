// SPDX-License-Identifier: GPL-3.0-or-later
//! Secrets never reach tracing output or error strings. Kept in its own test
//! binary: tracing caches callsite interest process-wide, so a capture
//! subscriber racing other test threads could miss events.

use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use vessel_core::download::{FetchRequest, Fetcher, NxmLink};
use vessel_core::nexus::{
    ApiKey, ApiKeyAuth, AuthProvider, NexusClient, NexusConfig, NexusError, NxmKey, RetryPolicy,
};
use vessel_core::store;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "SuperSecretApiKey0123456789";

fn config(server: &MockServer) -> NexusConfig {
    let mut c = NexusConfig::with_base(&server.uri()).unwrap();
    c.retry = RetryPolicy {
        attempts: 2,
        rate_limit_step: Duration::from_millis(5),
        server_step: Duration::from_millis(5),
    };
    c
}

fn auth() -> Arc<dyn AuthProvider> {
    Arc::new(ApiKeyAuth::new(ApiKey::new(KEY).unwrap()))
}

fn client(server: &MockServer) -> NexusClient {
    NexusClient::new(config(server), Some(auth()), None).unwrap()
}

// --- secrets never leak ---------------------------------------------------------

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<String>>);

struct Visitor<'a>(&'a Mutex<String>);

impl tracing::field::Visit for Visitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn Debug) {
        let mut s = self.0.lock().unwrap();
        s.push_str(&format!("{}={:?} ", field.name(), value));
    }
}

impl tracing::Subscriber for Capture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attrs: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        attrs.record(&mut Visitor(&self.0));
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        values.record(&mut Visitor(&self.0));
    }
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        self.0.lock().unwrap().push_str(event.metadata().target());
        event.record(&mut Visitor(&self.0));
        self.0.lock().unwrap().push('\n');
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[tokio::test]
async fn api_key_never_in_logs_or_error_strings() {
    let cap = Capture::default();
    let _guard = tracing::subscriber::set_default(cap.clone());
    tracing::callsite::rebuild_interest_cache();
    let nxm_secret = "NxmSecretKey987";

    let server = MockServer::start().await;
    Mock::given(path("/v1/users/validate.json"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    // An error body echoing both secrets.
    Mock::given(path("/v1/games/sse/mods/1/files/2/download_link.json"))
        .respond_with(
            ResponseTemplate::new(500)
                .set_body_json(json!({"message": format!("bad key {KEY} / {nxm_secret}")})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/v1/games/sse/mods/3.json"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&server)
        .await;
    let c = client(&server);
    let k = NxmKey {
        key: nxm_secret.into(),
        expires: store::now() + 60,
        user_id: None,
    };
    let mut errors = vec![
        c.validate().await.unwrap_err(),
        c.download_links("sse", 1, 2, Some(&k)).await.unwrap_err(),
        c.mod_info("sse", 3).await.unwrap_err(),
    ];
    // Network failure on a URL carrying the nxm key.
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut cfg = config(&server);
    cfg.rest_base = format!("http://127.0.0.1:{closed}/v1").parse().unwrap();
    let dead = NexusClient::new(cfg, Some(auth()), None).unwrap();
    errors.push(
        dead.download_links("sse", 1, 2, Some(&k))
            .await
            .unwrap_err(),
    );
    assert!(
        matches!(errors[3], NexusError::Network(_)),
        "{:?}",
        errors[3]
    );

    // CDN download whose signed URL carries the nxm key, failing mid-way.
    let link = NxmLink::parse(&format!(
        "nxm://sse/mods/1/files/2?key={nxm_secret}&expires=99999999999"
    ))
    .unwrap();
    tracing::info!(link = %link.redacted(), "nxm link received");
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("f.7z");
    let fetcher = Fetcher::new()
        .unwrap()
        .with_retry(1, Duration::from_millis(1));
    let derr = fetcher
        .fetch(FetchRequest {
            url: &format!("http://127.0.0.1:{closed}/f.7z?key={nxm_secret}"),
            dest: &dest,
            md5: None,
            sha256: None,
            events: None,
        })
        .await
        .unwrap_err();
    for text in [derr.to_string(), format!("{derr:?}"), format!("{link:?}")] {
        assert!(!text.contains(nxm_secret), "nxm key leaked: {text}");
    }

    let logs = cap.0.lock().unwrap().clone();
    assert!(logs.contains("Nexus request"), "capture works: {logs}");
    for e in &errors {
        for text in [e.to_string(), format!("{e:?}")] {
            assert!(!text.contains(KEY), "API key leaked: {text}");
            assert!(!text.contains(nxm_secret), "nxm key leaked: {text}");
        }
    }
    assert!(!logs.contains(KEY), "API key in logs: {logs}");
    assert!(!logs.contains(nxm_secret), "nxm key in logs: {logs}");
    assert!(!format!("{c:?}").contains(KEY));
}
