// SPDX-License-Identifier: GPL-3.0-or-later
//! Typed ModuleConfig.xml (FOMOD schema 5.x) as legacy `fomod_py` reads it.

use serde::Serialize;

use super::xml::El;

/// How a group's options may be selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum GroupType {
    SelectExactlyOne,
    SelectAtMostOne,
    SelectAtLeastOne,
    SelectAll,
    /// Also every unknown type (legacy default `SelectAny`, L1243).
    SelectAny,
}

impl GroupType {
    fn parse(s: Option<&str>) -> Self {
        match s {
            Some("SelectExactlyOne") => Self::SelectExactlyOne,
            Some("SelectAtMostOne") => Self::SelectAtMostOne,
            Some("SelectAtLeastOne") => Self::SelectAtLeastOne,
            Some("SelectAll") => Self::SelectAll,
            _ => Self::SelectAny,
        }
    }

    /// Short user-facing hint (legacy `GROUP_LABEL`).
    pub fn label(self) -> &'static str {
        match self {
            Self::SelectExactlyOne => "pick one",
            Self::SelectAtMostOne => "pick one or none",
            Self::SelectAtLeastOne => "pick at least one",
            Self::SelectAll => "all included",
            Self::SelectAny => "pick any",
        }
    }
}

/// Plugin type; unknown names count as `Optional` (legacy L1033-1049).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PluginType {
    Required,
    Optional,
    Recommended,
    NotUsable,
    CouldBeUsable,
}

impl PluginType {
    fn parse(s: Option<&str>) -> Self {
        match s {
            Some("Required") => Self::Required,
            Some("Recommended") => Self::Recommended,
            Some("NotUsable") => Self::NotUsable,
            Some("CouldBeUsable") => Self::CouldBeUsable,
            _ => Self::Optional,
        }
    }

    /// Short user-facing label; `None` for `Optional`.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Required => Some("required"),
            Self::Recommended => Some("recommended"),
            Self::NotUsable => Some("not usable"),
            Self::CouldBeUsable => Some("could be usable"),
            Self::Optional => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Operator {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FileState {
    Active,
    Missing,
    /// `Inactive` and anything else: always false (legacy L1022).
    Other,
}

/// One term of a condition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Dependency {
    /// True when flag `name` currently equals `value` (unset flags are "").
    Flag {
        name: String,
        value: String,
    },
    /// Checked against the live deploy folder.
    File {
        file: String,
        state: FileState,
    },
    Composite(Condition),
    /// Game / script extender / fomm version checks: always true.
    Other,
}

/// `dependencies` / `visible` / `compositeDependency`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Condition {
    pub operator: Operator,
    pub terms: Vec<Dependency>,
}

/// One `file` or `folder` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileEntry {
    /// Path inside the FOMOD root (Windows separators allowed).
    pub source: String,
    /// `None` when the attribute is absent (then the source path is kept).
    pub destination: Option<String>,
    pub folder: bool,
    pub priority: i64,
    pub always_install: bool,
    pub install_if_usable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum TypeDescriptor {
    Fixed(PluginType),
    /// First pattern whose condition holds wins, else `default`.
    Dependent {
        patterns: Vec<(Condition, PluginType)>,
        default: PluginType,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plugin {
    pub name: String,
    pub description: String,
    /// Image path inside the FOMOD root, for the GUI wizard.
    pub image: Option<String>,
    pub type_descriptor: TypeDescriptor,
    /// `conditionFlags`: (name, value) set when the plugin is selected.
    pub flags: Vec<(String, String)>,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Group {
    pub name: String,
    pub kind: GroupType,
    /// In display order (the `order` attribute already applied).
    pub plugins: Vec<Plugin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub name: String,
    pub visible: Option<Condition>,
    pub groups: Vec<Group>,
}

/// A parsed ModuleConfig.xml.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleConfig {
    pub module_name: String,
    pub module_image: Option<String>,
    pub required_files: Vec<FileEntry>,
    pub steps: Vec<Step>,
    /// `conditionalFileInstalls` patterns.
    pub conditional_files: Vec<(Condition, Vec<FileEntry>)>,
}

/// Children of `parent` named `tag`, sorted per its `order` attribute
/// (Explicit, Ascending (default) or Descending by lower-cased name).
fn ordered<'a>(parent: Option<&'a El>, tag: &str) -> Vec<&'a El> {
    let Some(parent) = parent else {
        return Vec::new();
    };
    let mut items: Vec<&El> = parent.all(tag);
    match parent.attr("order").unwrap_or("Ascending") {
        "Explicit" => {}
        order => {
            items.sort_by_key(|e| e.attr("name").unwrap_or("").to_lowercase());
            if order != "Ascending" {
                items.reverse();
            }
        }
    }
    items
}

fn condition(el: &El) -> Condition {
    let operator = match el.attr("operator").map(str::to_lowercase).as_deref() {
        Some("or") => Operator::Or,
        _ => Operator::And,
    };
    let terms = el
        .children
        .iter()
        .map(|c| match c.tag.as_str() {
            "flagDependency" => Dependency::Flag {
                name: c.attr("flag").unwrap_or("").into(),
                value: c.attr("value").unwrap_or("").into(),
            },
            "fileDependency" => Dependency::File {
                file: c.attr("file").unwrap_or("").into(),
                state: match c.attr("state").unwrap_or("Active") {
                    "Active" => FileState::Active,
                    "Missing" => FileState::Missing,
                    _ => FileState::Other,
                },
            },
            "dependencies" | "compositeDependency" => Dependency::Composite(condition(c)),
            _ => Dependency::Other,
        })
        .collect();
    Condition { operator, terms }
}

fn files(el: Option<&El>) -> Vec<FileEntry> {
    el.map(|el| {
        el.children
            .iter()
            .filter(|f| f.tag == "file" || f.tag == "folder")
            .map(|f| {
                let yes = |n| f.attr(n).is_some_and(|v| v.eq_ignore_ascii_case("true"));
                FileEntry {
                    source: f.attr("source").unwrap_or("").into(),
                    destination: f.attr("destination").map(str::to_string),
                    folder: f.tag == "folder",
                    priority: f
                        .attr("priority")
                        .and_then(|p| p.trim().parse().ok())
                        .unwrap_or(0),
                    always_install: yes("alwaysInstall"),
                    install_if_usable: yes("installIfUsable"),
                }
            })
            .collect()
    })
    .unwrap_or_default()
}

fn type_descriptor(p: &El) -> TypeDescriptor {
    let Some(td) = p.child("typeDescriptor") else {
        return TypeDescriptor::Fixed(PluginType::Optional);
    };
    if let Some(t) = td.child("type") {
        return TypeDescriptor::Fixed(PluginType::parse(t.attr("name")));
    }
    let Some(dt) = td.child("dependencyType") else {
        return TypeDescriptor::Fixed(PluginType::Optional);
    };
    let patterns = dt
        .child("patterns")
        .map(|ps| {
            ps.all("pattern")
                .into_iter()
                .map(|pat| {
                    let cond = pat.child("dependencies").map_or(
                        Condition {
                            operator: Operator::And,
                            terms: Vec::new(),
                        },
                        condition,
                    );
                    let ty = PluginType::parse(pat.child("type").and_then(|t| t.attr("name")));
                    (cond, ty)
                })
                .collect()
        })
        .unwrap_or_default();
    TypeDescriptor::Dependent {
        patterns,
        default: PluginType::parse(dt.child("defaultType").and_then(|t| t.attr("name"))),
    }
}

fn plugin(p: &El) -> Plugin {
    Plugin {
        name: p.attr("name").unwrap_or("").into(),
        description: p.child_text("description").unwrap_or("").trim().into(),
        image: p
            .child("image")
            .and_then(|i| i.attr("path"))
            .map(str::to_string),
        type_descriptor: type_descriptor(p),
        flags: p
            .child("conditionFlags")
            .map(|cf| {
                cf.all("flag")
                    .into_iter()
                    .map(|f| (f.attr("name").unwrap_or("").into(), f.text.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        files: files(p.child("files")),
    }
}

impl ModuleConfig {
    pub(crate) fn from_root(root: &El) -> Self {
        let steps = ordered(root.child("installSteps"), "installStep")
            .into_iter()
            .map(|s| Step {
                name: s.attr("name").unwrap_or("").into(),
                visible: s.child("visible").map(condition),
                groups: ordered(s.child("optionalFileGroups"), "group")
                    .into_iter()
                    .map(|g| Group {
                        name: g.attr("name").unwrap_or("").into(),
                        kind: GroupType::parse(g.attr("type")),
                        plugins: ordered(g.child("plugins"), "plugin")
                            .into_iter()
                            .map(plugin)
                            .collect(),
                    })
                    .collect(),
            })
            .collect();
        let conditional_files = root
            .child("conditionalFileInstalls")
            .and_then(|c| c.child("patterns"))
            .map(|ps| {
                ps.all("pattern")
                    .into_iter()
                    .map(|p| {
                        let cond = p.child("dependencies").map_or(
                            Condition {
                                operator: Operator::And,
                                terms: Vec::new(),
                            },
                            condition,
                        );
                        (cond, files(p.child("files")))
                    })
                    .collect()
            })
            .unwrap_or_default();
        ModuleConfig {
            module_name: root.child_text("moduleName").unwrap_or("").trim().into(),
            module_image: root
                .child("moduleImage")
                .and_then(|i| i.attr("path"))
                .map(str::to_string),
            required_files: files(root.child("requiredInstallFiles")),
            steps,
            conditional_files,
        }
    }
}
