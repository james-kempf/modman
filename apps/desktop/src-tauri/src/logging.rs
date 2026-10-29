// SPDX-License-Identifier: GPL-3.0-or-later
//! Tracing to the shared log file (`$XDG_STATE_HOME/vessel/vessel.log`), the
//! same file the CLI writes and the `log_tail` command reads.

use std::path::Path;

use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::prelude::*;

/// HTTP/TLS crates that may log request URLs (nxm `key`/`expires`, signed
/// CDN links) and webview internals: capped at WARN.
const NOISY_TARGETS: [&str; 7] = [
    "reqwest",
    "hyper",
    "hyper_util",
    "h2",
    "rustls",
    "tauri",
    "wry",
];

pub fn target_cap() -> Targets {
    NOISY_TARGETS
        .iter()
        .fold(Targets::new().with_default(LevelFilter::INFO), |t, n| {
            t.with_target(*n, LevelFilter::WARN)
        })
}

/// Installs the global subscriber; failures never stop the app.
pub fn init(log_path: &Path) {
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
        });
    let _ = tracing_subscriber::registry()
        .with(file_layer)
        .with(target_cap())
        .try_init();
}

#[cfg(test)]
mod tests {
    use tracing::Level;

    #[test]
    fn http_targets_are_capped_at_warn() {
        let t = super::target_cap();
        assert!(!t.would_enable("reqwest::connect", &Level::INFO));
        assert!(!t.would_enable("tauri::ipc", &Level::INFO));
        assert!(t.would_enable("vessel_core::app", &Level::INFO));
    }
}
