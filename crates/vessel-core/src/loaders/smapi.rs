// SPDX-License-Identifier: GPL-3.0-or-later
//! SMAPI (Stardew Modding API) as a loader plugin.
//!
//! By user decision (option A) Vessel runs SMAPI's own installer binary
//! directly, never the `install on *.sh/.bat` wrappers (they drop their
//! arguments and only work interactively):
//!
//! - native: `internal/linux/SMAPI.Installer` (Exe on the native host),
//! - Proton: `internal/windows/SMAPI.Installer.exe` (Exe under Proton),
//!
//! with `--install|--uninstall --no-prompt --game-path <dir>`. The
//! installer exits 0 on `--no-prompt` errors, so success is decided by
//! [`LoaderPlugin::verify`] against the game folder, not the exit code. It
//! also `chmod`s without waiting, so plans carry a [`Step::FixModes`].
//! This module only plans and checks; it never starts a process.
//!
//! Release layout (R1): `SMAPI <ver> installer/internal/{linux,macOS,windows}/`,
//! each with the installer and `install.dat` (a zip holding the bundle).
//! macOS may be missing; linux and windows may not.

use std::fs;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::glob::Glob;
use super::records::InstallStatus;
use super::snapshot::{Entry, Manifest};
use super::types::*;
use super::{LoaderError, LoaderPlugin};
use crate::store::Store;

pub const KEY: &str = "smapi";
pub const NEXUS_DOMAIN: &str = "stardewvalley";
pub const NEXUS_MOD_ID: u64 = 2400;
pub const GITHUB_REPO: &str = "Pathoschild/SMAPI";
/// UniqueIDs of the mods SMAPI bundles; it replaces them wherever they are
/// installed under `Mods/`.
pub const BUNDLED_MOD_IDS: [&str; 2] = ["SMAPI.ConsoleCommands", "SMAPI.SaveBackup"];
/// Text in SMAPI's `unix-launcher.sh` (installed as `StardewValley`) that
/// the vanilla launcher script does not contain.
const LAUNCHER_MARKER: &[u8] = b"StardewModdingAPI";

const LINUX_INSTALLER: &str = "internal/linux/SMAPI.Installer";
const WINDOWS_INSTALLER: &str = "internal/windows/SMAPI.Installer.exe";
const BUNDLE_DLL: &str = "StardewModdingAPI.dll";
/// Largest bundle member read for the version cross-check.
const DEPS_CAP: u64 = 1 << 20;
/// Largest game file read for the launcher / ELF checks.
const HEAD_CAP: u64 = 64 * 1024;

/// Paths SMAPI installs, replaces or deletes (plan decision plus R1's
/// `GetUninstallPaths`, InteractiveInstaller.cs:38-81).
pub const TOUCH_SCOPE: &[&str] = &[
    "StardewValley",
    "StardewValley-original",
    // StardewModdingAPI, .dll/.exe/.pdb/.xml/.deps.json/.runtimeconfig.json,
    // .exe.config/.exe.mdb, -settings.json, .config.json, .crash.marker,
    // .metadata.json, .update.marker, .AssemblyRewriters.dll, .Toolkit.*,
    // .Toolkit.CoreInterfaces.*, -x64.exe.
    "StardewModdingAPI*",
    "smapi-internal",
    "smapi-internal/**",
    "steam_appid.txt",
    "*.deps.json",
    "unix-launcher.sh",
    "libgdiplus.dylib",
    "Mono.Cecil.Rocks.dll",
    "Mono.Cecil.dll",
    "0Harmony.dll",
    "0Harmony.pdb",
    "Newtonsoft.Json.dll",
    "Mods/ConsoleCommands/**",
    "Mods/SaveBackup/**",
    "Mods/.cache",
    "Mods/.cache/**",
    "Mods/ErrorHandler/**",
    "Mods/TrainerMod/**",
];

pub use super::types::{classify_vanilla, VanillaState};

/// A validated SMAPI release, extracted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// `SMAPI <ver> installer`.
    pub top: String,
    pub version: String,
}

fn reject(reason: impl Into<String>) -> LoaderError {
    LoaderError::LayoutMismatch {
        loader: "SMAPI",
        reason: reason.into(),
    }
}

/// `4.5.2` from `SMAPI 4.5.2 installer`.
fn folder_version(name: &str) -> Option<&str> {
    let v = name.strip_prefix("SMAPI ")?.strip_suffix(" installer")?;
    let ok = !v.is_empty()
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'));
    ok.then_some(v)
}

fn is_regular(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.is_file())
}

/// Version declared by the bundle's `StardewModdingAPI.deps.json`
/// (`"StardewModdingAPI/<ver>"`), when the bundle has one.
fn deps_version(text: &str) -> Option<String> {
    let key = "\"StardewModdingAPI/";
    let i = text.find(key)? + key.len();
    let end = text[i..].find('"')?;
    Some(text[i..i + end].to_string())
}

/// Checks one `install.dat`: a zip whose root holds the SMAPI dll, and
/// whose deps.json (if any) names the folder's version.
fn check_bundle(path: &Path, version: &str) -> Result<(), LoaderError> {
    let rel = path.display();
    if !is_regular(path) {
        return Err(reject(format!("{rel} is missing")));
    }
    let file = fs::File::open(path).map_err(super::io_err(path))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| reject(format!("{rel} is not a zip ({e})")))?;
    let names: Vec<String> = zip
        .file_names()
        .filter_map(|n| n.ok().map(|n| n.into_owned()))
        .collect();
    if !names.iter().any(|n| n == BUNDLE_DLL) {
        return Err(reject(format!("{rel} does not contain {BUNDLE_DLL}")));
    }
    if names.iter().any(|n| n == "StardewModdingAPI.deps.json") {
        let mut text = String::new();
        zip.by_name("StardewModdingAPI.deps.json")
            .map_err(|e| reject(format!("{rel}: {e}")))?
            .take(DEPS_CAP)
            .read_to_string(&mut text)
            .map_err(|e| reject(format!("{rel}: deps.json: {e}")))?;
        if let Some(v) = deps_version(&text) {
            if v != version {
                return Err(reject(format!(
                    "folder says version {version} but install.dat holds {v}"
                )));
            }
        }
    }
    Ok(())
}

/// Validates an extracted SMAPI release. Any mismatch is a hard reject.
pub fn inspect(extracted: &Path) -> Result<Layout, LoaderError> {
    let tops: Vec<String> = fs::read_dir(extracted)
        .map_err(super::io_err(extracted))?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| folder_version(n).is_some())
        .collect();
    let [top] = tops.as_slice() else {
        return Err(reject(format!(
            "expected one 'SMAPI <version> installer' folder, found {}",
            tops.len()
        )));
    };
    let version = folder_version(top).expect("filtered").to_string();
    let root = extracted.join(top);
    for (installer, dat) in [
        (LINUX_INSTALLER, "internal/linux/install.dat"),
        (WINDOWS_INSTALLER, "internal/windows/install.dat"),
    ] {
        if !is_regular(&root.join(installer)) {
            return Err(reject(format!("{installer} is missing")));
        }
        check_bundle(&root.join(dat), &version)?;
    }
    // macOS is optional, but when shipped it must be sound too.
    let mac = root.join("internal/macOS/install.dat");
    if mac.exists() {
        check_bundle(&mac, &version)?;
    }
    Ok(Layout {
        top: top.clone(),
        version,
    })
}

/// First `HEAD_CAP` bytes of a game-root file, without following a link
/// or blocking on a FIFO. `None` when it is not a readable regular file.
fn head(game_dir: &Path, rel: &str) -> Option<Vec<u8>> {
    let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
    let f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits() as i32)
        .open(game_dir.join(rel))
        .ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut buf = Vec::new();
    f.take(HEAD_CAP).read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn is_smapi_launcher(game_dir: &Path) -> bool {
    head(game_dir, "StardewValley").is_some_and(|b| contains(&b, LAUNCHER_MARKER))
}

/// A script (`#!`) that is not SMAPI's launcher: the vanilla launcher.
fn is_vanilla_launcher(game_dir: &Path, rel: &str) -> bool {
    head(game_dir, rel).is_some_and(|b| b.starts_with(b"#!") && !contains(&b, LAUNCHER_MARKER))
}

fn is_elf(game_dir: &Path, rel: &str) -> bool {
    head(game_dir, rel).is_some_and(|b| b.starts_with(b"\x7fELF"))
}

/// Folders under `Mods/` (depth <= 4) whose `manifest.json` declares one
/// of SMAPI's bundled UniqueIDs; SMAPI replaces those wherever they are.
pub fn bundled_mod_dirs(game_dir: &Path) -> Vec<String> {
    let mods = game_dir.join("Mods");
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(&mods)
        .min_depth(2)
        .max_depth(4)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if !e.file_type().is_file() || !e.file_name().eq_ignore_ascii_case("manifest.json") {
            continue;
        }
        let Some(rel) = e.path().strip_prefix(game_dir).ok().and_then(Path::to_str) else {
            continue;
        };
        let text = head(game_dir, rel).unwrap_or_default().to_ascii_lowercase();
        let bundled = BUNDLED_MOD_IDS
            .iter()
            .any(|id| contains(&text, format!("\"{}\"", id.to_ascii_lowercase()).as_bytes()));
        if bundled {
            if let Some(dir) = Path::new(rel).parent().and_then(Path::to_str) {
                out.push(dir.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The touch scope for this game folder: the fixed list plus every
/// bundled-mod folder SMAPI would replace.
pub fn touch_scope(game_dir: &Path) -> Vec<Glob> {
    let mut v: Vec<Glob> = TOUCH_SCOPE.iter().map(|g| Glob::new(*g)).collect();
    for d in bundled_mod_dirs(game_dir) {
        let lit = Glob::literal(&d);
        v.push(Glob::new(format!("{}/**", lit.0)));
        v.push(lit);
    }
    v
}

/// Changes outside the game folder (R1 §5), shown as unrecoverable.
pub fn external_side_effects(runtime: &Runtime) -> Vec<String> {
    let data = match runtime {
        Runtime::Proton(p) => p
            .compat_data
            .join("pfx/drive_c/users/steamuser/AppData/Roaming/StardewValley")
            .display()
            .to_string(),
        _ => "$HOME/.config/StardewValley".to_string(),
    };
    let mut v = vec![
        format!("moves {data}/Saves/.smapi to {data}/.smapi (deletes the old one if both exist)"),
        format!("deletes {data}/ErrorLogs"),
    ];
    match runtime {
        // Wine's temp folder comes from the prefix, not TMPDIR.
        Runtime::Proton(p) => v.push(format!(
            "leaves an extracted copy of the installer (about 40 MB) in {}/SMAPI-installer-* (removed by Vessel cleanup)",
            p.compat_data
                .join("pfx/drive_c/users/steamuser/AppData/Local/Temp")
                .display()
        )),
        _ => v.push(
            "leaves an extracted copy of the installer (about 40 MB) in the run's private temp folder (removed by Vessel cleanup)"
                .into(),
        ),
    }
    v
}

/// Files that exist but are not tracked mean a hand-made install.
fn manual_evidence(game_dir: &Path) -> Vec<String> {
    [BUNDLE_DLL, "StardewValley-original"]
        .iter()
        .filter(|p| game_dir.join(p).symlink_metadata().is_ok())
        .map(|p| p.to_string())
        .collect()
}

/// The SMAPI plugin. Stateless.
#[derive(Debug, Clone, Copy, Default)]
pub struct Smapi;

impl Smapi {
    fn installer(
        &self,
        layout: &Layout,
        runtime: &Runtime,
        action: &str,
    ) -> Result<InstallerSpec, LoaderError> {
        let (rel, host, game) = match runtime {
            Runtime::Native => (LINUX_INSTALLER, Host::Native, Arg::GamePath),
            Runtime::Proton(p) => (WINDOWS_INSTALLER, Host::Proton(p.clone()), Arg::GamePathWin),
            Runtime::Unknown => {
                return Err(LoaderError::UnknownRuntime(
                    "set Stardew Valley's runtime to native or Proton".into(),
                ))
            }
        };
        Ok(InstallerSpec {
            target: InstallerTarget {
                rel_path: format!("{}/{rel}", layout.top),
                kind: TargetKind::Exe,
            },
            args: vec![
                Arg::Lit(action.into()),
                Arg::Lit("--no-prompt".into()),
                Arg::Lit("--game-path".into()),
                game,
            ],
            host,
            // `--no-prompt` error paths return 0 (R1).
            exit_code_reliable: false,
            // It extracts install.dat to $TMPDIR and never deletes it.
            private_tmp: true,
        })
    }

    fn mode_fixes(runtime: &Runtime, op: Op) -> Option<Step> {
        if !matches!(runtime, Runtime::Native) {
            return None;
        }
        let mut paths = vec!["StardewValley"];
        if op != Op::Uninstall {
            paths.push("StardewModdingAPI");
        }
        Some(Step::FixModes(
            paths
                .into_iter()
                .map(|p| ModeFix {
                    path: p.into(),
                    mode: 0o755,
                })
                .collect(),
        ))
    }

    fn launch(runtime: &Runtime) -> Option<LaunchChange> {
        matches!(runtime, Runtime::Proton(_)).then(|| LaunchChange {
            exe: Some("StardewModdingAPI.exe".into()),
            ..LaunchChange::default()
        })
    }
}

fn verify_err(op: Op, reason: impl Into<String>, steam_hint: bool) -> LoaderError {
    LoaderError::VerifyFailed {
        op: op.as_str(),
        reason: reason.into(),
        steam_hint,
    }
}

fn has_file(post: &Manifest, rel: &str) -> bool {
    matches!(post.entries.get(rel), Some(Entry::File { .. }))
}

impl LoaderPlugin for Smapi {
    fn key(&self) -> &str {
        KEY
    }

    /// SMAPI replaces `StardewValley` and keeps the vanilla one as
    /// `StardewValley-original`, which its installer restores first.
    fn vanilla(&self) -> Option<VanillaSpec> {
        Some(VanillaSpec {
            file: "StardewValley".into(),
            copies: vec!["StardewValley-original".into()],
        })
    }

    /// The signature is the release folder plus an `internal/` tree; full
    /// validation happens in `plan`, so a broken SMAPI archive is rejected
    /// loudly instead of being installed as a mod.
    fn matches(&self, _game: &GameCtx, probe: &ArchiveProbe) -> Option<LoaderMatch> {
        if probe.nexus_mod_id.is_some_and(|id| id != NEXUS_MOD_ID) {
            return None;
        }
        let version = probe.entries.iter().find_map(|e| {
            let (top, rest) = e.split_once('/')?;
            let v = folder_version(top)?;
            rest.starts_with("internal/").then(|| v.to_string())
        })?;
        Some(LoaderMatch {
            loader_key: KEY.into(),
            version: Some(version),
        })
    }

    fn provenance(&self, _game: &GameCtx, m: &LoaderMatch, origin: &DownloadOrigin) -> Provenance {
        let unverified = |reason: &str| Provenance::Unverified {
            origin: origin.clone(),
            reason: reason.into(),
        };
        match origin {
            DownloadOrigin::Nexus { domain, mod_id, .. }
                if domain == NEXUS_DOMAIN && *mod_id == NEXUS_MOD_ID =>
            {
                Provenance::Verified {
                    origin: origin.clone(),
                }
            }
            DownloadOrigin::Nexus { .. } => {
                unverified("not SMAPI's Nexus page (stardewvalley mod 2400)")
            }
            DownloadOrigin::GitHub { repo, tag, asset } => {
                let tag_v = tag.trim_start_matches('v');
                let asset_ok = asset.starts_with("SMAPI-")
                    && asset.ends_with(".zip")
                    && m.version.as_deref().is_none_or(|v| v == tag_v);
                if repo.eq_ignore_ascii_case(GITHUB_REPO) && asset_ok {
                    Provenance::Verified {
                        origin: origin.clone(),
                    }
                } else {
                    unverified("not a Pathoschild/SMAPI release asset")
                }
            }
            DownloadOrigin::Local { .. } => unverified("the archive was not downloaded by Vessel"),
        }
    }

    fn plan(
        &self,
        op: Op,
        game: &GameCtx,
        runtime: &Runtime,
        extracted: Option<&Path>,
    ) -> Result<LoaderPlan, LoaderError> {
        if matches!(runtime, Runtime::Unknown) {
            return Err(LoaderError::UnknownRuntime(
                "set Stardew Valley's runtime to native or Proton".into(),
            ));
        }
        let touch_scope = touch_scope(&game.game_dir);
        for o in &game.occupied {
            if o.owner == Owner::Deployed && touch_scope.iter().any(|g| g.matches(&o.path)) {
                return Err(LoaderError::Conflict {
                    path: o.path.clone(),
                    owner: Owner::Deployed,
                });
            }
        }
        let external_side_effects = external_side_effects(runtime);
        let Some(extracted) = extracted else {
            if op != Op::Uninstall {
                return Err(LoaderError::ArchiveRequired { op: op.as_str() });
            }
            return Ok(LoaderPlan {
                op: Some(op),
                steps: Vec::new(),
                touch_scope,
                external_side_effects,
                expected_version: None,
                uses_cached_archive: true,
            });
        };
        if op == Op::Install {
            if let Some(first) = manual_evidence(&game.game_dir).into_iter().next() {
                tracing::info!(evidence = %first, "SMAPI already installed by hand");
                return Err(LoaderError::ManualInstallDetected(KEY.into()));
            }
        }
        let layout = inspect(extracted)?;
        let mut steps = Vec::new();
        let push_run =
            |steps: &mut Vec<Step>, action: &str, after: Op| -> Result<(), LoaderError> {
                steps.push(Step::RunInstaller(
                    self.installer(&layout, runtime, action)?,
                ));
                steps.extend(Self::mode_fixes(runtime, after));
                steps.push(Step::Verify { after });
                Ok(())
            };
        match op {
            Op::Install | Op::Upgrade | Op::Repair => push_run(&mut steps, "--install", op)?,
            Op::Uninstall => {
                push_run(&mut steps, "--uninstall", Op::Uninstall)?;
                steps.extend(Self::launch(runtime).map(Step::RevertLaunch));
            }
            Op::Adopt => {
                // Remove the hand-made install, prove the game is vanilla
                // again, then install managed.
                push_run(&mut steps, "--uninstall", Op::Uninstall)?;
                push_run(&mut steps, "--install", Op::Adopt)?;
            }
        }
        if op != Op::Uninstall {
            steps.extend(Self::launch(runtime).map(Step::Launch));
        }
        Ok(LoaderPlan {
            op: Some(op),
            steps,
            touch_scope,
            external_side_effects,
            expected_version: Some(layout.version),
            uses_cached_archive: false,
        })
    }

    fn detect(&self, game: &GameCtx, store: &Store) -> Result<LoaderStatus, LoaderError> {
        let dir = &game.game_dir;
        let Some(i) = store.active_loader_install(&game.game_id, KEY)? else {
            let evidence = manual_evidence(dir);
            return Ok(if evidence.is_empty() {
                LoaderStatus::NotInstalled
            } else {
                LoaderStatus::ManualDetected { evidence }
            });
        };
        if InstallStatus::parse(&i.status) != Some(InstallStatus::Installed) {
            return Ok(LoaderStatus::Busy {
                install_id: i.id,
                status: i.status,
            });
        }
        let drift = if !is_regular(&dir.join(BUNDLE_DLL)) {
            Some(format!("{BUNDLE_DLL} is missing"))
        } else if i.runtime == "native" && !is_smapi_launcher(dir) {
            Some(
                "StardewValley is no longer SMAPI's launcher (the game was updated or verified)"
                    .into(),
            )
        } else {
            None
        };
        Ok(match drift {
            Some(reason) => LoaderStatus::Drifted {
                install_id: i.id,
                version: i.version,
                reason,
            },
            None => LoaderStatus::Installed {
                install_id: i.id,
                version: i.version,
            },
        })
    }

    fn verify(
        &self,
        op: Op,
        game: &GameCtx,
        runtime: &Runtime,
        post: &Manifest,
    ) -> Result<(), LoaderError> {
        let dir = &game.game_dir;
        let native = matches!(runtime, Runtime::Native);
        let fail = |r: &str| Err(verify_err(op, r, false));
        if op == Op::Uninstall {
            if has_file(post, BUNDLE_DLL) || post.entries.contains_key("smapi-internal") {
                return fail("SMAPI files are still present");
            }
            if native {
                let vanilla = post.entries.contains_key("StardewValley")
                    && !post.entries.contains_key("StardewValley-original")
                    && is_vanilla_launcher(dir, "StardewValley")
                    && is_elf(dir, "Stardew Valley");
                if !vanilla {
                    return Err(verify_err(
                        op,
                        "the game's launcher is not vanilla; verify the game files in Steam, then try again",
                        true,
                    ));
                }
            } else if has_file(post, "StardewModdingAPI.exe") {
                return fail("StardewModdingAPI.exe is still present");
            }
            return Ok(());
        }
        if !has_file(post, BUNDLE_DLL)
            || !matches!(post.entries.get("smapi-internal"), Some(Entry::Dir { .. }))
        {
            return fail("StardewModdingAPI.dll or smapi-internal/ is missing");
        }
        if native {
            if !is_smapi_launcher(dir) {
                return fail("StardewValley was not replaced by SMAPI's launcher");
            }
            if !is_vanilla_launcher(dir, "StardewValley-original") {
                return fail("StardewValley-original is not the vanilla launcher");
            }
        } else if !has_file(post, "StardewModdingAPI.exe") {
            return fail("StardewModdingAPI.exe is missing");
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::loaders::records::{NewLoaderInstall, Strategy};
    use crate::loaders::snapshot::snapshot;
    use std::io::Write;
    use std::path::PathBuf;

    pub(crate) const TOP: &str = "SMAPI 4.5.2 installer";

    pub(crate) fn zip_with(path: &Path, files: &[(&str, &str)]) {
        let mut w = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (n, d) in files {
            w.start_file(*n, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(d.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }

    const DEPS: &str = r#"{"libraries":{"StardewModdingAPI/4.5.2":{"type":"project"}}}"#;

    /// A stub release: installers are empty files, install.dat a tiny zip.
    pub(crate) fn release(mac: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(TOP);
        let mut plats = vec!["linux", "windows"];
        if mac {
            plats.push("macOS");
        }
        for p in plats {
            let d = root.join("internal").join(p);
            fs::create_dir_all(&d).unwrap();
            let installer = if p == "windows" {
                "SMAPI.Installer.exe"
            } else {
                "SMAPI.Installer"
            };
            fs::write(d.join(installer), "stub").unwrap();
            zip_with(
                &d.join("install.dat"),
                &[
                    (BUNDLE_DLL, "stub"),
                    ("StardewModdingAPI.deps.json", DEPS),
                    ("unix-launcher.sh", "#!/bin/bash\nexec StardewModdingAPI\n"),
                ],
            );
        }
        fs::write(root.join("install on Linux.sh"), "stub").unwrap();
        fs::write(root.join("install on Windows.bat"), "stub").unwrap();
        dir
    }

    fn entries(dir: &Path) -> Vec<String> {
        walkdir::WalkDir::new(dir)
            .min_depth(1)
            .into_iter()
            .flatten()
            .map(|e| {
                e.path()
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    fn probe(dir: &Path, mod_id: Option<u64>) -> ArchiveProbe {
        ArchiveProbe {
            file_name: "SMAPI-4.5.2-installer.zip".into(),
            nexus_mod_id: mod_id,
            nexus_file_id: None,
            entries: entries(dir),
        }
    }

    /// A vanilla native game folder: launcher script plus the ELF.
    pub(crate) fn vanilla_game() -> tempfile::TempDir {
        let g = tempfile::tempdir().unwrap();
        fs::write(
            g.path().join("StardewValley"),
            "#!/bin/bash\n./\"Stardew Valley\" $@\n",
        )
        .unwrap();
        fs::write(g.path().join("Stardew Valley"), b"\x7fELF\x02rest").unwrap();
        fs::write(g.path().join("steam_appid.txt"), "413150").unwrap();
        fs::create_dir_all(g.path().join("Mods")).unwrap();
        g
    }

    fn ctx(game: &Path) -> GameCtx {
        GameCtx {
            game_id: "stardewvalley".into(),
            nexus_domain: Some("stardewvalley".into()),
            game_dir: game.to_path_buf(),
            steam_appid: Some(413150),
            occupied: Vec::new(),
        }
    }

    fn proton() -> Runtime {
        Runtime::Proton(ProtonEnv {
            proton: "/s/common/Proton/proton".into(),
            compat_data: "/s/compatdata/413150".into(),
            client_root: "/s".into(),
            app_id: 413150,
            slr: None,
        })
    }

    fn installers(plan: &LoaderPlan) -> Vec<&InstallerSpec> {
        plan.steps
            .iter()
            .filter_map(|s| match s {
                Step::RunInstaller(i) => Some(i),
                _ => None,
            })
            .collect()
    }

    /// Catches: the release not being recognised (with or without macOS),
    /// or another Nexus mod's archive being claimed.
    #[test]
    fn release_layouts_match() {
        let g = vanilla_game();
        for mac in [true, false] {
            let r = release(mac);
            let m = Smapi
                .matches(&ctx(g.path()), &probe(r.path(), Some(2400)))
                .unwrap();
            assert_eq!(m.version.as_deref(), Some("4.5.2"));
            assert_eq!(inspect(r.path()).unwrap().version, "4.5.2");
        }
        let r = release(false);
        assert!(Smapi
            .matches(&ctx(g.path()), &probe(r.path(), Some(1)))
            .is_none());
        let other = tempfile::tempdir().unwrap();
        fs::create_dir_all(other.path().join("ContentPatcher")).unwrap();
        fs::write(other.path().join("ContentPatcher/manifest.json"), "{}").unwrap();
        assert!(Smapi
            .matches(&ctx(g.path()), &probe(other.path(), None))
            .is_none());
    }

    /// Catches: a broken or tampered release being installed.
    #[test]
    fn bad_layouts_are_hard_rejects() {
        let g = vanilla_game();
        type Breaker = Box<dyn Fn(&Path)>;
        let cases: Vec<Breaker> = vec![
            Box::new(|r| fs::remove_dir_all(r.join("internal/windows")).unwrap()),
            Box::new(|r| fs::remove_file(r.join(LINUX_INSTALLER)).unwrap()),
            Box::new(|r| fs::write(r.join("internal/linux/install.dat"), "not a zip").unwrap()),
            Box::new(|r| {
                zip_with(
                    &r.join("internal/windows/install.dat"),
                    &[("other.dll", "x")],
                )
            }),
            Box::new(|r| {
                zip_with(
                    &r.join("internal/linux/install.dat"),
                    &[
                        (BUNDLE_DLL, "x"),
                        (
                            "StardewModdingAPI.deps.json",
                            &DEPS.replace("4.5.2", "4.0.0"),
                        ),
                    ],
                )
            }),
            Box::new(|r| fs::write(r.join("internal/macOS/install.dat"), "not a zip").unwrap()),
        ];
        for (i, break_it) in cases.iter().enumerate() {
            let rel = release(true);
            break_it(&rel.path().join(TOP));
            let err = Smapi
                .plan(
                    Op::Install,
                    &ctx(g.path()),
                    &Runtime::Native,
                    Some(rel.path()),
                )
                .unwrap_err();
            assert!(
                matches!(err, LoaderError::LayoutMismatch { .. }),
                "case {i}: {err:?}"
            );
        }
    }

    /// Catches: running the vendor scripts, the wrong argv, an exit code
    /// trusted that lies, or the exec-bit race left unfixed.
    #[test]
    fn native_plan_runs_the_linux_installer() {
        let g = vanilla_game();
        let r = release(true);
        let plan = Smapi
            .plan(
                Op::Install,
                &ctx(g.path()),
                &Runtime::Native,
                Some(r.path()),
            )
            .unwrap();
        let [spec] = installers(&plan)[..] else {
            panic!("{plan:?}")
        };
        assert_eq!(
            spec.target,
            InstallerTarget {
                rel_path: format!("{TOP}/internal/linux/SMAPI.Installer"),
                kind: TargetKind::Exe
            }
        );
        assert_eq!(spec.host, Host::Native);
        assert_eq!(
            spec.args,
            [
                Arg::Lit("--install".into()),
                Arg::Lit("--no-prompt".into()),
                Arg::Lit("--game-path".into()),
                Arg::GamePath
            ]
        );
        assert!(!spec.exit_code_reliable && spec.private_tmp);
        assert!(plan.steps.contains(&Step::FixModes(vec![
            ModeFix {
                path: "StardewValley".into(),
                mode: 0o755
            },
            ModeFix {
                path: "StardewModdingAPI".into(),
                mode: 0o755
            },
        ])));
        assert_eq!(
            plan.steps.last(),
            Some(&Step::Verify { after: Op::Install })
        );
        assert_eq!(plan.expected_version.as_deref(), Some("4.5.2"));
        assert!(!plan.steps.iter().any(|s| matches!(s, Step::Launch(_))));
    }

    #[test]
    fn proton_plan_runs_the_windows_installer_and_sets_launch() {
        let g = vanilla_game();
        let r = release(false);
        let plan = Smapi
            .plan(Op::Install, &ctx(g.path()), &proton(), Some(r.path()))
            .unwrap();
        let [spec] = installers(&plan)[..] else {
            panic!("{plan:?}")
        };
        assert_eq!(
            spec.target.rel_path,
            format!("{TOP}/internal/windows/SMAPI.Installer.exe")
        );
        assert_eq!(spec.target.kind, TargetKind::Exe);
        assert!(matches!(spec.host, Host::Proton(_)));
        assert_eq!(spec.args[3], Arg::GamePathWin);
        assert!(!plan.steps.iter().any(|s| matches!(s, Step::FixModes(_))));
        assert_eq!(
            plan.steps.last(),
            Some(&Step::Launch(LaunchChange {
                exe: Some("StardewModdingAPI.exe".into()),
                ..LaunchChange::default()
            }))
        );
        assert!(plan
            .external_side_effects
            .iter()
            .any(|s| s.contains("drive_c/users/steamuser/AppData/Roaming/StardewValley")));
    }

    /// Catches: uninstall demanding an archive the caller does not have,
    /// instead of asking for the cached one.
    #[test]
    fn uninstall_plans_with_the_cached_archive() {
        let g = vanilla_game();
        let skeleton = Smapi
            .plan(Op::Uninstall, &ctx(g.path()), &proton(), None)
            .unwrap();
        assert!(skeleton.uses_cached_archive && skeleton.steps.is_empty());
        assert!(!skeleton.touch_scope.is_empty());
        assert!(matches!(
            Smapi.plan(Op::Install, &ctx(g.path()), &proton(), None),
            Err(LoaderError::ArchiveRequired { .. })
        ));
        let r = release(true);
        let plan = Smapi
            .plan(Op::Uninstall, &ctx(g.path()), &proton(), Some(r.path()))
            .unwrap();
        assert_eq!(installers(&plan)[0].args[0], Arg::Lit("--uninstall".into()));
        assert!(matches!(plan.steps.last(), Some(Step::RevertLaunch(_))));
        assert!(!plan.uses_cached_archive);
    }

    /// Catches: adopt reinstalling over a game that did not return to vanilla.
    #[test]
    fn adopt_uninstalls_verifies_then_installs() {
        let g = vanilla_game();
        let r = release(true);
        let plan = Smapi
            .plan(Op::Adopt, &ctx(g.path()), &Runtime::Native, Some(r.path()))
            .unwrap();
        let kinds: Vec<String> = plan
            .steps
            .iter()
            .map(|s| match s {
                Step::RunInstaller(i) => match &i.args[0] {
                    Arg::Lit(a) => a.clone(),
                    _ => unreachable!(),
                },
                Step::FixModes(_) => "fix".into(),
                Step::Verify { after } => format!("verify-{}", after.as_str()),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "--uninstall",
                "fix",
                "verify-uninstall",
                "--install",
                "fix",
                "verify-adopt"
            ]
        );
    }

    /// Catches: SMAPI's deletions (steam_appid.txt, obsolete DLLs) and
    /// bundled-mod replacements falling outside the backed-up scope, and a
    /// deployed copy of a bundled mod being overwritten silently.
    #[test]
    fn touch_scope_covers_r1_paths_and_bundled_mods() {
        let g = vanilla_game();
        let custom = g.path().join("Mods/Core/CC");
        fs::create_dir_all(&custom).unwrap();
        fs::write(
            custom.join("manifest.json"),
            r#"{ "UniqueID": "SMAPI.ConsoleCommands" }"#,
        )
        .unwrap();
        let other = g.path().join("Mods/CP");
        fs::create_dir_all(&other).unwrap();
        fs::write(
            other.join("manifest.json"),
            r#"{ "UniqueID": "Pathoschild.ContentPatcher" }"#,
        )
        .unwrap();
        let r = release(true);
        let plan = Smapi
            .plan(
                Op::Install,
                &ctx(g.path()),
                &Runtime::Native,
                Some(r.path()),
            )
            .unwrap();
        let in_scope = |p: &str| plan.touch_scope.iter().any(|g| g.matches(p));
        for p in [
            "StardewValley",
            "StardewValley-original",
            "StardewModdingAPI",
            "StardewModdingAPI.dll",
            "StardewModdingAPI.exe.config",
            "StardewModdingAPI.Toolkit.CoreInterfaces.xml",
            "StardewModdingAPI-x64.exe",
            "smapi-internal/config.user.json",
            "steam_appid.txt",
            "Stardew Valley.deps.json",
            "0Harmony.dll",
            "Mods/ConsoleCommands/ConsoleCommands.dll",
            "Mods/SaveBackup/manifest.json",
            "Mods/ErrorHandler/a.dll",
            "Mods/Core/CC/ConsoleCommands.dll",
        ] {
            assert!(in_scope(p), "{p}");
        }
        assert!(!in_scope("Mods/CP/manifest.json"));
        assert!(!in_scope("Stardew Valley"));

        let mut c = ctx(g.path());
        c.occupied.push(Occupied {
            path: "Mods/ConsoleCommands/ConsoleCommands.dll".into(),
            owner: Owner::Deployed,
        });
        assert!(matches!(
            Smapi.plan(Op::Install, &c, &Runtime::Native, Some(r.path())),
            Err(LoaderError::Conflict {
                owner: Owner::Deployed,
                ..
            })
        ));
        let fx = external_side_effects(&Runtime::Native);
        assert!(fx
            .iter()
            .any(|s| s.contains(".config/StardewValley/Saves/.smapi")));
        assert!(fx.iter().any(|s| s.contains("ErrorLogs")));
        assert!(fx.iter().any(|s| s.contains("temp")));
    }

    #[test]
    fn classify_vanilla_outcomes() {
        assert_eq!(classify_vanilla("p", "p", "v"), VanillaState::Intact);
        assert_eq!(classify_vanilla("v", "p", "v"), VanillaState::SameVanilla);
        assert_eq!(classify_vanilla("n", "p", "v"), VanillaState::SteamUpdated);
    }

    fn store() -> Store {
        let s = Store::open_in_memory().unwrap();
        s.conn()
            .execute(
                "INSERT INTO games (id, name, added_at) VALUES ('stardewvalley', 'SDV', 0)",
                [],
            )
            .unwrap();
        s
    }

    fn install_row(s: &Store, dir: &Path) -> i64 {
        s.insert_loader_install(&NewLoaderInstall {
            game_id: "stardewvalley".into(),
            game_dir: dir.display().to_string(),
            loader_key: KEY.into(),
            version: Some("4.5.2".into()),
            runtime: "native".into(),
            strategy: Strategy::Installer,
            status: InstallStatus::Installed,
            nexus_mod_id: Some(2400),
            nexus_file_id: None,
            origin: "nexus".into(),
            archive_sha256: None,
            archive_path: None,
            vanilla_sha256: None,
            game_build: None,
        })
        .unwrap()
    }

    /// What SMAPI's installer leaves in a native game folder.
    pub(crate) fn smapi_installed(g: &Path) {
        fs::rename(g.join("StardewValley"), g.join("StardewValley-original")).unwrap();
        fs::write(
            g.join("StardewValley"),
            "#!/bin/bash\nexec ./StardewModdingAPI \"$@\"\n",
        )
        .unwrap();
        fs::write(g.join(BUNDLE_DLL), "dll").unwrap();
        fs::write(g.join("StardewModdingAPI"), "apphost").unwrap();
        fs::create_dir_all(g.join("smapi-internal")).unwrap();
        fs::remove_file(g.join("steam_appid.txt")).unwrap();
    }

    /// Catches: manual installs not reported (and not blocking a normal
    /// install), and a Steam update that reverted the launcher not shown
    /// as drift.
    #[test]
    fn detect_reports_manual_managed_and_drift() {
        let g = vanilla_game();
        let s = store();
        let c = ctx(g.path());
        assert_eq!(Smapi.detect(&c, &s).unwrap(), LoaderStatus::NotInstalled);
        smapi_installed(g.path());
        assert!(
            matches!(Smapi.detect(&c, &s).unwrap(), LoaderStatus::ManualDetected { evidence } if evidence.len() == 2)
        );
        let r = release(true);
        assert!(matches!(
            Smapi.plan(Op::Install, &c, &Runtime::Native, Some(r.path())),
            Err(LoaderError::ManualInstallDetected(_))
        ));
        assert!(Smapi
            .plan(Op::Adopt, &c, &Runtime::Native, Some(r.path()))
            .is_ok());
        let id = install_row(&s, g.path());
        assert!(
            matches!(Smapi.detect(&c, &s).unwrap(), LoaderStatus::Installed { install_id, .. } if install_id == id)
        );
        // Steam "verify files" puts the vanilla launcher back.
        fs::write(
            g.path().join("StardewValley"),
            "#!/bin/bash\n./\"Stardew Valley\" $@\n",
        )
        .unwrap();
        assert!(matches!(
            Smapi.detect(&c, &s).unwrap(),
            LoaderStatus::Drifted { .. }
        ));
    }

    fn post(g: &Path) -> Manifest {
        snapshot(g, &touch_scope(g)).unwrap()
    }

    /// Catches: a silent installer failure (exit 0) being taken as success,
    /// and an uninstall that left SMAPI's launcher in place passing.
    #[test]
    fn verify_install_and_uninstall_native() {
        let g = vanilla_game();
        let c = ctx(g.path());
        let err = Smapi
            .verify(Op::Install, &c, &Runtime::Native, &post(g.path()))
            .unwrap_err();
        assert!(matches!(err, LoaderError::VerifyFailed { .. }));
        assert_eq!(err.code(), "loader_verify_failed", "no Steam action");
        assert!(Smapi
            .verify(Op::Uninstall, &c, &Runtime::Native, &post(g.path()))
            .is_ok());

        smapi_installed(g.path());
        Smapi
            .verify(Op::Install, &c, &Runtime::Native, &post(g.path()))
            .unwrap();
        Smapi
            .verify(Op::Adopt, &c, &Runtime::Native, &post(g.path()))
            .unwrap();
        assert!(Smapi
            .verify(Op::Uninstall, &c, &Runtime::Native, &post(g.path()))
            .is_err());

        // Uninstall that removed SMAPI but left its launcher: blocked with a hint.
        fs::remove_file(g.path().join(BUNDLE_DLL)).unwrap();
        fs::remove_dir_all(g.path().join("smapi-internal")).unwrap();
        fs::remove_file(g.path().join("StardewValley-original")).unwrap();
        let err = Smapi
            .verify(Op::Uninstall, &c, &Runtime::Native, &post(g.path()))
            .unwrap_err();
        assert!(
            err.to_string().contains("verify the game files in Steam"),
            "{err}"
        );
        assert_eq!(err.code(), "loader_verify_failed_steam");
        // ... or where the ELF is gone.
        fs::write(
            g.path().join("StardewValley"),
            "#!/bin/bash\n./\"Stardew Valley\" $@\n",
        )
        .unwrap();
        fs::write(g.path().join("Stardew Valley"), "#!/bin/bash\n").unwrap();
        assert!(Smapi
            .verify(Op::Uninstall, &c, &Runtime::Native, &post(g.path()))
            .is_err());
    }

    #[test]
    fn verify_proton_needs_the_windows_entry_point() {
        let g = tempfile::tempdir().unwrap();
        let c = ctx(g.path());
        fs::write(g.path().join(BUNDLE_DLL), "dll").unwrap();
        fs::create_dir_all(g.path().join("smapi-internal")).unwrap();
        assert!(Smapi
            .verify(Op::Install, &c, &proton(), &post(g.path()))
            .is_err());
        fs::write(g.path().join("StardewModdingAPI.exe"), "exe").unwrap();
        Smapi
            .verify(Op::Install, &c, &proton(), &post(g.path()))
            .unwrap();
        assert!(Smapi
            .verify(Op::Uninstall, &c, &proton(), &post(g.path()))
            .is_err());
    }

    /// Catches: archives not downloaded by Vessel from SMAPI's own sources
    /// being reported as verified.
    #[test]
    fn provenance_rules() {
        let g = vanilla_game();
        let c = ctx(g.path());
        let m = LoaderMatch {
            loader_key: KEY.into(),
            version: Some("4.5.2".into()),
        };
        let nexus = |domain: &str, mod_id| DownloadOrigin::Nexus {
            domain: domain.into(),
            mod_id,
            file_id: 1,
        };
        let gh = |repo: &str, tag: &str, asset: &str| DownloadOrigin::GitHub {
            repo: repo.into(),
            tag: tag.into(),
            asset: asset.into(),
        };
        assert!(Smapi
            .provenance(&c, &m, &nexus("stardewvalley", 2400))
            .is_verified());
        assert!(!Smapi
            .provenance(&c, &m, &nexus("stardewvalley", 2401))
            .is_verified());
        assert!(!Smapi
            .provenance(&c, &m, &nexus("skyrim", 2400))
            .is_verified());
        assert!(Smapi
            .provenance(
                &c,
                &m,
                &gh("Pathoschild/SMAPI", "4.5.2", "SMAPI-4.5.2-installer.zip")
            )
            .is_verified());
        assert!(
            !Smapi
                .provenance(
                    &c,
                    &m,
                    &gh("Pathoschild/SMAPI", "4.4.0", "SMAPI-4.4.0-installer.zip")
                )
                .is_verified(),
            "tag must match the archive version"
        );
        assert!(!Smapi
            .provenance(
                &c,
                &m,
                &gh("evil/SMAPI", "4.5.2", "SMAPI-4.5.2-installer.zip")
            )
            .is_verified());
        let local = DownloadOrigin::Local {
            path: PathBuf::from("/d/SMAPI.zip"),
        };
        assert!(!Smapi.provenance(&c, &m, &local).is_verified());
    }

    /// Catches: process spawning sneaking into the plugin.
    #[test]
    fn plugin_never_spawns() {
        let src = include_str!("smapi.rs");
        let body = &src[..src.find("#[cfg(test)]").unwrap()];
        for needle in ["Command::", "process::Command", "spawn("] {
            assert!(!body.contains(needle), "{needle}");
        }
    }
}
