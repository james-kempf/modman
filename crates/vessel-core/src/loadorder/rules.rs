// SPDX-License-Identifier: GPL-3.0-or-later
//! Collection plugin rules (`pluginRules.plugins[] {name, after[]}`) applied
//! without LOOT: a stable topological sort. Used as the input order for LOOT
//! and as the fallback when LOOT cannot sort.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::collections::PluginRule;

/// Orders `names` so every plugin comes after the plugins its rules name
/// (when both are in `names`; names compare case-insensitively). Otherwise
/// the input order is kept. Plugins left in a rule cycle keep their input
/// order at the end.
pub fn apply_rules(names: &[String], rules: &[PluginRule]) -> Vec<String> {
    let index: HashMap<String, usize> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.to_lowercase(), i))
        .collect();
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); names.len()];
    let mut indeg = vec![0usize; names.len()];
    for r in rules {
        let Some(&p) = index.get(&r.plugin.to_lowercase()) else {
            continue;
        };
        for a in &r.after {
            if let Some(&b) = index.get(&a.to_lowercase()) {
                if b != p && !succ[b].contains(&p) {
                    succ[b].push(p);
                    indeg[p] += 1;
                }
            }
        }
    }
    let mut ready: BinaryHeap<Reverse<usize>> = (0..names.len())
        .filter(|&i| indeg[i] == 0)
        .map(Reverse)
        .collect();
    let mut done = vec![false; names.len()];
    let mut out = Vec::with_capacity(names.len());
    while let Some(Reverse(i)) = ready.pop() {
        done[i] = true;
        out.push(names[i].clone());
        for &s in &succ[i] {
            indeg[s] -= 1;
            if indeg[s] == 0 {
                ready.push(Reverse(s));
            }
        }
    }
    out.extend(
        (0..names.len())
            .filter(|&i| !done[i])
            .map(|i| names[i].clone()),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(plugin: &str, after: &[&str]) -> PluginRule {
        PluginRule {
            collection: "c".into(),
            plugin: plugin.into(),
            after: after.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Catches: an `after` rule being ignored or applied in reverse.
    #[test]
    fn after_rule_moves_plugin_behind_its_dependency_case_insensitively() {
        let out = apply_rules(
            &names(&["Patch.esp", "A.esp", "B.esp"]),
            &[rule("patch.ESP", &["b.esp"])],
        );
        assert_eq!(out, names(&["A.esp", "B.esp", "Patch.esp"]));
    }

    /// Catches: a cycle dropping plugins or looping forever.
    #[test]
    fn cycle_keeps_every_plugin_in_input_order() {
        let out = apply_rules(
            &names(&["X.esp", "A.esp", "B.esp"]),
            &[rule("A.esp", &["B.esp"]), rule("B.esp", &["A.esp"])],
        );
        assert_eq!(out, names(&["X.esp", "A.esp", "B.esp"]));
    }

    /// Catches: rules naming plugins that are not present breaking the sort.
    #[test]
    fn rules_for_absent_plugins_are_ignored() {
        let out = apply_rules(
            &names(&["A.esp", "B.esp"]),
            &[
                rule("Missing.esp", &["A.esp"]),
                rule("A.esp", &["Gone.esm"]),
            ],
        );
        assert_eq!(out, names(&["A.esp", "B.esp"]));
    }
}
