// SPDX-License-Identifier: GPL-3.0-or-later
//! FOMOD parser, selection, plan and placement tests over
//! `testdata/ModuleConfig.xml`.

use std::path::{Path, PathBuf};

use super::*;

const FIXTURE: &str = include_str!("testdata/ModuleConfig.xml");

/// Every source file the fixture names, with distinct contents.
const SOURCES: &[&str] = &[
    "core/core.esp",
    "core/body_default.nif",
    "core/textures/a.dds",
    "core/textures/sub/b.dds",
    "body/slim.nif",
    "body/curvy.nif",
    "extras/zeta.esp",
    "extras/alpha.esp",
    "patches/patch_a.esp",
    "patches/skyui_curvy.esp",
    "patches/low_core.esp",
    "size/small.esp",
    "size/large.esp",
    "shared/shared.txt",
    "docs/readme.txt",
    "docs/broken.txt",
];

fn cfg() -> ModuleConfig {
    parse_module_config(FIXTURE.as_bytes()).unwrap()
}

/// A FOMOD root with the fixture and its files (upper-cased folder names
/// prove case-insensitive lookup).
pub(crate) fn tree(dir: &Path) -> PathBuf {
    let root = dir.join("root");
    // Placed under `dir`; callers may wrap it in more folders.
    std::fs::create_dir_all(root.join("FOMOD")).unwrap();
    std::fs::write(root.join("FOMOD/moduleconfig.xml"), FIXTURE).unwrap();
    for s in SOURCES {
        let p = root.join(s.replacen("core/", "Core/", 1));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }
    root
}

fn files_of(plan: &InstallPlan) -> Vec<(&str, Option<&str>, i64)> {
    plan.files
        .iter()
        .map(|f| (f.source.as_str(), f.destination.as_deref(), f.priority))
        .collect()
}

/// (step, group, [(option, idx)]).
type Picks = Vec<(String, String, Vec<(String, Option<i64>)>)>;

fn picks(c: &FomodChoices) -> Picks {
    c.options
        .iter()
        .flat_map(|s| {
            s.groups.iter().map(|g| {
                (
                    s.name.clone(),
                    g.name.clone(),
                    g.choices.iter().map(|c| (c.name.clone(), c.idx)).collect(),
                )
            })
        })
        .collect()
}

fn read_tree(out: &Path) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = walkdir::WalkDir::new(out)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            (
                e.path()
                    .strip_prefix(out)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                std::fs::read_to_string(e.path()).unwrap(),
            )
        })
        .collect();
    v.sort();
    v
}

/// Answers from a script; records what it was shown.
#[derive(Default)]
pub(crate) struct Scripted {
    pub answers: Vec<Vec<usize>>,
    pub seen: Vec<(String, String, Option<String>)>,
    pub began: bool,
}

impl FomodResolver for Scripted {
    fn begin(&mut self, m: &ModuleConfig) {
        self.began = m.module_name == "Fixture Mod";
    }
    fn choose(&mut self, p: &GroupPrompt<'_>) -> Result<Vec<usize>, FomodError> {
        self.seen.push((
            p.step.to_string(),
            p.group.name.clone(),
            p.error.map(str::to_string),
        ));
        if self.answers.is_empty() {
            return Err(FomodError::Cancelled);
        }
        Ok(self.answers.remove(0))
    }
}

// ── parser ──────────────────────────────────────────────────

#[test]
fn parses_steps_groups_types_and_order() {
    let c = cfg();
    assert_eq!(c.module_name, "Fixture Mod");
    assert_eq!(c.module_image.as_deref(), Some("fomod\\images\\main.png"));
    let steps: Vec<&str> = c.steps.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(steps, ["Main", "Curvy Options", "Docs"]);
    let main = &c.steps[0];
    let kinds: Vec<(&str, GroupType)> = main
        .groups
        .iter()
        .map(|g| (g.name.as_str(), g.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("Body", GroupType::SelectExactlyOne),
            ("Extras", GroupType::SelectAny),
            ("Patch", GroupType::SelectAtMostOne)
        ]
    );
    let names = |g: &Group| g.plugins.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
    // Explicit, Ascending, Descending.
    assert_eq!(names(&main.groups[0]), ["Slim", "Curvy"]);
    assert_eq!(names(&main.groups[1]), ["Alpha", "Zeta"]);
    assert_eq!(names(&main.groups[2]), ["Patch A", "None"]);
    // Default order (Ascending) for plugins without an order attribute.
    assert_eq!(names(&c.steps[1].groups[0]), ["Large", "Small"]);
    assert_eq!(c.steps[1].groups[1].kind, GroupType::SelectAll);
    assert_eq!(c.steps[1].groups[0].kind, GroupType::SelectAtLeastOne);

    let curvy = &main.groups[0].plugins[1];
    assert_eq!(curvy.description, "Curvy & tall");
    assert_eq!(curvy.flags, [("body".to_string(), "curvy".to_string())]);
    assert_eq!(
        curvy.type_descriptor,
        TypeDescriptor::Fixed(PluginType::Recommended)
    );
    assert_eq!(
        main.groups[0].plugins[0].image.as_deref(),
        Some("fomod/images/slim.png")
    );
    let TypeDescriptor::Dependent { patterns, default } =
        &main.groups[2].plugins[0].type_descriptor
    else {
        panic!("Patch A has a dependency type")
    };
    assert_eq!(*default, PluginType::NotUsable);
    assert_eq!(patterns[0].1, PluginType::Recommended);
    assert_eq!(
        c.steps[1].visible.as_ref().unwrap().terms,
        [Dependency::Flag {
            name: "body".into(),
            value: "curvy".into()
        }]
    );
    let docs = &c.steps[2].groups[0].plugins;
    assert!(docs[1].files[0].always_install);
    assert!(docs[0].files[0].install_if_usable);

    assert_eq!(c.required_files.len(), 3);
    assert!(c.required_files[2].folder);
    assert_eq!(c.conditional_files.len(), 2);
    let (cond, files) = &c.conditional_files[1];
    assert_eq!(cond.operator, Operator::Or);
    assert!(matches!(cond.terms[1], Dependency::Composite(_)));
    assert_eq!(files[0].priority, -1);
}

#[test]
fn parses_utf16_module_config() {
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(
        FIXTURE
            .replace("utf-8", "UTF-16")
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    assert_eq!(parse_module_config(&bytes).unwrap(), cfg());
}

#[test]
fn rejects_non_fomod_xml() {
    assert!(matches!(
        parse_module_config(b"<notconfig/>"),
        Err(FomodError::Parse(_))
    ));
    assert!(matches!(
        parse_module_config(b"<config><moduleName>x</config>"),
        Err(FomodError::Parse(_))
    ));
}

// ── selection and plan ──────────────────────────────────────

#[test]
fn default_selection_per_group_type() {
    use PluginType::*;
    let d = default_selection;
    assert_eq!(d(GroupType::SelectAll, &[Optional, NotUsable]), [0, 1]);
    assert_eq!(
        d(
            GroupType::SelectExactlyOne,
            &[Optional, Recommended, Required]
        ),
        [2]
    );
    assert_eq!(d(GroupType::SelectExactlyOne, &[NotUsable, Optional]), [1]);
    assert_eq!(d(GroupType::SelectExactlyOne, &[NotUsable]), [0]);
    assert_eq!(d(GroupType::SelectAtMostOne, &[Optional, Recommended]), [1]);
    assert!(d(GroupType::SelectAtMostOne, &[Optional]).is_empty());
    assert_eq!(
        d(
            GroupType::SelectAtLeastOne,
            &[Recommended, Optional, Required]
        ),
        [0, 2]
    );
    assert_eq!(d(GroupType::SelectAtLeastOne, &[NotUsable, Optional]), [1]);
    assert_eq!(
        d(GroupType::SelectAny, &[Required, Optional, Recommended]),
        [0, 2]
    );
    assert!(d(GroupType::SelectAny, &[Optional]).is_empty());
}

#[test]
fn validate_selection_enforces_group_rules() {
    use PluginType::*;
    let v = validate_selection;
    assert_eq!(
        v(GroupType::SelectAny, &[Required, Optional], &[1]),
        Ok(vec![0, 1])
    );
    assert!(v(GroupType::SelectAny, &[NotUsable, Optional], &[0])
        .unwrap_err()
        .contains("cannot be used"));
    assert!(v(GroupType::SelectExactlyOne, &[Optional, Optional], &[0, 1]).is_err());
    assert!(v(GroupType::SelectExactlyOne, &[Optional, Optional], &[]).is_err());
    assert!(v(GroupType::SelectAtMostOne, &[Optional, Optional], &[0, 1]).is_err());
    assert_eq!(v(GroupType::SelectAtMostOne, &[Optional], &[]), Ok(vec![]));
    assert!(v(GroupType::SelectAtLeastOne, &[Optional], &[]).is_err());
    assert!(v(GroupType::SelectAny, &[Optional], &[3]).is_err());
    assert_eq!(
        v(GroupType::SelectAll, &[Optional, Optional], &[]),
        Ok(vec![0, 1])
    );
}

#[test]
fn default_plan_follows_flags_visibility_and_priorities() {
    let plan = build_plan(&cfg(), Selection::Default, None).unwrap();
    assert_eq!(plan.mode, ResolvedMode::Default);
    assert_eq!(
        files_of(&plan),
        [
            // conditional pattern 2 (composite: Missing.esp missing), priority -1 first
            ("patches/low_core.esp", Some("core.esp"), -1),
            ("core\\core.esp", Some("core.esp"), 0),
            ("core/body_default.nif", Some("meshes\\body.nif"), 0),
            ("core/textures", Some("textures"), 0),
            ("extras/alpha.esp", Some("plugins/"), 0),
            // Patch A: dependency type became Recommended via flag body=curvy
            ("patches/patch_a.esp", None, 0),
            // Curvy Options visible; SelectAtLeastOne default = first usable
            ("size/large.esp", Some("large.esp"), 0),
            ("shared/shared.txt", Some("shared.txt"), 0),
            // unselected plugin, alwaysInstall
            ("docs/readme.txt", Some("docs/"), 0),
            ("body/curvy.nif", Some("meshes/body.nif"), 1),
        ]
    );
    assert_eq!(plan.flags.get("body").map(String::as_str), Some("curvy"));
    let s = |v: &[(&str, i64)]| {
        v.iter()
            .map(|(n, i)| (n.to_string(), Some(*i)))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        picks(&plan.choices),
        [
            ("Main".into(), "Body".into(), s(&[("Curvy", 1)])),
            ("Main".into(), "Extras".into(), s(&[("Alpha", 0)])),
            ("Main".into(), "Patch".into(), s(&[("Patch A", 0)])),
            ("Curvy Options".into(), "Size".into(), s(&[("Large", 0)])),
            ("Curvy Options".into(), "Always".into(), s(&[("Shared", 0)])),
            ("Docs".into(), "Readme".into(), vec![]),
        ]
    );
}

#[test]
fn file_dependency_checks_the_deploy_folder_case_insensitively() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("skyui_se.ESP"), "").unwrap();
    std::fs::write(tmp.path().join("missing.esp"), "").unwrap();
    let plan = build_plan(&cfg(), Selection::Default, Some(tmp.path())).unwrap();
    let srcs: Vec<&str> = plan.files.iter().map(|f| f.source.as_str()).collect();
    assert!(srcs.contains(&"patches/skyui_curvy.esp"), "{srcs:?}");
    // Missing.esp is present now, so pattern 2 no longer applies.
    assert!(!srcs.contains(&"patches/low_core.esp"), "{srcs:?}");
}

#[test]
fn choices_mode_replays_by_name_then_idx_and_hides_steps() {
    let choices = FomodChoices::from_json(
        r#"{"type":"fomod","options":[{"name":"main","groups":[
            {"name":"Body","choices":[{"name":"Slim","idx":0}]},
            {"name":"Extras","choices":[{"name":"renamed","idx":1}]},
            {"name":"Patch","choices":[{"name":"Patch A","idx":0}]}]}]}"#,
    )
    .unwrap();
    let plan = build_plan(&cfg(), Selection::Choices(&choices), None).unwrap();
    assert_eq!(plan.mode, ResolvedMode::Choices);
    let p = picks(&plan.choices);
    // Step matched case-insensitively; unknown name falls back to idx (Zeta).
    assert_eq!(p[0].2, [("Slim".to_string(), Some(0))]);
    assert_eq!(p[1].2, [("Zeta".to_string(), Some(1))]);
    // Patch A is NotUsable without body=curvy: dropped.
    assert!(p[2].2.is_empty());
    // Curvy Options hidden; Docs has no entry -> defaults.
    assert_eq!(p.len(), 4);
    assert_eq!((p[3].0.as_str(), p[3].2.len()), ("Docs", 0));
    let srcs: Vec<&str> = plan.files.iter().map(|f| f.source.as_str()).collect();
    assert!(srcs.contains(&"body/slim.nif") && srcs.contains(&"extras/zeta.esp"));
    assert!(!srcs.contains(&"size/large.esp"));
}

#[test]
fn choices_without_options_fall_back_to_defaults() {
    let empty = FomodChoices::from_json(r#"{"type":"fomod","options":[]}"#).unwrap();
    let plan = build_plan(&cfg(), Selection::Choices(&empty), None).unwrap();
    assert_eq!(plan.mode, ResolvedMode::Default);
    assert_eq!(plan, build_plan(&cfg(), Selection::Default, None).unwrap());
    assert!(FomodChoices::from_json(r#"{"type":"fomod"}"#).is_none());
    assert!(FomodChoices::from_json("null").is_none());
}

#[test]
fn ask_mode_uses_resolver_and_reasks_invalid_answers() {
    let mut r = Scripted {
        // Body: invalid (two), then Slim; Extras: Zeta; Patch: none;
        // Docs/Readme: NotUsable "Broken" rejected, then Readme.
        answers: vec![vec![0, 1], vec![0], vec![1], vec![], vec![0], vec![1]],
        ..Default::default()
    };
    let plan = build_plan(&cfg(), Selection::Ask(&mut r), None).unwrap();
    assert!(r.began);
    assert_eq!(plan.mode, ResolvedMode::Ask);
    let errs: Vec<Option<&str>> = r.seen.iter().map(|s| s.2.as_deref()).collect();
    assert_eq!(
        errs,
        [
            None,
            Some("choose exactly one"),
            None,
            None,
            None,
            Some("option(s) 1 cannot be used")
        ]
    );
    let p = picks(&plan.choices);
    assert_eq!(p[0].2, [("Slim".to_string(), Some(0))]);
    // Patch group shown "Patch A" as NotUsable (no curvy) -> picked none.
    assert!(p[2].2.is_empty());
    assert_eq!(p[3].2, [("Readme".to_string(), Some(1))]);
}

#[test]
fn ask_mode_cancel_aborts() {
    let mut r = Scripted::default();
    assert!(matches!(
        build_plan(&cfg(), Selection::Ask(&mut r), None),
        Err(FomodError::Cancelled)
    ));
}

#[test]
fn choices_round_trip_in_vortex_format() {
    let plan = build_plan(&cfg(), Selection::Default, None).unwrap();
    let json = plan.choices.to_json();
    assert!(
        json.starts_with(
            r#"{"type":"fomod","options":[{"name":"Main","groups":[{"name":"Body","choices":[{"name":"Curvy","idx":1}]}"#
        ),
        "{json}"
    );
    assert_eq!(FomodChoices::from_json(&json).unwrap(), plan.choices);
    // Replaying the recording reproduces the plan's files.
    let again = build_plan(&cfg(), Selection::Choices(&plan.choices), None).unwrap();
    assert_eq!(again.files, plan.files);
    assert_eq!(
        plan.choices.summary()[0],
        (
            "Main".to_string(),
            vec!["Curvy".to_string(), "Alpha".into(), "Patch A".into()]
        )
    );
}

// ── modes (legacy_spec.md §8 table) ─────────────────────────

#[test]
fn mode_resolution_matches_legacy_table() {
    use FomodMode as M;
    use ResolvedMode as R;
    let r = resolve_mode;
    assert_eq!(r(M::None, true, true), None);
    assert_eq!(r(M::Collection, true, false), Some(R::Choices));
    assert_eq!(r(M::Collection, false, true), Some(R::Default));
    assert_eq!(r(M::Reuse, true, true), Some(R::Choices));
    assert_eq!(r(M::Reuse, false, true), Some(R::Ask));
    assert_eq!(r(M::Reuse, false, false), Some(R::Default));
    assert_eq!(r(M::Ask, true, true), Some(R::Ask));
    assert_eq!(r(M::Ask, false, false), Some(R::Default));
    assert_eq!(r(M::Default, true, true), Some(R::Default));
    assert_eq!(r(M::Auto, false, true), Some(R::Ask));
    assert_eq!(r(M::Auto, true, false), Some(R::Default));
    assert_eq!("".parse::<M>().unwrap(), M::Auto);
    assert_eq!("collection".parse::<M>().unwrap(), M::Collection);
    assert!("bogus".parse::<M>().is_err());
}

// ── placement and confinement ───────────────────────────────

#[test]
fn run_places_files_with_priority_and_destination_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tree(tmp.path());
    let out = tmp.path().join("out");
    let r = run(&root, &out, Selection::Default, None).unwrap();
    assert_eq!(r.module_name, "Fixture Mod");
    assert!(
        r.report.missing.is_empty() && r.report.refused.is_empty(),
        "{:?}",
        r.report
    );
    assert_eq!(
        read_tree(&out),
        [
            ("core.esp".into(), "core/core.esp".to_string()),
            ("docs/readme.txt".into(), "docs/readme.txt".into()),
            ("large.esp".into(), "size/large.esp".into()),
            ("meshes/body.nif".into(), "body/curvy.nif".into()),
            ("patches/patch_a.esp".into(), "patches/patch_a.esp".into()),
            ("plugins/alpha.esp".into(), "extras/alpha.esp".into()),
            ("shared.txt".into(), "shared/shared.txt".into()),
            ("textures/a.dds".into(), "core/textures/a.dds".into()),
            (
                "textures/sub/b.dds".into(),
                "core/textures/sub/b.dds".into()
            ),
        ]
    );
    assert_eq!(r.report.placed, 11);
}

#[test]
fn sources_and_destinations_are_confined_to_the_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(root.join("fomod")).unwrap();
    std::fs::write(root.join("ok.txt"), "ok").unwrap();
    let secret = tmp.path().join("secret.txt");
    std::fs::write(&secret, "secret").unwrap();
    std::os::unix::fs::symlink(&secret, root.join("link.txt")).unwrap();
    std::os::unix::fs::symlink(tmp.path(), root.join("linkdir")).unwrap();
    let f = |s: &str, d: Option<&str>| PlannedFile {
        source: s.into(),
        destination: d.map(str::to_string),
        priority: 0,
    };
    let plan = InstallPlan {
        files: vec![
            f("../secret.txt", Some("a.txt")),
            f("..\\secret.txt", None),
            f(secret.to_str().unwrap(), Some("abs.txt")),
            f("link.txt", Some("b.txt")),
            f("linkdir/secret.txt", Some("c.txt")),
            f("linkdir", Some("d")),
            f("ok.txt", Some("../escape.txt")),
            f("ok.txt", Some("..\\..\\escape2.txt")),
            f("ok.txt", Some("/abs/dest.txt")),
        ],
        choices: FomodChoices::new(vec![]),
        mode: ResolvedMode::Default,
        flags: Default::default(),
    };
    let out = tmp.path().join("work/out");
    let rep = apply_plan(&root, &plan, &out).unwrap();
    assert_eq!(rep.missing.len(), 6, "{rep:?}");
    assert_eq!(rep.refused, ["../escape.txt", "..\\..\\escape2.txt"]);
    // Absolute destinations stay inside `out` (legacy safe_rel).
    assert_eq!(read_tree(&out), [("abs/dest.txt".into(), "ok".to_string())]);
    assert!(!tmp.path().join("work/escape.txt").exists());
    assert!(!tmp.path().join("escape2.txt").exists());
    assert_eq!(std::fs::read_to_string(&secret).unwrap(), "secret");
}

#[test]
fn missing_module_config_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(matches!(
        run(tmp.path(), &tmp.path().join("o"), Selection::Default, None),
        Err(FomodError::NotFound(_))
    ));
}
