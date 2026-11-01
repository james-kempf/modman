// SPDX-License-Identifier: GPL-3.0-or-later
//! Minimal path globs for loader definitions: `*` and `?` within one path
//! component, `**` as a whole component for any depth, `\` escapes the next
//! character. Matching is ASCII case-insensitive because the games are
//! Windows games and the deploy engine keys game paths the same way.

use serde::{Deserialize, Serialize};

/// A `/`-separated glob, relative to a root.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Glob(pub String);

impl Glob {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self(pattern.into())
    }

    /// A glob that matches exactly `path`.
    pub fn literal(path: &str) -> Self {
        let mut s = String::with_capacity(path.len());
        for c in path.chars() {
            if matches!(c, '*' | '?' | '\\') {
                s.push('\\');
            }
            s.push(c);
        }
        Self(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Refuses empty, absolute and `..`-escaping patterns.
    pub fn check(&self) -> Result<(), String> {
        let p = &self.0;
        if p.is_empty() {
            return Err("empty glob".into());
        }
        if p.starts_with('/') {
            return Err(format!("glob '{p}' must be relative"));
        }
        if p.split('/').any(|c| c == ".." || c == ".") {
            return Err(format!("glob '{p}' must not contain '.' or '..'"));
        }
        Ok(())
    }

    /// True when `path` (`/`-separated) matches the whole pattern.
    pub fn matches(&self, path: &str) -> bool {
        let pat: Vec<&str> = split(&self.0);
        let segs: Vec<&str> = split(path);
        match_segments(&pat, &segs)
    }

    /// True when `path` or any folder above it matches (so `BepInEx`
    /// matches `BepInEx/core/a.dll`).
    pub fn matches_or_contains(&self, path: &str) -> bool {
        let segs: Vec<&str> = split(path);
        let pat: Vec<&str> = split(&self.0);
        (1..=segs.len()).any(|n| match_segments(&pat, &segs[..n]))
    }
}

fn split(s: &str) -> Vec<&str> {
    s.split('/').filter(|c| !c.is_empty()).collect()
}

fn match_segments(pat: &[&str], segs: &[&str]) -> bool {
    match pat.split_first() {
        None => segs.is_empty(),
        Some((&"**", rest)) => (0..=segs.len()).any(|i| match_segments(rest, &segs[i..])),
        Some((p, rest)) => segs
            .split_first()
            .is_some_and(|(s, srest)| match_component(p, s) && match_segments(rest, srest)),
    }
}

/// One component, `*`/`?` wildcards with backslash escapes.
fn match_component(pat: &str, name: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let n: Vec<char> = name.chars().collect();
    fn go(p: &[char], n: &[char]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|i| go(rest, &n[i..])),
            Some(('?', rest)) => !n.is_empty() && go(rest, &n[1..]),
            Some(('\\', rest)) if !rest.is_empty() => eq_then(rest[0], &rest[1..], n),
            Some((c, rest)) => eq_then(*c, rest, n),
        }
    }
    fn eq_then(c: char, rest: &[char], n: &[char]) -> bool {
        n.first().is_some_and(|f| f.eq_ignore_ascii_case(&c)) && go(rest, &n[1..])
    }
    go(&p, &n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Catches: `*` crossing folders, `**` not matching zero folders, case
    /// sensitivity on Windows-game paths, and literal globs not escaping.
    #[test]
    fn glob_semantics() {
        let g = |p: &str, s: &str| Glob::new(p).matches(s);
        assert!(g("*.dll", "winhttp.dll"));
        assert!(!g("*.dll", "BepInEx/core/a.dll"));
        assert!(g("BepInEx/**", "BepInEx/core/a.dll"));
        assert!(g("**/a.dll", "a.dll"));
        assert!(g("BepInEx/**/a.dll", "BepInEx/a.dll"));
        assert!(g("WINHTTP.DLL", "winhttp.dll"));
        assert!(g(
            "skse64_?_*/skse64_loader.exe",
            "skse64_2_02/skse64_loader.exe"
        ));
        assert!(!g("a", "a/b"));
        assert!(Glob::literal("a*b?.txt").matches("a*b?.txt"));
        assert!(!Glob::literal("a*b.txt").matches("aXXb.txt"));
        assert!(Glob::new("BepInEx").matches_or_contains("BepInEx/core/a.dll"));
        assert!(!Glob::new("BepIn").matches_or_contains("BepInEx/core/a.dll"));
        for bad in ["", "/abs", "a/../b", "./a"] {
            assert!(Glob::new(bad).check().is_err(), "{bad}");
        }
    }
}
