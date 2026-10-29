// SPDX-License-Identifier: GPL-3.0-or-later
//! The read-only auto-update check, run once per app start
//! (`autoupdate::startup_check`: which deployed games a run would update or
//! redeploy, and what blocks them).

use vessel_core::autoupdate::{self, Host, RunOptions, StartupCheck};
use vessel_core::config::Config;

use crate::state::Inner;

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
