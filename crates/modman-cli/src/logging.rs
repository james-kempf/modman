// SPDX-License-Identifier: GPL-3.0-or-later
//! Tracing to `$XDG_STATE_HOME/modman/modman.log`; `--debug` also prints
//! debug lines on stderr (legacy `debug`, L67).

use std::path::Path;

use tracing::Level;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::{filter_fn, LevelFilter};
use tracing_subscriber::prelude::*;

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
        .try_init();
}
