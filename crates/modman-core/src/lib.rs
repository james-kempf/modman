// SPDX-License-Identifier: GPL-3.0-or-later
//! modman core engine.
//!
//! No UI code and no printing lives here. The CLI and the Tauri app are thin
//! adapters over this crate. Milestones refer to `linux_mod_manager_tech_stack.md` §8.

pub mod archive; // M3: zip / 7z / rar extraction
pub mod config; // M1: global settings
pub mod deploy; // M2: hardlink / reflink / symlink / copy, backups, purge
pub mod download; // M3: resumable downloads, nxm:// intake queue
pub mod error;
pub mod fomod; // M5: ModuleConfig.xml parser and install plan
pub mod games; // M1: game definitions, Steam detection, configured games
pub mod loadorder; // M6: LOOT sorting and collection rules
pub mod nexus; // M3: REST v1 + GraphQL v2 client, auth, rate limits
pub mod paths; // M1: XDG locations
pub mod staging; // M2: layout normalisation, staged file lists
pub mod store; // M1: SQLite schema, migrations, deploy lock

pub use error::{Error, Result};

/// Version of the core crate, for `--version` output.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert!(!super::VERSION.is_empty());
    }

    #[test]
    fn module_errors_convert_into_top_level_error() {
        fn takes(_: crate::Error) {}
        // Compile-time proof of the #[from] conversions; the stub enums are
        // empty, so only the conversions with constructible values run.
        let _ = |e: crate::archive::ArchiveError| takes(e.into());
        let _ = |e: crate::staging::StagingError| takes(e.into());
        let _ = |e: crate::deploy::DeployError| takes(e.into());
        let _ = |e: crate::nexus::NexusError| takes(e.into());
        let _ = |e: crate::download::DownloadError| takes(e.into());
        let _ = |e: crate::fomod::FomodError| takes(e.into());
        let _ = |e: crate::loadorder::LoadOrderError| takes(e.into());
        takes(crate::store::StoreError::LockBusy("/l".into()).into());
        takes(crate::games::GamesError::NoActiveGame.into());
        takes(crate::config::ConfigError::NoHome.into());
    }
}
