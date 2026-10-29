// SPDX-License-Identifier: GPL-3.0-or-later
//! Events sent to the webview (names and payloads mirrored in
//! `src/ipc/events.ts`). Emission goes through [`Sink`] so the glue is
//! testable without a window.

use serde::Serialize;

use vessel_core::app::Msg;
use vessel_core::download::DownloadEvent;

/// A core message shown in the activity log.
pub const ACTIVITY: &str = "vessel:activity";
/// A job started, finished or failed.
pub const JOB: &str = "vessel:job";
/// A question for the user; answer with the `prompt_answer` command.
pub const PROMPT: &str = "vessel:prompt";
/// A pending prompt was withdrawn (job ended, window closing).
pub const PROMPT_CLOSED: &str = "vessel:prompt-closed";
/// Download progress.
pub const DOWNLOAD: &str = "vessel:download";
/// Deploy/purge progress (files done of total).
pub const DEPLOY_PROGRESS: &str = "vessel:deploy-progress";
/// An nxm:// link from another instance or the paste box was accepted or
/// rejected.
pub const NXM: &str = "vessel:nxm";
/// Result of the one-time auto-update check at start.
pub const STARTUP_CHECK: &str = "vessel:startup-check";

/// Where events go: the app handle, or a recorder in tests.
pub trait Sink: Send + Sync {
    fn emit_value(&self, event: &str, payload: serde_json::Value);
}

/// Serializes `payload` and emits it; serialization never fails for the
/// payload types here, a failure is logged.
pub fn emit<T: Serialize>(sink: &dyn Sink, event: &str, payload: &T) {
    match serde_json::to_value(payload) {
        Ok(v) => sink.emit_value(event, v),
        Err(e) => tracing::warn!("cannot serialize {event}: {e}"),
    }
}

impl<R: tauri::Runtime> Sink for tauri::AppHandle<R> {
    fn emit_value(&self, event: &str, payload: serde_json::Value) {
        use tauri::Emitter;
        if let Err(e) = self.emit(event, payload) {
            tracing::warn!("cannot emit {event}: {e}");
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Activity<'a> {
    pub job: Option<u64>,
    pub msg: &'a Msg,
    /// The message without styles.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Started,
    Finished,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Job<'a> {
    pub id: u64,
    pub kind: &'a str,
    pub state: JobState,
    pub error: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Download<'a> {
    pub job: Option<u64>,
    pub name: &'a str,
    pub event: &'a DownloadEvent,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct DeployProgress {
    pub job: Option<u64>,
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NxmStatus {
    Accepted,
    Rejected,
}

/// Never carries the link's query (`key`, `expires`).
#[derive(Debug, Clone, Serialize)]
pub struct Nxm {
    pub status: NxmStatus,
    /// Redacted link (`nxm://game/mods/1/files/2`).
    pub link: Option<String>,
    /// `downloads` row.
    pub row: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PromptClosed {
    pub id: u64,
}

#[cfg(test)]
pub mod testing {
    use std::sync::Mutex;

    use super::Sink;

    /// Records emitted events.
    #[derive(Default)]
    pub struct Recorder {
        pub events: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl Recorder {
        pub fn named(&self, name: &str) -> Vec<serde_json::Value> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .collect()
        }
    }

    impl Sink for Recorder {
        fn emit_value(&self, event: &str, payload: serde_json::Value) {
            self.events
                .lock()
                .unwrap()
                .push((event.to_string(), payload));
        }
    }
}
