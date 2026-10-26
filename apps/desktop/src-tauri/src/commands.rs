// SPDX-License-Identifier: GPL-3.0-or-later
//! `#[tauri::command]` adapters over `modman_core::app` and the core
//! modules. No business logic here: each command picks the game, runs one
//! core call on a blocking thread and returns its (serializable) result.
//! Argument names are snake_case on the JS side too. Contract mirrored in
//! `src/ipc/commands.ts`.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use modman_core::app::collection::{
    self as appcol, CollectionInfo, CollectionResult, RemoveResult,
};
use modman_core::app::deploy::{run_deploy, run_purge};
use modman_core::app::games::{
    self as appgames, AddGame, Detected, GameFolder, GameView, Launched,
};
use modman_core::app::install::{self as appinst, InstallRequest, InstallResult, Reinstalled};
use modman_core::app::mods::{
    self as appmods, ModDetail, ModsView, MoveResult, ProfileLoaded, UninstallResult,
};
use modman_core::app::nexus::{self as appnx, AccountInfo, LoginResult};
use modman_core::app::updates::{self as appup, UpdateReport};
use modman_core::app::views::{self, ConfigChange, ConfigEntry, FilesView, SearchView};
use modman_core::app::{actions, art, games_hint, plugins as appplug, thumbs, Cx, R};
use modman_core::autoupdate::{self, LiveHost, RunLogEntry, StartupCheck, Status};
use modman_core::collections::{self, CollectionSummary};
use modman_core::deploy::{self, DeployReport, DeployStatus, PurgeReport, VerifyReport};
use modman_core::download::Download;
use modman_core::games::{self, GameConfig};
use modman_core::loadorder::{ConflictData, PluginLine, PluginStage};
use modman_core::mods::{self, ProfileSummary};
use modman_core::requirements::ReqRow;

use crate::events::Sink;
use crate::prompt::Answer;
use crate::state::{AppState, CmdError, CmdResult};

type St<'a> = State<'a, AppState>;

fn sink(app: &AppHandle) -> Arc<dyn Sink> {
    Arc::new(app.clone())
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| CmdError::new(format!("background task failed: {e}")))?
}

/// Quick core call: questions take defaults.
async fn query<T, F>(app: AppHandle, st: &AppState, game: Option<String>, f: F) -> CmdResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Cx<'_>) -> R<T> + Send + 'static,
{
    let (inner, sink) = (st.0.clone(), sink(&app));
    blocking(move || inner.query(sink, game.as_deref(), f)).await
}

/// Long core call with job events and prompts.
async fn job<T, F>(
    app: AppHandle,
    st: &AppState,
    kind: &'static str,
    game: Option<String>,
    f: F,
) -> CmdResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Cx<'_>) -> R<T> + Send + 'static,
{
    let (inner, sink) = (st.0.clone(), sink(&app));
    blocking(move || inner.job(sink, kind, game.as_deref(), true, f)).await
}

// ── app ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub log_path: PathBuf,
    pub db_path: PathBuf,
    /// Legacy bash modman state found (purge with the legacy tool first).
    pub legacy_state_dir: Option<PathBuf>,
}

#[tauri::command(rename_all = "snake_case")]
pub fn app_info(state: St<'_>) -> AppInfo {
    let p = &state.0.paths;
    AppInfo {
        version: modman_core::VERSION,
        config_dir: p.config_dir.clone(),
        data_dir: p.data_dir.clone(),
        log_path: p.log_path(),
        db_path: p.db_path(),
        legacy_state_dir: p.legacy_state_dir(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadyInfo {
    pub startup_check: Option<StartupCheck>,
    /// Queued nxm links now being processed.
    pub queued: usize,
}

/// Call once the event listeners are registered: returns the start-up
/// auto-update check and starts processing queued nxm links (they may
/// raise prompts). Prompts still open (the webview was reloaded) are
/// emitted again.
#[tauri::command(rename_all = "snake_case")]
pub async fn app_ready(app: AppHandle, state: St<'_>) -> CmdResult<ReadyInfo> {
    let (inner, sink) = (state.0.clone(), sink(&app));
    blocking(move || {
        let check = crate::startup::startup_check(&inner, &LiveHost::new(&inner.paths.home));
        inner.prompts.reemit(&*sink);
        let queued = inner.mark_ready(sink)?;
        Ok(ReadyInfo {
            startup_check: check,
            queued,
        })
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn startup_check(state: St<'_>) -> CmdResult<Option<StartupCheck>> {
    let inner = state.0.clone();
    blocking(move || {
        Ok(crate::startup::startup_check(
            &inner,
            &LiveHost::new(&inner.paths.home),
        ))
    })
    .await
}

/// Answers a `modman:prompt`; `false` when that prompt is gone.
#[tauri::command(rename_all = "snake_case")]
pub fn prompt_answer(state: St<'_>, id: u64, answer: Answer) -> bool {
    state.0.prompts.answer(id, answer)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn log_tail(state: St<'_>, lines: Option<usize>) -> CmdResult<Vec<String>> {
    let inner = state.0.clone();
    blocking(move || {
        Ok(views::log_tail(
            &inner.paths,
            lines.unwrap_or(200).min(5000),
        )?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn config_list(app: AppHandle, state: St<'_>) -> CmdResult<Vec<ConfigEntry>> {
    query(app, &state, None, |cx| views::config_entries(cx)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn config_set(
    app: AppHandle,
    state: St<'_>,
    key: String,
    value: String,
) -> CmdResult<ConfigChange> {
    query(app, &state, None, move |cx| {
        views::config_set(cx, &key, &value)
    })
    .await
}

// ── account ─────────────────────────────────────────────────

#[tauri::command(rename_all = "snake_case")]
pub async fn account(app: AppHandle, state: St<'_>) -> CmdResult<AccountInfo> {
    query(app, &state, None, |cx| {
        appnx::account_info(cx.paths, cx.store)
    })
    .await
}

/// Validates and stores the key. The key is never returned or logged.
#[tauri::command(rename_all = "snake_case")]
pub async fn login(app: AppHandle, state: St<'_>, api_key: String) -> CmdResult<LoginResult> {
    query(app, &state, None, move |cx| {
        appnx::login(cx.paths, cx.store, &api_key)
    })
    .await
}

/// `true` when `NEXUS_API_KEY` is still set (and used).
#[tauri::command(rename_all = "snake_case")]
pub async fn logout(app: AppHandle, state: St<'_>) -> CmdResult<bool> {
    query(app, &state, None, |cx| appnx::logout(cx.paths, cx.store)).await
}

// ── games ───────────────────────────────────────────────────

#[tauri::command(rename_all = "snake_case")]
pub async fn games_list(app: AppHandle, state: St<'_>) -> CmdResult<Vec<GameView>> {
    query(app, &state, None, |cx| appgames::overview(cx)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn games_detect(app: AppHandle, state: St<'_>) -> CmdResult<Vec<Detected>> {
    query(app, &state, None, |cx| appgames::detect(cx)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn game_add(
    app: AppHandle,
    state: St<'_>,
    domain: String,
    path: Option<String>,
    subdir: Option<String>,
    profile: Option<String>,
) -> CmdResult<GameConfig> {
    job(app, &state, "game_add", None, move |cx| {
        appgames::add_game(
            cx,
            AddGame {
                domain,
                path,
                subdir,
                profile,
            },
        )
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn game_use(app: AppHandle, state: St<'_>, domain: String) -> CmdResult<()> {
    query(app, &state, None, move |cx| {
        let d = domain.to_ascii_lowercase();
        games::require_game(cx.store.conn(), &d).map_err(games_hint)?;
        Ok(games::set_active_game(cx.store, &d)?)
    })
    .await
}

/// Job: starts the game through Steam; pending mod changes raise a
/// "Deploy before launching?" prompt first.
#[tauri::command(rename_all = "snake_case")]
pub async fn game_launch(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<Launched> {
    job(app, &state, "launch", game, |cx| {
        let g = cx.need_game()?;
        appgames::launch(cx, &g)
    })
    .await
}

/// `false` when the user declined the confirmation.
#[tauri::command(rename_all = "snake_case")]
pub async fn game_remove(
    app: AppHandle,
    state: St<'_>,
    domain: String,
    delete_data: bool,
) -> CmdResult<bool> {
    job(app, &state, "game_remove", None, move |cx| {
        appgames::remove(cx, &domain, delete_data)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn game_set(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    key: String,
    value: String,
) -> CmdResult<GameConfig> {
    query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        appgames::set_setting(cx, &g.domain, &key, &value)
    })
    .await
}

// ── mods ────────────────────────────────────────────────────

#[tauri::command(rename_all = "snake_case")]
pub async fn mods_list(app: AppHandle, state: St<'_>, game: Option<String>) -> CmdResult<ModsView> {
    query(app, &state, game, |cx| {
        let g = cx.need_game()?;
        appmods::mods_view(cx, &g)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mod_detail(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    reference: String,
) -> CmdResult<ModDetail> {
    query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        appmods::mod_detail(cx, &g, &reference)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mod_open_page(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    reference: String,
) -> CmdResult<String> {
    query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        appmods::open_mod_page(cx, &g, &reference, |url| {
            appgames::launch_opener(OsStr::new(url))
        })
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mods_set_enabled(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    ids: Vec<i64>,
    enabled: bool,
) -> CmdResult<appinst::AfterChangeView> {
    job(app, &state, "mods_set_enabled", game, move |cx| {
        let g = cx.need_game()?;
        appmods::set_enabled(cx, &g, &ids, enabled).map(Into::into)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mod_move(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    reference: String,
    spec: String,
    arg: Option<String>,
) -> CmdResult<MoveResult> {
    job(app, &state, "mod_move", game, move |cx| {
        let g = cx.need_game()?;
        appmods::move_mod(cx, &g, &reference, &spec, arg.as_deref())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mods_uninstall(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    refs: Vec<String>,
    delete_archive: bool,
) -> CmdResult<UninstallResult> {
    job(app, &state, "mods_uninstall", game, move |cx| {
        let g = cx.need_game()?;
        appmods::uninstall(cx, &g, &refs, delete_archive)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mod_reinstall(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    reference: String,
    fomod: Option<String>,
    ask: bool,
) -> CmdResult<Reinstalled> {
    job(app, &state, "mod_reinstall", game, move |cx| {
        let g = cx.need_game()?;
        appinst::reinstall(cx, &g, &reference, fomod.as_deref(), ask)
    })
    .await
}

/// The game's library art (`kind`: portrait `library_600x900`, the
/// default, or wide `library_hero`) as a `data:` URL: the local Steam
/// cache, else modman's cache of Steam's CDN copy; `None` = no art.
#[tauri::command(rename_all = "snake_case")]
pub async fn game_art(
    app: AppHandle,
    state: St<'_>,
    domain: String,
    kind: Option<art::ArtKind>,
) -> CmdResult<Option<String>> {
    query(app, &state, None, move |cx| {
        art::game_art(cx, &domain, kind.unwrap_or_default())
    })
    .await
}

/// Opens the game folder or the Proton prefix of `game` in the file
/// manager. The path comes from the game's stored configuration only (the
/// webview names a game and a kind, never a path); returns the path opened.
#[tauri::command(rename_all = "snake_case")]
pub async fn open_folder(
    app: AppHandle,
    state: St<'_>,
    game: String,
    which: GameFolder,
) -> CmdResult<PathBuf> {
    query(app, &state, None, move |cx| {
        appgames::open_folder(cx, &game, which, appgames::launch_file_manager)
    })
    .await
}

/// The mod's picture as a `data:` URL (cached; `None` when it has none).
#[tauri::command(rename_all = "snake_case")]
pub async fn thumbnail(
    app: AppHandle,
    state: St<'_>,
    game: String,
    mod_id: u64,
) -> CmdResult<Option<String>> {
    query(app, &state, None, move |cx| {
        thumbs::thumbnail(cx, &game, mod_id)
    })
    .await
}

// ── install & downloads ─────────────────────────────────────

#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)]
pub async fn install(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    target: String,
    file_id: Option<u64>,
    fomod: Option<String>,
    name: Option<String>,
    deploy: bool,
    optional: Option<String>,
    revision: Option<u64>,
    download_only: bool,
) -> CmdResult<InstallResult> {
    job(app, &state, "install", game, move |cx| {
        appinst::install(
            cx,
            InstallRequest {
                target,
                file: file_id,
                fomod,
                name,
                deploy,
                optional,
                revision,
                download_only,
            },
        )
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mod_files(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    mod_id: u64,
) -> CmdResult<FilesView> {
    query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        views::mod_files(cx, &g, mod_id)
    })
    .await
}

/// The paste-link box: the same intake as a forwarded link.
#[tauri::command(rename_all = "snake_case")]
pub async fn nxm_submit(
    app: AppHandle,
    state: St<'_>,
    url: String,
) -> CmdResult<crate::nxm::Received> {
    let (inner, sink) = (state.0.clone(), sink(&app));
    blocking(move || crate::nxm::receive(&inner, &*sink, &url)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn downloads_list(app: AppHandle, state: St<'_>) -> CmdResult<Vec<Download>> {
    query(app, &state, None, |cx| views::downloads(cx)).await
}

// ── deploy ──────────────────────────────────────────────────

#[tauri::command(rename_all = "snake_case")]
pub async fn deploy(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    dry_run: bool,
    method: Option<String>,
) -> CmdResult<DeployReport> {
    job(app, &state, "deploy", game, move |cx| {
        let g = cx.need_game()?;
        run_deploy(cx, &g, method.as_deref(), dry_run)
    })
    .await
}

/// `None` when nothing was deployed or the user declined.
#[tauri::command(rename_all = "snake_case")]
pub async fn purge(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<Option<PurgeReport>> {
    job(app, &state, "purge", game, |cx| {
        let g = cx.need_game()?;
        run_purge(cx, &g)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn verify(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<VerifyReport> {
    query(app, &state, game, |cx| {
        let g = cx.need_game()?;
        Ok(deploy::verify(cx.store.conn(), &g)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn deploy_status(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<DeployStatus> {
    query(app, &state, game, |cx| {
        let g = cx.need_game()?;
        Ok(deploy::status(cx.store.conn(), &g.domain)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn conflicts(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<ConflictData> {
    query(app, &state, game, |cx| {
        let g = cx.need_game()?;
        views::conflicts(cx, &g)
    })
    .await
}

// ── requirements ────────────────────────────────────────────

/// Requirement rows of all enabled mods, or of one mod (`mod_ref`).
#[tauri::command(rename_all = "snake_case")]
pub async fn deps(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    mod_ref: Option<String>,
    refresh: bool,
) -> CmdResult<Vec<ReqRow>> {
    query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        appup::deps_rows(cx, &g, mod_ref.as_deref(), refresh)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn deps_ack(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    name: String,
) -> CmdResult<ReqRow> {
    query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        appup::set_ack(cx, &g, &name, true, false)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn deps_unack(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    name: String,
) -> CmdResult<ReqRow> {
    query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        appup::set_ack(cx, &g, &name, false, false)
    })
    .await
}

// ── profiles ────────────────────────────────────────────────

#[tauri::command(rename_all = "snake_case")]
pub async fn profiles_list(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<Vec<ProfileSummary>> {
    query(app, &state, game, |cx| {
        let g = cx.need_game()?;
        Ok(mods::list_profiles(cx.store, &g.domain)?)
    })
    .await
}

/// `None` when the user declined overwriting an existing profile.
#[tauri::command(rename_all = "snake_case")]
pub async fn profile_save(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    name: String,
) -> CmdResult<Option<usize>> {
    job(app, &state, "profile_save", game, move |cx| {
        let g = cx.need_game()?;
        appmods::save_profile(cx, &g, &name)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn profile_load(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    name: String,
) -> CmdResult<ProfileLoaded> {
    job(app, &state, "profile_load", game, move |cx| {
        let g = cx.need_game()?;
        appmods::load_profile(cx, &g, &name)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn profile_delete(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    name: String,
) -> CmdResult<()> {
    query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        Ok(mods::delete_profile(cx.store, &g.domain, &name)?)
    })
    .await
}

// ── collections ─────────────────────────────────────────────

/// A collection's tile picture from Nexus as a `data:` URL (cached;
/// `None` = it has none or Nexus could not be asked).
#[tauri::command(rename_all = "snake_case")]
pub async fn collection_art(
    app: AppHandle,
    state: St<'_>,
    game: String,
    slug: String,
) -> CmdResult<Option<String>> {
    query(app, &state, None, move |cx| {
        thumbs::collection_art(cx, &game, &slug)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn collections_list(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<Vec<CollectionSummary>> {
    query(app, &state, game, |cx| {
        let g = cx.need_game()?;
        Ok(collections::list(cx.store.conn(), &g.domain)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn collection_info(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    reference: String,
) -> CmdResult<CollectionInfo> {
    query(app, &state, game, move |cx| appcol::info(cx, &reference)).await
}

#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)]
pub async fn collection_install(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    reference: String,
    optional: Option<String>,
    fomod: Option<String>,
    revision: Option<u64>,
    deploy: bool,
    download_only: bool,
) -> CmdResult<CollectionResult> {
    job(app, &state, "collection_install", game, move |cx| {
        appcol::install(
            cx,
            &appcol::InstallOpts {
                reference,
                optional,
                fomod,
                revision,
                deploy,
                download_only,
            },
        )
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn collection_update(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    reference: String,
    optional: Option<String>,
    fomod: Option<String>,
    deploy: bool,
) -> CmdResult<CollectionResult> {
    job(app, &state, "collection_update", game, move |cx| {
        appcol::update(
            cx,
            &appcol::InstallOpts {
                reference,
                optional,
                fomod,
                deploy,
                ..Default::default()
            },
        )
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn collection_remove(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    reference: String,
    keep_mods: bool,
) -> CmdResult<RemoveResult> {
    job(app, &state, "collection_remove", game, move |cx| {
        appcol::remove(cx, &reference, keep_mods)
    })
    .await
}

// ── plugins ─────────────────────────────────────────────────

/// plugins.txt entries; `None` before the file exists.
#[tauri::command(rename_all = "snake_case")]
pub async fn plugins_list(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<Option<Vec<PluginLine>>> {
    query(app, &state, game, |cx| {
        let g = cx.need_game()?;
        appplug::list(cx, &g)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugins_sort(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    update_masterlist: bool,
) -> CmdResult<PluginStage> {
    job(app, &state, "plugins_sort", game, move |cx| {
        let g = cx.need_game()?;
        appplug::sort(cx, &g, update_masterlist)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugins_masterlist(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<PathBuf> {
    job(app, &state, "plugins_masterlist", game, |cx| {
        let g = cx.need_game()?;
        appplug::masterlist(cx, &g)
    })
    .await
}

// ── updates & search ────────────────────────────────────────

#[tauri::command(rename_all = "snake_case")]
pub async fn updates_check(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    refresh: bool,
) -> CmdResult<UpdateReport> {
    job(app, &state, "updates_check", game, move |cx| {
        let g = cx.need_game()?;
        let nexus = appnx::require(cx.paths, cx.store)?;
        appup::check(cx, &nexus, &g, refresh)
    })
    .await
}

/// Checks, then installs the updates (pinned ones only with `all`).
#[tauri::command(rename_all = "snake_case")]
pub async fn updates_install(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    all: bool,
    refresh: bool,
) -> CmdResult<UpdateReport> {
    job(app, &state, "updates_install", game, move |cx| {
        let g = cx.need_game()?;
        appup::run(cx, &g, true, all, refresh)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn search(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    query: String,
) -> CmdResult<SearchView> {
    self::query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        views::search(cx, &g, &query)
    })
    .await
}

// ── auto-update ─────────────────────────────────────────────

const STATUS_RUNS: usize = 50;

#[tauri::command(rename_all = "snake_case")]
pub async fn autoupdate_status(
    app: AppHandle,
    state: St<'_>,
    runs: Option<usize>,
) -> CmdResult<Status> {
    query(app, &state, None, move |cx| {
        actions::status(cx, &actions::systemctl(), runs.unwrap_or(STATUS_RUNS))
    })
    .await
}

/// Installs the systemd user timer for the `modman` CLI next to the app
/// (or on PATH) and records the baseline; returns the new status.
#[tauri::command(rename_all = "snake_case")]
pub async fn autoupdate_enable(app: AppHandle, state: St<'_>) -> CmdResult<Status> {
    let (inner, sink) = (state.0.clone(), sink(&app));
    blocking(move || {
        inner.job(sink, "autoupdate_enable", None, false, |cx| {
            let gui = std::env::current_exe()?;
            let exec = actions::timer_exec(&actions::cli_binary(&gui)?);
            let ctl = actions::systemctl();
            actions::enable(cx, exec, &ctl)?;
            actions::status(cx, &ctl, STATUS_RUNS)
        })
    })
    .await
}

/// `false` when it was not enabled.
#[tauri::command(rename_all = "snake_case")]
pub async fn autoupdate_disable(app: AppHandle, state: St<'_>) -> CmdResult<bool> {
    query(app, &state, None, |cx| {
        Ok(autoupdate::disable(cx.paths, &actions::systemctl())?)
    })
    .await
}

/// The last-run log, newest first.
#[tauri::command(rename_all = "snake_case")]
pub async fn autoupdate_runs(
    app: AppHandle,
    state: St<'_>,
    limit: Option<usize>,
) -> CmdResult<Vec<RunLogEntry>> {
    query(app, &state, None, move |cx| {
        Ok(autoupdate::last_runs(
            cx.store.conn(),
            limit.unwrap_or(STATUS_RUNS).min(500),
        )?)
    })
    .await
}
