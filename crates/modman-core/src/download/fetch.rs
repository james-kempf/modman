// SPDX-License-Identifier: GPL-3.0-or-later
//! Resumable, verified streaming downloads (legacy `fetch_file` L1443-1466).
//! Data goes to `<dest>.part`, resumed with `Range` after an interruption,
//! and is renamed to `dest` only after the checksums match.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use md5::Md5;
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_RANGE, RANGE, USER_AGENT};
use reqwest::StatusCode;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::UnboundedSender;

use super::DownloadError;
use crate::nexus::install_crypto_provider;

/// Progress events, sent best-effort (a dropped receiver is ignored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadEvent {
    Started {
        resumed_from: u64,
        total: Option<u64>,
    },
    Progress {
        done: u64,
        total: Option<u64>,
    },
    Retrying {
        attempt: u32,
        reason: String,
    },
    Finished {
        size: u64,
        sha256: String,
    },
}

pub struct FetchRequest<'a> {
    /// CDN URL; may carry signed query parameters, so it is never logged.
    pub url: &'a str,
    pub dest: &'a Path,
    /// Expected checksums (lower-case hex), when known.
    pub md5: Option<&'a str>,
    pub sha256: Option<&'a str>,
    pub events: Option<&'a UnboundedSender<DownloadEvent>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub path: PathBuf,
    pub size: u64,
    pub sha256: String,
    pub md5: String,
    /// Bytes reused from an earlier `.part`.
    pub resumed_from: u64,
}

const PROGRESS_STEP: u64 = 256 * 1024;

#[derive(Debug, Clone)]
pub struct Fetcher {
    http: reqwest::Client,
    attempts: u32,
    retry_step: Duration,
}

fn net(e: reqwest::Error) -> String {
    crate::nexus::client_network_message(e)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash_file(path: &Path) -> std::io::Result<(String, String, u64)> {
    let mut f = std::fs::File::open(path)?;
    let (mut sha, mut md5) = (Sha256::new(), Md5::new());
    let mut buf = vec![0u8; 1 << 16];
    let mut n_total = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        sha.update(&buf[..n]);
        md5.update(&buf[..n]);
        n_total += n as u64;
    }
    Ok((hex(&sha.finalize()), hex(&md5.finalize()), n_total))
}

/// Lower-case hex md5 of a file (identifies local archives on Nexus).
pub fn md5_file(path: &Path) -> Result<String, DownloadError> {
    hash_file(path)
        .map(|(_, md5, _)| md5)
        .map_err(DownloadError::io(path))
}

/// `bytes <start>-<end>/<total>` -> (start, total).
fn content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let rest = v.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let start = range.split_once('-')?.0.trim().parse().ok()?;
    Some((start, total.trim().parse().ok()))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hexval = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hexval(b[i + 1]), hexval(b[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A safe local file name: no separators, NUL or leading dots.
fn sanitize_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\0') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_owned();
    (!cleaned.is_empty() && cleaned.len() <= 255).then_some(cleaned)
}

/// File name from the URI path, url-decoded (legacy L1489-1491). `None`
/// when the last segment has no extension (opaque CDN ids).
pub fn file_name_from_uri(uri: &str) -> Option<String> {
    let path = uri.split(['?', '#']).next()?;
    let last = path.rsplit('/').next()?;
    let name = sanitize_name(&percent_decode(last))?;
    name.contains('.').then_some(name)
}

fn disposition_name(v: &str) -> Option<String> {
    let idx = v.to_ascii_lowercase().find("filename=")?;
    let rest = &v[idx + "filename=".len()..];
    let name = match rest.strip_prefix('"') {
        Some(q) => q.split('"').next()?,
        None => rest.split(';').next()?.trim(),
    };
    sanitize_name(name)
}

impl Fetcher {
    pub fn new() -> Result<Self, DownloadError> {
        install_crypto_provider();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| DownloadError::Network(net(e)))?;
        Ok(Self {
            http,
            attempts: 3,
            retry_step: Duration::from_secs(2),
        })
    }

    /// Attempts per download and the back-off step (waits `step * attempt`).
    pub fn with_retry(mut self, attempts: u32, step: Duration) -> Self {
        self.attempts = attempts.max(1);
        self.retry_step = step;
        self
    }

    pub fn part_path(dest: &Path) -> PathBuf {
        let mut s = dest.as_os_str().to_owned();
        s.push(".part");
        PathBuf::from(s)
    }

    fn user_agent() -> String {
        format!("modman/{}", crate::VERSION)
    }

    /// The `Content-Disposition` file name from a HEAD request (newer CDN
    /// URLs are opaque ids, legacy L1492-1497).
    pub async fn head_file_name(&self, url: &str) -> Option<String> {
        let resp = self
            .http
            .head(url)
            .header(USER_AGENT, Self::user_agent())
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .ok()?;
        disposition_name(resp.headers().get(CONTENT_DISPOSITION)?.to_str().ok()?)
    }

    pub async fn fetch(&self, req: FetchRequest<'_>) -> Result<Fetched, DownloadError> {
        let emit = |e: DownloadEvent| {
            if let Some(tx) = req.events {
                let _ = tx.send(e);
            }
        };
        let part = Self::part_path(req.dest);
        if let Some(dir) = req.dest.parent() {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(DownloadError::io(dir))?;
        }
        let mut last_err = DownloadError::Network("no attempt made".into());
        let mut first_resume = None;
        for attempt in 1..=self.attempts {
            if attempt > 1 {
                emit(DownloadEvent::Retrying {
                    attempt,
                    reason: last_err.to_string(),
                });
                tokio::time::sleep(self.retry_step * (attempt - 1)).await;
            }
            match self.try_once(&req, &part, &emit).await {
                Ok(have) => {
                    first_resume.get_or_insert(have.resumed_from);
                    match self.verify(&req, &part).await {
                        Ok(mut f) => {
                            f.resumed_from = first_resume.unwrap_or(0);
                            emit(DownloadEvent::Finished {
                                size: f.size,
                                sha256: f.sha256.clone(),
                            });
                            return Ok(f);
                        }
                        Err(e @ DownloadError::ChecksumMismatch { .. }) => {
                            let _ = tokio::fs::remove_file(&part).await;
                            tracing::warn!(attempt, "checksum mismatch; restarting download");
                            last_err = e;
                        }
                        Err(e) => return Err(e),
                    }
                }
                Err(Attempt::Retry(e)) => {
                    first_resume.get_or_insert(0);
                    tracing::debug!(attempt, error = %e, "download interrupted");
                    last_err = e;
                }
                Err(Attempt::Fatal(e)) => return Err(e),
            }
        }
        if matches!(last_err, DownloadError::ChecksumMismatch { .. }) {
            let _ = tokio::fs::remove_file(&part).await;
        }
        Err(last_err)
    }

    async fn try_once(
        &self,
        req: &FetchRequest<'_>,
        part: &Path,
        emit: &impl Fn(DownloadEvent),
    ) -> Result<Progressed, Attempt> {
        let have = match tokio::fs::metadata(part).await {
            Ok(m) => m.len(),
            Err(_) => 0,
        };
        let mut rq = self
            .http
            .get(req.url)
            .header(USER_AGENT, Self::user_agent());
        if have > 0 {
            rq = rq.header(RANGE, format!("bytes={have}-"));
        }
        let resp = rq
            .send()
            .await
            .map_err(|e| Attempt::Retry(DownloadError::Network(net(e))))?;
        let status = resp.status();
        if status == StatusCode::RANGE_NOT_SATISFIABLE && have > 0 {
            // The part is complete or garbage. A checksum (verify) or the
            // server's `bytes */<len>` can tell; with neither, start over.
            let total = resp
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes */"))
                .and_then(|n| n.trim().parse::<u64>().ok());
            if req.md5.is_some() || req.sha256.is_some() || total == Some(have) {
                return Ok(Progressed { resumed_from: have });
            }
            tracing::warn!(
                "server refused the resume range and nothing verifies the partial file; restarting"
            );
            let _ = tokio::fs::remove_file(part).await;
            return Err(Attempt::Retry(DownloadError::Http {
                status: status.as_u16(),
            }));
        }
        if !status.is_success() {
            let e = DownloadError::Http {
                status: status.as_u16(),
            };
            let transient = status.is_server_error()
                || status == StatusCode::REQUEST_TIMEOUT
                || status == StatusCode::TOO_MANY_REQUESTS;
            return Err(if transient {
                Attempt::Retry(e)
            } else {
                Attempt::Fatal(e)
            });
        }
        let (start, total) = if status == StatusCode::PARTIAL_CONTENT {
            let cr = resp
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(content_range);
            match cr {
                Some((s, t)) if s == have => (have, t),
                _ => {
                    // Range we did not ask for: start over.
                    let _ = tokio::fs::remove_file(part).await;
                    return Err(Attempt::Retry(DownloadError::Network(
                        "unexpected Content-Range".into(),
                    )));
                }
            }
        } else {
            // 200: the server ignored Range; restart from zero.
            (0, resp.content_length())
        };
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(start > 0)
            .truncate(start == 0)
            .open(part)
            .await
            .map_err(|e| Attempt::Fatal(DownloadError::io(part)(e)))?;
        emit(DownloadEvent::Started {
            resumed_from: start,
            total,
        });
        let mut done = start;
        let mut last_emit = done;
        let mut resp = resp;
        let result = loop {
            match resp.chunk().await {
                Ok(Some(bytes)) => {
                    if let Err(e) = file.write_all(&bytes).await {
                        break Err(Attempt::Fatal(DownloadError::io(part)(e)));
                    }
                    done += bytes.len() as u64;
                    if done - last_emit >= PROGRESS_STEP {
                        last_emit = done;
                        emit(DownloadEvent::Progress { done, total });
                    }
                }
                Ok(None) => break Ok(()),
                Err(e) => break Err(Attempt::Retry(DownloadError::Network(net(e)))),
            }
        };
        file.flush()
            .await
            .map_err(|e| Attempt::Fatal(DownloadError::io(part)(e)))?;
        file.sync_all()
            .await
            .map_err(|e| Attempt::Fatal(DownloadError::io(part)(e)))?;
        drop(file);
        if done != last_emit {
            emit(DownloadEvent::Progress { done, total });
        }
        result?;
        if let Some(t) = total {
            if done != t {
                return Err(Attempt::Retry(DownloadError::SizeMismatch {
                    got: done,
                    expected: t,
                }));
            }
        }
        Ok(Progressed {
            resumed_from: start,
        })
    }

    async fn verify(&self, req: &FetchRequest<'_>, part: &Path) -> Result<Fetched, DownloadError> {
        let p = part.to_path_buf();
        let (sha256, md5, size) = tokio::task::spawn_blocking(move || hash_file(&p))
            .await
            .map_err(|e| DownloadError::Network(format!("hash task failed: {e}")))?
            .map_err(DownloadError::io(part))?;
        let check = |expected: Option<&str>, actual: &str| match expected {
            Some(x) if !x.is_empty() && !x.eq_ignore_ascii_case(actual) => {
                Err(DownloadError::ChecksumMismatch {
                    path: req.dest.to_path_buf(),
                    expected: x.to_ascii_lowercase(),
                    actual: actual.to_owned(),
                })
            }
            _ => Ok(()),
        };
        check(req.md5, &md5)?;
        check(req.sha256, &sha256)?;
        tokio::fs::rename(part, req.dest)
            .await
            .map_err(DownloadError::io(req.dest))?;
        Ok(Fetched {
            path: req.dest.to_path_buf(),
            size,
            sha256,
            md5,
            resumed_from: 0,
        })
    }
}

struct Progressed {
    resumed_from: u64,
}

enum Attempt {
    Retry(DownloadError),
    Fatal(DownloadError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_uri() {
        assert_eq!(
            file_name_from_uri("https://cdn/x/SkyUI%205.2-12-5-2.7z?md5=a&expires=1").as_deref(),
            Some("SkyUI 5.2-12-5-2.7z")
        );
        assert_eq!(file_name_from_uri("https://cdn/x/1234567"), None);
        assert_eq!(
            file_name_from_uri("https://cdn/x/..%2F..%2Fetc.passwd").as_deref(),
            Some("_.._etc.passwd")
        );
        assert_eq!(file_name_from_uri("https://cdn/x/100%"), None);
    }

    #[test]
    fn content_disposition_names() {
        assert_eq!(
            disposition_name("attachment; filename=\"A B.zip\"").as_deref(),
            Some("A B.zip")
        );
        assert_eq!(
            disposition_name("attachment; filename=../x.7z; size=1").as_deref(),
            Some("_x.7z")
        );
        assert_eq!(disposition_name("inline"), None);
    }

    #[test]
    fn parses_content_range() {
        assert_eq!(content_range("bytes 10-19/20"), Some((10, Some(20))));
        assert_eq!(content_range("bytes 10-19/*"), Some((10, None)));
        assert_eq!(content_range("items 1-2/3"), None);
    }
}
