// SPDX-License-Identifier: GPL-3.0-or-later
//! CLI tests for M5 collections against a local HTTP mock of the Nexus
//! GraphQL, collection-archive and download endpoints: install (FOMOD
//! choices, load-order rules, --optional, --revision), update (dropped
//! mods), remove [--keep-mods], download-only, list/info and nxm collection
//! links. No request leaves 127.0.0.1.

mod support;

use std::collections::BTreeMap;
use std::sync::Arc;

use predicates::prelude::*;
use predicates::str::contains;
use support::{zip_bytes, Env, Mock, Request, Response};

const G: &str = "testgame";
const KEY: &str = "CollectionsApiKey123";
const SLUG: &str = "abc123";

const FOMOD_XML: &str = r#"<config>
  <moduleName>Beta</moduleName>
  <installSteps order="Explicit">
    <installStep name="Main">
      <optionalFileGroups order="Explicit">
        <group name="Body" type="SelectExactlyOne">
          <plugins order="Explicit">
            <plugin name="Slim"><description>s</description>
              <files><file source="slim.esp" destination="body.esp"/></files>
              <typeDescriptor><type name="Optional"/></typeDescriptor></plugin>
            <plugin name="Curvy"><description>c</description>
              <files><file source="curvy.esp" destination="body.esp"/></files>
              <typeDescriptor><type name="Recommended"/></typeDescriptor></plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#;

/// Fake Nexus with collection `abc123`: revision 1 has Alpha, Beta (FOMOD),
/// optional Gamma and a bundled patch; revision 2 drops Alpha and adds Delta.
struct Fake {
    /// (mod id, file id) -> archive bytes.
    files: BTreeMap<(u64, u64), Vec<u8>>,
    /// revision -> collection archive bytes.
    archives: BTreeMap<u64, Vec<u8>>,
}

fn md5_of(bytes: &[u8]) -> String {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("f");
    std::fs::write(&p, bytes).unwrap();
    vessel_core::download::md5_file(&p).unwrap()
}

fn nexus_mod(name: &str, m: u64, f: u64, md5: &str, extra: &str) -> String {
    format!(
        r#"{{"name":"{name}","version":"1.0","author":"Ann"{extra},"source":{{"type":"nexus","modId":{m},"fileId":{f},"md5":"{md5}","fileSize":100,"tag":"t{m}"}}}}"#
    )
}

impl Fake {
    fn new() -> Self {
        let mut files = BTreeMap::new();
        files.insert((100, 1001), zip_bytes(&[("Data/alpha.esp", b"alpha")]));
        files.insert(
            (200, 2001),
            zip_bytes(&[
                ("Beta/fomod/ModuleConfig.xml", FOMOD_XML.as_bytes()),
                ("Beta/slim.esp", b"slim"),
                ("Beta/curvy.esp", b"curvy"),
            ]),
        );
        files.insert((300, 3001), zip_bytes(&[("Data/gamma.esp", b"gamma")]));
        files.insert((400, 4001), zip_bytes(&[("Data/delta.esp", b"delta")]));
        let bundled = zip_bytes(&[("Data/patch.esp", b"patch")]);
        let md5 = |k: (u64, u64)| md5_of(&files[&k]);
        let choices = r#","choices":{"type":"fomod","options":[{"name":"Main","groups":[{"name":"Body","choices":[{"name":"Slim","idx":0}]}]}]}"#;
        let alpha = nexus_mod("Alpha", 100, 1001, &md5((100, 1001)), "");
        let beta = nexus_mod("Beta", 200, 2001, &md5((200, 2001)), choices);
        let gamma = nexus_mod("Gamma", 300, 3001, &md5((300, 3001)), r#","optional":true"#);
        let delta = nexus_mod("Delta", 400, 4001, &md5((400, 4001)), "");
        let patch = format!(
            r#"{{"name":"Bundled Patch","source":{{"type":"bundle","logicalFilename":"Bundled Patch","md5":"{}"}}}}"#,
            md5_of(&bundled)
        );
        // Alpha loads after Beta.
        let rules = r#"[{"type":"after","source":{"tag":"t100"},"reference":{"tag":"t200"}},{"type":"requires","source":{"tag":"t100"},"reference":{"tag":"t300"}}]"#;
        let plugin_rules = r#"{"plugins":[{"name":"alpha.esp","after":["body.esp"]}]}"#;
        let cj = |mods: &[&str]| {
            format!(
                r#"{{"info":{{"name":"Test Col","installInstructions":"Read me"}},"mods":[{}],"modRules":{rules},"pluginRules":{plugin_rules}}}"#,
                mods.join(",")
            )
        };
        let mut archives = BTreeMap::new();
        for (rev, mods) in [
            (1, vec![&alpha, &beta, &gamma, &patch]),
            (2, vec![&beta, &gamma, &patch, &delta]),
        ] {
            let mods: Vec<&str> = mods.iter().map(|s| s.as_str()).collect();
            let json = cj(&mods);
            archives.insert(
                rev,
                zip_bytes(&[
                    ("collection.json", json.as_bytes()),
                    ("bundled/Bundled Patch-1.zip", &bundled),
                ]),
            );
        }
        Self { files, archives }
    }

    fn revision_body(&self, rev: u64) -> String {
        format!(
            r#"{{"data":{{"collectionRevision":{{"revisionNumber":{rev},"revisionStatus":"published",
            "downloadLink":"/v1/collections/{SLUG}/revisions/{rev}/download_link","totalSize":"500","modCount":4,
            "collection":{{"name":"Test Col","slug":"{SLUG}","summary":"A test","latestPublishedRevision":{{"revisionNumber":2}},
              "user":{{"name":"Curator"}},"game":{{"domainName":"{G}","name":"Test Game"}}}},
            "modFiles":[],"externalResources":[{{"name":"Tool X","resourceUrl":"https://tool.example/x"}}]}}}}}}"#
        )
    }

    fn handle(&self, req: &Request, base: &str) -> Response {
        let path = req.path().to_string();
        if path == "/v1/users/validate.json" {
            return Response::json(
                r#"{"user_id":7,"name":"tester","is_premium":true,"is_supporter":false}"#,
            );
        }
        if path == format!("/v1/games/{G}.json") {
            return Response::json(&format!(
                r#"{{"id":77,"name":"Test Game","domain_name":"{G}"}}"#
            ));
        }
        if path == "/v2/graphql" {
            if req.body.contains("collectionRevision") {
                let rev = if req.body.contains("\"revision\":1") {
                    1
                } else {
                    2
                };
                return Response::json(&self.revision_body(rev));
            }
            return Response::json(r#"{"data":{}}"#);
        }
        for rev in self.archives.keys() {
            if path == format!("/v1/collections/{SLUG}/revisions/{rev}/download_link") {
                return Response::json(&format!(
                    r#"{{"download_links":[{{"name":"CDN","short_name":"cdn","URI":"{base}/cdn/col{rev}.bin"}}]}}"#
                ));
            }
            if path == format!("/cdn/col{rev}.bin") {
                return Response::bytes(self.archives[rev].clone());
            }
        }
        for ((m, f), data) in &self.files {
            if path == format!("/v1/games/{G}/mods/md5_search/{}.json", md5_of(data)) {
                return Response::json(&format!(
                    r#"[{{"mod":{{"mod_id":{m},"name":"Alpha"}},"file_details":{{"file_id":{f},"version":"1.0"}}}}]"#
                ));
            }
            if path == format!("/v1/games/{G}/mods/{m}/files/{f}/download_link.json") {
                return Response::json(&format!(
                    r#"[{{"name":"Nexus CDN","short_name":"Nexus","URI":"{base}/cdn/m{m}/f{f}.zip"}}]"#
                ));
            }
            if path == format!("/cdn/m{m}/f{f}.zip") {
                return Response::bytes(data.clone());
            }
        }
        Response::status(404, r#"{"message":"not mocked"}"#)
    }
}

fn setup() -> (Env, Mock, std::path::PathBuf) {
    let fake = Arc::new(Fake::new());
    let mock = Mock::start(move |r, base| fake.handle(r, base));
    let mut env = Env::new();
    env.nexus = Some(mock.url.clone());
    env.cmd()
        .arg("login")
        .write_stdin(format!("{KEY}\n"))
        .assert()
        .success();
    let game = env.add_game(G);
    (env, mock, game)
}

fn graphql_bodies(mock: &Mock) -> Vec<String> {
    mock.requests()
        .into_iter()
        .filter(|r| r.path() == "/v2/graphql" && r.body.contains("collectionRevision"))
        .map(|r| r.body)
        .collect()
}

fn staged(env: &Env, uid: &str, file: &str) -> String {
    let p = env
        .home()
        .join(".local/share/vessel")
        .join(G)
        .join("staging")
        .join(uid)
        .join(file);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn record(env: &Env) -> Option<(i64, i64)> {
    let s = env.store();
    s.conn()
        .query_row(
            "SELECT c.revision, (SELECT COUNT(*) FROM collection_mods WHERE collection_id = c.id)
             FROM collections c WHERE slug = ?1",
            [SLUG],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
}

#[test]
fn collection_install_applies_choices_rules_and_skips_optional() {
    let (env, mock, game) = setup();
    env.run(&[
        "collection",
        "install",
        &format!("nxm://{G}/collections/{SLUG}/revisions/1"),
        "--deploy",
    ])
    .success()
    .stdout(
        contains("Skipping 1 optional mods")
            .and(contains("Read me"))
            .and(contains(
                "Applied the collection's load order (3 mods, 2 rules)",
            ))
            .and(contains("Installed  : 3"))
            .and(contains("Tool X")),
    );
    // Rule "Alpha after Beta" moved Alpha behind Beta; optional Gamma skipped.
    assert_eq!(
        env.uids(),
        ["Beta-200-2001", "Alpha-100-1001", "Bundled_Patch"]
    );
    // Recorded FOMOD choice (Slim) beat the installer default (Curvy).
    assert_eq!(staged(&env, "Beta-200-2001", "body.esp"), "slim");
    assert_eq!(
        env.query_i64("SELECT COUNT(*) FROM fomod_choices WHERE mode = 'choices'"),
        1
    );
    assert_eq!(std::fs::read(game.join("Data/body.esp")).unwrap(), b"slim");
    assert_eq!(record(&env), Some((1, 3)));
    // Rules persisted for the plugin stage.
    let s = env.store();
    let rules = vessel_core::collections::plugin_rules(s.conn(), G).unwrap();
    assert_eq!(rules[0].plugin, "alpha.esp");
    let bodies = graphql_bodies(&mock);
    assert!(bodies
        .iter()
        .all(|b| b.contains("\"revision\":1") && b.contains("\"viewAdultContent\":false")));

    // Re-running skips everything (md5 / modId-fileId dedupe).
    env.run(&[
        "collection",
        "install",
        &format!("nxm://{G}/collections/{SLUG}/revisions/1"),
    ])
    .success()
    .stdout(contains("Already installed : 3").and(contains("To install        : 0")));
    assert_eq!(env.uids().len(), 3);
}

#[test]
fn install_command_routes_collections_with_optional_all_and_revision() {
    let (env, mock, _) = setup();
    env.run(&["config", "adult", "true"]).success();
    env.run(&[
        "install",
        &format!("https://www.nexusmods.com/games/{G}/collections/{SLUG}"),
        "--optional",
        "all",
        "-r",
        "1",
    ])
    .success();
    assert!(env.uids().contains(&"Gamma-300-3001".to_string()));
    assert_eq!(env.uids().len(), 4);
    assert_eq!(record(&env), Some((1, 4)));
    let bodies = graphql_bodies(&mock);
    assert!(
        bodies
            .iter()
            .all(|b| b.contains("\"revision\":1") && b.contains("\"viewAdultContent\":true")),
        "{bodies:?}"
    );
}

#[test]
fn collection_update_removes_dropped_mods_then_remove_keep_mods_keeps_them() {
    let (env, _mock, _) = setup();
    env.run(&[
        "collection",
        "install",
        &format!("nxm://{G}/collections/{SLUG}/revisions/1"),
    ])
    .success();
    env.run(&["collection", "update", SLUG])
        .success()
        .stdout(contains("revision 1 → 2").and(contains("Removed 1 dropped mod(s)")));
    let uids = env.uids();
    assert!(!uids.contains(&"Alpha-100-1001".to_string()), "{uids:?}");
    assert!(uids.contains(&"Delta-400-4001".to_string()), "{uids:?}");
    assert_eq!(uids.len(), 3);
    assert_eq!(record(&env), Some((2, 3)));
    env.run(&["collection", "update", SLUG])
        .success()
        .stdout(contains("is up to date (revision 2)"));
    env.run(&["collection", "list"])
        .success()
        .stdout(contains("Test Col").and(contains("3 mods (3 enabled)")));

    env.run(&["collection", "remove", SLUG, "--keep-mods"])
        .success()
        .stdout(contains("removed"));
    assert_eq!(env.uids().len(), 3);
    assert_eq!(record(&env), None);
}

#[test]
fn collection_remove_uninstalls_exclusive_mods() {
    let (env, _mock, _) = setup();
    env.run(&[
        "collection",
        "install",
        &format!("nxm://{G}/collections/{SLUG}/revisions/1"),
    ])
    .success();
    env.run(&["collection", "rm", &format!("nxm://{G}/collections/{SLUG}")])
        .success()
        .stdout(contains("3 mod(s) will be uninstalled"));
    assert!(env.uids().is_empty());
    assert_eq!(record(&env), None);
}

#[test]
fn nxm_collection_link_installs_the_pinned_revision() {
    let (env, mock, _) = setup();
    env.run(&["nxm", &format!("nxm://{G}/collections/{SLUG}/revisions/1")])
        .success();
    assert_eq!(record(&env), Some((1, 3)));
    assert!(env.uids().contains(&"Alpha-100-1001".to_string()));
    assert!(graphql_bodies(&mock)
        .iter()
        .all(|b| b.contains("\"revision\":1")));
    assert_eq!(
        env.query_i64(&format!(
            "SELECT COUNT(*) FROM downloads WHERE collection_slug = '{SLUG}' AND collection_revision = 1 AND status = 'done'"
        )),
        1
    );
    assert!(env
        .calls("notify-send")
        .contains("Installed collection abc123"));
}

#[test]
fn download_only_fetches_archives_without_installing() {
    let (env, _mock, _) = setup();
    env.run(&[
        "download",
        &format!("nxm://{G}/collections/{SLUG}/revisions/2"),
    ])
    .success()
    .stdout(contains("Downloaded 3 archive(s)").and(contains("Install with:")));
    assert!(env.uids().is_empty());
    assert_eq!(record(&env), None);
    let dl = env
        .home()
        .join(".local/share/vessel")
        .join(G)
        .join("downloads");
    assert!(dl.join("f2001.zip").is_file() && dl.join("f4001.zip").is_file());
    env.run(&["collection", "dl", SLUG, "--optional", "all"])
        .success()
        .stdout(contains("Downloaded 4 archive(s)"));
}

#[test]
fn collection_info_and_bad_references() {
    let (env, _mock, _) = setup();
    env.run(&["collection", "info", SLUG])
        .success()
        .stdout(contains("Curator").and(contains("Not installed")));
    env.run(&["collection", "install", "not/a ref"])
        .failure()
        .stderr(contains("Cannot parse collection reference"));
    env.run(&["collection", "frobnicate"])
        .failure()
        .stderr(contains("Unknown collection command"));
    env.run(&["collection"])
        .success()
        .stdout(contains("No collections installed"));
}

#[test]
fn local_install_is_identified_by_md5_and_dedupes_in_collections() {
    let (env, _mock, _) = setup();
    let fake = Fake::new();
    let zip = env.tmp.path().join("archives/alpha-local.zip");
    std::fs::create_dir_all(zip.parent().unwrap()).unwrap();
    std::fs::write(&zip, &fake.files[&(100, 1001)]).unwrap();
    env.run(&["install", zip.to_str().unwrap()])
        .success()
        .stdout(contains("Identified on Nexus: Alpha (mod 100, file 1001)"));
    assert_eq!(env.uids(), ["Alpha-100-1001"]);
    assert_eq!(
        env.query_i64("SELECT COUNT(*) FROM mods WHERE source = 'nexus' AND nexus_file_id = 1001 AND md5 IS NOT NULL"),
        1
    );
    env.run(&[
        "collection",
        "install",
        &format!("nxm://{G}/collections/{SLUG}/revisions/1"),
    ])
    .success()
    .stdout(contains("Already installed : 1"));
    assert_eq!(env.uids().len(), 3);
}
