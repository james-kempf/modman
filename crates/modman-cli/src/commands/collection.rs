// SPDX-License-Identifier: GPL-3.0-or-later
//! Collection commands (T09b, M5).
//!
//! Placeholder: commands parse any arguments and report "not implemented yet".

use anyhow::Result;

use super::{not_yet, StubArgs};
use crate::context::Ctx;

pub type CollectionArgs = StubArgs;

pub fn run(_ctx: &mut Ctx, _args: CollectionArgs) -> Result<()> {
    not_yet("collection", "M5")
}
