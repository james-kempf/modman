// SPDX-License-Identifier: GPL-3.0-or-later
//! A visible error for a start-up abort (e.g. the migration refusing while
//! the legacy app runs, R6), shown before any window exists: a GTK message
//! dialog, else zenity, kdialog or notify-send. Best effort; the caller
//! also prints to stderr.

use std::process::{Command, Stdio};

const TITLE: &str = "Vessel";

/// Shows `msg` and returns once the user closed it (or nothing could show it).
pub fn show(msg: &str) {
    if gtk_dialog(msg) {
        return;
    }
    for (bin, args) in fallbacks(msg) {
        let Some(path) = vessel_core::app::which(bin) else {
            continue;
        };
        let shown = Command::new(path)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if shown {
            return;
        }
    }
}

fn gtk_dialog(msg: &str) -> bool {
    use gtk::prelude::*;
    if gtk::init().is_err() {
        return false; // no display
    }
    let d = gtk::MessageDialog::new(
        None::<&gtk::Window>,
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Error,
        gtk::ButtonsType::Close,
        msg,
    );
    d.set_title(TITLE);
    d.run();
    d.close();
    true
}

/// External dialog programs, in order of preference, with their arguments.
fn fallbacks(msg: &str) -> [(&'static str, Vec<String>); 3] {
    let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    [
        (
            "zenity",
            s(&["--error", "--no-markup", "--title", TITLE, "--text", msg]),
        ),
        ("kdialog", s(&["--title", TITLE, "--error", msg])),
        (
            "notify-send",
            s(&[
                "-a",
                TITLE,
                "-u",
                "critical",
                "-i",
                "dialog-error",
                TITLE,
                msg,
            ]),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fails when the fallback order changes or markup in the message
    // (paths with `&`/`<`) could be interpreted by zenity.
    #[test]
    fn fallbacks_prefer_dialogs_and_pass_the_message_verbatim() {
        let msg = "Close x before starting Vessel (<a&b>)";
        let f = fallbacks(msg);
        let names: Vec<_> = f.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["zenity", "kdialog", "notify-send"]);
        assert!(f[0].1.contains(&"--no-markup".to_string()));
        assert!(f
            .iter()
            .all(|(_, a)| a.last().map(String::as_str) == Some(msg)));
    }
}
