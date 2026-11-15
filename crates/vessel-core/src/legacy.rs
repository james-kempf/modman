// SPDX-License-Identifier: GPL-3.0-or-later
//! Names used before the rebrand to Vessel. This is the only place the old
//! "modman" names live; migration code reads them from here.

/// XDG directory leaf (`~/.config/modman`, `~/.local/share/modman`,
/// `~/.local/state/modman`).
pub const LEGACY_APP_DIR: &str = "modman";

/// Database file name in the legacy data dir.
pub const LEGACY_DB_FILE: &str = "modman.db";

/// Log file name in the legacy state dir.
pub const LEGACY_LOG_FILE: &str = "modman.log";

/// Secret Service (keyring) service name of the stored Nexus API key.
pub const LEGACY_KEYRING_SERVICE: &str = "modman";

/// systemd user unit base name (`.service` / `.timer`) of the auto-updater.
pub const LEGACY_UNIT: &str = "modman-autoupdate";

/// Desktop file registered as the `nxm://` scheme handler.
pub const LEGACY_NXM_DESKTOP_FILE: &str = "modman-nxm.desktop";

/// `AppName` of the non-Steam-game shortcut in Steam's `shortcuts.vdf`.
pub const LEGACY_STEAM_APP_NAME: &str = "modman";

/// Per-library staging dir, relative to a Steam library root.
pub const LEGACY_STAGING_DIR: &str = ".modman/staging";

/// Session D-Bus name owned by a running legacy GUI (single instance).
pub const LEGACY_DBUS_NAME: &str = "org.modman.app.SingleInstance";

/// Prefix of the legacy environment variables (`MODMAN_DATA_DIR`, ...).
pub const LEGACY_ENV_PREFIX: &str = "MODMAN_";

/// Legacy application identifier (Tauri identifier, Flatpak app id).
pub const LEGACY_APP_ID: &str = "org.modman.app";
