// SPDX-License-Identifier: GPL-3.0-or-later
//! Auto-update commands (T12, M7).
//!
//! Placeholder: commands parse any arguments and report "not implemented yet".

use anyhow::Result;

use super::{not_yet, StubArgs};
use crate::context::Ctx;

pub type AutoupdateArgs = StubArgs;

pub fn run(_ctx: &mut Ctx, _args: AutoupdateArgs) -> Result<()> {
    not_yet("autoupdate", "M7")
}
