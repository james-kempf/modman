// SPDX-License-Identifier: GPL-3.0-or-later
//! Mod management commands (T07, M3).
//!
//! Placeholder: commands parse any arguments and report "not implemented yet".

use anyhow::Result;

use super::{not_yet, StubArgs};
use crate::context::Ctx;

pub type ListArgs = StubArgs;
pub type InfoArgs = StubArgs;
pub type EnableArgs = StubArgs;
pub type DisableArgs = StubArgs;
pub type UninstallArgs = StubArgs;
pub type OrderArgs = StubArgs;
pub type ConflictsArgs = StubArgs;
pub type DepsArgs = StubArgs;
pub type UpdatesArgs = StubArgs;
pub type SearchArgs = StubArgs;
pub type ProfileArgs = StubArgs;
pub type LaunchArgs = StubArgs;
pub type CleanArgs = StubArgs;

pub fn list(_ctx: &mut Ctx, _args: ListArgs) -> Result<()> {
    not_yet("list", "M3")
}

pub fn info(_ctx: &mut Ctx, _args: InfoArgs) -> Result<()> {
    not_yet("info", "M3")
}

pub fn enable(_ctx: &mut Ctx, _args: EnableArgs) -> Result<()> {
    not_yet("enable", "M3")
}

pub fn disable(_ctx: &mut Ctx, _args: DisableArgs) -> Result<()> {
    not_yet("disable", "M3")
}

pub fn uninstall(_ctx: &mut Ctx, _args: UninstallArgs) -> Result<()> {
    not_yet("uninstall", "M3")
}

pub fn order(_ctx: &mut Ctx, _args: OrderArgs) -> Result<()> {
    not_yet("order", "M3")
}

pub fn conflicts(_ctx: &mut Ctx, _args: ConflictsArgs) -> Result<()> {
    not_yet("conflicts", "M3")
}

pub fn deps(_ctx: &mut Ctx, _args: DepsArgs) -> Result<()> {
    not_yet("deps", "M3")
}

pub fn updates(_ctx: &mut Ctx, _args: UpdatesArgs) -> Result<()> {
    not_yet("updates", "M3")
}

pub fn search(_ctx: &mut Ctx, _args: SearchArgs) -> Result<()> {
    not_yet("search", "M3")
}

pub fn profile(_ctx: &mut Ctx, _args: ProfileArgs) -> Result<()> {
    not_yet("profile", "M3")
}

pub fn launch(_ctx: &mut Ctx, _args: LaunchArgs) -> Result<()> {
    not_yet("launch", "M4")
}

pub fn clean(_ctx: &mut Ctx, _args: CleanArgs) -> Result<()> {
    not_yet("clean", "M3")
}
