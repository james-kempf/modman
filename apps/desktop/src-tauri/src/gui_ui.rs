// SPDX-License-Identifier: GPL-3.0-or-later
//! The webview front end of `vessel_core::app`: messages become activity
//! events, questions become prompts (see [`crate::prompt`]), including the
//! FOMOD resolver bridge (`FomodResolver::choose` is synchronous; the job
//! thread blocks until the webview answers).

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vessel_core::app::{InstallPrompts, Msg, Ui};
use vessel_core::collections::{Manifest, ManifestMod};
use vessel_core::deploy::Progress;
use vessel_core::download::DownloadEvent;
use vessel_core::fomod::{FomodError, FomodResolver, GroupPrompt};
use vessel_core::nexus::FileInfo;

use crate::events::{self, Sink};
use crate::prompt::{Answer, FomodOption, OptionalMod, PromptKind, Prompts};

/// Progress events are sent at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(150);

pub struct GuiUi<'a> {
    pub sink: Arc<dyn Sink>,
    pub prompts: &'a Prompts,
    pub job: Option<u64>,
    /// `false` = every question takes its default (auto-update baseline,
    /// read-only queries).
    pub interactive: bool,
    last_download: Mutex<Option<Instant>>,
}

impl<'a> GuiUi<'a> {
    pub fn new(
        sink: Arc<dyn Sink>,
        prompts: &'a Prompts,
        job: Option<u64>,
        interactive: bool,
    ) -> Self {
        Self {
            sink,
            prompts,
            job,
            interactive,
            last_download: Mutex::new(None),
        }
    }

    fn ask(&self, prompt: PromptKind) -> Option<Answer> {
        if !self.interactive {
            return None;
        }
        self.prompts.ask(&*self.sink, self.job, prompt)
    }
}

/// FOMOD groups as prompts.
struct GuiFomod<'u, 'a> {
    ui: &'u GuiUi<'a>,
}

impl FomodResolver for GuiFomod<'_, '_> {
    fn choose(&mut self, p: &GroupPrompt<'_>) -> Result<Vec<usize>, FomodError> {
        let options = p
            .group
            .plugins
            .iter()
            .zip(p.types)
            .map(|(pl, t)| FomodOption {
                name: pl.name.clone(),
                description: pl.description.clone(),
                kind: *t,
                kind_label: t.label().map(str::to_string),
            })
            .collect();
        match self.ui.ask(PromptKind::FomodGroup {
            module: p.module_name.to_string(),
            step: p.step.to_string(),
            group: p.group.name.clone(),
            kind: p.group.kind,
            kind_label: p.group.kind.label().to_string(),
            options,
            defaults: p.defaults.to_vec(),
            error: p.error.map(str::to_string),
        }) {
            // Range and group rules are checked by core, which asks again.
            Some(Answer::Choices { indices }) => Ok(indices),
            _ => Err(FomodError::Cancelled),
        }
    }
}

impl Ui for GuiUi<'_> {
    fn say(&self, msg: Msg) {
        tracing::info!("{}", msg.text());
        events::emit(
            &*self.sink,
            events::ACTIVITY,
            &events::Activity {
                job: self.job,
                text: msg.text(),
                msg: &msg,
            },
        );
    }

    /// A closed dialog counts as "no".
    fn confirm(&self, question: &str, default_yes: bool) -> bool {
        if !self.interactive {
            return default_yes;
        }
        matches!(
            self.ask(PromptKind::Confirm {
                question: question.to_string(),
                default_yes,
            }),
            Some(Answer::Confirm { yes: true })
        )
    }

    fn ask(&self, question: &str, default: &str) -> Option<String> {
        match self.ask(PromptKind::Text {
            question: question.to_string(),
            default: default.to_string(),
        })? {
            Answer::Text { value } if value.trim().is_empty() => Some(default.to_string()),
            Answer::Text { value } => Some(value.trim().to_string()),
            _ => None,
        }
    }

    fn choose_file(&self, files: &[&FileInfo], default: usize) -> usize {
        match self.ask(PromptKind::ChooseFile {
            files: files.iter().map(|f| (*f).clone()).collect(),
            default,
        }) {
            Some(Answer::Choice { index }) if index < files.len() => index,
            _ => default,
        }
    }

    fn choose_optional(&self, m: &Manifest, optional: &[usize]) -> Option<Vec<usize>> {
        let mods = optional
            .iter()
            .map(|&i| OptionalMod {
                name: m.mods[i].name.clone(),
                version: m.mods[i].version.clone(),
                size: m.mods[i].file_size,
            })
            .collect();
        let collection = m
            .raw
            .pointer("/info/name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        match self.ask(PromptKind::ChooseOptional { collection, mods })? {
            Answer::Choices { mut indices } => {
                indices.retain(|i| *i < optional.len());
                indices.sort_unstable();
                indices.dedup();
                Some(indices)
            }
            _ => None,
        }
    }

    fn manual_download(&self, mm: &ManifestMod) -> Option<PathBuf> {
        match self.ask(PromptKind::ManualDownload {
            name: mm.name.clone(),
            instructions: mm.instructions().map(str::to_string),
            url: mm.url().map(str::to_string),
        })? {
            Answer::Path { path } => Some(PathBuf::from(path.trim())),
            _ => None,
        }
    }

    fn install_prompts(&self, _ask_explicit: bool) -> InstallPrompts {
        InstallPrompts {
            variant: self.interactive,
            fomod: self.interactive,
        }
    }

    fn choose_variant(&self, choices: &[String]) -> Option<usize> {
        match self.ask(PromptKind::ChooseVariant {
            choices: choices.to_vec(),
        })? {
            Answer::Choice { index } if index < choices.len() => Some(index),
            _ => None,
        }
    }

    fn fomod_resolver(&self, _mod_name: &str) -> Option<Box<dyn FomodResolver + '_>> {
        Some(Box::new(GuiFomod { ui: self }))
    }

    fn download_event(&self, name: &str, event: &DownloadEvent) {
        if matches!(event, DownloadEvent::Progress { .. }) {
            let mut last = self.last_download.lock().unwrap_or_else(|p| p.into_inner());
            if last.is_some_and(|t| t.elapsed() < PROGRESS_EVERY) {
                return;
            }
            *last = Some(Instant::now());
        }
        events::emit(
            &*self.sink,
            events::DOWNLOAD,
            &events::Download {
                job: self.job,
                name,
                event,
            },
        );
    }

    fn deploy_progress(&self) -> Option<Sender<Progress>> {
        let (tx, rx) = std::sync::mpsc::channel::<Progress>();
        let job = self.job;
        let sink = self.sink.clone();
        std::thread::Builder::new()
            .name("deploy-progress".into())
            .spawn(move || {
                let mut last: Option<Instant> = None;
                for p in rx {
                    let final_one = p.done >= p.total;
                    if !final_one && last.is_some_and(|t| t.elapsed() < PROGRESS_EVERY) {
                        continue;
                    }
                    last = Some(Instant::now());
                    events::emit(
                        &*sink,
                        events::DEPLOY_PROGRESS,
                        &events::DeployProgress {
                            job,
                            done: p.done,
                            total: p.total,
                        },
                    );
                }
            })
            .ok()?;
        Some(tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::testing::Recorder;
    use std::time::Duration;

    #[test]
    fn non_interactive_questions_take_defaults_without_prompting() {
        let sink = Arc::new(Recorder::default());
        let prompts = Prompts::default();
        let ui = GuiUi::new(sink.clone(), &prompts, Some(3), false);
        assert!(ui.confirm("q", true));
        assert!(!ui.confirm("q", false));
        assert_eq!(Ui::ask(&ui, "q", "d"), None);
        assert_eq!(ui.install_prompts(true), InstallPrompts::default());
        assert!(sink.named(events::PROMPT).is_empty());
        ui.say(Msg::info("hello ").b("world"));
        let a = &sink.named(events::ACTIVITY)[0];
        assert_eq!(a["job"], 3);
        assert_eq!(a["text"], "hello world");
        assert_eq!(a["msg"]["level"], "info");
        assert_eq!(a["msg"]["spans"][1]["style"], "bold");
    }

    fn plugin(name: &str) -> vessel_core::fomod::Plugin {
        use vessel_core::fomod::{Plugin, PluginType, TypeDescriptor};
        Plugin {
            name: name.into(),
            description: format!("{name} text"),
            image: None,
            type_descriptor: TypeDescriptor::Fixed(PluginType::Optional),
            flags: vec![],
            files: vec![],
        }
    }

    #[test]
    fn fomod_groups_are_bridged_to_prompts_from_a_blocking_thread() {
        use vessel_core::fomod::{Group, GroupType, PluginType};
        let sink = Arc::new(Recorder::default());
        let prompts = Arc::new(Prompts::default());
        let (s2, p2) = (sink.clone(), prompts.clone());
        let worker = std::thread::spawn(move || {
            let ui = GuiUi::new(s2, &p2, Some(1), true);
            let group = Group {
                name: "Textures".into(),
                kind: GroupType::SelectExactlyOne,
                plugins: vec![plugin("2K"), plugin("4K")],
            };
            let types = [PluginType::Recommended, PluginType::Optional];
            let mut r = ui.fomod_resolver("Mod").unwrap();
            let prompt = |error| GroupPrompt {
                module_name: "Mod Installer",
                step: "Main",
                group: &group,
                types: &types,
                defaults: &[0],
                error,
            };
            let first = r.choose(&prompt(None));
            let second = r.choose(&prompt(Some("pick exactly one")));
            (first, second)
        });
        let answer_next = |n: usize, a: Answer| {
            for _ in 0..400 {
                let evs = sink.named(events::PROMPT);
                if evs.len() > n {
                    let id = evs[n]["id"].as_u64().unwrap();
                    assert!(prompts.answer(id, a));
                    return evs[n].clone();
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("no prompt");
        };
        let ev = answer_next(0, Answer::Choices { indices: vec![1] });
        let p = &ev["prompt"];
        assert_eq!(p["type"], "fomod_group");
        assert_eq!(p["group"], "Textures");
        assert_eq!(p["kind"], "SelectExactlyOne");
        assert_eq!(p["kind_label"], "pick one");
        assert_eq!(p["options"][0]["kind_label"], "recommended");
        assert_eq!(p["defaults"], serde_json::json!([0]));
        let ev = answer_next(1, Answer::Cancel);
        assert_eq!(ev["prompt"]["error"], "pick exactly one");
        let (first, second) = worker.join().unwrap();
        assert_eq!(first.unwrap(), vec![1]);
        assert!(matches!(second, Err(FomodError::Cancelled)));
    }
}
