// SPDX-License-Identifier: GPL-3.0-or-later
//! Front-end-neutral orchestration shared by the CLI and the desktop app:
//! the install pipeline, the collection driver, updates, requirement
//! reports, deploy/purge runs, nxm intake, Nexus login and the auto-update
//! actions.
//!
//! Nothing here prints. User-facing text goes to a [`Ui`] as [`Msg`]s
//! (styled spans the CLI renders as before and the GUI streams as events);
//! questions go to the same [`Ui`], which answers from a terminal prompt, a
//! webview dialog or its defaults.

pub mod actions;
pub mod collection;
pub mod deploy;
pub mod games;
pub mod install;
pub mod mods;
pub mod nexus;
pub mod nxm;
pub mod plugins;
pub mod thumbs;
pub mod updates;
pub mod views;

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use serde::Serialize;

use crate::autoupdate::AutoupdateError;
use crate::collections::{CollectionError, Manifest, ManifestMod};
use crate::download::DownloadEvent;
use crate::fomod::FomodResolver;
use crate::games::{GameConfig, GamesError};
use crate::mods::ModsError;
use crate::nexus::FileInfo;
use crate::paths::Paths;
use crate::store::{DeployLock, Store, StoreError};

/// Errors of the orchestration layer. Messages are user-facing.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AppError {
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Core(#[from] crate::Error),
    #[error(transparent)]
    Mods(#[from] ModsError),
    #[error(transparent)]
    Collection(#[from] CollectionError),
    #[error(transparent)]
    Autoupdate(#[from] AutoupdateError),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}

macro_rules! via_core {
    ($($t:ty),* $(,)?) => {$(
        impl From<$t> for AppError {
            fn from(e: $t) -> Self {
                AppError::Core(e.into())
            }
        }
    )*};
}

via_core!(
    crate::archive::ArchiveError,
    crate::staging::StagingError,
    crate::deploy::DeployError,
    crate::nexus::NexusError,
    crate::download::DownloadError,
    crate::fomod::FomodError,
    crate::loadorder::LoadOrderError,
    StoreError,
    GamesError,
    crate::config::ConfigError,
    std::io::Error,
);

pub type R<T> = Result<T, AppError>;

/// `return Err(AppError::Msg(format!(...)))`.
macro_rules! fail {
    ($($t:tt)*) => {
        return Err($crate::app::AppError::Msg(format!($($t)*)))
    };
}
pub(crate) use fail;

/// An error and its sources joined with `": "` (like anyhow's `{:#}`).
pub fn chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(c) = src {
        let t = c.to_string();
        if !s.ends_with(&t) {
            s.push_str(": ");
            s.push_str(&t);
        }
        src = c.source();
    }
    s
}

// ── messages ────────────────────────────────────────────────

/// Where a message goes (CLI: prefix and stream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Section heading.
    Heading,
    Info,
    Success,
    Warn,
    Error,
    /// A plain output line (stdout).
    Line,
    /// A plain diagnostic line (stderr).
    ErrLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    Plain,
    Bold,
    Dim,
    Green,
    Yellow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

/// One user-facing message made of styled spans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Msg {
    pub level: Level,
    pub spans: Vec<Span>,
}

impl Msg {
    pub fn new(level: Level, text: impl Into<String>) -> Self {
        Self {
            level,
            spans: Vec::new(),
        }
        .p(text)
    }

    pub fn heading(t: impl Into<String>) -> Self {
        Self::new(Level::Heading, t)
    }
    pub fn info(t: impl Into<String>) -> Self {
        Self::new(Level::Info, t)
    }
    pub fn success(t: impl Into<String>) -> Self {
        Self::new(Level::Success, t)
    }
    pub fn warn(t: impl Into<String>) -> Self {
        Self::new(Level::Warn, t)
    }
    pub fn error(t: impl Into<String>) -> Self {
        Self::new(Level::Error, t)
    }
    pub fn line(t: impl Into<String>) -> Self {
        Self::new(Level::Line, t)
    }
    pub fn err_line(t: impl Into<String>) -> Self {
        Self::new(Level::ErrLine, t)
    }
    /// An empty output line.
    pub fn blank() -> Self {
        Self::line("")
    }

    fn push(mut self, style: Style, t: impl Into<String>) -> Self {
        let text = t.into();
        if !text.is_empty() {
            self.spans.push(Span { text, style });
        }
        self
    }
    /// Plain text.
    pub fn p(self, t: impl Into<String>) -> Self {
        self.push(Style::Plain, t)
    }
    pub fn b(self, t: impl Into<String>) -> Self {
        self.push(Style::Bold, t)
    }
    pub fn d(self, t: impl Into<String>) -> Self {
        self.push(Style::Dim, t)
    }
    pub fn g(self, t: impl Into<String>) -> Self {
        self.push(Style::Green, t)
    }
    pub fn y(self, t: impl Into<String>) -> Self {
        self.push(Style::Yellow, t)
    }

    /// The text without styles.
    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

/// Which install-time questions may be asked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InstallPrompts {
    /// Pick one of several archive variants.
    pub variant: bool,
    /// Run FOMOD installers interactively.
    pub fomod: bool,
}

/// Messages and questions of a front end. Every question has a
/// non-interactive answer (the default), which [`Headless`] gives.
pub trait Ui {
    fn say(&self, msg: Msg);

    /// Yes/no question.
    fn confirm(&self, _question: &str, default_yes: bool) -> bool {
        default_yes
    }
    /// Free-text question; `None` = cannot ask (non-interactive).
    fn ask(&self, _question: &str, _default: &str) -> Option<String> {
        None
    }
    /// Which file of a mod to install; `files` are in display order.
    fn choose_file(&self, _files: &[&FileInfo], default: usize) -> usize {
        default
    }
    /// Optional collection mods to install (indexes into `optional`);
    /// `None` = cannot ask, so none are installed.
    fn choose_optional(&self, _m: &Manifest, _optional: &[usize]) -> Option<Vec<usize>> {
        None
    }
    /// A file the user downloaded by hand for a `browse`/`manual` source;
    /// `None` = skipped (or cannot ask).
    fn manual_download(&self, _m: &ManifestMod) -> Option<PathBuf> {
        None
    }
    /// Questions allowed while installing an archive. `ask_explicit` is an
    /// explicit `--fomod ask` / `reinstall --ask`.
    fn install_prompts(&self, _ask_explicit: bool) -> InstallPrompts {
        InstallPrompts::default()
    }
    /// Pick one variant folder (0-based); `None` = cancel (first variant).
    fn choose_variant(&self, _choices: &[String]) -> Option<usize> {
        Some(0)
    }
    /// The interactive FOMOD resolver (only used when
    /// [`InstallPrompts::fomod`] is set).
    fn fomod_resolver(&self, _mod_name: &str) -> Option<Box<dyn FomodResolver + '_>> {
        None
    }
    /// Download progress of `name`.
    fn download_event(&self, _name: &str, _event: &DownloadEvent) {}
    /// Deploy/purge progress sink.
    fn deploy_progress(&self) -> Option<Sender<crate::deploy::Progress>> {
        None
    }

    fn info(&self, t: &str) {
        self.say(Msg::info(t));
    }
    fn success(&self, t: &str) {
        self.say(Msg::success(t));
    }
    fn warn(&self, t: &str) {
        self.say(Msg::warn(t));
    }
    fn error(&self, t: &str) {
        self.say(Msg::error(t));
    }
    fn heading(&self, t: &str) {
        self.say(Msg::heading(t));
    }
    fn line(&self, t: &str) {
        self.say(Msg::line(t));
    }
    fn blank(&self) {
        self.say(Msg::blank());
    }
}

/// Non-interactive front end: every question takes its default; messages
/// go to the log.
#[derive(Debug, Default, Clone, Copy)]
pub struct Headless;

impl Ui for Headless {
    fn say(&self, msg: Msg) {
        tracing::info!("{}", msg.text());
    }
}

/// What every orchestration call works on.
pub struct Cx<'a> {
    pub paths: &'a Paths,
    pub store: &'a mut Store,
    pub ui: &'a dyn Ui,
    /// Game named on the command line (`-g`), else the active one applies.
    pub game: Option<&'a str>,
}

impl<'a> Cx<'a> {
    pub fn new(paths: &'a Paths, store: &'a mut Store, ui: &'a dyn Ui) -> Self {
        Self {
            paths,
            store,
            ui,
            game: None,
        }
    }

    /// `-g`, the active game, else the only one.
    pub fn need_game(&self) -> R<GameConfig> {
        crate::games::resolve_game(self.store, self.game).map_err(games_hint)
    }

    /// The game named by a target, else [`Self::need_game`].
    pub fn game_for(&self, domain: Option<&str>) -> R<GameConfig> {
        match domain {
            Some(d) => crate::games::require_game(self.store.conn(), d).map_err(games_hint),
            None => self.need_game(),
        }
    }

    pub fn config(&self) -> R<crate::config::Config> {
        Ok(crate::config::Config::load(self.store, self.paths)?)
    }
}

/// Adds the "Run: ..." hints to game errors.
pub fn games_hint(e: GamesError) -> AppError {
    match e {
        GamesError::NoActiveGame => AppError::Msg(
            "No game selected. Run: modman game add <domain>  (or pass -g <domain>)".into(),
        ),
        GamesError::NotConfigured(d) => AppError::Msg(format!(
            "Game '{d}' is not set up. Run: modman game add {d}"
        )),
        GamesError::DeploymentActive(d) => AppError::Msg(format!(
            "Mods are still deployed for {d}. Purge first: modman -g {d} purge"
        )),
        other => other.into(),
    }
}

/// Holds the deploy lock while staging trees change.
pub fn staging_lock(paths: &Paths) -> R<DeployLock> {
    DeployLock::try_acquire(&paths.deploy_lock_path()).map_err(|e| match e {
        StoreError::LockBusy(_) => AppError::Msg(
            "Another modman process is deploying or changing mods; try again when it has finished."
                .into(),
        ),
        e => e.into(),
    })
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

/// First executable named `name` on `PATH`.
pub fn which(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p).map(|d| d.join(name)).find(|f| {
            f.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

/// `~/` expansion for paths typed by the user.
pub fn expand_home(p: &str, home: &Path) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(p),
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::cell::RefCell;

    use super::{Msg, Ui};

    /// Records messages; answers with defaults.
    #[derive(Default)]
    pub struct Recorder {
        pub msgs: RefCell<Vec<Msg>>,
    }

    impl Recorder {
        pub fn texts(&self) -> Vec<String> {
            self.msgs.borrow().iter().map(Msg::text).collect()
        }
    }

    impl Ui for Recorder {
        fn say(&self, msg: Msg) {
            self.msgs.borrow_mut().push(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_keep_styles_and_drop_empty_spans() {
        let m = Msg::info("a ").b("bold").p("").d(" dim");
        assert_eq!(m.text(), "a bold dim");
        assert_eq!(m.spans.len(), 3);
        assert_eq!(m.spans[1].style, Style::Bold);
        assert!(Msg::blank().spans.is_empty());
    }

    #[test]
    fn error_chain_joins_sources_once() {
        let e = AppError::Mods(ModsError::Io {
            path: "/x".into(),
            source: std::io::Error::other("boom"),
        });
        assert_eq!(chain(&e), "I/O error on /x: boom");
    }

    #[test]
    fn game_errors_carry_the_cli_hints() {
        assert!(games_hint(GamesError::NoActiveGame)
            .to_string()
            .contains("modman game add"));
    }
}
