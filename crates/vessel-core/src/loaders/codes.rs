// SPDX-License-Identifier: GPL-3.0-or-later
//! Stable string codes for [`LoaderError`] and [`LaunchError`], sent over
//! IPC next to the message so the UI can pick a title and an action
//! (`apps/desktop/src/ipc/loaderErrors.ts`). Codes never change once
//! shipped; the matches have no wildcard so a new variant must get one.
//! Every code is listed in `apps/desktop/src/ipc/loaderErrorCodes.json`,
//! which the tests below and the vitest both check.

use super::launch::LaunchError;
use super::LoaderError;

impl LoaderError {
    /// Stable code of this error (`loader_*` or `launch_*`).
    pub fn code(&self) -> &'static str {
        match self {
            LoaderError::Conflict { .. } => "loader_conflict",
            LoaderError::ArchiveRequired { .. } => "loader_archive_required",
            LoaderError::Payload { .. } => "loader_payload",
            LoaderError::UnsafePath { .. } => "loader_unsafe_path",
            LoaderError::UnsafeForCmd(_) => "loader_unsafe_for_cmd",
            LoaderError::ManualInstallDetected(_) => "loader_manual_install_detected",
            LoaderError::AmbiguousMatch(..) => "loader_ambiguous_match",
            LoaderError::DuplicateKey(_) => "loader_duplicate_key",
            LoaderError::UnknownLoader(_) => "loader_unknown_loader",
            LoaderError::NoCompatTool { .. } => "loader_no_compat_tool",
            LoaderError::CompatToolMissing(_) => "loader_compat_tool_missing",
            LoaderError::ProtonUntrusted { .. } => "loader_proton_untrusted",
            LoaderError::SlrMissing { .. } => "loader_slr_missing",
            LoaderError::FlatpakUnsupported => "loader_flatpak_unsupported",
            LoaderError::FlatpakSteamUnsupported => "loader_flatpak_steam_unsupported",
            LoaderError::GameRunning { .. } => "loader_game_running",
            LoaderError::NotFullyInstalled { .. } => "loader_not_fully_installed",
            LoaderError::PrefixMissing(_) => "loader_prefix_missing",
            LoaderError::UnknownRuntime(_) => "loader_unknown_runtime",
            LoaderError::HostMismatch { .. } => "loader_host_mismatch",
            LoaderError::Spawn { .. } => "loader_spawn",
            LoaderError::LayoutMismatch { .. } => "loader_layout_mismatch",
            // A game file that is not vanilla gets its own action (verify
            // the game files in Steam).
            LoaderError::VerifyFailed {
                steam_hint: true, ..
            } => "loader_verify_failed_steam",
            LoaderError::VerifyFailed { .. } => "loader_verify_failed",
            LoaderError::Consent(_) => "loader_consent",
            LoaderError::AlreadyInstalled(_) => "loader_already_installed",
            LoaderError::NotInstalled(_) => "loader_not_installed",
            LoaderError::Launch(e) => e.code(),
            LoaderError::Store(_) => "loader_store",
            LoaderError::Io { .. } => "loader_io",
        }
    }
}

impl LaunchError {
    /// Stable code of this error (`launch_*`).
    pub fn code(&self) -> &'static str {
        match self {
            LaunchError::Conflict(_) => "launch_conflict",
            LaunchError::Invalid(_) => "launch_invalid",
            LaunchError::SteamRunning => "launch_steam_running",
            LaunchError::Changed(_) => "launch_changed",
            LaunchError::NoAccount => "launch_no_account",
            LaunchError::UnknownAccount(_) => "launch_unknown_account",
            LaunchError::NothingToApply(_) => "launch_nothing_to_apply",
            LaunchError::UnknownGame(_) => "launch_unknown_game",
            LaunchError::Vdf { .. } => "launch_vdf",
            LaunchError::Store(_) => "launch_store",
            LaunchError::Io { .. } => "launch_io",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::*;
    use crate::loaders::Owner;
    use crate::store::StoreError;

    const FIXTURE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/desktop/src/ipc/loaderErrorCodes.json"
    );

    fn io() -> std::io::Error {
        std::io::Error::other("x")
    }

    fn store() -> StoreError {
        StoreError::SchemaTooNew {
            found: 2,
            supported: 1,
        }
    }

    fn launch_samples() -> Vec<LaunchError> {
        vec![
            LaunchError::Conflict("a".into()),
            LaunchError::Invalid("a".into()),
            LaunchError::SteamRunning,
            LaunchError::Changed(PathBuf::from("p")),
            LaunchError::NoAccount,
            LaunchError::UnknownAccount(1),
            LaunchError::NothingToApply("g".into()),
            LaunchError::UnknownGame("g".into()),
            LaunchError::Vdf {
                path: PathBuf::from("p"),
                message: "m".into(),
            },
            LaunchError::Store(store()),
            LaunchError::Io {
                path: PathBuf::from("p"),
                source: io(),
            },
        ]
    }

    /// One value per variant (two for VerifyFailed). A variant added to the
    /// enum fails to compile in `code()`; add its sample here too.
    fn samples() -> Vec<LoaderError> {
        let p = || PathBuf::from("p");
        let mut v = vec![
            LoaderError::Conflict {
                path: "a".into(),
                owner: Owner::Deployed,
            },
            LoaderError::ArchiveRequired { op: "install" },
            LoaderError::Payload {
                key: "k".into(),
                message: "m".into(),
            },
            LoaderError::UnsafePath {
                path: "a".into(),
                message: "m".into(),
            },
            LoaderError::UnsafeForCmd("a".into()),
            LoaderError::ManualInstallDetected("smapi".into()),
            LoaderError::AmbiguousMatch("a".into(), "b".into()),
            LoaderError::DuplicateKey("k".into()),
            LoaderError::UnknownLoader("k".into()),
            LoaderError::NoCompatTool { appid: 1 },
            LoaderError::CompatToolMissing("t".into()),
            LoaderError::ProtonUntrusted {
                path: p(),
                reason: "r".into(),
            },
            LoaderError::SlrMissing { appid: 1 },
            LoaderError::FlatpakUnsupported,
            LoaderError::FlatpakSteamUnsupported,
            LoaderError::GameRunning { pid: 1 },
            LoaderError::NotFullyInstalled { flags: 6 },
            LoaderError::PrefixMissing(p()),
            LoaderError::UnknownRuntime("r".into()),
            LoaderError::HostMismatch {
                kind: "windows",
                host: "linux",
            },
            LoaderError::Spawn {
                program: p(),
                source: io(),
            },
            LoaderError::LayoutMismatch {
                loader: "smapi",
                reason: "r".into(),
            },
            LoaderError::VerifyFailed {
                op: "install",
                reason: "files are still present".into(),
                steam_hint: false,
            },
            LoaderError::VerifyFailed {
                op: "repair",
                reason:
                    "the launcher is not vanilla; verify the game files in Steam, then try again"
                        .into(),
                steam_hint: true,
            },
            LoaderError::Consent("c".into()),
            LoaderError::AlreadyInstalled("k".into()),
            LoaderError::NotInstalled("k".into()),
            LoaderError::Store(store()),
            LoaderError::Io {
                path: p(),
                source: io(),
            },
        ];
        v.extend(launch_samples().into_iter().map(LoaderError::Launch));
        v
    }

    /// Catches: two variants sharing a code (the UI could not tell them
    /// apart) and a code missing from the fixture the UI mapping is tested
    /// against. `VESSEL_WRITE_FIXTURES=1` rewrites the fixture.
    #[test]
    fn codes_are_unique_and_match_fixture() {
        let codes: Vec<&str> = samples().iter().map(LoaderError::code).collect();
        let unique: BTreeSet<&str> = codes.iter().copied().collect();
        assert_eq!(unique.len(), codes.len(), "duplicate code in {codes:?}");
        let expected = serde_json::to_string_pretty(&codes).unwrap() + "\n";
        if std::env::var_os("VESSEL_WRITE_FIXTURES").is_some() {
            std::fs::write(FIXTURE, &expected).unwrap();
        }
        let actual = std::fs::read_to_string(FIXTURE).unwrap_or_default();
        assert_eq!(
            actual, expected,
            "{FIXTURE} is stale; rerun with VESSEL_WRITE_FIXTURES=1"
        );
    }

    /// Catches: the Steam action keyed on reason wording (a reason that
    /// mentions Steam must not get it; only the structured hint does).
    #[test]
    fn verify_failed_steam_split() {
        let e = |r: &str, steam_hint| LoaderError::VerifyFailed {
            op: "install",
            reason: r.into(),
            steam_hint,
        };
        assert_eq!(e("missing", false).code(), "loader_verify_failed");
        assert_eq!(
            e("verify the game files in Steam", false).code(),
            "loader_verify_failed"
        );
        assert_eq!(e("missing", true).code(), "loader_verify_failed_steam");
    }

    /// Catches: a wrapped LaunchError losing its own code.
    #[test]
    fn launch_codes_pass_through() {
        assert_eq!(
            LoaderError::Launch(LaunchError::SteamRunning).code(),
            "launch_steam_running"
        );
    }
}
