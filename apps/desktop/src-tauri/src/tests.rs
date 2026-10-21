// SPDX-License-Identifier: GPL-3.0-or-later
//! Configuration and contract checks: identifier/D-Bus name, CSP, window
//! size, capabilities = exactly the app commands, no deep-link plugin, no
//! HTTP server, and the API key never reaching the webview.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::sync::Arc;

use serde_json::Value;

use crate::events::testing::Recorder;
use crate::state::AppState;
use crate::COMMANDS;

const CONF: &str = include_str!("../tauri.conf.json");
const CAPS: &str = include_str!("../capabilities/default.json");
const CARGO: &str = include_str!("../Cargo.toml");
const LIB: &str = include_str!("lib.rs");
const COMMANDS_RS: &str = include_str!("commands.rs");

fn conf() -> Value {
    serde_json::from_str(CONF).unwrap()
}

#[test]
fn identifier_matches_the_single_instance_bus_name() {
    let c = conf();
    assert_eq!(c["identifier"], "org.modman.app");
    assert_eq!(
        modman_core::ipc::SINGLE_INSTANCE_BUS,
        format!("{}.SingleInstance", c["identifier"].as_str().unwrap())
    );
    // The plugin's `semver` feature would suffix the bus name.
    let line = CARGO
        .lines()
        .find(|l| l.starts_with("tauri-plugin-single-instance"))
        .unwrap();
    assert!(line.contains("\"=2.5.4\"") && line.contains("default-features = false"));
    assert!(!line.contains("semver") && !line.contains("deep-link"));
}

#[test]
fn no_deep_link_plugin_and_no_http_server() {
    for banned in [
        "tauri-plugin-deep-link",
        "tauri-plugin-localhost",
        "tauri-plugin-http",
        "tiny_http",
        "axum",
        "warp",
        "actix",
        "hyper",
    ] {
        assert!(!CARGO.contains(banned), "{banned}");
    }
    assert!(conf()["plugins"].get("deep-link").is_none());
    let src = [LIB, COMMANDS_RS].concat();
    for gone in ["ui-state", "ui-detect", "ui-thumbs", "TcpListener", "bind("] {
        assert!(!src.contains(gone), "{gone}");
    }
}

/// Sources a CSP directive allows.
fn directive<'a>(csp: &'a str, name: &str) -> Vec<&'a str> {
    csp.split(';')
        .map(str::trim)
        .find_map(|d| d.strip_prefix(name).filter(|r| r.starts_with(' ')))
        .map(|r| r.split_whitespace().collect())
        .unwrap_or_default()
}

#[test]
fn csp_has_no_unsafe_eval_and_no_remote_origins() {
    let c = conf();
    let sec = &c["app"]["security"];
    let csp = sec["csp"].as_str().expect("a CSP is set");
    let dev = sec["devCsp"].as_str().unwrap();
    for policy in [csp, dev] {
        assert!(!policy.contains("unsafe-eval"), "{policy}");
        assert!(!policy.contains('*'), "{policy}");
        assert_eq!(directive(policy, "default-src"), ["'self'"]);
        assert_eq!(directive(policy, "script-src"), ["'self'"]);
        assert_eq!(directive(policy, "object-src"), ["'none'"]);
        for src in policy.split([';', ' ']).filter(|s| s.contains("://")) {
            let local = src == "http://ipc.localhost" || src.starts_with("ws://localhost:");
            assert!(local, "remote origin {src} in {policy}");
        }
    }
    assert!(!csp.contains("unsafe-inline"));
    assert!(!csp.contains("ws://"));
    assert_eq!(sec["freezePrototype"], true);
    assert_eq!(c["app"]["withGlobalTauri"], false);
}

#[test]
fn main_window_defaults_to_1280x800() {
    let c = conf();
    let w = &c["app"]["windows"][0];
    assert_eq!(w["label"], "main");
    assert_eq!(
        (w["width"].as_u64(), w["height"].as_u64()),
        (Some(1280), Some(800))
    );
}

#[test]
fn capabilities_grant_exactly_the_app_commands_and_event_listening() {
    let caps: Value = serde_json::from_str(CAPS).unwrap();
    assert_eq!(caps["windows"], serde_json::json!(["main"]));
    let granted: BTreeSet<String> = caps["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    let mut want: BTreeSet<String> = COMMANDS
        .iter()
        .map(|c| format!("allow-{}", c.replace('_', "-")))
        .collect();
    want.insert("core:event:allow-listen".into());
    want.insert("core:event:allow-unlisten".into());
    assert_eq!(granted, want);
}

#[test]
fn every_listed_command_is_registered_and_defined() {
    let registered: BTreeSet<&str> = LIB
        .lines()
        .filter_map(|l| l.trim().strip_prefix("commands::"))
        .map(|l| l.trim_end_matches(','))
        .collect();
    let listed: BTreeSet<&str> = COMMANDS.iter().copied().collect();
    assert_eq!(registered, listed);
    for c in COMMANDS {
        assert!(
            COMMANDS_RS.contains(&format!("pub async fn {c}("))
                || COMMANDS_RS.contains(&format!("pub fn {c}(")),
            "{c}"
        );
    }
}

/// A one-shot HTTP server answering 401, as Nexus does for a bad key.
fn unauthorized_server() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming().take(4).flatten() {
            let mut s = s;
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = s.write_all(
                b"HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: 27\r\nconnection: close\r\n\r\n{\"message\":\"Invalid key\"}\n\n",
            );
        }
    });
    format!("http://{addr}")
}

#[test]
fn a_rejected_api_key_is_never_echoed_to_the_webview() {
    const KEY: &str = "SECRETKEY0123456789";
    let base = unauthorized_server();
    // Only this test talks to Nexus in this crate.
    std::env::set_var(modman_core::app::nexus::BASE_URL_ENV, &base);
    let home = tempfile::tempdir().unwrap();
    let st = AppState::new(modman_core::paths::Paths::for_home(home.path()));
    let sink = Arc::new(Recorder::default());
    let r = st.0.query(sink.clone(), None, |cx| {
        modman_core::app::nexus::login(cx.paths, cx.store, KEY)
    });
    let e = r.unwrap_err();
    assert!(
        e.message.starts_with("Authentication failed"),
        "{}",
        e.message
    );
    assert!(!e.message.contains(KEY));
    let events = serde_json::to_string(&*sink.events.lock().unwrap()).unwrap();
    assert!(!events.contains(KEY));
    // Nothing was stored.
    let store = st.0.open_store().unwrap();
    assert_eq!(
        store
            .setting(modman_core::app::nexus::AUTH_BACKEND_KEY)
            .unwrap(),
        None
    );
}

const TS_COMMANDS: &str = include_str!("../../src/ipc/commands.ts");
const TS_EVENTS: &str = include_str!("../../src/ipc/events.ts");

#[test]
fn ts_bindings_list_the_same_commands_and_events() {
    let start = TS_COMMANDS.find("export const COMMANDS = [").unwrap();
    let body = &TS_COMMANDS[start..TS_COMMANDS[start..].find("] as const").unwrap() + start];
    let ts: BTreeSet<&str> = body
        .lines()
        .filter_map(|l| l.trim().strip_prefix('"')?.strip_suffix("\","))
        .collect();
    assert_eq!(ts, COMMANDS.iter().copied().collect::<BTreeSet<_>>());
    for c in COMMANDS {
        assert!(
            TS_COMMANDS.contains(&format!("(\"{c}\"")),
            "no wrapper invokes {c}"
        );
    }
    for ev in [
        crate::events::ACTIVITY,
        crate::events::JOB,
        crate::events::PROMPT,
        crate::events::PROMPT_CLOSED,
        crate::events::DOWNLOAD,
        crate::events::DEPLOY_PROGRESS,
        crate::events::NXM,
        crate::events::STARTUP_CHECK,
    ] {
        assert!(TS_EVENTS.contains(&format!("\"{ev}\"")), "{ev}");
    }
}
