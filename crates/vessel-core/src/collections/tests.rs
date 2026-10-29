// SPDX-License-Identifier: GPL-3.0-or-later
use std::collections::BTreeMap;

use rusqlite::params;
use serde_json::json;

use super::*;
use crate::mods::tests::{add, store_with_game};
use crate::mods::{list_mods, set_enabled};

fn manifest(v: Value) -> Manifest {
    Manifest::from_value(v).unwrap()
}

fn uids(s: &Store) -> Vec<String> {
    list_mods(s.conn(), "g")
        .unwrap()
        .into_iter()
        .map(|m| m.uid)
        .collect()
}

fn set_ids(s: &Store, id: i64, md5: Option<&str>, m: Option<i64>, f: Option<i64>) {
    s.conn()
        .execute(
            "UPDATE mods SET md5 = ?1, nexus_mod_id = ?2, nexus_file_id = ?3 WHERE id = ?4",
            params![md5, m, f, id],
        )
        .unwrap();
}

fn revision(slug: &str, number: u64) -> Revision {
    Revision::from_value(
        "g",
        slug,
        json!({"revisionNumber": number, "collection": {"name": format!("Col {slug}"), "user": {"name": "Cur"}},
               "externalResources": [{"name": "SKSE", "resourceUrl": "https://skse.example"}]}),
    )
    .unwrap()
}

// ── references ──────────────────────────────────────────────

#[test]
fn parse_ref_accepts_legacy_forms_and_rejects_others() {
    let r = |g: Option<&str>, s: &str, n: Option<u64>| {
        Some(CollectionRef {
            game: g.map(str::to_string),
            slug: s.to_string(),
            revision: n,
        })
    };
    assert_eq!(
        parse_ref("nxm://SkyrimSpecialEdition/collections/abc123/revisions/7"),
        r(Some("skyrimspecialedition"), "abc123", Some(7))
    );
    assert_eq!(
        parse_ref("https://www.nexusmods.com/games/skyrimspecialedition/collections/qwe9z"),
        r(Some("skyrimspecialedition"), "qwe9z", None)
    );
    assert_eq!(
        parse_ref(
            "https://next.nexusmods.com/stardewvalley/collections/tckf0m/revisions/3?tab=mods"
        ),
        r(Some("stardewvalley"), "tckf0m", Some(3))
    );
    assert_eq!(parse_ref("tckf0m"), r(None, "tckf0m", None));
    for bad in [
        "ab",
        "toolongslug12345",
        "has space",
        "nxm://g/mods/1/files/2",
        "https://nexusmods.com/g/collections/a-b",
        "nxm://evil.example/collections/abc",
    ] {
        assert_eq!(parse_ref(bad), None, "{bad}");
    }
}

// ── optional selection ──────────────────────────────────────

#[test]
fn parse_selection_matches_legacy() {
    assert_eq!(parse_selection("all", 3), Some(vec![0, 1, 2]));
    assert_eq!(parse_selection("*", 2), Some(vec![0, 1]));
    assert_eq!(parse_selection("none", 3), Some(vec![]));
    assert_eq!(parse_selection("", 3), Some(vec![]));
    assert_eq!(parse_selection("1,3-5", 4), Some(vec![0, 2, 3]));
    assert_eq!(parse_selection("2 2 9", 3), Some(vec![1]));
    assert_eq!(parse_selection("x", 3), None);
    assert_eq!(parse_selection("1-x", 3), None);
    assert!("ask".parse::<OptionalMode>().unwrap() == OptionalMode::Ask);
    assert!("some".parse::<OptionalMode>().is_err());
}

fn three_mods() -> Manifest {
    manifest(json!({"mods": [
        {"name": "Req A", "source": {"type": "nexus", "modId": 1, "fileId": 11, "md5": "AAAA", "fileSize": 10}},
        {"name": "Opt B", "optional": true, "source": {"type": "nexus", "modId": 2, "fileId": 21, "fileSize": 20}},
        {"source": {"type": "bundle", "logicalFilename": "Bundled C"}},
        {"name": "Opt D", "optional": true, "source": {"modId": "4", "fileId": "41"}}
    ]}))
}

#[test]
fn select_keeps_required_and_picked_optional() {
    let m = three_mods();
    assert_eq!(optional_indexes(&m), [1, 3]);
    assert_eq!(select(&m, &[]), [0, 2]);
    assert_eq!(select(&m, &[1]), [0, 2, 3]);
    assert_eq!(select(&m, &[0, 1]), [0, 1, 2, 3]);
    // Defaults: name from logicalFilename, type nexus, string ids.
    assert_eq!(m.mods[2].name, "Bundled C");
    assert_eq!(m.mods[3].source_type, "nexus");
    assert_eq!((m.mods[3].mod_id, m.mods[3].file_id), (Some(4), Some(41)));
    assert_eq!(m.mods[0].md5.as_deref(), Some("aaaa"));
}

// ── dedupe ──────────────────────────────────────────────────

#[test]
fn plan_dedupes_by_md5_then_mod_and_file_id() {
    let s = store_with_game("g");
    let by_md5 = add(&s, "g", "x", "x", None);
    set_ids(&s, by_md5, Some("aaaa"), Some(99), Some(99));
    let by_file = add(&s, "g", "y", "y", None);
    set_ids(&s, by_file, None, Some(2), Some(21));
    // Same mod, other file: not a match.
    let other = add(&s, "g", "z", "z", None);
    set_ids(&s, other, None, Some(4), Some(40));
    let installed = list_mods(s.conn(), "g").unwrap();
    let m = three_mods();
    let p = plan(&m, &[0, 1, 2, 3], &installed);
    assert_eq!(p.have, BTreeMap::from([(0, by_md5), (1, by_file)]));
    assert_eq!(p.todo, [2, 3]);
    // md5 wins over a modId-fileId match elsewhere.
    let mm =
        &manifest(json!({"mods": [{"source": {"modId": 2, "fileId": 21, "md5": "AAAA"}}]})).mods[0];
    assert_eq!(match_installed(mm, &installed), Some(by_md5));
}

// ── load order ──────────────────────────────────────────────

fn ordered_manifest(rules: Value) -> Manifest {
    manifest(json!({
        "mods": [
            {"name": "A", "source": {"tag": "ta", "md5": "M-A"}},
            {"name": "B", "source": {"logicalFilename": "B.zip"}},
            {"name": "C", "phase": 1, "source": {"fileExpression": "C-*"}},
            {"name": "D", "source": {"md5": "m-d"}}
        ],
        "modRules": rules
    }))
}

#[test]
fn rule_order_applies_before_and_after_rules() {
    // A after B (logical name, case-insensitive); D before B (md5).
    let m = ordered_manifest(json!([
        {"type": "after", "source": {"tag": "ta"}, "reference": {"logicalFileName": "b.ZIP"}},
        {"type": "before", "source": {"fileMD5": "M-D"}, "reference": {"logicalFileName": "B.zip"}},
        {"type": "requires", "source": {"tag": "ta"}, "reference": {"fileExpression": "c-*"}},
        {"type": "after", "source": {"tag": "nope"}, "reference": {"tag": "ta"}}
    ]));
    let o = rule_order(&m, &[0, 1, 2, 3]);
    // D(0,3) -> B -> A; C has phase 1 so it sorts after phase-0 ties.
    assert_eq!(o.order, [3, 1, 0, 2]);
    assert_eq!(o.in_cycle, 0);
}

#[test]
fn rule_order_ties_by_phase_then_index_and_appends_cycles() {
    let m = ordered_manifest(json!([]));
    assert_eq!(rule_order(&m, &[0, 1, 2, 3]).order, [0, 1, 3, 2]);
    // Only present mods take part; rules naming absent mods are ignored.
    assert_eq!(rule_order(&m, &[2, 0]).order, [0, 2]);
    let m = ordered_manifest(json!([
        {"type": "before", "source": {"tag": "ta"}, "reference": {"logicalFileName": "B.zip"}},
        {"type": "before", "source": {"logicalFileName": "B.zip"}, "reference": {"tag": "ta"}},
        {"type": "before", "source": {"fileMD5": "m-d"}, "reference": {"tag": "ta"}}
    ]));
    let o = rule_order(&m, &[0, 1, 2, 3]);
    assert_eq!(o.order, [3, 2, 0, 1]);
    assert_eq!(o.in_cycle, 2);
}

#[test]
fn reorder_block_places_ids_at_lowest_existing_position() {
    let mut s = store_with_game("g");
    let ids: Vec<i64> = ["u1", "c1", "u2", "c2", "u3", "c3"]
        .iter()
        .map(|u| add(&s, "g", u, u, None))
        .collect();
    // Collection order: c3, c1, c2 (and an unknown id).
    reorder_block(&mut s, "g", &[ids[5], ids[1], 9999, ids[3]]).unwrap();
    assert_eq!(uids(&s), ["u1", "c3", "c1", "c2", "u2", "u3"]);
    let prios: Vec<i64> = list_mods(s.conn(), "g")
        .unwrap()
        .iter()
        .map(|m| m.priority)
        .collect();
    assert_eq!(prios, [0, 1, 2, 3, 4, 5]);
}

// ── finish / record / remove ────────────────────────────────

fn finish_with(
    s: &mut Store,
    slug: &str,
    number: u64,
    m: &Manifest,
    ids: &BTreeMap<usize, i64>,
) -> Finished {
    let rev = revision(slug, number);
    let selected: Vec<usize> = ids.keys().copied().collect();
    let tmp = std::env::temp_dir();
    finish(
        s,
        &Finish {
            game: "g",
            revision: &rev,
            manifest: m,
            dir: &tmp,
            selected: &selected,
            ids,
            failed: &["Broken".to_string()],
        },
    )
    .unwrap()
}

#[test]
fn finish_orders_records_and_persists_rules() {
    let mut s = store_with_game("g");
    let other = add(&s, "g", "other", "other", None);
    let a = add(&s, "g", "A", "A", None);
    let b = add(&s, "g", "B", "B", None);
    let m = manifest(json!({
        "mods": [{"name": "A", "source": {"tag": "a"}}, {"name": "B", "source": {"tag": "b"}}],
        "modRules": [{"type": "before", "source": {"tag": "b"}, "reference": {"tag": "a"}}],
        "pluginRules": {"plugins": [{"name": "A.esp", "after": ["B.esp"]}]}
    }));
    let f = finish_with(&mut s, "abc", 3, &m, &BTreeMap::from([(0, a), (1, b)]));
    assert_eq!(f.ordered, [b, a]);
    assert_eq!(f.rules, 1);
    assert_eq!(uids(&s), ["other", "B", "A"]);
    let r = get(s.conn(), "g", "abc").unwrap().unwrap();
    assert_eq!((r.revision, r.mods.clone()), (3, vec![b, a]));
    assert_eq!(r.name.as_deref(), Some("Col abc"));
    assert_eq!(r.failed, ["Broken"]);
    assert_eq!(crate::mods::collections_of(s.conn(), a).unwrap(), ["abc"]);
    assert!(crate::mods::collections_of(s.conn(), other)
        .unwrap()
        .is_empty());
    // Rules persisted for the plugin stage.
    assert_eq!(
        mod_rules(s.conn(), "g", "abc")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        plugin_rules(s.conn(), "g").unwrap(),
        [PluginRule {
            collection: "abc".into(),
            plugin: "A.esp".into(),
            after: vec!["B.esp".into()]
        }]
    );
    let ext: String = s
        .conn()
        .query_row("SELECT external_resources FROM collections", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(ext.contains("skse.example"));
    // Listing counts members and enabled members.
    set_enabled(&mut s, "g", &[a], false).unwrap();
    let l = list(s.conn(), "g").unwrap();
    assert_eq!((l[0].record.mods.len(), l[0].enabled), (2, 1));
    // Re-recording keeps earlier members after the new ones.
    let f = finish_with(&mut s, "abc", 4, &m, &BTreeMap::from([(0, a)]));
    assert_eq!(f.ordered, [a]);
    assert_eq!(get(s.conn(), "g", "abc").unwrap().unwrap().mods, [a, b]);
}

#[test]
fn remove_uninstalls_exclusive_mods_and_keep_mods_keeps_them() {
    let mut s = store_with_game("g");
    let a = add(&s, "g", "A", "A", None);
    let shared = add(&s, "g", "S", "S", None);
    let m = manifest(json!({"mods": [{"name": "A"}, {"name": "S"}]}));
    finish_with(&mut s, "one", 1, &m, &BTreeMap::from([(0, a), (1, shared)]));
    finish_with(&mut s, "two", 1, &m, &BTreeMap::from([(1, shared)]));
    let (ex, sh) = exclusive_members(s.conn(), "g", "one").unwrap();
    assert_eq!((ex.len(), sh.len()), (1, 1));

    let r = remove(&mut s, "g", "one", false).unwrap();
    assert_eq!((r.uninstalled, r.kept), (vec!["A".to_string()], 1));
    assert_eq!(uids(&s), ["S"]);
    assert!(get(s.conn(), "g", "one").unwrap().is_none());
    assert_eq!(
        crate::mods::collections_of(s.conn(), shared).unwrap(),
        ["two"]
    );

    let r = remove(&mut s, "g", "two", true).unwrap();
    assert!(r.uninstalled.is_empty());
    assert_eq!(uids(&s), ["S"]);
    assert!(matches!(
        remove(&mut s, "g", "two", false),
        Err(CollectionError::NotInstalled(_))
    ));
}

#[test]
fn dropped_members_skips_kept_and_still_listed_mods() {
    let mut s = store_with_game("g");
    let keep = add(&s, "g", "keep", "keep", None);
    let opt = add(&s, "g", "opt", "opt", None);
    set_ids(&s, opt, None, Some(5), Some(50));
    let gone = add(&s, "g", "gone", "gone", None);
    set_ids(&s, gone, Some("dead"), None, None);
    let shared = add(&s, "g", "shared", "shared", None);
    let m = manifest(json!({"mods": [{"name": "x"}, {"name": "y"}, {"name": "z"}, {"name": "w"}]}));
    finish_with(
        &mut s,
        "c",
        1,
        &m,
        &BTreeMap::from([(0, keep), (1, opt), (2, gone), (3, shared)]),
    );
    finish_with(&mut s, "d", 1, &m, &BTreeMap::from([(3, shared)]));
    // The new revision still lists the optional mod (deselected now).
    let new = manifest(
        json!({"mods": [{"name": "o", "optional": true, "source": {"modId": 5, "fileId": 50}}]}),
    );
    let old = get(s.conn(), "g", "c").unwrap().unwrap().mods;
    let d = dropped_members(s.conn(), "g", "c", &old, &[keep], &new).unwrap();
    let ids = |v: &[ModRecord]| v.iter().map(|m| m.id).collect::<Vec<_>>();
    assert_eq!(ids(&d.exclusive), [gone]);
    assert_eq!(ids(&d.shared), [shared]);
    detach(&s, "g", "c", &[shared]).unwrap();
    assert_eq!(
        crate::mods::collections_of(s.conn(), shared).unwrap(),
        ["d"]
    );
}

// ── per-mod helpers ─────────────────────────────────────────

#[test]
fn install_meta_carries_collection_choices_and_ids() {
    let m = manifest(json!({"mods": [{
        "name": "Fancy", "version": "2.0", "author": "Ann", "details": {"category": "UI"},
        "source": {"type": "nexus", "modId": 7, "fileId": 70, "md5": "ABC", "updatePolicy": "exact"},
        "choices": {"type": "fomod", "options": [{"name": "Main", "groups": [{"name": "Body", "choices": [{"name": "Slim", "idx": 0}]}]}]}
    }]}));
    let meta = m.mods[0].install_meta(FomodMode::Collection);
    assert_eq!(meta.fomod_mode, FomodMode::Collection);
    assert!(meta.fomod_choices.is_some());
    assert_eq!((meta.nexus_mod_id, meta.nexus_file_id), (Some(7), Some(70)));
    assert_eq!(meta.md5.as_deref(), Some("abc"));
    assert_eq!(meta.update_policy.as_deref(), Some("exact"));
    assert_eq!(meta.category.as_deref(), Some("UI"));
    assert_eq!(meta.source.as_deref(), Some("nexus"));
    let plain = manifest(json!({"mods": [{"name": "P", "choices": null}]}));
    assert!(plain.mods[0]
        .install_meta(FomodMode::Collection)
        .fomod_choices
        .is_none());
}

#[test]
fn find_bundled_matches_prefix_case_insensitively() {
    let tmp = tempfile::tempdir().unwrap();
    let b = tmp.path().join("content/bundled");
    std::fs::create_dir_all(b.join("My Patch-1.0")).unwrap();
    std::fs::write(b.join("other.7z"), "x").unwrap();
    let m = manifest(json!({"mods": [
        {"name": "Patch", "source": {"type": "bundle", "fileExpression": "my patch"}},
        {"name": "OTHER", "source": {"type": "bundle"}},
        {"name": "Missing", "source": {"type": "bundle"}}
    ]}));
    assert_eq!(
        find_bundled(tmp.path(), &m.mods[0]),
        Some(b.join("My Patch-1.0"))
    );
    assert_eq!(
        find_bundled(tmp.path(), &m.mods[1]),
        Some(b.join("other.7z"))
    );
    assert_eq!(find_bundled(tmp.path(), &m.mods[2]), None);
}

#[test]
fn direct_file_name_uses_url_segment_or_name() {
    assert_eq!(
        direct_file_name("https://h/x/My%20Mod.zip?sig=1", "n"),
        "My Mod.zip"
    );
    assert_eq!(
        direct_file_name("https://h/dl/", "Cool Mod"),
        "Cool_Mod.archive"
    );
    assert_eq!(direct_file_name("https://h/%2e%2e", "Cool"), "Cool.archive");
}

#[test]
fn manifest_from_revision_lists_api_mod_files() {
    let rev = Revision::from_value(
        "g",
        "abc",
        json!({"revisionNumber": "2", "totalSize": "300",
               "collection": {"name": "N", "user": {"name": "U"}, "latestPublishedRevision": {"revisionNumber": 5}},
               "modFiles": [{"fileId": 11, "optional": true, "file": {"modId": 1, "name": "f", "version": "1", "sizeInBytes": "100", "mod": {"name": "Mod One", "author": "A"}}}]}),
    )
    .unwrap();
    assert_eq!(
        (rev.number, rev.latest, rev.total_size, rev.optional_count),
        (2, Some(5), 300, 1)
    );
    let m = Manifest::from_revision(&rev);
    let mm = &m.mods[0];
    assert_eq!(mm.name, "Mod One");
    assert!(mm.optional);
    assert_eq!(
        (mm.mod_id, mm.file_id, mm.file_size),
        (Some(1), Some(11), 100)
    );
    assert!(m.mod_rules().is_empty());
}
