// SPDX-License-Identifier: GPL-3.0-or-later
//! Per-run state shared by commands: paths, store, output and global flags.

use std::io::{BufRead, Write};

use anyhow::Result;
use modman_core::app::{Cx, Ui};
use modman_core::games::{self, GameConfig, GameDefinition, GamesError};
use modman_core::paths::Paths;
use modman_core::store::Store;

use crate::output::Printer;
use crate::ui::CliUi;

pub struct Ctx {
    pub paths: Paths,
    pub store: Store,
    pub out: Printer,
    /// `-y/--yes`.
    pub yes: bool,
    /// `-g/--game` (lower-cased).
    pub game: Option<String>,
}

impl Ctx {
    /// The terminal UI for core orchestration calls.
    pub fn ui(&self) -> CliUi {
        CliUi {
            out: self.out,
            yes: self.yes,
        }
    }

    /// Core orchestration context over this run's paths and store.
    pub fn cx<'a>(&'a mut self, ui: &'a CliUi) -> Cx<'a> {
        Cx {
            paths: &self.paths,
            store: &mut self.store,
            ui,
            game: self.game.as_deref(),
        }
    }

    /// Prompts are allowed (legacy `is_interactive`, L87).
    pub fn is_interactive(&self) -> bool {
        self.ui().interactive()
    }

    /// Legacy `confirm`: yes with `-y`, the default without a tty.
    pub fn confirm(&self, question: &str, default_yes: bool) -> bool {
        self.ui().confirm(question, default_yes)
    }

    /// Asks for a value; empty input takes `default`.
    pub fn ask(&self, question: &str, default: &str) -> String {
        let prompt = if default.is_empty() {
            format!("{question}: ")
        } else {
            format!("{question} [{default}]: ")
        };
        match read_line(&prompt) {
            Some(a) if !a.is_empty() => a,
            _ => default.to_string(),
        }
    }

    /// Embedded definitions plus user overrides.
    pub fn definitions(&self) -> Result<Vec<GameDefinition>> {
        Ok(games::load_definitions(Some(
            &self.paths.definitions_dir(),
        ))?)
    }

    /// Game to act on: `-g`, the active game, else the only one.
    pub fn need_game(&self) -> Result<GameConfig> {
        self.game_or(self.game.as_deref())
    }

    pub fn game_or(&self, domain: Option<&str>) -> Result<GameConfig> {
        games::resolve_game(&self.store, domain).map_err(games_hint)
    }
}

/// Adds the legacy "Run: ..." hints to game errors.
pub fn games_hint(e: GamesError) -> anyhow::Error {
    modman_core::app::games_hint(e).into()
}

pub(crate) fn read_line(prompt: &str) -> Option<String> {
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    let mut s = String::new();
    match std::io::stdin().lock().read_line(&mut s) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(s.trim().to_string()),
    }
}
