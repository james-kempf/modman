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
/// off with `--no-color`, a non-empty `NO_COLOR`, or `VESSEL_NO_COLOR=1`.
pub fn use_color(no_color_flag: bool) -> bool {
    let env_set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    if no_color_flag
        || env_set("NO_COLOR")
        || std::env::var("VESSEL_NO_COLOR").is_ok_and(|v| v == "1")
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

/// JSON string literal (the CLI has no JSON library; output is small).
pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// `1.5 MB`-style size.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Cuts `s` to `n` characters with an ellipsis.
pub fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(json_str("a\"b\\c\n\u{1}"), r#""a\"b\\c\n\u0001""#);
    }

    #[test]
    fn sizes_and_truncation() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(trunc("abcdef", 4), "abc…");
        assert_eq!(trunc("abc", 4), "abc");
    }
}
