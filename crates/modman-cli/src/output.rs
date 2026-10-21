// SPDX-License-Identifier: GPL-3.0-or-later
//! Terminal rendering. Mirrors legacy prefixes (`[INFO]`, `[ OK ]`, `[WARN]`,
//! `[ERR ]`); every message is also written to the log.

use std::io::IsTerminal;

const CYAN: &str = "\x1b[0;36m";
const GREEN: &str = "\x1b[0;32m";
const YELLOW: &str = "\x1b[1;33m";
const RED: &str = "\x1b[0;31m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Colour decision (legacy L45 plus `CLICOLOR_FORCE` for non-tty output):
/// off with `--no-color`, a non-empty `NO_COLOR`, or `MODMAN_NO_COLOR=1`.
pub fn use_color(no_color_flag: bool) -> bool {
    let env_set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    if no_color_flag
        || env_set("NO_COLOR")
        || std::env::var("MODMAN_NO_COLOR").is_ok_and(|v| v == "1")
    {
        return false;
    }
    std::io::stdout().is_terminal()
        || std::env::var("CLICOLOR_FORCE").is_ok_and(|v| !v.is_empty() && v != "0")
}

#[derive(Debug, Clone, Copy)]
pub struct Printer {
    pub color: bool,
}

impl Printer {
    fn paint(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("{code}{s}{RESET}")
        } else {
            s.to_string()
        }
    }

    pub fn bold(&self, s: &str) -> String {
        self.paint(BOLD, s)
    }

    pub fn dim(&self, s: &str) -> String {
        self.paint(DIM, s)
    }

    pub fn green(&self, s: &str) -> String {
        self.paint(GREEN, s)
    }

    pub fn yellow(&self, s: &str) -> String {
        self.paint(YELLOW, s)
    }

    pub fn info(&self, msg: &str) {
        println!("{}  {msg}", self.paint(CYAN, "[INFO]"));
        tracing::info!("{msg}");
    }

    pub fn success(&self, msg: &str) {
        println!("{}  {msg}", self.paint(GREEN, "[ OK ]"));
        tracing::info!("OK {msg}");
    }

    pub fn warn(&self, msg: &str) {
        eprintln!("{}  {msg}", self.paint(YELLOW, "[WARN]"));
        tracing::warn!("{msg}");
    }

    pub fn error(&self, msg: &str) {
        eprintln!("{}  {msg}", self.paint(RED, "[ERR ]"));
        tracing::error!("{msg}");
    }

    /// Section heading (legacy `banner`).
    pub fn heading(&self, title: &str) {
        println!("\n{}", self.bold(&format!("── {title} ")));
    }
}
