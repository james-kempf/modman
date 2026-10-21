// SPDX-License-Identifier: GPL-3.0-or-later
//! Deployment commands: deploy, purge, verify, status (T05, M2).
//!
//! Placeholder: commands parse any arguments and report "not implemented yet".

use anyhow::Result;

use super::{not_yet, StubArgs};
use crate::context::Ctx;

pub type DeployArgs = StubArgs;
pub type PurgeArgs = StubArgs;
pub type VerifyArgs = StubArgs;
pub type StatusArgs = StubArgs;

pub fn deploy(_ctx: &mut Ctx, _args: DeployArgs) -> Result<()> {
    not_yet("deploy", "M2")
}

pub fn purge(_ctx: &mut Ctx, _args: PurgeArgs) -> Result<()> {
    not_yet("purge", "M2")
}

pub fn verify(_ctx: &mut Ctx, _args: VerifyArgs) -> Result<()> {
    not_yet("verify", "M2")
}

pub fn status(_ctx: &mut Ctx, _args: StatusArgs) -> Result<()> {
    not_yet("status", "M2")
}
