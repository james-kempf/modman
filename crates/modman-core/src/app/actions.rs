// SPDX-License-Identifier: GPL-3.0-or-later
//! Auto-update glue: the [`Actions`] a run performs (collection update,
//! mod updates, redeploy), the systemctl runner with its test hook, and
//! `enable` with its baseline run (legacy L4705).

use std::path::{Path, PathBuf};

use super::collection::{self, InstallOpts};
use super::{chain, AppError, Cx, R};
use crate::autoupdate::{
    self, Account, Actions, LiveHost, RunOptions, Status, Systemctl, UnitSpec, UserSystemctl,
};
use crate::games::GameConfig;
use crate::store::Store;

/// Test hook: a `systemctl` stand-in (path to an executable).
pub const SYSTEMCTL_ENV: &str = "MODMAN_SYSTEMCTL";

/// `systemctl --user`, or the [`SYSTEMCTL_ENV`] stand-in.
pub struct Ctl(Option<PathBuf>);

impl Systemctl for Ctl {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let Some(bin) = &self.0 else {
            return UserSystemctl.run(args);
        };
        let out = std::process::Command::new(bin)
            .arg("--user")
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() {
            Ok(stdout)
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(if stderr.is_empty() { stdout } else { stderr })
        }
    }
}

pub fn systemctl() -> Ctl {
    Ctl(std::env::var_os(SYSTEMCTL_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from))
}

/// Run options from the current settings.
pub fn options(cx: &Cx<'_>) -> R<RunOptions> {
    Ok(RunOptions::new(&cx.config()?, cx.paths))
}

/// Runs the auto-update actions through the shared orchestration. Prompts
/// take their defaults (legacy ASSUME_YES=1) because `cx.ui` should be
/// non-interactive.
pub struct AppActions<'c, 'a> {
    pub cx: &'c mut Cx<'a>,
}

impl AppActions<'_, '_> {
    fn on_game(
        &mut self,
        game: &GameConfig,
        f: impl FnOnce(&mut Cx<'_>) -> R<()>,
    ) -> Result<(), String> {
        let r = f(self.cx).map_err(|e| chain(&e));
        if let Err(e) = &r {
            tracing::warn!("auto-update {}: {e}", game.domain);
        }
        r
    }

    fn updates(&mut self, game: &GameConfig, install: bool) -> Result<(), String> {
        self.on_game(game, |cx| {
            super::updates::run(cx, game, install, false, false).map(|_| ())
        })
    }
}

impl Actions for AppActions<'_, '_> {
    fn store(&mut self) -> &mut Store {
        self.cx.store
    }

    fn account(&mut self) -> Account {
        let (paths, store) = (self.cx.paths, &*self.cx.store);
        if !matches!(super::nexus::load_key(paths, store), Ok(Some(_))) {
            return Account::None;
        }
        // Unknown tier counts as free (legacy: IS_PREMIUM empty).
        match super::nexus::require(paths, store).and_then(|n| super::nexus::is_premium(store, &n))
        {
            Ok(true) => Account::Premium,
            _ => Account::Free,
        }
    }

    fn update_collection(&mut self, game: &GameConfig, slug: &str) -> Result<(), String> {
        let opts = InstallOpts {
            reference: format!("nxm://{}/collections/{slug}", game.domain),
            optional: Some("none".into()),
            ..Default::default()
        };
        self.on_game(game, |cx| collection::update(cx, &opts).map(|_| ()))
    }

    fn install_mod_updates(&mut self, game: &GameConfig) -> Result<(), String> {
        self.updates(game, true)
    }

    fn check_mod_updates(&mut self, game: &GameConfig) -> Result<(), String> {
        self.updates(game, false)
    }

    fn redeploy(&mut self, game: &GameConfig) -> Result<(), String> {
        self.on_game(game, |cx| {
            super::deploy::run_deploy(cx, game, None, false).map(|_| ())
        })
    }
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// `appimage` when `exe` really runs from that AppImage's mount (`appdir`);
/// an inherited `$APPIMAGE` alone (e.g. a terminal opened from the app)
/// does not count.
pub fn appimage_of(exe: &Path, appimage: Option<&Path>, appdir: Option<&Path>) -> Option<PathBuf> {
    let (image, dir) = (appimage?, appdir?);
    canonical(exe)
        .starts_with(canonical(dir))
        .then(|| image.to_path_buf())
}

/// [`appimage_of`] for this process (`$APPIMAGE`, `$APPDIR`).
pub fn running_appimage(exe: &Path) -> Option<PathBuf> {
    appimage_of(
        exe,
        env_path("APPIMAGE").as_deref(),
        env_path("APPDIR").as_deref(),
    )
}

/// The program the timer runs, which must be the CLI: `$APPIMAGE` only
/// when `program` is this very process (the CLI) running from the
/// AppImage, else `program` (the resolved CLI binary).
pub fn timer_exec_for(
    program: &Path,
    exe: &Path,
    appimage: Option<&Path>,
    appdir: Option<&Path>,
) -> PathBuf {
    let program = canonical(program);
    let is_self = program == canonical(exe);
    match appimage_of(exe, appimage, appdir) {
        Some(image) if is_self => image,
        _ => program,
    }
}

/// [`timer_exec_for`] for this process.
pub fn timer_exec(program: &Path) -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| program.to_path_buf());
    timer_exec_for(
        program,
        &exe,
        env_path("APPIMAGE").as_deref(),
        env_path("APPDIR").as_deref(),
    )
}

/// The `modman` CLI next to `gui_exe`, else on `PATH` (the GUI enables the
/// timer for the CLI, which performs the runs).
pub fn cli_binary(gui_exe: &Path) -> R<PathBuf> {
    if let Some(sib) = gui_exe.parent().map(|d| d.join("modman")) {
        if sib.is_file() {
            return Ok(sib);
        }
    }
    super::which("modman").ok_or_else(|| {
        AppError::Msg(
            "the modman command-line program was not found next to the app or on PATH".into(),
        )
    })
}

/// Installs and starts the timer for `exec`, then records the current state
/// so the first timer run does not redeploy for no reason (legacy L4705).
pub fn enable(cx: &mut Cx<'_>, exec: PathBuf, systemctl: &dyn Systemctl) -> R<()> {
    let cfg = cx.config()?;
    let opts = RunOptions::new(&cfg, cx.paths);
    if exec.starts_with("/tmp") {
        cx.ui.warn(&format!(
            "modman runs from {} — move it somewhere permanent first, the timer will call this path.",
            exec.display()
        ));
    }
    // The user manager may not share this process's environment.
    let mut env = Vec::new();
    for k in ["PATH", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME"] {
        if let Some(v) = std::env::var_os(k).filter(|v| !v.is_empty()) {
            env.push((k.to_string(), v.to_string_lossy().into_owned()));
        }
    }
    let spec = UnitSpec {
        exec,
        interval_hours: cfg.autoupdate_interval_hours,
        env,
    };
    autoupdate::enable(cx.paths, &spec, systemctl)?;
    let hours = autoupdate::timer_hours(cfg.autoupdate_interval_hours);
    cx.ui.success(&format!(
        "Auto-update enabled (systemd user timer: at boot and every {hours} h)."
    ));
    let host = LiveHost::new(&cx.paths.home);
    if let Err(e) = autoupdate::run(&mut AppActions { cx }, &host, &opts) {
        tracing::warn!("auto-update baseline failed: {e}");
    }
    cx.ui.info(&format!(
        "Triggers: system updates, Steam game updates{}.",
        if cfg.autoupdate_interval_hours > 0 {
            format!(", every {} h", cfg.autoupdate_interval_hours)
        } else {
            String::new()
        }
    ));
    cx.ui.info(&format!(
        "Collections follow their newest revision: {}  (modman config autoupdate-collections true|false)",
        cfg.autoupdate_collections
    ));
    Ok(())
}

/// Timer state, per-game state and the newest `runs` log rows.
pub fn status(cx: &Cx<'_>, systemctl: &dyn Systemctl, runs: usize) -> R<Status> {
    let opts = options(cx)?;
    let host = LiveHost::new(&cx.paths.home);
    Ok(autoupdate::status(
        cx.store.conn(),
        cx.paths,
        &host,
        systemctl,
        &opts,
        runs,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_binary_prefers_the_sibling_of_the_gui() {
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("modman");
        std::fs::write(&cli, "").unwrap();
        let found = cli_binary(&dir.path().join("modman-desktop")).unwrap();
        assert_eq!(found, cli);
    }

    #[test]
    fn the_gui_in_an_appimage_schedules_the_cli_not_the_image() {
        let mount = tempfile::tempdir().unwrap();
        let bin = mount.path().join("usr/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let (gui, cli) = (bin.join("modman-desktop"), bin.join("modman"));
        std::fs::write(&gui, "").unwrap();
        std::fs::write(&cli, "").unwrap();
        let image = Path::new("/home/u/Apps/modman.AppImage");
        let exec = timer_exec_for(&cli, &gui, Some(image), Some(mount.path()));
        assert_eq!(exec, cli.canonicalize().unwrap());
    }

    #[test]
    fn the_cli_in_an_appimage_schedules_the_image() {
        let mount = tempfile::tempdir().unwrap();
        let cli = mount.path().join("modman");
        std::fs::write(&cli, "").unwrap();
        let image = Path::new("/home/u/Apps/modman.AppImage");
        let exec = timer_exec_for(&cli, &cli, Some(image), Some(mount.path()));
        assert_eq!(exec, image);
    }

    #[test]
    fn an_inherited_appimage_variable_is_ignored() {
        let (mount, elsewhere) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let cli = elsewhere.path().join("modman");
        std::fs::write(&cli, "").unwrap();
        let image = Path::new("/home/u/Apps/modman.AppImage");
        let canon = cli.canonicalize().unwrap();
        assert_eq!(
            timer_exec_for(&cli, &cli, Some(image), Some(mount.path())),
            canon
        );
        assert_eq!(timer_exec_for(&cli, &cli, Some(image), None), canon);
        assert_eq!(appimage_of(&cli, Some(image), Some(mount.path())), None);
    }
}
