// SPDX-License-Identifier: GPL-3.0-or-later
//! M2: hardlink, reflink, symlink and copy deployment; backups; purge.
//!
//! Staging location per Steam library lives here (`<library>/.modman/staging`).
//!
//! Placeholder: owned by T05.

/// Errors from this module. Variants are added by the module owner (T05).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DeployError {}
