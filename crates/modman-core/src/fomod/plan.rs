// SPDX-License-Identifier: GPL-3.0-or-later
//! Option selection and the install plan (legacy `fomod_py` L1006-1283).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::apply::lookup;
use super::model::{
    Condition, Dependency, FileEntry, FileState, Group, GroupType, ModuleConfig, Operator,
    PluginType, Step, TypeDescriptor,
};
use super::{FomodChoices, FomodError, GroupChoice, OptionChoice, ResolvedMode, StepChoice};

/// One group put to a [`FomodResolver`].
#[derive(Debug, Clone, Copy)]
pub struct GroupPrompt<'a> {
    pub module_name: &'a str,
    pub step: &'a str,
    pub group: &'a Group,
    /// Plugin types as evaluated now (flags set by earlier groups apply).
    pub types: &'a [PluginType],
    /// Indices the installer would pick by default.
    pub defaults: &'a [usize],
    /// Why the previous answer for this group was rejected.
    pub error: Option<&'a str>,
}

/// Supplies interactive choices: the CLI prompt and the GUI wizard.
pub trait FomodResolver {
    /// Called once before the first group.
    fn begin(&mut self, _module: &ModuleConfig) {}
    /// Called when a visible step starts.
    fn step(&mut self, _step: &Step) {}
    /// Selected plugin indices (0-based into `group.plugins`). An invalid
    /// answer is put to the resolver again with `error` set. Return
    /// [`FomodError::Cancelled`] to abort the install.
    fn choose(&mut self, prompt: &GroupPrompt<'_>) -> Result<Vec<usize>, FomodError>;
}

/// How options are picked.
pub enum Selection<'a> {
    Default,
    /// Recorded choices (collection / reuse); groups without an entry use defaults.
    Choices(&'a FomodChoices),
    Ask(&'a mut dyn FomodResolver),
}

/// A file or folder to copy, in application order (later entries win).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    pub source: String,
    pub destination: Option<String>,
    pub priority: i64,
}

/// Result of [`build_plan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    /// Sorted by (priority, document order).
    pub files: Vec<PlannedFile>,
    /// What was selected, in Vortex format.
    pub choices: FomodChoices,
    /// How the options were picked.
    pub mode: ResolvedMode,
    /// Condition flags at the end of the installer.
    pub flags: BTreeMap<String, String>,
}

/// Invalid answers tolerated per group before giving up.
const MAX_ATTEMPTS: usize = 25;

struct Eval<'a> {
    flags: BTreeMap<String, String>,
    deploy_dir: Option<&'a Path>,
}

impl Eval<'_> {
    fn holds(&self, c: &Condition) -> bool {
        if c.terms.is_empty() {
            return true;
        }
        let mut it = c.terms.iter().map(|t| match t {
            Dependency::Flag { name, value } => {
                self.flags.get(name).map_or("", String::as_str) == value
            }
            Dependency::File { file, state } => {
                let present = self
                    .deploy_dir
                    .filter(|d| d.is_dir())
                    .and_then(|d| lookup(d, file, true))
                    .is_some_and(|p| p.exists());
                match state {
                    FileState::Active => present,
                    FileState::Missing => !present,
                    FileState::Other => false,
                }
            }
            Dependency::Composite(c) => self.holds(c),
            Dependency::Other => true,
        });
        match c.operator {
            Operator::And => it.all(|b| b),
            Operator::Or => it.any(|b| b),
        }
    }

    fn plugin_type(&self, td: &TypeDescriptor) -> PluginType {
        match td {
            TypeDescriptor::Fixed(t) => *t,
            TypeDescriptor::Dependent { patterns, default } => patterns
                .iter()
                .find(|(c, _)| self.holds(c))
                .map_or(*default, |(_, t)| *t),
        }
    }
}

/// Default picks per group type (legacy `defaults`, L1061-1074).
pub fn default_selection(kind: GroupType, types: &[PluginType]) -> Vec<usize> {
    let idx = |f: &dyn Fn(PluginType) -> bool| -> Vec<usize> {
        (0..types.len()).filter(|&i| f(types[i])).collect()
    };
    let req = idx(&|t| t == PluginType::Required);
    let rec = idx(&|t| t == PluginType::Recommended);
    let usable = idx(&|t| t != PluginType::NotUsable);
    let union = || -> Vec<usize> {
        req.iter()
            .chain(&rec)
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    match kind {
        GroupType::SelectAll => (0..types.len()).collect(),
        GroupType::SelectExactlyOne => [&req, &rec, &usable]
            .into_iter()
            .find(|v| !v.is_empty())
            .map_or_else(|| (0..types.len().min(1)).collect(), |v| v[..1].to_vec()),
        GroupType::SelectAtMostOne => [&req, &rec]
            .into_iter()
            .find(|v| !v.is_empty())
            .map_or_else(Vec::new, |v| v[..1].to_vec()),
        GroupType::SelectAtLeastOne => {
            let u = union();
            if u.is_empty() {
                usable.into_iter().take(1).collect()
            } else {
                u
            }
        }
        GroupType::SelectAny => union(),
    }
}

/// Checks an interactive answer (legacy `ask_group` L1196-1211): Required
/// plugins are added, NotUsable ones and group-type violations rejected.
pub fn validate_selection(
    kind: GroupType,
    types: &[PluginType],
    picked: &[usize],
) -> Result<Vec<usize>, String> {
    if kind == GroupType::SelectAll {
        return Ok((0..types.len()).collect());
    }
    if let Some(bad) = picked.iter().find(|&&i| i >= types.len()) {
        return Err(format!("option {} does not exist", bad + 1));
    }
    let sel: BTreeSet<usize> = picked
        .iter()
        .copied()
        .chain((0..types.len()).filter(|&i| types[i] == PluginType::Required))
        .collect();
    let bad: Vec<String> = sel
        .iter()
        .filter(|&&i| types[i] == PluginType::NotUsable)
        .map(|i| (i + 1).to_string())
        .collect();
    if !bad.is_empty() {
        return Err(format!("option(s) {} cannot be used", bad.join(", ")));
    }
    match kind {
        GroupType::SelectExactlyOne if sel.len() != 1 => Err("choose exactly one".into()),
        GroupType::SelectAtMostOne if sel.len() > 1 => Err("choose at most one".into()),
        GroupType::SelectAtLeastOne if sel.is_empty() => Err("choose at least one".into()),
        _ => Ok(sel.into_iter().collect()),
    }
}

fn find_named<'a, T>(items: &'a [T], name: &str, key: impl Fn(&T) -> &str) -> Option<&'a T> {
    items.iter().find(|i| key(i) == name).or_else(|| {
        let low = name.to_lowercase();
        items.iter().find(|i| key(i).to_lowercase() == low)
    })
}

/// Recorded choices applied to a group (legacy `from_preset`, L1088-1110).
fn from_preset(
    gc: &GroupChoice,
    group: &Group,
    types: &[PluginType],
    dflt: &[usize],
) -> Vec<usize> {
    let n = group.plugins.len();
    let mut sel = BTreeSet::new();
    for ch in &gc.choices {
        let hits: Vec<usize> = (0..n)
            .filter(|&i| group.plugins[i].name == ch.name)
            .collect();
        let idx = ch.idx.and_then(|i| usize::try_from(i).ok());
        match (hits.as_slice(), idx) {
            ([_, _, ..], Some(i)) if hits.contains(&i) => {
                sel.insert(i);
            }
            ([first, ..], _) => {
                sel.insert(*first);
            }
            ([], Some(i)) if i < n => {
                sel.insert(i);
            }
            _ => {}
        }
    }
    sel.extend((0..n).filter(|&i| types[i] == PluginType::Required));
    if group.kind == GroupType::SelectAll {
        sel = (0..n).collect();
    }
    sel.retain(|&i| types[i] != PluginType::NotUsable);
    match group.kind {
        GroupType::SelectExactlyOne if sel.len() != 1 => {
            sel = match sel.first() {
                Some(&f) => [f].into(),
                None => dflt.iter().copied().collect(),
            }
        }
        GroupType::SelectAtMostOne if sel.len() > 1 => {
            sel = [*sel.first().expect("non-empty")].into();
        }
        GroupType::SelectAtLeastOne if sel.is_empty() => sel = dflt.iter().copied().collect(),
        _ => {}
    }
    sel.into_iter().collect()
}

fn add_files(
    out: &mut Vec<(i64, usize, PlannedFile)>,
    files: &[FileEntry],
    keep: impl Fn(&FileEntry) -> bool,
) {
    for f in files.iter().filter(|f| keep(f)) {
        let seq = out.len();
        out.push((
            f.priority,
            seq,
            PlannedFile {
                source: f.source.clone(),
                destination: f.destination.clone(),
                priority: f.priority,
            },
        ));
    }
}

/// Walks the installer: evaluates visibility and plugin types, picks options
/// per `how`, sets condition flags and collects files (legacy L1219-1283).
/// `deploy_dir` is the live deploy folder for `fileDependency` checks.
pub fn build_plan(
    cfg: &ModuleConfig,
    how: Selection<'_>,
    deploy_dir: Option<&Path>,
) -> Result<InstallPlan, FomodError> {
    let (mode, preset, mut resolver) = match how {
        Selection::Default => (ResolvedMode::Default, None, None),
        // Choices without any options fall back to defaults (legacy L1112-1120).
        Selection::Choices(c) if c.options.is_empty() => (ResolvedMode::Default, None, None),
        Selection::Choices(c) => (ResolvedMode::Choices, Some(c), None),
        Selection::Ask(r) => (ResolvedMode::Ask, None, Some(r)),
    };
    let mut ev = Eval {
        flags: BTreeMap::new(),
        deploy_dir,
    };
    let mut files = Vec::new();
    add_files(&mut files, &cfg.required_files, |_| true);
    if let Some(r) = resolver.as_deref_mut() {
        r.begin(cfg);
    }
    let mut record = Vec::new();
    for step in &cfg.steps {
        if !step.visible.as_ref().is_none_or(|c| ev.holds(c)) {
            continue;
        }
        if let Some(r) = resolver.as_deref_mut() {
            r.step(step);
        }
        let pstep = preset.and_then(|p| find_named(&p.options, &step.name, |s| &s.name));
        let mut rgroups = Vec::new();
        for group in &step.groups {
            if group.plugins.is_empty() {
                continue;
            }
            let types: Vec<PluginType> = group
                .plugins
                .iter()
                .map(|p| ev.plugin_type(&p.type_descriptor))
                .collect();
            let dflt = default_selection(group.kind, &types);
            let sel = if let Some(r) = resolver.as_deref_mut() {
                ask(r, cfg, step, group, &types, &dflt)?
            } else if let Some(gc) =
                pstep.and_then(|s| find_named(&s.groups, &group.name, |g| &g.name))
            {
                from_preset(gc, group, &types, &dflt)
            } else {
                dflt.clone()
            };
            rgroups.push(GroupChoice {
                name: group.name.clone(),
                choices: sel
                    .iter()
                    .map(|&i| OptionChoice {
                        name: group.plugins[i].name.clone(),
                        idx: Some(i as i64),
                    })
                    .collect(),
            });
            for (i, p) in group.plugins.iter().enumerate() {
                if sel.contains(&i) {
                    for (k, v) in &p.flags {
                        ev.flags.insert(k.clone(), v.clone());
                    }
                    add_files(&mut files, &p.files, |_| true);
                } else {
                    let usable = types[i] != PluginType::NotUsable;
                    add_files(&mut files, &p.files, |f| {
                        f.always_install || (usable && f.install_if_usable)
                    });
                }
            }
        }
        record.push(StepChoice {
            name: step.name.clone(),
            groups: rgroups,
        });
    }
    for (cond, fs) in &cfg.conditional_files {
        if ev.holds(cond) {
            add_files(&mut files, fs, |_| true);
        }
    }
    files.sort_by_key(|(p, s, _)| (*p, *s));
    Ok(InstallPlan {
        files: files.into_iter().map(|(_, _, f)| f).collect(),
        choices: FomodChoices::new(record),
        mode,
        flags: ev.flags,
    })
}

fn ask(
    r: &mut dyn FomodResolver,
    cfg: &ModuleConfig,
    step: &Step,
    group: &Group,
    types: &[PluginType],
    dflt: &[usize],
) -> Result<Vec<usize>, FomodError> {
    if group.kind == GroupType::SelectAll {
        return Ok((0..types.len()).collect());
    }
    let mut error: Option<String> = None;
    for _ in 0..MAX_ATTEMPTS {
        let picked = r.choose(&GroupPrompt {
            module_name: &cfg.module_name,
            step: &step.name,
            group,
            types,
            defaults: dflt,
            error: error.as_deref(),
        })?;
        match validate_selection(group.kind, types, &picked) {
            Ok(sel) => return Ok(sel),
            Err(e) => error = Some(e),
        }
    }
    Err(FomodError::InvalidSelection {
        group: group.name.clone(),
        reason: error.unwrap_or_default(),
    })
}
