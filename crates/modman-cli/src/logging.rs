// SPDX-License-Identifier: GPL-3.0-or-later
//! Tracing to `$XDG_STATE_HOME/modman/modman.log`; `--debug` also prints
//! debug lines on stderr (legacy `debug`, L67).

use std::path::Path;

use tracing::Level;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::{filter_fn, LevelFilter, Targets};
use tracing_subscriber::prelude::*;

/// HTTP/TLS crates that may log request URLs (nxm `key`/`expires`, signed
/// CDN links): capped at WARN whatever `--debug` says.
const NOISY_TARGETS: [&str; 5] = ["reqwest", "hyper", "hyper_util", "h2", "rustls"];

/// Global target filter applied to every layer.
pub fn target_cap() -> Targets {
    NOISY_TARGETS
        .iter()
        .fold(Targets::new().with_default(LevelFilter::TRACE), |t, n| {
            t.with_target(*n, LevelFilter::WARN)
        })
}

/// Installs the global subscriber. Logging failures never stop a command.
pub fn init(log_path: &Path, debug: bool, color: bool) {
    let file_layer = log_path
        .parent()
        .zip(log_path.file_name())
        .and_then(|(dir, name)| {
            std::fs::create_dir_all(dir).ok()?;
            RollingFileAppender::builder()
                .rotation(Rotation::NEVER)
                .filename_prefix(name.to_string_lossy())
                .build(dir)
                .ok()
        })
        .map(|appender| {
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_target(false)
                .with_writer(appender)
                .with_filter(if debug {
                    LevelFilter::DEBUG
                } else {
                    LevelFilter::INFO
                })
        });
    let stderr_layer = debug.then(|| {
        tracing_subscriber::fmt::layer()
            .with_ansi(color)
            .with_writer(std::io::stderr)
            .with_filter(filter_fn(|m| *m.level() == Level::DEBUG))
    });
    let _ = tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .with(target_cap())
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_and_tls_targets_are_capped_at_warn() {
        let t = target_cap();
        for target in [
            "reqwest",
            "reqwest::connect",
            "hyper::proto",
            "hyper_util::client",
            "rustls::conn",
            "h2",
        ] {
            assert!(!t.would_enable(target, &Level::INFO), "{target}");
            assert!(!t.would_enable(target, &Level::DEBUG), "{target}");
            assert!(t.would_enable(target, &Level::WARN), "{target}");
        }
        assert!(t.would_enable("modman", &Level::DEBUG));
        assert!(t.would_enable("modman_core::nexus", &Level::TRACE));
    }
}
