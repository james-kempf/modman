// SPDX-License-Identifier: GPL-3.0-or-later
//! `log [-f] [lines]` (legacy `cmd_log` L3949-3960): tail the log file.

use std::io::{Read, Seek, SeekFrom, Write};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Args;

use crate::context::Ctx;

#[derive(Args, Debug)]
pub struct LogArgs {
    /// Keep printing new lines as they are written.
    #[arg(short = 'f', long = "follow")]
    pub follow: bool,
    /// Number of lines to show.
    #[arg(default_value_t = 40)]
    pub lines: usize,
}

pub fn run(ctx: &mut Ctx, args: LogArgs) -> Result<()> {
    let path = ctx.paths.log_path();
    let mut file = std::fs::File::open(&path)
        .with_context(|| format!("cannot open log {}", path.display()))?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    let lines: Vec<&str> = text.lines().collect();
    let mut stdout = std::io::stdout().lock();
    for line in &lines[lines.len().saturating_sub(args.lines)..] {
        writeln!(stdout, "{line}")?;
    }
    stdout.flush()?;
    if !args.follow {
        return Ok(());
    }
    let mut pos = file.stream_position()?;
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let len = file.metadata()?.len();
        if len < pos {
            // Truncated: start over.
            pos = 0;
        }
        if len > pos {
            file.seek(SeekFrom::Start(pos))?;
            let mut chunk = Vec::new();
            file.read_to_end(&mut chunk)?;
            pos += chunk.len() as u64;
            stdout.write_all(&chunk)?;
            stdout.flush()?;
        }
    }
}
