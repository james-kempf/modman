// SPDX-License-Identifier: GPL-3.0-or-later
//! Blocking question/answer bridge between a job thread and the webview.
//! The job emits a [`events::PROMPT`] event and waits; the webview replies
//! with the `prompt_answer` command. Closing the window cancels every
//! pending prompt so no job thread waits forever. A reloaded webview gets
//! the open questions again from [`Prompts::reemit`] (on `app_ready`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use modman_core::fomod::{GroupType, PluginType};
use modman_core::nexus::FileInfo;

use crate::events::{self, Sink};

/// A question, by kind.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptKind {
    /// Answer: `confirm`.
    Confirm { question: String, default_yes: bool },
    /// Answer: `text` (empty = default).
    Text { question: String, default: String },
    /// Answer: `choice` (index into `files`).
    ChooseFile {
        files: Vec<FileInfo>,
        default: usize,
    },
    /// Answer: `choices` (indexes into `mods`).
    ChooseOptional {
        collection: String,
        mods: Vec<OptionalMod>,
    },
    /// Answer: `path` of the file the user downloaded.
    ManualDownload {
        name: String,
        instructions: Option<String>,
        url: Option<String>,
    },
    /// Answer: `choice` (index into `choices`).
    ChooseVariant { choices: Vec<String> },
    /// Answer: `choices` (indexes into `options`); asked again with
    /// `error` set when the selection breaks the group rule.
    FomodGroup {
        module: String,
        step: String,
        group: String,
        kind: GroupType,
        /// e.g. "pick one".
        kind_label: String,
        options: Vec<FomodOption>,
        defaults: Vec<usize>,
        error: Option<String>,
    },
}

/// One option of a FOMOD group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FomodOption {
    pub name: String,
    pub description: String,
    /// As evaluated now (flags set by earlier groups apply).
    pub kind: PluginType,
    /// e.g. "required"; `None` for plain optional.
    pub kind_label: Option<String>,
}

/// An optional collection mod offered for selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OptionalMod {
    pub name: String,
    pub version: Option<String>,
    pub size: u64,
}

/// What the webview sends back.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    Confirm { yes: bool },
    Text { value: String },
    Choice { index: usize },
    Choices { indices: Vec<usize> },
    Path { path: String },
    Cancel,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptEvent {
    pub id: u64,
    pub job: Option<u64>,
    pub prompt: PromptKind,
}

struct Pending {
    tx: Sender<Answer>,
    event: PromptEvent,
}

/// Pending prompts by id.
#[derive(Default)]
pub struct Prompts {
    next: AtomicU64,
    pending: Mutex<HashMap<u64, Pending>>,
    closed: AtomicBool,
}

impl Prompts {
    /// Emits the question and blocks until it is answered. `None` when it
    /// was cancelled (answer `cancel`, window closing).
    pub fn ask(&self, sink: &dyn Sink, job: Option<u64>, prompt: PromptKind) -> Option<Answer> {
        if self.closed.load(Ordering::SeqCst) {
            return None;
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = channel();
        let event = PromptEvent { id, job, prompt };
        self.lock().insert(
            id,
            Pending {
                tx,
                event: event.clone(),
            },
        );
        events::emit(sink, events::PROMPT, &event);
        let answer = rx.recv().ok();
        if self.lock().remove(&id).is_some() || answer.is_none() {
            events::emit(sink, events::PROMPT_CLOSED, &events::PromptClosed { id });
        }
        answer.filter(|a| *a != Answer::Cancel)
    }

    /// Delivers an answer; `false` when no prompt `id` is waiting.
    pub fn answer(&self, id: u64, answer: Answer) -> bool {
        match self.lock().remove(&id) {
            Some(p) => p.tx.send(answer).is_ok(),
            None => false,
        }
    }

    /// Cancels every pending prompt and refuses new ones (window closing).
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.lock().clear();
    }

    pub fn pending(&self) -> usize {
        self.lock().len()
    }

    /// Emits every still-open prompt again, oldest first (the webview was
    /// reloaded and lost them). Returns how many.
    pub fn reemit(&self, sink: &dyn Sink) -> usize {
        let mut open: Vec<PromptEvent> = self.lock().values().map(|p| p.event.clone()).collect();
        open.sort_by_key(|e| e.id);
        for e in &open {
            events::emit(sink, events::PROMPT, e);
        }
        open.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Pending>> {
        self.pending.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::events::testing::Recorder;

    fn wait_for_prompt(p: &Prompts) {
        for _ in 0..200 {
            if p.pending() > 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no prompt");
    }

    #[test]
    fn an_answer_unblocks_the_asking_thread() {
        let p = Arc::new(Prompts::default());
        let sink = Arc::new(Recorder::default());
        let (p2, s2) = (p.clone(), sink.clone());
        let t = std::thread::spawn(move || {
            p2.ask(
                &*s2,
                Some(7),
                PromptKind::Confirm {
                    question: "Proceed?".into(),
                    default_yes: true,
                },
            )
        });
        wait_for_prompt(&p);
        let ev = &sink.named(events::PROMPT)[0];
        assert_eq!(ev["job"], 7);
        assert_eq!(ev["prompt"]["type"], "confirm");
        let id = ev["id"].as_u64().unwrap();
        assert!(!p.answer(id + 1, Answer::Cancel));
        assert!(p.answer(id, Answer::Confirm { yes: false }));
        assert_eq!(t.join().unwrap(), Some(Answer::Confirm { yes: false }));
        assert_eq!(p.pending(), 0);
    }

    #[test]
    fn closing_cancels_waiting_and_future_prompts() {
        let p = Arc::new(Prompts::default());
        let sink = Arc::new(Recorder::default());
        let (p2, s2) = (p.clone(), sink.clone());
        let t = std::thread::spawn(move || {
            p2.ask(
                &*s2,
                None,
                PromptKind::ChooseVariant {
                    choices: vec!["a".into()],
                },
            )
        });
        wait_for_prompt(&p);
        p.close();
        assert_eq!(t.join().unwrap(), None);
        assert_eq!(sink.named(events::PROMPT_CLOSED).len(), 1);
        let r = p.ask(
            &*sink,
            None,
            PromptKind::Text {
                question: "q".into(),
                default: String::new(),
            },
        );
        assert_eq!(r, None);
    }

    #[test]
    fn open_prompts_are_emitted_again_after_a_reload() {
        let p = Arc::new(Prompts::default());
        let sink = Arc::new(Recorder::default());
        let (p2, s2) = (p.clone(), sink.clone());
        let t = std::thread::spawn(move || {
            p2.ask(
                &*s2,
                Some(3),
                PromptKind::Text {
                    question: "Folder?".into(),
                    default: ".".into(),
                },
            )
        });
        wait_for_prompt(&p);
        let reloaded = Recorder::default();
        assert_eq!(p.reemit(&reloaded), 1);
        let ev = &reloaded.named(events::PROMPT)[0];
        assert_eq!(ev, &sink.named(events::PROMPT)[0]);
        let id = ev["id"].as_u64().unwrap();
        assert!(p.answer(id, Answer::Text { value: "x".into() }));
        assert_eq!(t.join().unwrap(), Some(Answer::Text { value: "x".into() }));
        assert_eq!(p.reemit(&reloaded), 0);
    }

    #[test]
    fn answers_parse_from_tagged_json() {
        let a: Answer = serde_json::from_str(r#"{"type":"choices","indices":[0,2]}"#).unwrap();
        assert_eq!(
            a,
            Answer::Choices {
                indices: vec![0, 2]
            }
        );
        assert!(serde_json::from_str::<Answer>(r#"{"type":"eval","code":"x"}"#).is_err());
    }
}
