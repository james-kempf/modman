// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared app state and the two ways commands run core code: quick queries
//! (non-interactive, no job events) and jobs (job events, prompts). Each
//! call opens its own SQLite connection (WAL), so a long job never blocks
//! other commands.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;

use vessel_core::app::nxm::{self as appnxm, Accepted};
use vessel_core::app::{chain, AppError, Cx, R};
use vessel_core::autoupdate::StartupCheck;
use vessel_core::paths::Paths;
use vessel_core::store::Store;

use crate::events::{self, JobState, Sink};
use crate::gui_ui::GuiUi;
use crate::prompt::Prompts;

/// Error returned to the webview: a message only (never a key, never a
/// query string; core errors redact both).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CmdError {
    pub message: String,
}

impl CmdError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<AppError> for CmdError {
    fn from(e: AppError) -> Self {
        Self::new(chain(&e))
    }
}

pub type CmdResult<T> = Result<T, CmdError>;

/// nxm links wait in the `downloads` table until the webview is ready to
/// answer prompts; then one worker processes them in order.
#[derive(Default)]
struct NxmQueue {
    worker: Option<Sender<Accepted>>,
}

pub struct Inner {
    pub paths: Paths,
    pub prompts: Prompts,
    next_job: AtomicU64,
    /// The one auto-update check at start.
    pub startup: OnceLock<Option<StartupCheck>>,
    nxm: Mutex<NxmQueue>,
}

/// Managed Tauri state.
#[derive(Clone)]
pub struct AppState(pub Arc<Inner>);

impl AppState {
    pub fn new(paths: Paths) -> Self {
        Self(Arc::new(Inner {
            paths,
            prompts: Prompts::default(),
            next_job: AtomicU64::new(0),
            startup: OnceLock::new(),
            nxm: Mutex::new(NxmQueue::default()),
        }))
    }
}

impl Inner {
    pub fn open_store(&self) -> R<Store> {
        Ok(Store::open(&self.paths.db_path())?)
    }

    fn run<T>(
        &self,
        ui: &GuiUi<'_>,
        game: Option<&str>,
        f: impl FnOnce(&mut Cx<'_>) -> R<T>,
    ) -> R<T> {
        let mut store = self.open_store()?;
        let mut cx = Cx::new(&self.paths, &mut store, ui);
        cx.game = game;
        f(&mut cx)
    }

    /// Quick work: questions take their defaults, messages still go to the
    /// activity log.
    pub fn query<T>(
        &self,
        sink: Arc<dyn Sink>,
        game: Option<&str>,
        f: impl FnOnce(&mut Cx<'_>) -> R<T>,
    ) -> CmdResult<T> {
        let ui = GuiUi::new(sink, &self.prompts, None, false);
        Ok(self.run(&ui, game, f)?)
    }

    /// A job: announced with job events; questions become prompts when
    /// `interactive`.
    pub fn job<T>(
        &self,
        sink: Arc<dyn Sink>,
        kind: &str,
        game: Option<&str>,
        interactive: bool,
        f: impl FnOnce(&mut Cx<'_>) -> R<T>,
    ) -> CmdResult<T> {
        let id = self.next_job.fetch_add(1, Ordering::SeqCst) + 1;
        fn job<'a>(
            id: u64,
            kind: &'a str,
            state: JobState,
            error: Option<&'a str>,
        ) -> events::Job<'a> {
            events::Job {
                id,
                kind,
                state,
                error,
            }
        }
        events::emit(&*sink, events::JOB, &job(id, kind, JobState::Started, None));
        let ui = GuiUi::new(sink.clone(), &self.prompts, Some(id), interactive);
        let r = self.run(&ui, game, f).map_err(CmdError::from);
        match &r {
            Ok(_) => events::emit(
                &*sink,
                events::JOB,
                &job(id, kind, JobState::Finished, None),
            ),
            Err(e) => {
                tracing::warn!("{kind} failed: {}", e.message);
                events::emit(
                    &*sink,
                    events::JOB,
                    &job(id, kind, JobState::Failed, Some(&e.message)),
                );
            }
        }
        r
    }

    /// Hands an accepted link to the worker once the webview is ready;
    /// before that it stays queued and [`Self::mark_ready`] picks it up.
    pub fn dispatch(&self, a: Accepted) {
        let q = self.nxm.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(tx) = &q.worker {
            let _ = tx.send(a);
        }
    }

    /// The webview listens: starts the nxm worker and hands it every queued
    /// link. Returns how many were queued. Idempotent.
    pub fn mark_ready(self: &Arc<Self>, sink: Arc<dyn Sink>) -> CmdResult<usize> {
        let mut q = self.nxm.lock().unwrap_or_else(|p| p.into_inner());
        if q.worker.is_some() {
            return Ok(0);
        }
        let (tx, rx) = channel::<Accepted>();
        let me = self.clone();
        std::thread::Builder::new()
            .name("nxm-worker".into())
            .spawn(move || {
                for a in rx {
                    let still_queued = me.open_store().is_ok_and(|s| appnxm::is_queued(&s, a.row));
                    if still_queued {
                        let _ = me.job(sink.clone(), "nxm", None, true, |cx| {
                            appnxm::process(cx, &a)
                        });
                    }
                }
            })
            .map_err(|e| CmdError::new(format!("cannot start the nxm worker: {e}")))?;
        let pending = appnxm::pending(&self.open_store()?)?;
        for a in &pending {
            let _ = tx.send(a.clone());
        }
        q.worker = Some(tx);
        Ok(pending.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::testing::Recorder;

    #[test]
    fn jobs_announce_start_and_failure_with_the_message() {
        let home = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::for_home(home.path()));
        let sink = Arc::new(Recorder::default());
        let r: CmdResult<()> = st.0.job(sink.clone(), "deploy", None, true, |cx| {
            cx.need_game().map(|_| ())
        });
        let e = r.unwrap_err();
        assert!(e.message.contains("No game selected"), "{}", e.message);
        let jobs = sink.named(events::JOB);
        assert_eq!(jobs[0]["state"], "started");
        assert_eq!(jobs[1]["state"], "failed");
        assert_eq!(jobs[1]["kind"], "deploy");
        assert_eq!(jobs[0]["id"], jobs[1]["id"]);
    }
}
