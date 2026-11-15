// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared helpers for the M3 CLI tests: a temp HOME with isolated XDG dirs
//! and D-Bus address, fake helper binaries on PATH, a minimal HTTP mock of
//! the Nexus API, and a stored-ZIP writer. Nothing touches the real home,
//! keyring, session bus, Steam or the network.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use assert_cmd::Command;
use vessel_core::paths::Paths;
use vessel_core::store::Store;

pub struct Env {
    pub tmp: tempfile::TempDir,
    pub nexus: Option<String>,
    pub extra_env: Vec<(String, String)>,
}

impl Env {
    pub fn new() -> Self {
        let env = Self {
            tmp: tempfile::tempdir().unwrap(),
            nexus: None,
            extra_env: Vec::new(),
        };
        let bin = env.bin();
        std::fs::create_dir_all(&bin).unwrap();
        // Fake desktop helpers record their arguments.
        for name in [
            "notify-send",
            "steam",
            "xdg-open",
            "update-desktop-database",
        ] {
            let log = env.tmp.path().join(format!("{name}.log"));
            let script = format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n", log.display());
            let f = bin.join(name);
            std::fs::write(&f, script).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        env
    }

    pub fn bin(&self) -> PathBuf {
        self.tmp.path().join("bin")
    }

    pub fn home(&self) -> PathBuf {
        self.tmp.path().join("home")
    }

    pub fn paths(&self) -> Paths {
        Paths::for_home(&self.home())
    }

    /// Arguments recorded by a fake helper (one line per call).
    pub fn calls(&self, helper: &str) -> String {
        std::fs::read_to_string(self.tmp.path().join(format!("{helper}.log"))).unwrap_or_default()
    }

    pub fn cmd(&self) -> Command {
        let home = self.home();
        let path = format!(
            "{}:{}",
            self.bin().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut c = Command::cargo_bin("vessel").unwrap();
        c.env_clear()
            .env("PATH", path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_RUNTIME_DIR", self.tmp.path().join("run"))
            // No session bus: the keyring probe and GUI forwarding fail fast.
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", self.tmp.path().join("no-bus").display()),
            );
        if let Some(n) = &self.nexus {
            c.env("VESSEL_NEXUS_URL", n);
        }
        for (k, v) in &self.extra_env {
            c.env(k, v);
        }
        c
    }

    pub fn run(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        self.cmd().args(args).assert()
    }

    pub fn store(&self) -> Store {
        Store::open(&self.paths().db_path()).unwrap()
    }

    pub fn log(&self) -> String {
        std::fs::read_to_string(self.paths().log_path()).unwrap_or_default()
    }

    pub fn game_dir(&self, name: &str) -> PathBuf {
        let d = self.tmp.path().join("games").join(name);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A custom bethesda-like game `domain` with deploy subdir `Data`.
    pub fn add_game(&self, domain: &str) -> PathBuf {
        let dir = self.game_dir(domain);
        std::fs::create_dir_all(dir.join("Data")).unwrap();
        self.run(&[
            "-y",
            "game",
            "add",
            domain,
            dir.to_str().unwrap(),
            "--profile",
            "bethesda",
            "--subdir",
            "Data",
        ])
        .success();
        dir
    }

    /// Writes a stored zip under the temp dir and returns its path.
    pub fn zip(&self, name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let p = self.tmp.path().join("archives").join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, zip_bytes(files)).unwrap();
        p
    }

    pub fn query_i64(&self, sql: &str) -> i64 {
        self.store()
            .conn()
            .query_row(sql, [], |r| r.get(0))
            .unwrap()
    }

    pub fn uids(&self) -> Vec<String> {
        let s = self.store();
        let mut st = s
            .conn()
            .prepare("SELECT uid FROM mods ORDER BY priority, id")
            .unwrap();
        st.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    pub fn enabled(&self) -> Vec<(String, bool)> {
        let s = self.store();
        let mut st = s
            .conn()
            .prepare("SELECT uid, enabled FROM mods ORDER BY priority, id")
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }
}

// ── stored ZIP writer ────────────────────────────────────────

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A ZIP archive with uncompressed ("stored") entries.
pub fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let crc = crc32(data);
        let off = out.len() as u32;
        let n = name.as_bytes();
        let sz = data.len() as u32;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0x21, 0]);
        for v in [crc, sz, sz] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&(n.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(n);
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&[20, 3, 20, 0, 0, 0, 0, 0, 0, 0, 0x21, 0]);
        for v in [crc, sz, sz] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        central.extend_from_slice(&(n.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        central.extend_from_slice(&(0o100644u32 << 16).to_le_bytes());
        central.extend_from_slice(&off.to_le_bytes());
        central.extend_from_slice(n);
    }
    let cd_off = out.len() as u32;
    let cd_len = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    let count = files.len() as u16;
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_len.to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

// ── HTTP mock ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// Path with query.
    pub target: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Request {
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(body: &str) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn bytes(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type: "application/octet-stream",
            body,
        }
    }
}

type Handler = dyn Fn(&Request, &str) -> Response + Send + Sync;

/// HTTP/1.1 server on 127.0.0.1; one request per connection.
pub struct Mock {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Request>>>,
}

impl Mock {
    /// `handler(request, base_url)`.
    pub fn start(handler: impl Fn(&Request, &str) -> Response + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler: Arc<Handler> = Arc::new(handler);
        let (reqs, base) = (requests.clone(), url.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (reqs, base, handler) = (reqs.clone(), base.clone(), handler.clone());
                std::thread::spawn(move || {
                    let _ = serve(stream, &reqs, &base, &*handler);
                });
            }
        });
        Self { url, requests }
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

fn serve(
    stream: TcpStream,
    reqs: &Mutex<Vec<Request>>,
    base: &str,
    handler: &Handler,
) -> std::io::Result<()> {
    let mut r = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    let req = Request {
        method,
        target,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    };
    let resp = handler(&req, base);
    let head = req.method == "HEAD";
    reqs.lock().unwrap().push(req);
    let mut w = stream;
    write!(
        w,
        "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        resp.status,
        resp.content_type,
        resp.body.len()
    )?;
    if !head {
        w.write_all(&resp.body)?;
    }
    w.flush()
}

/// Standard Nexus routes for one mod with files `files`
/// (`(file_id, name, category, version, archive bytes)`).
pub struct FakeNexus {
    pub premium: bool,
    pub game: &'static str,
    pub game_id: u64,
    pub mod_id: u64,
    pub mod_name: &'static str,
    pub files: Vec<(u64, &'static str, &'static str, &'static str, Vec<u8>)>,
    pub file_updates: Vec<(u64, u64)>,
    pub search_body: String,
}

impl FakeNexus {
    pub fn handle(&self, req: &Request, base: &str) -> Response {
        let g = self.game;
        let m = self.mod_id;
        let path = req.path().to_string();
        if path == "/v1/users/validate.json" {
            return Response::json(&format!(
                r#"{{"user_id":7,"name":"tester","is_premium":{},"is_supporter":false,"key":"SECRET-ECHO"}}"#,
                self.premium
            ));
        }
        if path == format!("/v1/games/{g}.json") {
            return Response::json(&format!(
                r#"{{"id":{},"name":"Test Game","domain_name":"{g}"}}"#,
                self.game_id
            ));
        }
        if path.starts_with("/v1/games/")
            && path.ends_with(".json")
            && path.matches('/').count() == 3
        {
            return Response::status(404, r#"{"message":"Game not found"}"#);
        }
        if path == format!("/v1/games/{g}/mods/{m}.json") {
            return Response::json(&format!(
                r#"{{"mod_id":{m},"name":"{}","version":"1.0","author":"Ann","status":"published","available":true}}"#,
                self.mod_name
            ));
        }
        if path == format!("/v1/games/{g}/mods/{m}/files.json") {
            let files: Vec<String> = self
                .files
                .iter()
                .enumerate()
                .map(|(i, (id, name, cat, ver, data))| {
                    format!(
                        r#"{{"file_id":{id},"name":"{name}","version":"{ver}","category_name":"{cat}","file_name":"{name}.zip","size_in_bytes":{},"uploaded_timestamp":{}}}"#,
                        data.len(),
                        1000 + i
                    )
                })
                .collect();
            let ups: Vec<String> = self
                .file_updates
                .iter()
                .map(|(o, n)| format!(r#"{{"old_file_id":{o},"new_file_id":{n}}}"#))
                .collect();
            return Response::json(&format!(
                r#"{{"files":[{}],"file_updates":[{}]}}"#,
                files.join(","),
                ups.join(",")
            ));
        }
        if path == format!("/v1/games/{g}/mods/updated.json") {
            return Response::json(&format!(
                r#"[{{"mod_id":{m},"latest_file_update":9999999999,"latest_mod_activity":9999999999}}]"#
            ));
        }
        for (id, name, ..) in &self.files {
            if path == format!("/v1/games/{g}/mods/{m}/files/{id}/download_link.json") {
                let keyed = req.target.contains("key=");
                if !self.premium && !keyed {
                    return Response::status(403, r#"{"message":"premium only"}"#);
                }
                return Response::json(&format!(
                    r#"[{{"name":"Nexus CDN","short_name":"Nexus","URI":"{base}/cdn/{id}/{name}.zip?md5=x&expires=1"}}]"#
                ));
            }
            if path.starts_with(&format!("/cdn/{id}/")) {
                let data = self.files.iter().find(|f| f.0 == *id).unwrap().4.clone();
                return Response::bytes(data);
            }
        }
        if path == "/v2/graphql" {
            if req.body.contains("nameStemmed") {
                return Response::json(&self.search_body);
            }
            return Response::json(r#"{"data":{}}"#);
        }
        Response::status(404, r#"{"message":"not mocked"}"#)
    }
}
