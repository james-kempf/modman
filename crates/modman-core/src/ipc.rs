// SPDX-License-Identifier: GPL-3.0-or-later
//! nxm:// routing constants shared by the CLI handler and the GUI
//! (spikes.md §5). The CLI forwards links to a running GUI through the
//! D-Bus endpoint served by `tauri-plugin-single-instance`; renaming the
//! Tauri `identifier` (`org.modman.app`) breaks forwarding.

use std::time::Duration;

/// Bus name: tauri.conf.json `identifier` + `.SingleInstance`.
pub const SINGLE_INSTANCE_BUS: &str = "org.modman.app.SingleInstance";
pub const SINGLE_INSTANCE_PATH: &str = "/org/modman/app/SingleInstance";
pub const SINGLE_INSTANCE_INTERFACE: &str = "org.SingleInstance.DBus";
/// `ExecuteCallback(argv: as, cwd: s)`.
pub const SINGLE_INSTANCE_METHOD: &str = "ExecuteCallback";
/// argv[0] sent to the GUI; argv is `[GUI_ARGV0, "nxm", <url>]`.
pub const GUI_ARGV0: &str = "modman-desktop";
/// How long the CLI waits for the GUI to accept a link.
pub const FORWARD_TIMEOUT: Duration = Duration::from_secs(5);

/// Desktop entry written by `modman nxm register` (legacy name, L1533).
pub const NXM_DESKTOP_FILE: &str = "modman-nxm.desktop";
/// URL schemes the desktop entry handles.
pub const NXM_SCHEMES: [&str; 2] = ["x-scheme-handler/nxm", "x-scheme-handler/nxm-protocol"];

/// Quotes a program path for a desktop entry `Exec` key: double quotes with
/// `"`, `` ` ``, `$` and `\` backslash-escaped (quoting rule), then every
/// backslash doubled (string escape rule), and `%` doubled (field codes).
/// `None` for paths that cannot be represented (control characters).
pub fn desktop_exec_quote(path: &str) -> Option<String> {
    if path.chars().any(char::is_control) {
        return None;
    }
    let mut quoted = String::with_capacity(path.len() + 2);
    for c in path.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    let escaped = quoted.replace('\\', "\\\\").replace('%', "%%");
    Some(format!("\"{escaped}\""))
}

/// The `Exec` line: `Exec="<path>" nxm %u` (`%u` last and unquoted).
pub fn nxm_exec_line(program: &str) -> Option<String> {
    Some(format!("Exec={} nxm %u", desktop_exec_quote(program)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_path_is_quoted_and_u_is_last() {
        assert_eq!(
            nxm_exec_line("/usr/bin/modman").unwrap(),
            "Exec=\"/usr/bin/modman\" nxm %u"
        );
    }

    #[test]
    fn special_characters_cannot_break_out_of_the_quotes() {
        assert_eq!(
            desktop_exec_quote("/a b/$x`y\"z\\w%u").unwrap(),
            r#""/a b/\\$x\\`y\\"z\\\\w%%u""#
        );
        assert_eq!(desktop_exec_quote("/a\nb"), None);
    }
}
