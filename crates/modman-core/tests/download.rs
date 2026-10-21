// SPDX-License-Identifier: GPL-3.0-or-later
//! Resumable downloads (local servers only) and the downloads queue.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use modman_core::download::{
    self, DownloadError, DownloadEvent, DownloadSource, DownloadStatus, FetchRequest, Fetcher,
    NewDownload, NxmLink,
};
use modman_core::store::Store;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn payload(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 31 % 251) as u8).collect()
}

fn sha_hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

fn md5_hex(b: &[u8]) -> String {
    use md5::Md5;
    Md5::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn fetcher() -> Fetcher {
    Fetcher::new()
        .unwrap()
        .with_retry(3, Duration::from_millis(5))
}

async fn read_head(sock: &mut tokio::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut b = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        if sock.read(&mut b).await.unwrap() == 0 {
            break;
        }
        buf.push(b[0]);
    }
    String::from_utf8(buf).unwrap()
}

/// First connection sends half the body then drops; later ones honour Range.
async fn flaky_server(body: Vec<u8>, seen: Arc<Mutex<Vec<String>>>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut n = 0;
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let head = read_head(&mut sock).await;
            seen.lock().unwrap().push(head.clone());
            n += 1;
            let total = body.len();
            if n == 1 {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
                );
                sock.write_all(hdr.as_bytes()).await.unwrap();
                sock.write_all(&body[..total / 2]).await.unwrap();
                sock.flush().await.unwrap();
                drop(sock);
                continue;
            }
            let start: usize = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("range: bytes=")
                        .map(str::to_owned)
                })
                .and_then(|r| r.trim_end_matches('-').parse().ok())
                .unwrap_or(0);
            let rest = &body[start..];
            let hdr = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{}/{total}\r\nConnection: close\r\n\r\n",
                rest.len(),
                total - 1
            );
            sock.write_all(hdr.as_bytes()).await.unwrap();
            sock.write_all(rest).await.unwrap();
            sock.flush().await.unwrap();
        }
    });
    format!("http://{addr}/files/mod.7z?md5=sig&expires=1")
}

#[tokio::test]
async fn interrupted_download_resumes_with_range_and_sha_matches() {
    let body = payload(300_000);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let url = flaky_server(body.clone(), seen.clone()).await;
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("dl/mod.7z");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let got = fetcher()
        .fetch(FetchRequest {
            url: &url,
            dest: &dest,
            md5: Some(&md5_hex(&body)),
            sha256: Some(&sha_hex(&body)),
            events: Some(&tx),
        })
        .await
        .unwrap();
    assert_eq!(got.sha256, sha_hex(&body));
    assert_eq!(got.size, body.len() as u64);
    assert_eq!(std::fs::read(&dest).unwrap(), body);
    assert!(!Fetcher::part_path(&dest).exists());

    let heads = seen.lock().unwrap().clone();
    assert_eq!(heads.len(), 2, "{heads:?}");
    assert!(!heads[0].to_ascii_lowercase().contains("range:"));
    assert!(
        heads[1]
            .to_ascii_lowercase()
            .contains(&format!("range: bytes={}-", body.len() / 2)),
        "{}",
        heads[1]
    );

    drop(tx);
    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    assert!(events.contains(&DownloadEvent::Started {
        resumed_from: body.len() as u64 / 2,
        total: Some(body.len() as u64)
    }));
    assert!(events
        .iter()
        .any(|e| matches!(e, DownloadEvent::Retrying { attempt: 2, .. })));
    assert_eq!(
        events.last(),
        Some(&DownloadEvent::Finished {
            size: body.len() as u64,
            sha256: sha_hex(&body)
        })
    );
}

#[tokio::test]
async fn existing_part_file_resumed_from_its_length() {
    let body = payload(10_000);
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/f.zip"))
        .and(header("range", "bytes=4000-"))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header("content-range", "bytes 4000-9999/10000")
                .set_body_bytes(body[4000..].to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("f.zip");
    std::fs::write(Fetcher::part_path(&dest), &body[..4000]).unwrap();
    let got = fetcher()
        .fetch(FetchRequest {
            url: &format!("{}/f.zip", server.uri()),
            dest: &dest,
            md5: None,
            sha256: Some(&sha_hex(&body)),
            events: None,
        })
        .await
        .unwrap();
    assert_eq!(got.resumed_from, 4000);
    assert_eq!(std::fs::read(&dest).unwrap(), body);
}

#[tokio::test]
async fn server_ignoring_range_restarts_from_zero() {
    let body = payload(5_000);
    let server = MockServer::start().await;
    Mock::given(path("/f.zip"))
        .and(header_exists("range"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("f.zip");
    std::fs::write(Fetcher::part_path(&dest), b"garbage-prefix").unwrap();
    let got = fetcher()
        .fetch(FetchRequest {
            url: &format!("{}/f.zip", server.uri()),
            dest: &dest,
            md5: Some(&md5_hex(&body)),
            sha256: None,
            events: None,
        })
        .await
        .unwrap();
    assert_eq!(got.resumed_from, 0);
    assert_eq!(std::fs::read(&dest).unwrap(), body);
}

#[tokio::test]
async fn checksum_mismatch_retries_then_fails_without_leaving_files() {
    let server = MockServer::start().await;
    Mock::given(path("/f.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"corrupt".to_vec()))
        .expect(3)
        .mount(&server)
        .await;
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("f.zip");
    let err = fetcher()
        .fetch(FetchRequest {
            url: &format!("{}/f.zip", server.uri()),
            dest: &dest,
            md5: Some("00000000000000000000000000000000"),
            sha256: None,
            events: None,
        })
        .await
        .unwrap_err();
    assert!(
        matches!(err, DownloadError::ChecksumMismatch { .. }),
        "{err:?}"
    );
    assert!(!dest.exists());
    assert!(!Fetcher::part_path(&dest).exists());
}

#[tokio::test]
async fn client_error_is_not_retried() {
    let server = MockServer::start().await;
    Mock::given(path("/gone.zip"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    let tmp = tempfile::tempdir().unwrap();
    let err = fetcher()
        .fetch(FetchRequest {
            url: &format!("{}/gone.zip", server.uri()),
            dest: &tmp.path().join("gone.zip"),
            md5: None,
            sha256: None,
            events: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, DownloadError::Http { status: 404 }));
}

#[tokio::test]
async fn head_file_name_from_content_disposition() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/opaque/123"))
        .respond_with(move |_: &Request| {
            ResponseTemplate::new(200).insert_header(
                "content-disposition",
                "attachment; filename=\"Real Name-1-0.7z\"",
            )
        })
        .mount(&server)
        .await;
    let name = fetcher()
        .head_file_name(&format!("{}/opaque/123", server.uri()))
        .await;
    assert_eq!(name.as_deref(), Some("Real Name-1-0.7z"));
}

// --- queue / intake ---------------------------------------------------------------

#[test]
fn nxm_intake_stores_url_without_secrets_and_dedups() {
    let store = Store::open_in_memory().unwrap();
    let link = NxmLink::parse(
        "nxm://SkyrimSpecialEdition/mods/266/files/1000?key=K1&expires=100&user_id=7",
    )
    .unwrap();
    let id = download::enqueue_nxm(&store, &link).unwrap();
    let row = download::get(&store, id).unwrap();
    assert_eq!(
        row.url.as_deref(),
        Some("nxm://skyrimspecialedition/mods/266/files/1000")
    );
    assert_eq!(row.source, DownloadSource::Nxm);
    assert_eq!(row.status, DownloadStatus::Queued);
    assert_eq!(row.nxm.as_ref().unwrap().key, "K1");
    assert_eq!(row.nxm.as_ref().unwrap().user_id, Some(7));

    download::mark_failed(&store, id, "network").unwrap();
    let again = NxmLink::parse("nxm://skyrimspecialedition/mods/266/files/1000?key=K2&expires=200")
        .unwrap();
    assert_eq!(download::enqueue_nxm(&store, &again).unwrap(), id);
    let row = download::get(&store, id).unwrap();
    assert_eq!(row.status, DownloadStatus::Queued, "failed row re-queued");
    assert_eq!(row.nxm.as_ref().unwrap().key, "K2");
    assert_eq!(row.error, None);
    assert_eq!(download::list(&store, None).unwrap().len(), 1);
}

#[test]
fn collection_intake_and_lifecycle() {
    let store = Store::open_in_memory().unwrap();
    let link = NxmLink::parse("nxm://sse/collections/qdurkx/revisions/4").unwrap();
    let id = download::enqueue_nxm(&store, &link).unwrap();
    assert_eq!(download::enqueue_nxm(&store, &link).unwrap(), id);
    let row = download::get(&store, id).unwrap();
    assert_eq!(row.source, DownloadSource::Collection);
    assert_eq!(row.collection_slug.as_deref(), Some("qdurkx"));
    assert_eq!(row.collection_revision, Some(4));
    let latest = NxmLink::parse("nxm://sse/collections/qdurkx").unwrap();
    assert_ne!(download::enqueue_nxm(&store, &latest).unwrap(), id);

    let mut d = NewDownload::new(DownloadSource::Direct);
    d.url = Some("https://cdn.example/f.7z?md5=abc&expires=9&key=SECRET".into());
    d.dest_path = Some("/tmp/x/f.7z".into());
    let did = download::enqueue(&store, &d).unwrap();
    let row = download::get(&store, did).unwrap();
    assert_eq!(row.url.as_deref(), Some("https://cdn.example/f.7z?md5=abc"));
    assert_eq!(row.part_path.as_deref(), Some("/tmp/x/f.7z.part"));
    download::update_progress(&store, did, 10, Some(100)).unwrap();
    let row = download::get(&store, did).unwrap();
    assert_eq!(
        (row.bytes_done, row.size, row.status),
        (10, Some(100), DownloadStatus::Downloading)
    );
    download::set_status(&store, did, DownloadStatus::Paused).unwrap();
    assert_eq!(
        download::list(&store, Some(DownloadStatus::Paused))
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        download::get(&store, 999),
        Err(DownloadError::NotFound(999))
    ));
}

#[tokio::test]
async fn mark_done_records_hashes_and_drops_nxm_key() {
    let body = payload(1000);
    let server = MockServer::start().await;
    Mock::given(path("/f.7z"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;
    let store = Store::open_in_memory().unwrap();
    let link = NxmLink::parse("nxm://sse/mods/1/files/2?key=K&expires=100").unwrap();
    let id = download::enqueue_nxm(&store, &link).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("f.7z");
    let got = fetcher()
        .fetch(FetchRequest {
            url: &format!("{}/f.7z", server.uri()),
            dest: &dest,
            md5: None,
            sha256: None,
            events: None,
        })
        .await
        .unwrap();
    download::mark_done(&store, id, &got).unwrap();
    let row = download::get(&store, id).unwrap();
    assert_eq!(row.status, DownloadStatus::Done);
    assert_eq!(row.sha256.as_deref(), Some(sha_hex(&body).as_str()));
    assert_eq!(row.md5.as_deref(), Some(md5_hex(&body).as_str()));
    assert_eq!(row.file_name.as_deref(), Some("f.7z"));
    assert_eq!(row.nxm, None);
}
