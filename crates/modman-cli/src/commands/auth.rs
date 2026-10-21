// SPDX-License-Identifier: GPL-3.0-or-later
//! Account commands: login, logout (T07, M3).
//!
//! Placeholder: commands parse any arguments and report "not implemented yet".

use anyhow::Result;

use super::{not_yet, StubArgs};
use crate::context::Ctx;

pub type LoginArgs = StubArgs;
pub type LogoutArgs = StubArgs;

pub fn login(_ctx: &mut Ctx, _args: LoginArgs) -> Result<()> {
    not_yet("login", "M3")
}

pub fn logout(_ctx: &mut Ctx, _args: LogoutArgs) -> Result<()> {
    not_yet("logout", "M3")
}
