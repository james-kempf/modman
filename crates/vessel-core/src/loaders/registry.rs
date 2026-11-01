// SPDX-License-Identifier: GPL-3.0-or-later
//! The loaders available for one game: its declarative `[[loaders]]` plus
//! any Rust plugins core registers (SMAPI, ...).

use super::declarative::DeclarativeLoader;
use super::types::{ArchiveProbe, GameCtx, LoaderMatch};
use super::{LoaderError, LoaderPlugin};
use crate::games::definitions::GameDefinition;

/// Loader plugins by key, in registration order.
#[derive(Default)]
pub struct LoaderRegistry {
    plugins: Vec<Box<dyn LoaderPlugin>>,
}

impl std::fmt::Debug for LoaderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.keys()).finish()
    }
}

impl LoaderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The declarative loaders of `def`.
    pub fn for_definition(def: &GameDefinition) -> Result<Self, LoaderError> {
        let mut r = Self::new();
        for l in &def.loaders {
            r.register(Box::new(DeclarativeLoader::new(l.clone())))?;
        }
        Ok(r)
    }

    /// Declarative loaders of `def` plus the Rust plugins Vessel ships for
    /// that game definition (`definition_id` = `games.definition`).
    pub fn for_game(
        def: Option<&GameDefinition>,
        definition_id: Option<&str>,
    ) -> Result<Self, LoaderError> {
        let mut r = match def {
            Some(d) => Self::for_definition(d)?,
            None => Self::new(),
        };
        if definition_id == Some(super::smapi::NEXUS_DOMAIN) {
            r.register(Box::new(super::smapi::Smapi))?;
        }
        Ok(r)
    }

    /// Adds a plugin; keys are unique.
    pub fn register(&mut self, plugin: Box<dyn LoaderPlugin>) -> Result<(), LoaderError> {
        if self.get(plugin.key()).is_some() {
            return Err(LoaderError::DuplicateKey(plugin.key().to_string()));
        }
        self.plugins.push(plugin);
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&dyn LoaderPlugin> {
        self.plugins
            .iter()
            .find(|p| p.key() == key)
            .map(|p| p.as_ref())
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.plugins.iter().map(|p| p.key())
    }

    /// The single loader claiming `probe`. Two claims are an error rather
    /// than a guess, because the wrong loader would write the wrong files.
    pub fn match_archive(
        &self,
        game: &GameCtx,
        probe: &ArchiveProbe,
    ) -> Result<Option<(&dyn LoaderPlugin, LoaderMatch)>, LoaderError> {
        let mut found: Option<(&dyn LoaderPlugin, LoaderMatch)> = None;
        for p in &self.plugins {
            if let Some(m) = p.matches(game, probe) {
                if let Some((first, _)) = &found {
                    return Err(LoaderError::AmbiguousMatch(
                        first.key().to_string(),
                        p.key().to_string(),
                    ));
                }
                found = Some((p.as_ref(), m));
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::definitions::parse_definition;
    use crate::loaders::declarative::tests::ctx;
    use crate::loaders::declarative::tests::{probe, BEPINEX_TOML};

    /// Catches: archives routed to no loader or to the wrong one, a second
    /// claimant being ignored, and duplicate keys shadowing each other.
    #[test]
    fn match_archive_picks_one_loader_or_fails() {
        let def = parse_definition(BEPINEX_TOML, "f.toml").unwrap();
        let mut r = LoaderRegistry::for_definition(&def).unwrap();
        let g = ctx("valheim");
        let files = ["winhttp.dll", "BepInEx/core/BepInEx.Preloader.dll"];
        let (p, m) = r.match_archive(&g, &probe(&files, None)).unwrap().unwrap();
        assert_eq!((p.key(), m.loader_key.as_str()), ("bepinex", "bepinex"));
        assert!(r
            .match_archive(&g, &probe(&["Mod.dll"], None))
            .unwrap()
            .is_none());

        let mut twin = def.loaders[0].clone();
        assert!(matches!(
            r.register(Box::new(DeclarativeLoader::new(twin.clone()))),
            Err(LoaderError::DuplicateKey(_))
        ));
        twin.key = "bepinex-twin".into();
        r.register(Box::new(DeclarativeLoader::new(twin))).unwrap();
        assert!(matches!(
            r.match_archive(&g, &probe(&files, None)),
            Err(LoaderError::AmbiguousMatch(..))
        ));
    }
}
