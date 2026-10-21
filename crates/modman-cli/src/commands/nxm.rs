// SPDX-License-Identifier: GPL-3.0-or-later
//! nxm:// handling: register, unregister, status, <url> (T07, M3).
//!
//! Placeholder: parses any arguments and reports "not implemented yet".

use anyhow::Result;
use clap::Args;

use super::not_yet;
use crate::context::Ctx;

#[derive(Args, Debug)]
pub struct NxmArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub args: Vec<String>,
}

pub fn run(_ctx: &mut Ctx, _args: NxmArgs) -> Result<()> {
    not_yet("nxm", "M3")
}
