// SPDX-License-Identifier: GPL-3.0-or-later
//! modman desktop backend (Tauri v2): command adapters over
//! `modman_core::app`, core progress/messages as events, the nxm intake of
//! the single-instance D-Bus endpoint (spikes.md §5) and the one-time
//! auto-update check at start. No HTTP server; no deep-link plugin.

pub mod commands;
pub mod events;
pub mod gui_ui;
pub mod logging;
pub mod nxm;
pub mod prompt;
pub mod startup;
pub mod state;

include!("command_names.rs");

use std::sync::Arc;

use tauri::Manager;

use modman_core::autoupdate::LiveHost;
use modman_core::paths::Paths;

use crate::events::Sink;
use crate::state::AppState;

fn focus_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Starts the app. Exits with a message when the XDG paths are unusable.
pub fn run() {
    let paths = match Paths::from_env() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("modman: {e}");
            std::process::exit(1);
        }
    };
    logging::init(&paths.log_path());
    tracing::info!("GUI start (modman {})", modman_core::VERSION);
    let state = AppState::new(paths);
    let own_argv: Vec<String> = std::env::args().collect();

    let result = tauri::Builder::default()
        // First: a second instance hands its argv over D-Bus and exits.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            focus_main(app);
            let st = app.state::<AppState>();
            let sink: Arc<dyn Sink> = Arc::new(app.clone());
            // Runs on the D-Bus thread; `modman nxm` waits for this reply,
            // so only parse + queue here (the worker installs).
            let _ = nxm::on_second_instance(&st.0, &*sink, &argv);
        }))
        .manage(state)
        .setup(move |app| {
            let handle = app.handle().clone();
            let st = app.state::<AppState>().0.clone();
            if nxm::url_in_argv(&own_argv).is_some() {
                let sink: Arc<dyn Sink> = Arc::new(handle.clone());
                let _ = nxm::on_second_instance(&st, &*sink, &own_argv);
            }
            std::thread::Builder::new()
                .name("startup-check".into())
                .spawn(move || {
                    let check = startup::startup_check(&st, &LiveHost::new(&st.paths.home));
                    events::emit(&handle, events::STARTUP_CHECK, &check);
                })?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                window.state::<AppState>().0.prompts.close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::app_ready,
            commands::startup_check,
            commands::prompt_answer,
            commands::log_tail,
            commands::config_list,
            commands::config_set,
            commands::account,
            commands::login,
            commands::logout,
            commands::games_list,
            commands::games_detect,
            commands::game_add,
            commands::game_use,
            commands::game_remove,
            commands::game_set,
            commands::game_launch,
            commands::game_art,
            commands::mods_list,
            commands::mod_detail,
            commands::mods_set_enabled,
            commands::mod_move,
            commands::mods_uninstall,
            commands::mod_reinstall,
            commands::thumbnail,
            commands::install,
            commands::mod_files,
            commands::nxm_submit,
            commands::downloads_list,
            commands::deploy,
            commands::purge,
            commands::verify,
            commands::deploy_status,
            commands::conflicts,
            commands::deps,
            commands::deps_ack,
            commands::deps_unack,
            commands::profiles_list,
            commands::profile_save,
            commands::profile_load,
            commands::profile_delete,
            commands::collections_list,
            commands::collection_info,
            commands::collection_install,
            commands::collection_update,
            commands::collection_remove,
            commands::plugins_list,
            commands::plugins_sort,
            commands::plugins_masterlist,
            commands::updates_check,
            commands::updates_install,
            commands::search,
            commands::autoupdate_status,
            commands::autoupdate_enable,
            commands::autoupdate_disable,
            commands::autoupdate_runs,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        tracing::error!("modman GUI stopped: {e}");
        eprintln!("modman: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests;
