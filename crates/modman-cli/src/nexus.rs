// SPDX-License-Identifier: GPL-3.0-or-later
//! Nexus glue for commands; the logic lives in `modman_core::app::nexus`.

use anyhow::Result;
use modman_core::app::nexus as nx;

pub use nx::{Nexus, PREMIUM_KEY, USER_KEY};

use crate::context::Ctx;

/// A short description of where the key comes from, without loading it.
pub fn key_source(ctx: &Ctx) -> Result<Option<String>> {
    Ok(nx::key_source(&ctx.paths, &ctx.store)?)
}

/// Client with the stored key; anonymous when not logged in.
pub fn connect(ctx: &Ctx) -> Result<Option<Nexus>> {
    Ok(nx::connect(&ctx.paths, &ctx.store, false)?)
}
