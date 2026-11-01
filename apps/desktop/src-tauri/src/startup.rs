// SPDX-License-Identifier: GPL-3.0-or-later
//! The read-only auto-update check, run once per app start
//! (`autoupdate::startup_check`: which deployed games a run would update or
//! redeploy, and what blocks them).

use vessel_core::app::loaders::{self as apploaders, LoaderEnv, Recovered};
use vessel_core::autoupdate::{self, Host, RunOptions, StartupCheck};
use vessel_core::config::Config;
use vessel_core::loaders::housekeeping;

use crate::state::Inner;

/// Loader startup, once per app start and before any loader command can
/// race it: finishes runs a crash left (`recover`: kills a stale installer
/// group, records its changes and offers a rollback; never starts an
/// installer), then prunes logs, scratch dirs and unreferenced archives.
/// Returns the recovered runs (empty when there were none or it failed;
/// failures are logged).
pub fn loader_recovery(inner: &Inner) -> Vec<Recovered> {
    inner
        .loader_recovered
        .get_or_init(|| {
            let res = (|| -> vessel_core::app::R<Vec<Recovered>> {
                let mut store = inner.open_store()?;
                let cfg = Config::load(&store, &inner.paths)?;
                let env = LoaderEnv::new(&inner.paths, &cfg.data_dir);
                let recovered = apploaders::recover(&env, &mut store)?;
                match housekeeping::run(&env, &store) {
                    Ok(r) => {
                        for e in &r.errors {
                            tracing::warn!("loader housekeeping: {e}");
                        }
                        if let Some(why) = &r.skipped {
                            tracing::info!("loader housekeeping skipped: {why}");
                        }
                    }
                    Err(e) => tracing::warn!("loader housekeeping failed: {e}"),
                }
                Ok(recovered)
            })();
            res.unwrap_or_else(|e| {
                tracing::warn!("loader startup recovery failed: {e}");
                Vec::new()
            })
        })
        .clone()
}

/// Runs the check on first use and returns the cached result afterwards.
/// `None` when it failed (logged).
pub fn startup_check(inner: &Inner, host: &dyn Host) -> Option<StartupCheck> {
    inner
        .startup
        .get_or_init(|| {
            let res = (|| -> vessel_core::app::R<StartupCheck> {
                let store = inner.open_store()?;
                let opts = RunOptions::new(&Config::load(&store, &inner.paths)?, &inner.paths);
                Ok(autoupdate::startup_check(store.conn(), host, &opts)?)
            })();
            res.map_err(|e| tracing::warn!("startup auto-update check failed: {e}"))
                .ok()
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::state::AppState;
    use vessel_core::autoupdate::{SteamState, SystemSnapshot};
    use vessel_core::games::GameConfig;
    use vessel_core::paths::Paths;

    /// Counts how often the machine is inspected.
    #[derive(Default)]
    struct CountingHost {
        snapshots: AtomicUsize,
    }

    impl Host for CountingHost {
        fn now(&self) -> i64 {
            1_000_000
        }
        fn system_snapshot(&self) -> SystemSnapshot {
            self.snapshots.fetch_add(1, Ordering::SeqCst);
            SystemSnapshot {
                fingerprint: "f".into(),
                newest_change: 0,
            }
        }
        fn package_manager_busy(&self) -> Option<String> {
            None
        }
        fn steam_state(&self, _: &GameConfig) -> Option<SteamState> {
            None
        }
        fn game_running(&self, _: &GameConfig) -> bool {
            false
        }
    }

    /// Catches: recovery running more than once per start (a second pass
    /// would act on runs the first one already finished) or failing on a
    /// fresh install with no loader tables in use.
    #[test]
    fn loader_recovery_runs_once_and_is_empty_on_a_fresh_profile() {
        let home = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::for_home(home.path()));
        assert!(loader_recovery(&st.0).is_empty());
        assert!(st.0.loader_recovered.get().is_some());
        assert!(loader_recovery(&st.0).is_empty());
    }

    #[test]
    fn startup_runs_the_autoupdate_check_once() {
        let home = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::for_home(home.path()));
        let host = CountingHost::default();
        let first = startup_check(&st.0, &host).expect("check ran");
        assert!(first.pending.is_empty() && first.system_busy.is_none());
        let second = startup_check(&st.0, &host);
        assert_eq!(second, Some(first));
        assert_eq!(host.snapshots.load(Ordering::SeqCst), 1);
    }
}
