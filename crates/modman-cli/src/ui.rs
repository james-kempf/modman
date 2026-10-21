// SPDX-License-Identifier: GPL-3.0-or-later
//! The terminal front end of `modman_core::app`: renders messages with the
//! legacy prefixes and asks questions on stderr/stdin.

use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;

use modman_core::app::{InstallPrompts, Msg, Style, Ui};
use modman_core::collections::{self, Manifest, ManifestMod};
use modman_core::fomod::{FomodError, FomodResolver, GroupPrompt, GroupType, ModuleConfig, Step};
use modman_core::nexus::FileInfo;

use crate::context::read_line;
use crate::output::{human_size, Printer};

/// Terminal UI; `yes` = `-y` (every question takes its default).
#[derive(Debug, Clone, Copy)]
pub struct CliUi {
    pub out: Printer,
    pub yes: bool,
}

impl CliUi {
    /// Prompts are allowed (legacy `is_interactive`, L87).
    pub fn interactive(&self) -> bool {
        !self.yes && std::io::stdin().is_terminal()
    }

    fn ask_line(&self, question: &str, default: &str) -> String {
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
}

/// Styled spans as terminal text.
pub fn render(out: Printer, m: &Msg) -> String {
    m.spans
        .iter()
        .map(|s| match s.style {
            Style::Plain => s.text.clone(),
            Style::Bold => out.bold(&s.text),
            Style::Dim => out.dim(&s.text),
            Style::Green => out.green(&s.text),
            Style::Yellow => out.yellow(&s.text),
        })
        .collect()
}

impl Printer {
    /// Prints a core message with the legacy prefix and stream.
    pub fn say(&self, m: &Msg) {
        use modman_core::app::Level;
        match m.level {
            Level::Heading => self.heading(&m.text()),
            Level::Info => self.info(&render(*self, m)),
            Level::Success => self.success(&render(*self, m)),
            Level::Warn => self.warn(&render(*self, m)),
            Level::Error => self.error(&render(*self, m)),
            Level::Line => println!("{}", render(*self, m)),
            Level::ErrLine => eprintln!("{}", render(*self, m)),
        }
    }
}

fn variant_prompt(choices: &[String]) -> Option<usize> {
    eprintln!("This archive has several variants:");
    for (i, c) in choices.iter().enumerate() {
        eprintln!("  {:>2}. {c}", i + 1);
    }
    eprint!("Which one? [1]: ");
    let mut s = String::new();
    std::io::stdin().read_line(&mut s).ok()?;
    let s = s.trim();
    if s.is_empty() {
        return Some(0);
    }
    s.parse::<usize>()
        .ok()
        .filter(|n| (1..=choices.len()).contains(n))
        .map(|n| n - 1)
}

/// Parses a FOMOD answer: numbers and ranges (`1,3-4`), 1-based.
fn parse_selection(text: &str, n: usize) -> Option<Vec<usize>> {
    let mut sel = std::collections::BTreeSet::new();
    for tok in text.split([',', ' ', '\t']).filter(|t| !t.is_empty()) {
        let num = |s: &str| s.parse::<usize>().ok().filter(|&k| (1..=n).contains(&k));
        match tok.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (num(a)?, num(b)?);
                sel.extend(a - 1..b);
            }
            None => {
                sel.insert(num(tok)? - 1);
            }
        }
    }
    Some(sel.into_iter().collect())
}

/// FOMOD questions on stderr, answers from stdin (legacy `ask_group`,
/// L1162-1211).
struct PromptResolver {
    out: Printer,
    mod_name: String,
}

impl PromptResolver {
    fn read(&self, prompt: &str) -> Result<String, FomodError> {
        eprint!("{prompt}");
        let mut s = String::new();
        match std::io::stdin().lock().read_line(&mut s) {
            Ok(0) | Err(_) => Err(FomodError::Cancelled),
            Ok(_) => Ok(s.trim().to_string()),
        }
    }
}

impl FomodResolver for PromptResolver {
    fn begin(&mut self, m: &ModuleConfig) {
        let title = if m.module_name.is_empty() {
            &self.mod_name
        } else {
            &m.module_name
        };
        eprintln!(
            "\n{}",
            self.out.bold(&format!("── FOMOD installer: {title} ──"))
        );
    }

    fn step(&mut self, s: &Step) {
        eprintln!("\n{}", self.out.bold(&format!("▸ {}", s.name)));
    }

    fn choose(&mut self, p: &GroupPrompt<'_>) -> Result<Vec<usize>, FomodError> {
        let n = p.group.plugins.len();
        if let Some(e) = p.error {
            eprintln!("    {e}");
        } else {
            eprintln!(
                "\n  {}  {}",
                self.out.bold(&p.group.name),
                self.out.dim(&format!("({})", p.group.kind.label()))
            );
            for (i, pl) in p.group.plugins.iter().enumerate() {
                let mark = if p.defaults.contains(&i) { '*' } else { ' ' };
                let mut line = format!("   {mark}{:>2}. {}", i + 1, pl.name);
                if let Some(l) = p.types[i].label() {
                    line.push_str(&format!("  [{l}]"));
                }
                let desc = pl
                    .description
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if !desc.is_empty() {
                    let short: String = desc.chars().take(90).collect();
                    let more = if desc.chars().count() > 90 { "…" } else { "" };
                    line.push_str(&self.out.dim(&format!(" — {short}{more}")));
                }
                eprintln!("{line}");
            }
        }
        let dtext = if p.defaults.is_empty() {
            "none".to_string()
        } else {
            p.defaults
                .iter()
                .map(|i| (i + 1).to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        let hint = match p.group.kind {
            GroupType::SelectExactlyOne => "number",
            GroupType::SelectAtMostOne => "number, 0 = none",
            _ => "e.g. 1,3-4 · a = all · 0 = none",
        };
        loop {
            let ans = self.read(&format!("  ? Choose ({hint}, ?N = details) [{dtext}]: "))?;
            let lower = ans.to_lowercase();
            if ans.is_empty() {
                return Ok(p.defaults.to_vec());
            } else if let Some(k) = ans.strip_prefix('?') {
                match k.trim().parse::<usize>() {
                    Ok(k) if (1..=n).contains(&k) => {
                        let d = p.group.plugins[k - 1].description.trim();
                        eprintln!("\n{}\n", if d.is_empty() { "(no description)" } else { d });
                    }
                    _ => eprintln!("    invalid selection"),
                }
            } else if matches!(lower.as_str(), "0" | "n" | "none") {
                return Ok(Vec::new());
            } else if matches!(lower.as_str(), "a" | "all") {
                return Ok((0..n).collect());
            } else if let Some(sel) = parse_selection(&ans, n) {
                return Ok(sel);
            } else {
                eprintln!("    invalid selection");
            }
        }
    }
}

fn open_url(url: &str) {
    if let Some(x) = modman_core::app::which("xdg-open") {
        let _ = std::process::Command::new(x)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

impl Ui for CliUi {
    fn say(&self, msg: Msg) {
        self.out.say(&msg);
    }

    /// Legacy `confirm`: yes with `-y`, the default without a tty.
    fn confirm(&self, question: &str, default_yes: bool) -> bool {
        if self.yes {
            return true;
        }
        if !std::io::stdin().is_terminal() {
            return default_yes;
        }
        let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
        match read_line(&format!("{question} {hint} ")) {
            Some(a) if a.is_empty() => default_yes,
            Some(a) => matches!(a.to_ascii_lowercase().as_str(), "y" | "yes"),
            None => default_yes,
        }
    }

    fn ask(&self, question: &str, default: &str) -> Option<String> {
        self.interactive().then(|| self.ask_line(question, default))
    }

    /// Legacy `choose_file`: asks when interactive, else takes the default.
    fn choose_file(&self, rows: &[&FileInfo], def: usize) -> usize {
        if !self.interactive() {
            return def;
        }
        eprintln!("Files:");
        for (i, f) in rows.iter().enumerate() {
            eprintln!(
                "  {:>2}. {:<13} {:<44} {:<12} {:>9}",
                i + 1,
                f.category_name.as_deref().unwrap_or(""),
                f.name,
                f.version.as_deref().unwrap_or(""),
                human_size(f.size_in_bytes.unwrap_or(0))
            );
        }
        loop {
            let a = self.ask_line("Which file?", &(def + 1).to_string());
            match a.parse::<usize>() {
                Ok(n) if (1..=rows.len()).contains(&n) => return n - 1,
                _ => self
                    .out
                    .warn(&format!("Enter a number between 1 and {}.", rows.len())),
            }
        }
    }

    fn choose_optional(&self, m: &Manifest, opt: &[usize]) -> Option<Vec<usize>> {
        if !self.interactive() {
            return None;
        }
        let out = self.out;
        println!("{}", out.bold("Optional mods:"));
        for (n, &i) in opt.iter().enumerate() {
            let mm = &m.mods[i];
            println!(
                "  {:>3}. {} {}",
                n + 1,
                mm.name,
                out.dim(&format!(
                    "{}  ({})",
                    mm.version.as_deref().unwrap_or(""),
                    human_size(mm.file_size)
                ))
            );
        }
        loop {
            let a = self.ask_line(
                "Install which optional mods? (all, none, or e.g. 1,3-5)",
                "all",
            );
            match collections::parse_selection(&a, opt.len()) {
                Some(p) => return Some(p),
                None => out.warn("Invalid selection."),
            }
        }
    }

    /// `browse` / `manual` sources (legacy L2637-2651).
    fn manual_download(&self, mm: &ManifestMod) -> Option<PathBuf> {
        if !self.interactive() {
            return None;
        }
        let out = self.out;
        println!(
            "\n  {} {}",
            out.yellow("Manual download required:"),
            out.bold(&mm.name)
        );
        if let Some(i) = mm.instructions() {
            println!("    {i}");
        }
        if let Some(u) = mm.url() {
            println!("    {u}");
            open_url(u);
        }
        Some(PathBuf::from(
            self.ask_line("Path to the downloaded file (empty to skip)", ""),
        ))
    }

    /// FOMOD questions are asked when the session is interactive, or, for
    /// an explicit `--fomod ask` / `reinstall --ask`, whenever `-y` is not
    /// given: answers are then read from stdin even when it is not a
    /// terminal (scripted installs).
    fn install_prompts(&self, ask_explicit: bool) -> InstallPrompts {
        let interactive = self.interactive();
        InstallPrompts {
            variant: interactive,
            fomod: if ask_explicit { !self.yes } else { interactive },
        }
    }

    fn choose_variant(&self, choices: &[String]) -> Option<usize> {
        variant_prompt(choices)
    }

    fn fomod_resolver(&self, mod_name: &str) -> Option<Box<dyn FomodResolver + '_>> {
        Some(Box::new(PromptResolver {
            out: self.out,
            mod_name: mod_name.to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fomod_answers_parse_numbers_and_ranges() {
        assert_eq!(parse_selection("1,3-4", 4), Some(vec![0, 2, 3]));
        assert_eq!(parse_selection("2 1", 3), Some(vec![0, 1]));
        assert_eq!(parse_selection("5", 4), None);
        assert_eq!(parse_selection("0", 4), None);
        assert_eq!(parse_selection("x", 4), None);
        assert_eq!(parse_selection("2-9", 4), None);
    }

    #[test]
    fn rendering_keeps_text_without_color() {
        let out = Printer { color: false };
        let m = Msg::info("a ").b("b").d(" c");
        assert_eq!(render(out, &m), "a b c");
        let out = Printer { color: true };
        assert!(render(out, &m).contains("\x1b[1mb\x1b[0m"));
    }
}
