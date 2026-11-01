// SPDX-License-Identifier: GPL-3.0-or-later
//! `#[tauri::command]` adapters over `vessel_core::app` and the core
//! modules. No business logic here: each command picks the game, runs one
//! core call on a blocking thread and returns its (serializable) result.
//! Argument names are snake_case on the JS side too. Contract mirrored in
//! `src/ipc/commands.ts`.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use vessel_core::app::collection::{
    self as appcol, CollectionInfo, CollectionResult, RemoveResult,
};
use vessel_core::app::deploy::{run_deploy, run_purge};
use vessel_core::app::games::{
    self as appgames, AddGame, Detected, GameFolder, GameView, Launched,
};
use vessel_core::app::install::{self as appinst, InstallRequest, InstallResult, Reinstalled};
use vessel_core::app::mods::{
    self as appmods, ModDetail, ModsView, MoveResult, ProfileLoaded, UninstallResult,
};
use vessel_core::app::nexus::{self as appnx, AccountInfo, LoginResult};
use vessel_core::app::shortcut::{self, ShortcutStatus};
use vessel_core::app::updates::{self as appup, UpdateReport};
use vessel_core::app::views::{
    self, BrowseRequest, BrowseView, ConfigChange, ConfigEntry, FilesView,
};
use vessel_core::app::{actions, art, games_hint, plugins as appplug, thumbs, Cx, R};
use vessel_core::autoupdate::{self, LiveHost, RunLogEntry, StartupCheck, Status};
use vessel_core::collections::{self, CollectionSummary};
use vessel_core::deploy::{self, DeployReport, DeployStatus, PurgeReport, VerifyReport};
use vessel_core::download::Download;
use vessel_core::games::{self, GameConfig};
use vessel_core::loadorder::{ConflictData, PluginLine, PluginStage};
use vessel_core::mods::{self, ProfileSummary};
use vessel_core::requirements::ReqRow;

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
    /// Legacy bash tool state found (purge with the legacy tool first).
    pub legacy_state_dir: Option<PathBuf>,
}

/// Closes the app (the window's own close button is out of reach in Game Mode).
#[tauri::command(rename_all = "snake_case")]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command(rename_all = "snake_case")]
pub fn app_info(state: St<'_>) -> AppInfo {
    let p = &state.0.paths;
    AppInfo {
        version: vessel_core::VERSION,
        config_dir: p.config_dir.clone(),
        data_dir: p.data_dir.clone(),
        log_path: p.log_path(),
        db_path: p.db_path(),
        legacy_state_dir: p.legacy_state_dir(),
    }
}

fn exe() -> CmdResult<std::path::PathBuf> {
    std::env::current_exe().map_err(|e| CmdError::new(format!("cannot find this program: {e}")))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn steam_shortcut_status(state: St<'_>) -> CmdResult<ShortcutStatus> {
    let home = state.0.paths.home.clone();
    blocking(move || Ok(shortcut::status(&home, &exe()?)?)).await
}

/// Adds vessel to (or removes it from) Steam as a non-Steam game; refused
/// while Steam runs.
#[tauri::command(rename_all = "snake_case")]
pub async fn steam_shortcut_set(state: St<'_>, enabled: bool) -> CmdResult<ShortcutStatus> {
    let home = state.0.paths.home.clone();
    blocking(move || Ok(shortcut::set(&home, &exe()?, enabled)?)).await
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadyInfo {
    pub startup_check: Option<StartupCheck>,
    /// Queued nxm links now being processed.
    pub queued: usize,
    /// Loader runs startup recovery finished (a rollback may be offered).
    pub loader_recovered: Vec<vessel_core::app::loaders::Recovered>,
}

/// Call once the event listeners are registered: returns the start-up
/// auto-update check and starts processing queued nxm links (they may
/// raise prompts). Prompts still open (the webview was reloaded) are
/// emitted again.
#[tauri::command(rename_all = "snake_case")]
pub async fn app_ready(app: AppHandle, state: St<'_>) -> CmdResult<ReadyInfo> {
    let (inner, sink) = (state.0.clone(), sink(&app));
    blocking(move || {
        let loader_recovered = crate::startup::loader_recovery(&inner);
        let check = crate::startup::startup_check(&inner, &LiveHost::new(&inner.paths.home));
        inner.prompts.reemit(&*sink);
        let queued = inner.mark_ready(sink)?;
        Ok(ReadyInfo {
            startup_check: check,
            queued,
            loader_recovered,
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

/// Answers a `vessel:prompt`; `false` when that prompt is gone.
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
pub async fn game_use(app: AppHandle, state: St<'_>, game: String) -> CmdResult<()> {
    query(app, &state, None, move |cx| {
        let d = game.to_ascii_lowercase();
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
    game: String,
    delete_data: bool,
) -> CmdResult<bool> {
    job(app, &state, "game_remove", None, move |cx| {
        appgames::remove(cx, &game, delete_data)
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
        appgames::set_setting(cx, &g.id, &key, &value)
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

/// Opens a link from a Nexus description in the browser; returns the URL.
/// The webview never navigates: the text is parsed here and only absolute
/// http/https URLs reach the opener (anything else is an error).
#[tauri::command(rename_all = "snake_case")]
pub async fn open_url(url: String) -> CmdResult<String> {
    blocking(move || {
        Ok(appgames::open_web_url(&url, |u| {
            appgames::launch_opener(OsStr::new(u))
        })?)
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
/// cache, else vessel's cache of Steam's CDN copy; `None` = no art.
#[tauri::command(rename_all = "snake_case")]
pub async fn game_art(
    app: AppHandle,
    state: St<'_>,
    game: String,
    kind: Option<art::ArtKind>,
) -> CmdResult<Option<String>> {
    query(app, &state, None, move |cx| {
        art::game_art(cx, &game, kind.unwrap_or_default())
    })
    .await
}

/// Library art of any Steam app by id (installed games not managed yet),
/// as [`game_art`]. Only a numeric `appid` crosses the bridge: serde
/// rejects anything else, and the cache path and CDN URL derive from it.
#[tauri::command(rename_all = "snake_case")]
pub async fn steam_art(
    app: AppHandle,
    state: St<'_>,
    appid: u32,
    kind: Option<art::ArtKind>,
) -> CmdResult<Option<String>> {
    query(app, &state, None, move |cx| {
        Ok(art::steam_art(cx.paths, appid, kind.unwrap_or_default()))
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

/// A Nexus picture by URL as a `data:` URL (cached; `None` when the URL is
/// not an https Nexus one or the fetch fails).
#[tauri::command(rename_all = "snake_case")]
pub async fn picture(app: AppHandle, state: St<'_>, url: String) -> CmdResult<Option<String>> {
    query(app, &state, None, move |cx| thumbs::picture(cx, &url)).await
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
        Ok(deploy::status(cx.store.conn(), &g.id)?)
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
        Ok(mods::list_profiles(cx.store, &g.id)?)
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
        Ok(mods::delete_profile(cx.store, &g.id, &name)?)
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
        Ok(collections::list(cx.store.conn(), &g.id)?)
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

/// A page of the game's mods or collections on Nexus (top, newest or
/// matching words).
#[tauri::command(rename_all = "snake_case")]
pub async fn browse(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    request: BrowseRequest,
) -> CmdResult<BrowseView> {
    query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        views::browse(cx, &g, &request)
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

/// Installs the systemd user timer for the `vessel` CLI next to the app
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

// ── backup cleanup and restage ──────────────────────────────

use vessel_core::app::backups as appbk;
use vessel_core::app::restage::{self as apprs, RestageReport, RestageRequest};
use vessel_core::deploy::{PoisonedBackup, QuarantineManifest, QuarantineReport, RestoreReport};

/// Held backups that hold mod files instead of game originals (dry run of
/// `deploy clean-backups`; changes nothing).
#[tauri::command(rename_all = "snake_case")]
pub async fn backups_poisoned(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<Vec<PoisonedBackup>> {
    query(app, &state, game, |cx| {
        let g = cx.need_game()?;
        Ok(deploy::poisoned_backups(cx.store.conn(), &g)?)
    })
    .await
}

/// Quarantines the poisoned backups. The webview asks first and passes
/// `confirmed`; the core refuses without it (a GUI prompt never answers
/// for the user here).
#[tauri::command(rename_all = "snake_case")]
pub async fn backups_clean(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    include_ambiguous: bool,
    confirmed: bool,
) -> CmdResult<QuarantineReport> {
    job(app, &state, "backups_clean", game, move |cx| {
        let g = cx.need_game()?;
        appbk::apply(cx, &g, include_ambiguous, confirmed)
    })
    .await
}

/// Quarantines of the game, oldest first.
#[tauri::command(rename_all = "snake_case")]
pub async fn quarantines_list(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<Vec<QuarantineManifest>> {
    query(app, &state, game, |cx| {
        let g = cx.need_game()?;
        appbk::list_quarantines(cx, &g)
    })
    .await
}

/// Puts quarantine `id` back; refused unless `confirmed`.
#[tauri::command(rename_all = "snake_case")]
pub async fn quarantine_restore(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    id: String,
    confirmed: bool,
) -> CmdResult<RestoreReport> {
    job(app, &state, "quarantine_restore", game, move |cx| {
        let g = cx.need_game()?;
        appbk::restore_quarantine(cx, &g, &id, confirmed)
    })
    .await
}

/// Re-stages mods from their archives (the outdated ones, `ids`, or `all`).
/// Fails while confirmed poisoned backups exist unless `ignore_poisoned`.
/// Does not deploy.
#[tauri::command(rename_all = "snake_case")]
pub async fn restage(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    ids: Vec<i64>,
    all: bool,
    ignore_poisoned: bool,
) -> CmdResult<RestageReport> {
    job(app, &state, "restage", game, move |cx| {
        let g = cx.need_game()?;
        let req = RestageRequest {
            ids,
            all,
            ignore_poisoned,
        };
        apprs::restage(cx, &g, &req)
    })
    .await
}

// ── loaders ─────────────────────────────────────────────────

use std::sync::atomic::{AtomicBool, Ordering};

use vessel_core::app::loaders::{
    self as apploaders, LoaderEnv, LoaderJunk, LoaderView, Preview, PreviewRequest, Recovered,
    RunReport,
};
use vessel_core::loaders::launch::{self as launch, ApplyOutcome, LaunchState};
use vessel_core::loaders::{housekeeping, DownloadOrigin, LoaderError, LoaderStatus, Op};

fn loader_env(cx: &Cx<'_>) -> R<LoaderEnv> {
    Ok(LoaderEnv::new(cx.paths, &cx.config()?.data_dir))
}

/// The configured Nexus key, only to redact it from installer logs.
fn nexus_secret(cx: &Cx<'_>) -> Option<String> {
    appnx::load_key(cx.paths, cx.store)
        .ok()
        .flatten()
        .map(|k| k.expose().to_string())
}

/// A loader of the active game for the Requirements card.
#[derive(Debug, Clone, Serialize)]
pub struct LoaderCard {
    #[serde(flatten)]
    pub view: LoaderView,
    /// What startup recovery did with this install's last run (this app
    /// start); a rollback is offered while the status is `interrupted`.
    pub recovered: Option<Recovered>,
}

fn install_of(s: &LoaderStatus) -> Option<i64> {
    match s {
        LoaderStatus::Installed { install_id, .. }
        | LoaderStatus::Busy { install_id, .. }
        | LoaderStatus::Drifted { install_id, .. } => Some(*install_id),
        _ => None,
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn loader_status(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<Vec<LoaderCard>> {
    let recovered = state.0.loader_recovered.get().cloned().unwrap_or_default();
    query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        let env = loader_env(cx)?;
        Ok(apploaders::views(&env, cx.store, &g)?
            .into_iter()
            .map(|view| {
                let id = install_of(&view.status);
                LoaderCard {
                    recovered: recovered.iter().find(|r| Some(r.install_id) == id).cloned(),
                    view,
                }
            })
            .collect())
    })
    .await
}

/// A preview as the consent dialog shows it.
#[derive(Debug, Clone, Serialize)]
pub struct PreviewView {
    #[serde(flatten)]
    pub preview: Preview,
    /// The installer is killed after this long.
    pub timeout_secs: u64,
}

fn preview_view(env: &LoaderEnv, preview: Preview) -> PreviewView {
    PreviewView {
        preview,
        timeout_secs: env.timeout.as_secs(),
    }
}

/// Previews a loader op (nothing runs). `archive`: a local loader archive
/// (install, upgrade, adopt; unverified unless `unverified_ack`). Repair
/// and uninstall use the install's cached archive.
#[tauri::command(rename_all = "snake_case")]
pub async fn loader_preview(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    op: Op,
    loader_key: String,
    archive: Option<String>,
    unverified_ack: bool,
) -> CmdResult<PreviewView> {
    query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        let env = loader_env(cx)?;
        let archive = archive.map(PathBuf::from);
        let origin = DownloadOrigin::Local {
            path: archive.clone().unwrap_or_default(),
        };
        let req = PreviewRequest {
            game_id: g.id,
            op,
            loader_key: Some(loader_key),
            archive,
            origin,
            unverified_ack,
        };
        let p = apploaders::preview(&env, cx.store, &req)?;
        Ok(preview_view(&env, p))
    })
    .await
}

/// A pending preview (from install, a collection or an nxm link) in full,
/// re-previewed with `unverified_ack`; the old token is dropped.
#[tauri::command(rename_all = "snake_case")]
pub async fn loader_preview_reopen(
    app: AppHandle,
    state: St<'_>,
    preview_id: String,
    unverified_ack: bool,
) -> CmdResult<PreviewView> {
    query(app, &state, None, move |cx| {
        let env = loader_env(cx)?;
        let p = apploaders::reopen_preview(&env, cx.store, &preview_id, unverified_ack)?;
        Ok(preview_view(&env, p))
    })
    .await
}

/// Follows a run's step logs (`<run dir>/step-N.log`) and emits complete,
/// redacted lines as `vessel:loader-log` until stopped.
struct LogTail {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl LogTail {
    fn start(
        sink: Arc<dyn Sink>,
        dir: PathBuf,
        preview_id: String,
        home: PathBuf,
        secret: Option<String>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("loader-log".into())
            .spawn(move || {
                // Per step: bytes read so far and the unfinished last line.
                let mut seen: Vec<(u64, String)> = Vec::new();
                loop {
                    let last = flag.load(Ordering::SeqCst);
                    for step in 1u32.. {
                        let path = dir.join(format!("step-{step}.log"));
                        let Ok(bytes) =
                            read_from(&path, seen.get(step as usize - 1).map_or(0, |s| s.0))
                        else {
                            break;
                        };
                        if seen.len() < step as usize {
                            seen.push((0, String::new()));
                        }
                        let (off, buf) = &mut seen[step as usize - 1];
                        *off += bytes.len() as u64;
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                        // Whole lines only, so a secret is never split
                        // across two redaction passes.
                        let cut = if last {
                            buf.len()
                        } else {
                            buf.rfind('\n').map_or(0, |i| i + 1)
                        };
                        if cut > 0 {
                            let chunk: String = buf.drain(..cut).collect();
                            let secrets: Vec<&str> = secret.as_deref().into_iter().collect();
                            let text = housekeeping::redact_with(&chunk, &home, &secrets);
                            crate::events::emit(
                                &*sink,
                                crate::events::LOADER_LOG,
                                &crate::events::LoaderLog {
                                    preview_id: &preview_id,
                                    step,
                                    text: &text,
                                },
                            );
                        }
                    }
                    if last {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            })
            .ok();
        Self { stop, thread }
    }

    fn finish(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn read_from(path: &std::path::Path, offset: u64) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut out = Vec::new();
    f.read_to_end(&mut out)?;
    Ok(out)
}

/// Job: runs a confirmed preview. `plan_hash` must be the one the user
/// saw. Runs on this blocking thread until the installer is gone (the
/// runner's PDEATHSIG contract) and streams `vessel:loader-log`.
#[tauri::command(rename_all = "snake_case")]
pub async fn loader_run(
    app: AppHandle,
    state: St<'_>,
    preview_id: String,
    plan_hash: String,
) -> CmdResult<RunReport> {
    let s = sink(&app);
    job(app, &state, "loader_run", None, move |cx| {
        let env = loader_env(cx)?;
        let tail = preview_id.bytes().all(|b| b.is_ascii_hexdigit()).then(|| {
            LogTail::start(
                s,
                env.data_dir.join("loaders/runs").join(&preview_id),
                preview_id.clone(),
                env.home.clone(),
                nexus_secret(cx),
            )
        });
        let r = apploaders::run(&env, cx.store, &preview_id, &plan_hash);
        if let Some(t) = tail {
            t.finish();
        }
        r
    })
    .await
}

/// Job: undoes every tracked change of an install (the offered rollback
/// after an interrupted run). Returns what could not be restored.
#[tauri::command(rename_all = "snake_case")]
pub async fn loader_rollback(
    app: AppHandle,
    state: St<'_>,
    install_id: i64,
) -> CmdResult<Vec<String>> {
    job(app, &state, "loader_rollback", None, move |cx| {
        let env = loader_env(cx)?;
        apploaders::rollback(&env, cx.store, install_id)
    })
    .await
}

/// Keeps an interrupted install's files (the offered rollback declined).
#[tauri::command(rename_all = "snake_case")]
pub async fn loader_decline_rollback(
    app: AppHandle,
    state: St<'_>,
    install_id: i64,
) -> CmdResult<()> {
    query(app, &state, None, move |cx| {
        let env = loader_env(cx)?;
        apploaders::decline_rollback(&env, cx.store, install_id)
    })
    .await
}

/// `runtime`: native, proton, or null to detect it again.
#[tauri::command(rename_all = "snake_case")]
pub async fn game_runtime_set(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    runtime: Option<String>,
) -> CmdResult<()> {
    query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        apploaders::set_runtime_override(cx.store, &g.id, runtime.as_deref())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn launch_options_get(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<LaunchState> {
    query(app, &state, game, |cx| {
        let g = cx.need_game()?;
        Ok(cx.store.launch_state(&g.id).map_err(LoaderError::from)?)
    })
    .await
}

/// The user set the desired launch options by hand; returns them.
#[tauri::command(rename_all = "snake_case")]
pub async fn launch_options_ack(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<Option<String>> {
    query(app, &state, game, |cx| {
        let g = cx.need_game()?;
        Ok(cx
            .store
            .ack_launch_options(&g.id)
            .map_err(LoaderError::from)?)
    })
    .await
}

/// Writes the desired launch options into Steam's localconfig.vdf for
/// `account` (refused while Steam runs). Without `account` and several
/// accounts, returns the list to pick from and writes nothing.
#[tauri::command(rename_all = "snake_case")]
pub async fn launch_options_apply(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    account: Option<u32>,
) -> CmdResult<ApplyOutcome> {
    query(app, &state, game, move |cx| {
        let g = cx.need_game()?;
        let appid = g.appid.ok_or_else(|| {
            vessel_core::app::AppError::Msg(format!("{} has no Steam app id.", g.name))
        })?;
        Ok(
            launch::apply(cx.store, &cx.paths.home, &g.id, appid, account)
                .map_err(LoaderError::from)?,
        )
    })
    .await
}

/// Mod rows that are really loader archives (an old SMAPI installer under Mods/).
#[tauri::command(rename_all = "snake_case")]
pub async fn loader_junk_scan(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
) -> CmdResult<Vec<LoaderJunk>> {
    query(app, &state, game, |cx| {
        let g = cx.need_game()?;
        apploaders::find_loader_junk(cx, &g)
    })
    .await
}

/// Job: removes those rows through the normal mod removal (purge, delete,
/// redeploy). Returns how many were removed.
#[tauri::command(rename_all = "snake_case")]
pub async fn loader_junk_remove(
    app: AppHandle,
    state: St<'_>,
    game: Option<String>,
    mod_ids: Vec<i64>,
) -> CmdResult<usize> {
    job(app, &state, "loader_junk_remove", game, move |cx| {
        let g = cx.need_game()?;
        apploaders::remove_loader_junk(cx, &g, &mod_ids)
    })
    .await
}

/// A run's installer output, redacted (API keys, tokens, home path).
#[tauri::command(rename_all = "snake_case")]
pub async fn loader_log_get(app: AppHandle, state: St<'_>, run_id: i64) -> CmdResult<String> {
    query(app, &state, None, move |cx| {
        let env = loader_env(cx)?;
        let key = nexus_secret(cx);
        Ok(housekeeping::read_log(
            &env,
            cx.store,
            run_id,
            key.as_deref(),
        )?)
    })
    .await
}

#[cfg(test)]
mod loader_tests {
    use super::*;
    use crate::events::testing::Recorder;

    /// Catches: installer output reaching the webview unredacted, a line
    /// split across events (a key could straddle two redaction passes),
    /// or the tail losing output written just before the run returned.
    #[test]
    fn log_tail_streams_whole_redacted_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let dir = tmp.path().join("run");
        std::fs::create_dir_all(&dir).unwrap();
        let rec = Arc::new(Recorder::default());
        let tail = LogTail::start(
            rec.clone(),
            dir.clone(),
            "ab12".into(),
            home.clone(),
            Some("SECRETKEY0123456789".into()),
        );
        std::fs::write(
            dir.join("step-1.log"),
            format!("in {}/game\nkey SECRETKEY0123456789\npart", home.display()),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::fs::write(dir.join("step-2.log"), "two\n").unwrap();
        tail.finish();
        let evs = rec.named(crate::events::LOADER_LOG);
        let text: String = evs
            .iter()
            .map(|e| e["text"].as_str().unwrap().to_string())
            .collect();
        assert!(!text.contains("SECRETKEY"), "{text}");
        assert!(!text.contains(&home.display().to_string()), "{text}");
        assert!(text.contains("~/game"), "{text}");
        assert!(text.contains("part") && text.contains("two"), "{text}");
        assert!(evs.iter().all(|e| e["preview_id"] == "ab12"));
        assert!(evs.iter().any(|e| e["step"] == 2));
        // "part" had no newline: held back until the run ended, never split.
        assert!(evs.iter().any(|e| e["text"] == "part"));
    }

    /// Catches: the loader card losing the recovery note for its install.
    #[test]
    fn install_ids_come_from_every_tracked_state() {
        assert_eq!(
            install_of(&LoaderStatus::Busy {
                install_id: 3,
                status: "interrupted".into()
            }),
            Some(3)
        );
        assert_eq!(install_of(&LoaderStatus::NotInstalled), None);
    }
}
