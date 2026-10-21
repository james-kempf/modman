//! modman core engine.
//!
//! No UI code and no printing lives here. The CLI and the Tauri app are thin
//! adapters over this crate. Every module below is an empty placeholder for
//! the milestone noted in `linux_mod_manager_tech_stack.md` (section 8).

pub mod archive; // M3: zip / 7z / rar extraction
pub mod deploy; // M2: hardlink / reflink / symlink / copy, backups, purge
pub mod download; // M3: resumable downloads, nxm:// intake queue
pub mod error;
pub mod fomod; // M5: ModuleConfig.xml parser and install plan
pub mod games; // M1: Steam library and appmanifest detection
pub mod loadorder; // M6: LOOT sorting and collection rules
pub mod nexus; // M3: REST v1 + GraphQL v2 client, auth, rate limits
pub mod staging; // M2: one folder per mod version
pub mod store; // M1: SQLite schema and migrations

pub use error::{Error, Result};

/// Version of the core crate, for `--version` output.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert!(!super::VERSION.is_empty());
    }
}
