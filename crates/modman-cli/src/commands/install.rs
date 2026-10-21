// SPDX-License-Identifier: GPL-3.0-or-later
//! Install, download, reinstall (T07 M3, FOMOD T09a M5).
//!
//! Placeholder: commands parse any arguments and report "not implemented yet".

use anyhow::Result;

use super::{not_yet, StubArgs};
use crate::context::Ctx;

pub type InstallArgs = StubArgs;
pub type DownloadArgs = StubArgs;
pub type ReinstallArgs = StubArgs;

pub fn install(_ctx: &mut Ctx, _args: InstallArgs) -> Result<()> {
    not_yet("install", "M3")
}

pub fn download(_ctx: &mut Ctx, _args: DownloadArgs) -> Result<()> {
    not_yet("download", "M3")
}

pub fn reinstall(_ctx: &mut Ctx, _args: ReinstallArgs) -> Result<()> {
    not_yet("reinstall", "M5")
}
