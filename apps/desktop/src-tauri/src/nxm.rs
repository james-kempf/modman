// SPDX-License-Identifier: GPL-3.0-or-later
//! nxm:// intake in the GUI (spikes.md §5). `modman nxm <url>` forwards
//! `["modman-desktop", "nxm", <url>]` over the single-instance D-Bus
//! endpoint; the paste box sends the URL directly. argv is untrusted: every
//! URL goes through the core parser ([`modman_core::app::nxm::accept`],
//! i.e. `NxmLink::parse`) before a `downloads` row is written, and nothing
//! else in argv is acted on.

use std::sync::Arc;

use serde::Serialize;

use modman_core::app::nxm::{self as appnxm, Accepted};
use modman_core::download::NxmLink;
use modman_core::ipc;

use crate::events::{self, NxmStatus, Sink};
use crate::state::{CmdError, Inner};

/// The URL in a forwarded argv: exactly `[argv0, "nxm", url]` (what the CLI
/// sends) or `[argv0, url]`. Anything else carries no link.
pub fn url_in_argv(argv: &[String]) -> Option<&str> {
    match argv {
        [_, cmd, url] if cmd == "nxm" => Some(url),
        [_, url]
            if url
                .get(..6)
                .is_some_and(|p| p.eq_ignore_ascii_case("nxm://")) =>
        {
            Some(url)
        }
        _ => None,
    }
}

/// What the webview learns about an accepted link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Received {
    pub row: i64,
    /// Redacted link (no key/expires).
    pub link: String,
    /// `mod` or `collection`.
    pub kind: String,
}

/// Parses and queues `url`, reports it to the webview and hands it to the
/// worker. A malformed or foreign URL is rejected with no row written.
pub fn receive(inner: &Inner, sink: &dyn Sink, url: &str) -> Result<Received, CmdError> {
    let accepted = inner
        .open_store()
        .map_err(CmdError::from)
        .and_then(|s| appnxm::accept(&s, url).map_err(CmdError::from));
    match accepted {
        Ok(a) => {
            let r = Received {
                row: a.row,
                link: a.link.redacted(),
                kind: match a.link {
                    NxmLink::Mod(_) => "mod",
                    NxmLink::Collection { .. } => "collection",
                }
                .into(),
            };
            events::emit(
                sink,
                events::NXM,
                &events::Nxm {
                    status: NxmStatus::Accepted,
                    link: Some(r.link.clone()),
                    row: Some(r.row),
                    error: None,
                },
            );
            inner.dispatch(Accepted {
                link: a.link,
                row: a.row,
            });
            Ok(r)
        }
        Err(e) => {
            tracing::warn!("nxm link rejected: {}", e.message);
            events::emit(
                sink,
                events::NXM,
                &events::Nxm {
                    status: NxmStatus::Rejected,
                    link: None,
                    row: None,
                    error: Some(e.message.clone()),
                },
            );
            Err(e)
        }
    }
}

/// The single-instance callback body (also used for the first instance's
/// own argv). `None` when argv names no link.
pub fn on_second_instance(
    inner: &Arc<Inner>,
    sink: &dyn Sink,
    argv: &[String],
) -> Option<Result<Received, CmdError>> {
    if argv.first().is_some_and(|a| a != ipc::GUI_ARGV0) {
        tracing::debug!("second instance argv[0] is not {}", ipc::GUI_ARGV0);
    }
    url_in_argv(argv).map(|url| receive(inner, sink, url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::testing::Recorder;
    use crate::state::AppState;
    use modman_core::download;
    use modman_core::paths::Paths;

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn rows(inner: &Inner) -> usize {
        download::list(&inner.open_store().unwrap(), None)
            .unwrap()
            .len()
    }

    #[test]
    fn argv_must_be_the_forwarded_shape() {
        let u = "nxm://skyrimspecialedition/mods/1/files/2";
        assert_eq!(url_in_argv(&argv(&["modman-desktop", "nxm", u])), Some(u));
        assert_eq!(url_in_argv(&argv(&["modman-desktop", u])), Some(u));
        assert_eq!(url_in_argv(&argv(&["modman-desktop"])), None);
        assert_eq!(url_in_argv(&argv(&["modman-desktop", "nxm", u, "x"])), None);
        assert_eq!(url_in_argv(&argv(&["modman-desktop", "deploy"])), None);
        assert_eq!(url_in_argv(&argv(&["x", "--flag", u])), None);
    }

    #[test]
    fn forwarded_malformed_or_foreign_urls_are_rejected_without_enqueue() {
        let home = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::for_home(home.path()));
        let sink = Recorder::default();
        for bad in [
            "nxm://evil.example/mods/1/files/2?key=SECRET",
            "https://www.nexusmods.com/skyrimspecialedition/mods/266",
            "nxm://skyrimspecialedition/mods/1/files/2/../../x",
            "nxm://skyrimspecialedition/mods/-1/files/2",
            "file:///etc/passwd",
        ] {
            let r = on_second_instance(&st.0, &sink, &argv(&["modman-desktop", "nxm", bad]));
            assert!(r.unwrap().is_err(), "{bad}");
        }
        assert_eq!(rows(&st.0), 0);
        let ev = sink.named(events::NXM);
        assert_eq!(ev.len(), 5);
        assert!(ev.iter().all(|e| e["status"] == "rejected"));
        assert!(!serde_json::to_string(&ev).unwrap().contains("SECRET"));
    }

    #[test]
    fn a_forwarded_valid_url_is_parsed_queued_and_redacted() {
        let home = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::for_home(home.path()));
        let sink = Recorder::default();
        let url = "nxm://skyrimspecialedition/mods/266/files/1000?key=SECRET&expires=9999999999&user_id=2";
        let r = on_second_instance(&st.0, &sink, &argv(&["modman-desktop", "nxm", url]))
            .unwrap()
            .unwrap();
        assert_eq!(r.kind, "mod");
        assert_eq!(r.link, "nxm://skyrimspecialedition/mods/266/files/1000");
        assert_eq!(rows(&st.0), 1);
        let ev = sink.named(events::NXM);
        assert_eq!(ev[0]["status"], "accepted");
        assert!(!serde_json::to_string(&sink.events.lock().unwrap().clone())
            .unwrap()
            .contains("SECRET"));
        // Not ready yet: nothing is processed, the row waits in the queue.
        assert_eq!(
            appnxm::pending(&st.0.open_store().unwrap()).unwrap().len(),
            1
        );
    }
}
