// SPDX-License-Identifier: GPL-3.0-or-later
//! Declarative loaders: `[[loaders]]` tables in a game TOML that deploy a
//! payload into the game root and change the launch, without running any
//! downloaded code.
//!
//! ```toml
//! [[loaders]]
//! key = "bepinex"
//! name = "BepInEx"
//! required_paths = ["winhttp.dll", "BepInEx/core/BepInEx.Preloader.dll"]
//! [loaders.payload]
//! exclude = ["changelog.txt"]
//! modes = [{ glob = "run_bepinex.sh", mode = 0o755 }]
//! [loaders.launch]
//! dll_overrides = ["winhttp"]
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::glob::Glob;
use super::records::InstallStatus;
use super::types::*;
use super::{io_err, LoaderError, LoaderPlugin};
use crate::deploy::confine;
use crate::store::Store;

/// One `[[loaders]]` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoaderDef {
    /// Stable key, equal to the requirements tool key.
    pub key: String,
    pub name: String,
    /// Nexus mod id on the game's domain. When both this and the archive's
    /// mod id are known they must agree; a match is then also verifiable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nexus_mod_id: Option<u64>,
    /// Globs (archive-root relative) that must each match an entry or a
    /// folder of the archive. At least one is required.
    pub required_paths: Vec<Glob>,
    /// Globs that must match nothing in the archive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbidden_paths: Vec<Glob>,
    /// Game-root files whose presence shows the loader is installed (used
    /// to spot installs made outside Vessel).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detect: Vec<String>,
    #[serde(default)]
    pub payload: PayloadRecipe,
    #[serde(default, skip_serializing_if = "LaunchChange::is_empty")]
    pub launch: LaunchChange,
    /// Game-root file whose build the loader is tied to (version coupling).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub couples_to: Option<String>,
}

fn rel_path(what: &str, v: &str) -> Result<(), String> {
    match confine::components(v) {
        Ok(c) if !c.is_empty() => Ok(()),
        Ok(_) => Err(format!("{what} must not be empty")),
        Err(m) => Err(format!("{what} '{v}': {m}")),
    }
}

impl LoaderDef {
    /// Checks everything serde cannot: keys, globs, paths, modes, names.
    pub fn validate(&self) -> Result<(), String> {
        let k = &self.key;
        if k.is_empty()
            || !k
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        {
            return Err(format!("loader key '{k}' must be [a-z0-9_-]+"));
        }
        let ctx = |m: String| format!("loader '{k}': {m}");
        if self.required_paths.is_empty() {
            return Err(ctx("required_paths must not be empty".into()));
        }
        let p = &self.payload;
        let globs = self
            .required_paths
            .iter()
            .chain(&self.forbidden_paths)
            .chain(&p.exclude)
            .chain(p.select.iter())
            .chain(p.modes.iter().map(|m| &m.glob));
        for g in globs {
            g.check().map_err(ctx)?;
        }
        for m in &p.modes {
            // Plain permission bits only: no setuid, setgid or sticky.
            if m.mode > 0o777 {
                return Err(ctx(format!(
                    "mode {:o} for '{}' is not 0..=0o777",
                    m.mode, m.glob.0
                )));
            }
        }
        for m in &p.map {
            rel_path("map.from", &m.from).map_err(ctx)?;
            rel_path("map.to", &m.to).map_err(ctx)?;
        }
        for d in &self.detect {
            rel_path("detect", d).map_err(ctx)?;
        }
        if let Some(c) = &self.couples_to {
            rel_path("couples_to", c).map_err(ctx)?;
        }
        let l = &self.launch;
        if let Some(e) = &l.exe {
            rel_path("launch.exe", e).map_err(ctx)?;
        }
        for d in &l.dll_overrides {
            let ok = !d.is_empty()
                && d.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
            if !ok {
                return Err(ctx(format!("dll override '{d}' is not a plain DLL name")));
            }
        }
        for name in l.env.keys() {
            let mut b = name.bytes();
            let ok = b
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
                && b.all(|c| c.is_ascii_alphanumeric() || c == b'_');
            if !ok {
                return Err(ctx(format!("env name '{name}' is invalid")));
            }
        }
        Ok(())
    }
}

/// A [`LoaderPlugin`] backed by a [`LoaderDef`].
#[derive(Debug, Clone)]
pub struct DeclarativeLoader {
    def: LoaderDef,
}

impl DeclarativeLoader {
    pub fn new(def: LoaderDef) -> Self {
        Self { def }
    }

    pub fn def(&self) -> &LoaderDef {
        &self.def
    }

    fn payload_err(&self, message: impl Into<String>) -> LoaderError {
        LoaderError::Payload {
            key: self.def.key.clone(),
            message: message.into(),
        }
    }

    /// The folder `payload.select` names, or `extracted` itself.
    fn payload_root(&self, extracted: &Path) -> Result<PathBuf, LoaderError> {
        let Some(sel) = &self.def.payload.select else {
            return Ok(extracted.to_path_buf());
        };
        let mut hits = Vec::new();
        for e in walkdir::WalkDir::new(extracted)
            .min_depth(1)
            .follow_links(false)
        {
            let e = e.map_err(|e| walk_err(extracted, e))?;
            if e.file_type().is_dir()
                && rel_of(extracted, e.path()).is_some_and(|r| sel.matches(&r))
            {
                hits.push(e.into_path());
            }
        }
        match hits.len() {
            1 => Ok(hits.remove(0)),
            0 => Err(self.payload_err(format!("select '{}' matched no folder", sel.0))),
            n => Err(self.payload_err(format!("select '{}' matched {n} folders", sel.0))),
        }
    }

    fn map_dest(&self, rel: &str) -> String {
        for m in &self.def.payload.map {
            let from = m.from.trim_matches('/');
            let to = m.to.trim_matches('/');
            if rel == from {
                return to.to_string();
            }
            if let Some(rest) = rel.strip_prefix(from).and_then(|r| r.strip_prefix('/')) {
                return format!("{to}/{rest}");
            }
        }
        rel.to_string()
    }

    fn deploy_files(&self, game: &GameCtx, extracted: &Path) -> Result<DeployFiles, LoaderError> {
        let recipe = &self.def.payload;
        let root = self.payload_root(extracted)?;
        let taken: HashMap<String, &Owner> = game
            .occupied
            .iter()
            .map(|o| (o.path.to_ascii_lowercase(), &o.owner))
            .collect();
        let mut seen: HashMap<String, String> = HashMap::new();
        let mut files = Vec::new();
        let walk = walkdir::WalkDir::new(&root)
            .min_depth(1)
            .follow_links(false)
            .sort_by_file_name();
        for e in walk {
            let e = e.map_err(|e| walk_err(&root, e))?;
            let rel = rel_of(&root, e.path()).ok_or_else(|| LoaderError::UnsafePath {
                path: e.path().display().to_string(),
                message: "not valid UTF-8".into(),
            })?;
            let ft = e.file_type();
            if ft.is_dir() {
                continue;
            }
            if recipe.exclude.iter().any(|g| g.matches_or_contains(&rel)) {
                continue;
            }
            if !ft.is_file() {
                // A link in the payload could point anywhere once placed.
                return Err(LoaderError::UnsafePath {
                    path: rel,
                    message: "payload entries must be regular files".into(),
                });
            }
            let dest = self.map_dest(&rel);
            confine::components(&dest).map_err(|m| LoaderError::UnsafePath {
                path: dest.clone(),
                message: m.into(),
            })?;
            let key = dest.to_ascii_lowercase();
            if let Some(owner) = taken.get(&key) {
                return Err(LoaderError::Conflict {
                    path: dest,
                    owner: (*owner).clone(),
                });
            }
            if let Some(prev) = seen.insert(key, rel.clone()) {
                return Err(self.payload_err(format!("'{prev}' and '{rel}' both map to '{dest}'")));
            }
            let mode = recipe
                .modes
                .iter()
                .find(|m| m.glob.matches(&dest))
                .map(|m| m.mode);
            files.push(PlannedFile {
                source: e.into_path(),
                dest,
                mode,
            });
        }
        if files.is_empty() {
            return Err(self.payload_err("payload is empty"));
        }
        Ok(DeployFiles {
            recipe: recipe.clone(),
            files,
        })
    }
}

fn rel_of(root: &Path, p: &Path) -> Option<String> {
    p.strip_prefix(root).ok()?.to_str().map(str::to_string)
}

fn walk_err(root: &Path, e: walkdir::Error) -> LoaderError {
    let path = e
        .path()
        .map_or_else(|| root.to_path_buf(), Path::to_path_buf);
    io_err(&path)(e.into())
}

impl LoaderPlugin for DeclarativeLoader {
    fn key(&self) -> &str {
        &self.def.key
    }

    fn matches(&self, _game: &GameCtx, probe: &ArchiveProbe) -> Option<LoaderMatch> {
        if let (Some(want), Some(got)) = (self.def.nexus_mod_id, probe.nexus_mod_id) {
            if want != got {
                return None;
            }
        }
        let any = |g: &Glob| probe.entries.iter().any(|e| g.matches_or_contains(e));
        if !self.def.required_paths.iter().all(any) || self.def.forbidden_paths.iter().any(any) {
            return None;
        }
        Some(LoaderMatch {
            loader_key: self.def.key.clone(),
            version: None,
        })
    }

    fn provenance(&self, game: &GameCtx, _m: &LoaderMatch, origin: &DownloadOrigin) -> Provenance {
        let unverified = |reason: &str| Provenance::Unverified {
            origin: origin.clone(),
            reason: reason.into(),
        };
        match origin {
            DownloadOrigin::Nexus { domain, mod_id, .. } => {
                let same_game = game.nexus_domain.as_deref() == Some(domain.as_str());
                if same_game && self.def.nexus_mod_id == Some(*mod_id) {
                    Provenance::Verified {
                        origin: origin.clone(),
                    }
                } else {
                    unverified("Nexus mod is not the one this loader is published as")
                }
            }
            DownloadOrigin::GitHub { .. } => {
                unverified("no GitHub source is declared for this loader")
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
        let launch = self.def.launch.for_runtime(runtime);
        if op == Op::Uninstall {
            let mut steps = Vec::new();
            if !launch.is_empty() {
                steps.push(Step::RevertLaunch(launch));
            }
            steps.push(Step::RestoreTracked);
            return Ok(LoaderPlan {
                op: Some(op),
                steps,
                ..LoaderPlan::default()
            });
        }
        let extracted = extracted.ok_or(LoaderError::ArchiveRequired { op: op.as_str() })?;
        let deploy = self.deploy_files(game, extracted)?;
        let touch_scope = deploy
            .files
            .iter()
            .map(|f| Glob::literal(&f.dest))
            .collect();
        let mut steps = vec![Step::DeployFiles(deploy)];
        if !launch.is_empty() {
            steps.push(Step::Launch(launch));
        }
        Ok(LoaderPlan {
            op: Some(op),
            steps,
            touch_scope,
            external_side_effects: Vec::new(),
            expected_version: None,
            uses_cached_archive: false,
        })
    }

    fn detect(&self, game: &GameCtx, store: &Store) -> Result<LoaderStatus, LoaderError> {
        if let Some(i) = store.active_loader_install(&game.game_id, &self.def.key)? {
            return Ok(match InstallStatus::parse(&i.status) {
                Some(InstallStatus::Installed) => LoaderStatus::Installed {
                    install_id: i.id,
                    version: i.version,
                },
                _ => LoaderStatus::Busy {
                    install_id: i.id,
                    status: i.status,
                },
            });
        }
        let evidence: Vec<String> = self
            .def
            .detect
            .iter()
            .filter(|d| game.game_dir.join(d).symlink_metadata().is_ok())
            .cloned()
            .collect();
        Ok(if evidence.is_empty() {
            LoaderStatus::NotInstalled
        } else {
            LoaderStatus::ManualDetected { evidence }
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::games::definitions::parse_definition;
    use crate::loaders::records::{FileChange, FileKind, LoaderFile, NewLoaderInstall, Strategy};

    pub(crate) const BEPINEX_TOML: &str = r#"
domain = "valheim"
name = "Valheim"
profile = "bepinex"

[[loaders]]
key = "bepinex"
name = "BepInEx"
required_paths = ["winhttp.dll", "BepInEx/core/BepInEx.Preloader.dll"]
forbidden_paths = ["BepInEx/plugins"]
detect = ["BepInEx/core/BepInEx.Preloader.dll"]

[loaders.payload]
exclude = ["changelog.txt"]
modes = [{ glob = "run_bepinex.sh", mode = 0o755 }]

[loaders.launch]
dll_overrides = ["winhttp"]
"#;

    pub(crate) const SKSE_TOML: &str = r#"
domain = "skyrimspecialedition"
name = "Skyrim SE"
profile = "bethesda"
subdir = "Data"

[[loaders]]
key = "skse"
name = "SKSE64"
nexus_mod_id = 30379
required_paths = ["skse64_*/skse64_loader.exe", "skse64_*/skse64_*.dll"]
couples_to = "SkyrimSE.exe"

[loaders.payload]
select = "skse64_*"
exclude = ["src", "*.txt"]

[loaders.launch]
exe = "skse64_loader.exe"
"#;

    const BEPINEX_FILES: &[&str] = &[
        "winhttp.dll",
        "doorstop_config.ini",
        "changelog.txt",
        "run_bepinex.sh",
        "BepInEx/core/BepInEx.Preloader.dll",
        "BepInEx/core/BepInEx.dll",
    ];

    const SKSE_FILES: &[&str] = &[
        "skse64_2_02_06/skse64_loader.exe",
        "skse64_2_02_06/skse64_1_6_1170.dll",
        "skse64_2_02_06/skse64_readme.txt",
        "skse64_2_02_06/Data/Scripts/Actor.pex",
        "skse64_2_02_06/src/skse64/main.cpp",
    ];

    pub(crate) fn loader(toml: &str) -> DeclarativeLoader {
        let def = parse_definition(toml, "fixture.toml").unwrap();
        assert_eq!(def.loaders.len(), 1);
        DeclarativeLoader::new(def.loaders[0].clone())
    }

    pub(crate) fn extract(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            let p = dir.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, f.as_bytes()).unwrap();
        }
        dir
    }

    pub(crate) fn probe(files: &[&str], mod_id: Option<u64>) -> ArchiveProbe {
        ArchiveProbe {
            file_name: "a.zip".into(),
            nexus_mod_id: mod_id,
            nexus_file_id: None,
            entries: files.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub(crate) fn ctx(id: &str) -> GameCtx {
        GameCtx {
            game_id: id.into(),
            nexus_domain: Some(id.into()),
            game_dir: "/nonexistent/game".into(),
            steam_appid: Some(1),
            occupied: Vec::new(),
        }
    }

    pub(crate) fn proton() -> Runtime {
        Runtime::Proton(ProtonEnv {
            proton: "/steam/common/Proton/proton".into(),
            compat_data: "/steam/compatdata/1".into(),
            client_root: "/steam".into(),
            app_id: 1,
            slr: None,
        })
    }

    fn dests(plan: &LoaderPlan) -> Vec<(String, Option<u32>)> {
        match &plan.steps[0] {
            Step::DeployFiles(d) => d.files.iter().map(|f| (f.dest.clone(), f.mode)).collect(),
            s => panic!("expected DeployFiles, got {s:?}"),
        }
    }

    /// Catches: BepInEx payload not landing in the game root, the exclude or
    /// per-file mode being dropped, or the DLL override not reaching the
    /// launch step.
    #[test]
    fn bepinex_fixture_matches_and_plans_deploy_and_launch() {
        let l = loader(BEPINEX_TOML);
        let g = ctx("valheim");
        let m = l.matches(&g, &probe(BEPINEX_FILES, None)).unwrap();
        assert_eq!(m.loader_key, "bepinex");
        let dir = extract(BEPINEX_FILES);
        let plan = l
            .plan(Op::Install, &g, &proton(), Some(dir.path()))
            .unwrap();
        assert_eq!(
            dests(&plan),
            [
                ("BepInEx/core/BepInEx.Preloader.dll".into(), None),
                ("BepInEx/core/BepInEx.dll".into(), None),
                ("doorstop_config.ini".into(), None),
                ("run_bepinex.sh".into(), Some(0o755)),
                ("winhttp.dll".into(), None),
            ]
        );
        assert_eq!(
            plan.steps[1],
            Step::Launch(LaunchChange {
                dll_overrides: vec!["winhttp".into()],
                ..LaunchChange::default()
            })
        );
        assert_eq!(plan.steps.len(), 2);
        assert!(plan.touch_scope.contains(&Glob::literal("winhttp.dll")));
        // Wine DLL overrides mean nothing to a native game.
        let native = l
            .plan(Op::Install, &g, &Runtime::Native, Some(dir.path()))
            .unwrap();
        assert_eq!(native.steps.len(), 1);
    }

    /// Catches: the nested top folder not being selected (files deployed
    /// under `skse64_2_02_06/`), source/readme files leaking, or the launch
    /// exe missing.
    #[test]
    fn skse_fixture_matches_and_plans_deploy_and_launch() {
        let l = loader(SKSE_TOML);
        let g = ctx("skyrimspecialedition");
        assert!(l.matches(&g, &probe(SKSE_FILES, Some(30379))).is_some());
        assert!(l.matches(&g, &probe(SKSE_FILES, None)).is_some());
        assert!(
            l.matches(&g, &probe(SKSE_FILES, Some(1))).is_none(),
            "wrong Nexus mod"
        );
        let dir = extract(SKSE_FILES);
        let plan = l
            .plan(Op::Install, &g, &proton(), Some(dir.path()))
            .unwrap();
        let d: Vec<String> = dests(&plan).into_iter().map(|(p, _)| p).collect();
        assert_eq!(
            d,
            [
                "Data/Scripts/Actor.pex",
                "skse64_1_6_1170.dll",
                "skse64_loader.exe"
            ]
        );
        assert_eq!(
            plan.steps[1],
            Step::Launch(LaunchChange {
                exe: Some("skse64_loader.exe".into()),
                ..LaunchChange::default()
            })
        );
        assert_eq!(l.def().couples_to.as_deref(), Some("SkyrimSE.exe"));
    }

    /// Catches: a mod pack that bundles the loader being taken for the
    /// loader, or archives missing a required file still matching.
    #[test]
    fn required_and_forbidden_paths_gate_the_match() {
        let l = loader(BEPINEX_TOML);
        let g = ctx("valheim");
        let mut pack: Vec<&str> = BEPINEX_FILES.to_vec();
        pack.push("BepInEx/plugins/SomeMod.dll");
        assert!(l.matches(&g, &probe(&pack, None)).is_none());
        assert!(l.matches(&g, &probe(&["winhttp.dll"], None)).is_none());
        assert!(l.matches(&g, &probe(&["Mod/Mod.dll"], None)).is_none());
    }

    /// Catches: a payload silently overwriting a deployed mod file or a file
    /// owned by another loader (case-insensitively, as on Windows games).
    #[test]
    fn payload_collision_is_a_conflict() {
        let l = loader(BEPINEX_TOML);
        let dir = extract(BEPINEX_FILES);
        for owner in [Owner::Deployed, Owner::Loader("other".into())] {
            let mut g = ctx("valheim");
            g.occupied.push(Occupied {
                path: "WinHTTP.dll".into(),
                owner: owner.clone(),
            });
            match l.plan(Op::Install, &g, &proton(), Some(dir.path())) {
                Err(LoaderError::Conflict { path, owner: o }) => {
                    assert_eq!((path.as_str(), o), ("winhttp.dll", owner));
                }
                other => panic!("expected Conflict, got {other:?}"),
            }
        }
    }

    /// Catches: occupied paths from the store not reaching the conflict
    /// check (deployments rows and other loaders' files).
    #[test]
    fn store_occupied_paths_feed_the_conflict_check() {
        let s = Store::open_in_memory().unwrap();
        s.conn()
            .execute_batch(
                "INSERT INTO games (id, name, added_at) VALUES ('valheim', 'Valheim', 0);
                 INSERT INTO deployments (game, target, rel_path, method, source, deployed_at)
                   VALUES ('valheim', 'game_dir', 'doorstop_config.ini', 'copy', '/s', 0);",
            )
            .unwrap();
        let mut g = ctx("valheim");
        g.occupied = s.occupied_paths("valheim", "bepinex").unwrap();
        let dir = extract(BEPINEX_FILES);
        let err = loader(BEPINEX_TOML)
            .plan(Op::Install, &g, &proton(), Some(dir.path()))
            .unwrap_err();
        assert!(
            matches!(&err, LoaderError::Conflict { path, owner: Owner::Deployed } if path == "doorstop_config.ini"),
            "{err:?}"
        );
    }

    /// Catches: uninstall demanding an archive, or forgetting to revert the
    /// launch change; other ops planning without one.
    #[test]
    fn uninstall_plans_without_an_archive() {
        let l = loader(BEPINEX_TOML);
        let g = ctx("valheim");
        let plan = l.plan(Op::Uninstall, &g, &proton(), None).unwrap();
        assert_eq!(
            plan.steps,
            [
                Step::RevertLaunch(LaunchChange {
                    dll_overrides: vec!["winhttp".into()],
                    ..LaunchChange::default()
                }),
                Step::RestoreTracked,
            ]
        );
        for op in [Op::Install, Op::Upgrade, Op::Repair, Op::Adopt] {
            assert!(matches!(
                l.plan(op, &g, &proton(), None),
                Err(LoaderError::ArchiveRequired { .. })
            ));
        }
    }

    /// Catches: a symlink in the payload being placed into the game root.
    #[test]
    fn symlinked_payload_entry_is_refused() {
        let dir = extract(BEPINEX_FILES);
        std::os::unix::fs::symlink("/etc/passwd", dir.path().join("evil.dll")).unwrap();
        let err = loader(BEPINEX_TOML)
            .plan(Op::Install, &ctx("valheim"), &proton(), Some(dir.path()))
            .unwrap_err();
        assert!(matches!(err, LoaderError::UnsafePath { .. }), "{err:?}");
    }

    /// Catches: an ambiguous or missing `select` folder being guessed.
    #[test]
    fn select_must_match_exactly_one_folder() {
        let l = loader(SKSE_TOML);
        let g = ctx("skyrimspecialedition");
        let mut two: Vec<&str> = SKSE_FILES.to_vec();
        two.push("skse64_old/skse64_loader.exe");
        for files in [&two[..], &["other/a.dll"][..]] {
            let dir = extract(files);
            assert!(matches!(
                l.plan(Op::Install, &g, &proton(), Some(dir.path())),
                Err(LoaderError::Payload { .. })
            ));
        }
    }

    /// Catches: typos in a definition being ignored, and unsafe values
    /// (escaping paths, setuid modes, odd DLL/env names) being accepted.
    #[test]
    fn unknown_fields_and_unsafe_values_are_rejected() {
        let bad = [
            BEPINEX_TOML.replace("name = \"BepInEx\"", "name = \"BepInEx\"\nbogus = 1"),
            BEPINEX_TOML.replace("dll_overrides", "dll_overides"),
            BEPINEX_TOML.replace("exclude = [", "excludes = ["),
            BEPINEX_TOML.replace("0o755", "0o4755"),
            BEPINEX_TOML.replace("[\"winhttp\"]", "[\"winhttp=n,b\"]"),
            BEPINEX_TOML.replace("changelog.txt", "../x"),
            BEPINEX_TOML.replace("key = \"bepinex\"", "key = \"Bep In\""),
            BEPINEX_TOML.replace(
                "required_paths = [\"winhttp.dll\", \"BepInEx/core/BepInEx.Preloader.dll\"]",
                "required_paths = []",
            ),
            SKSE_TOML.replace("skse64_loader.exe\"\n", "/usr/bin/sh\"\n"),
            SKSE_TOML.replace("couples_to = \"SkyrimSE.exe\"", "couples_to = \"../x\""),
            format!("{BEPINEX_TOML}env = {{ \"A B\" = \"1\" }}\n"),
            format!("{BEPINEX_TOML}\n[[loaders]]\nkey = \"bepinex\"\nname = \"B\"\nrequired_paths = [\"a\"]\n"),
        ];
        for text in bad {
            assert!(parse_definition(&text, "x.toml").is_err(), "{text}");
        }
    }

    /// Catches: manual installs not being reported, and tracked installs
    /// not taking precedence over file evidence.
    #[test]
    fn detect_reports_tracked_and_manual_installs() {
        let l = loader(BEPINEX_TOML);
        let s = Store::open_in_memory().unwrap();
        s.conn()
            .execute(
                "INSERT INTO games (id, name, added_at) VALUES ('valheim', 'Valheim', 0)",
                [],
            )
            .unwrap();
        let game = extract(&["BepInEx/core/BepInEx.Preloader.dll"]);
        let mut g = ctx("valheim");
        assert_eq!(l.detect(&g, &s).unwrap(), LoaderStatus::NotInstalled);
        g.game_dir = game.path().to_path_buf();
        assert!(matches!(
            l.detect(&g, &s).unwrap(),
            LoaderStatus::ManualDetected { .. }
        ));
        let id = s
            .insert_loader_install(&NewLoaderInstall {
                game_id: "valheim".into(),
                game_dir: game.path().display().to_string(),
                loader_key: "bepinex".into(),
                version: Some("5.4.23".into()),
                runtime: "proton".into(),
                strategy: Strategy::Deploy,
                status: InstallStatus::Installed,
                nexus_mod_id: None,
                nexus_file_id: None,
                origin: "local".into(),
                archive_sha256: None,
                archive_path: None,
                vanilla_sha256: None,
                game_build: None,
            })
            .unwrap();
        s.insert_loader_file(
            id,
            &LoaderFile {
                path: "winhttp.dll".into(),
                change: FileChange::Created,
                kind: FileKind::File,
                pre_sha256: None,
                post_sha256: None,
                backup_ref: None,
                out_of_scope: false,
            },
        )
        .unwrap();
        assert_eq!(
            l.detect(&g, &s).unwrap(),
            LoaderStatus::Installed {
                install_id: id,
                version: Some("5.4.23".into())
            }
        );
    }

    /// Catches: archives not downloaded by Vessel, or from another Nexus
    /// mod or game, being reported as verified.
    #[test]
    fn provenance_is_verified_only_for_the_declared_nexus_mod() {
        let l = loader(SKSE_TOML);
        let g = ctx("skyrimspecialedition");
        let m = l.matches(&g, &probe(SKSE_FILES, None)).unwrap();
        let nexus = |domain: &str, mod_id| DownloadOrigin::Nexus {
            domain: domain.into(),
            mod_id,
            file_id: 7,
        };
        assert!(l
            .provenance(&g, &m, &nexus("skyrimspecialedition", 30379))
            .is_verified());
        assert!(!l
            .provenance(&g, &m, &nexus("skyrimspecialedition", 1))
            .is_verified());
        assert!(!l.provenance(&g, &m, &nexus("skyrim", 30379)).is_verified());
        let local = DownloadOrigin::Local {
            path: "/a.zip".into(),
        };
        assert!(!l.provenance(&g, &m, &local).is_verified());
    }
}
