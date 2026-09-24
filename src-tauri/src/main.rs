#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod mcp;
mod mockserver;
mod scheduler;

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use mcp::{ConnAcl, McpConfig, McpHandle};
use tokio::sync::Mutex as AsyncMutex;

use og_testdesk_core::{
    apply_environment, drivers, requests as http_requests, Column, ConnConfig, Environment,
    HistoryEntry, HttpRequest, HttpResponse, MetadataStore, QueryResult, QueryTab,
    RequestCollection, RequestTab, SavedChart, SavedQuery, SavedQueryFolder, SavedRequest, Schema,
    SecretsBackend, SecretsStore, ServerInfo,
};
use tauri::{Manager, State};

struct AppState {
    metadata: Arc<MetadataStore>,
    mcp: Arc<AsyncMutex<Option<McpHandle>>>,
    mock: Arc<AsyncMutex<Option<mockserver::MockHandle>>>,
    exports_dir: std::path::PathBuf,
    /// Last debug-state snapshot the frontend reported (raw JSON string,
    /// opaque to the backend) — see `debug_state_set`/`debug_state_get`
    /// and the MCP `get_app_state` tool.
    debug_state: Arc<AsyncMutex<String>>,
    /// Set once in `.setup()`, once an AppHandle actually exists — MCP
    /// needs this to emit events (`mcp:sql-tab-opened` etc.) straight
    /// into the window so an AI-opened tab shows up live instead of
    /// waiting for a reload.
    app_handle: OnceLock<tauri::AppHandle>,
}

async fn load_mcp_config(meta: &MetadataStore) -> McpConfig {
    let mut cfg: McpConfig = meta
        .get_state("mcp_config")
        .await
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    if cfg.token.is_empty() {
        cfg.token = uuid::Uuid::new_v4().simple().to_string();
    }
    cfg
}

async fn save_mcp_config(meta: &MetadataStore, cfg: &McpConfig) -> Result<(), String> {
    meta.set_state("mcp_config", &serde_json::to_string(cfg).map_err(err)?)
        .await
        .map_err(err)
}

type R<T> = Result<T, String>;

/// Every command's `.map_err(err)` funnels through here, so this is the
/// one choke point that puts backend errors into the error log without
/// having to touch every call site individually. `#[track_caller]` gets
/// us the call site (file:line) for free as the log's `source`.
#[track_caller]
fn err<E: std::fmt::Display>(e: E) -> String {
    let msg = e.to_string();
    let loc = std::panic::Location::caller();
    og_testdesk_core::record_error(&format!("{}:{}", loc.file(), loc.line()), &msg);
    msg
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Best-effort detection of a tiling window manager, where the OS min/max
/// window controls are meaningless.
fn is_tiling_wm() -> bool {
    use std::env::var;
    if var("SWAYSOCK").is_ok()
        || var("I3SOCK").is_ok()
        || var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
    {
        return true;
    }
    const TILERS: &[&str] = &[
        "sway", "i3", "hyprland", "river", "dwm", "bspwm", "qtile", "xmonad", "awesome",
        "herbstluftwm", "wmii", "spectrwm", "niri", "dk", "leftwm", "cwm",
    ];
    let de = var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_ascii_lowercase();
    let sess = var("DESKTOP_SESSION").unwrap_or_default().to_ascii_lowercase();
    let wm = var("XDG_SESSION_DESKTOP").unwrap_or_default().to_ascii_lowercase();
    TILERS
        .iter()
        .any(|t| de.contains(t) || sess.contains(t) || wm.contains(t))
}

#[tauri::command]
fn window_environment() -> serde_json::Value {
    serde_json::json!({
        "tiling": is_tiling_wm(),
        "os": std::env::consts::OS,
    })
}

/// Build-time capability flags the frontend can't otherwise detect — right
/// now just whether this is the Mac App Store build (App Sandbox), where
/// the pre-connect-command field is unavailable (arbitrary shell exec).
/// Hiding the field there beats showing one that just errors when used.
#[tauri::command]
fn app_capabilities() -> serde_json::Value {
    serde_json::json!({
        "preConnectCmd": !cfg!(feature = "app-store"),
    })
}

// ----------------------------------------------------------------- connections

#[tauri::command]
async fn connections_list(state: State<'_, AppState>) -> R<Vec<ConnConfig>> {
    state.metadata.list_connections().await.map_err(err)
}

#[tauri::command]
async fn connection_save(
    state: State<'_, AppState>,
    mut config: ConnConfig,
    password: Option<String>,
) -> R<ConnConfig> {
    if config.id.is_empty() {
        config.id = new_id();
    }
    if let Some(pw) = password {
        if pw.is_empty() {
            let _ = SecretsStore::delete(&config.id);
        } else {
            SecretsStore::set(&config.id, &pw).map_err(err)?;
        }
    }
    // A saved edit might have changed the SSH settings (or turned the
    // tunnel off) — never let a stale tunnel from the old config linger
    // and get reused. A fresh one is started transparently on next use.
    drivers::tunnel::stop_tunnel(&config.id);
    state.metadata.upsert_connection(&config).await.map_err(err)?;
    Ok(config)
}

#[tauri::command]
async fn connection_delete(state: State<'_, AppState>, id: String) -> R<()> {
    let _ = SecretsStore::delete(&id);
    drivers::tunnel::stop_tunnel(&id);
    state.metadata.delete_connection(&id).await.map_err(err)
}

#[tauri::command]
async fn connections_reorder(state: State<'_, AppState>, ids: Vec<String>) -> R<()> {
    state.metadata.reorder_connections(&ids).await.map_err(err)
}

#[tauri::command]
async fn connection_test(config: ConnConfig, password: Option<String>) -> R<ServerInfo> {
    let pw = match password {
        Some(p) => Some(p),
        None => drivers::tunnel::resolve_password_for(&config).await.map_err(err)?,
    };
    drivers::driver_for(config.kind)
        .test_connection(&config, pw.as_deref())
        .await
        .map_err(err)
}

// ------------------------------------------------------------------- schema

#[tauri::command]
async fn schemas_list(config: ConnConfig) -> R<Vec<Schema>> {
    let pw = drivers::tunnel::resolve_password_for(&config).await.map_err(err)?;
    drivers::driver_for(config.kind)
        .list_schemas(&config, pw.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
async fn columns_list(config: ConnConfig, schema: String, relation: String) -> R<Vec<Column>> {
    let pw = drivers::tunnel::resolve_password_for(&config).await.map_err(err)?;
    drivers::driver_for(config.kind)
        .list_columns(&config, pw.as_deref(), &schema, &relation)
        .await
        .map_err(err)
}

#[tauri::command]
async fn foreign_keys_list(config: ConnConfig) -> R<Vec<og_testdesk_core::ForeignKey>> {
    let pw = drivers::tunnel::resolve_password_for(&config).await.map_err(err)?;
    drivers::driver_for(config.kind)
        .list_foreign_keys(&config, pw.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
async fn functions_list(config: ConnConfig) -> R<Vec<og_testdesk_core::SqlFunction>> {
    let pw = drivers::tunnel::resolve_password_for(&config).await.map_err(err)?;
    drivers::driver_for(config.kind)
        .list_functions(&config, pw.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
async fn db_time(config: ConnConfig) -> R<og_testdesk_core::DbTime> {
    let pw = drivers::tunnel::resolve_password_for(&config).await.map_err(err)?;
    drivers::driver_for(config.kind)
        .server_time(&config, pw.as_deref())
        .await
        .map_err(err)
}

// -------------------------------------------------------------------- query

#[tauri::command]
async fn query_run(
    state: State<'_, AppState>,
    config: ConnConfig,
    sql: String,
    page: Option<usize>,
    page_size: Option<usize>,
    count: Option<bool>,
) -> R<QueryResult> {
    let pw = drivers::tunnel::resolve_password_for(&config).await.map_err(err)?;
    let opts = match page_size {
        Some(size) if size > 0 => {
            og_testdesk_core::QueryOpts::page(page.unwrap_or(0), size, count.unwrap_or(false))
        }
        _ => og_testdesk_core::QueryOpts::full(),
    };
    let result = drivers::driver_for(config.kind)
        .run_query(&config, pw.as_deref(), &sql, opts)
        .await;

    // Only record history for the first page of a query.
    if page.unwrap_or(0) == 0 {
        let entry = HistoryEntry {
            id: new_id(),
            connection_id: Some(config.id.clone()),
            sql_text: sql.clone(),
            duration_ms: result.as_ref().ok().map(|r| r.duration_ms as i64),
            row_count: result
                .as_ref()
                .ok()
                .map(|r| r.total.unwrap_or(r.row_count) as i64),
            success: result.is_ok(),
            error: result.as_ref().err().map(|e| e.to_string()),
            result_json: result.as_ref().ok().and_then(scheduler::cache_result_json),
            has_result: false,
            ran_at: now(),
            via_mcp: false,
        };
        let _ = state.metadata.add_history(&entry).await;
    }

    result.map_err(err)
}

/// Fetches only the total row count for a statement (LIMIT 0 + COUNT(*),
/// long time budget) — the frontend calls this in the background to fill
/// in a total that the initial `query_run`'s fast, time-boxed count
/// gave up on.
#[tauri::command]
async fn query_count(config: ConnConfig, sql: String) -> R<QueryResult> {
    let pw = drivers::tunnel::resolve_password_for(&config).await.map_err(err)?;
    drivers::driver_for(config.kind)
        .run_query(&config, pw.as_deref(), &sql, og_testdesk_core::QueryOpts::count_only())
        .await
        .map_err(err)
}

#[tauri::command]
async fn history_result(state: State<'_, AppState>, id: String) -> R<Option<String>> {
    state.metadata.history_result(&id).await.map_err(err)
}

#[tauri::command]
async fn history_request_result(state: State<'_, AppState>, id: String) -> R<Option<String>> {
    state.metadata.request_history_result(&id).await.map_err(err)
}

#[tauri::command]
async fn query_limits_get(state: State<'_, AppState>) -> R<usize> {
    Ok(state
        .metadata
        .get_state("query_max_rows")
        .await
        .map_err(err)?
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000))
}

#[tauri::command]
async fn query_limits_set(state: State<'_, AppState>, max_rows: usize) -> R<()> {
    og_testdesk_core::set_max_rows(max_rows);
    state
        .metadata
        .set_state("query_max_rows", &max_rows.to_string())
        .await
        .map_err(err)
}

#[tauri::command]
async fn history_request_recent(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> R<Vec<og_testdesk_core::RequestHistoryEntry>> {
    state
        .metadata
        .recent_request_history(limit.unwrap_or(100))
        .await
        .map_err(err)
}

// --------------------------------------------------------------------- tabs

#[tauri::command]
async fn tabs_list_all(state: State<'_, AppState>) -> R<Vec<QueryTab>> {
    state.metadata.list_all_tabs().await.map_err(err)
}

#[tauri::command]
async fn tab_save(state: State<'_, AppState>, mut tab: QueryTab) -> R<QueryTab> {
    if tab.id.is_empty() {
        tab.id = new_id();
    }
    state.metadata.upsert_tab(&tab).await.map_err(err)?;
    Ok(tab)
}

#[tauri::command]
async fn tab_delete(state: State<'_, AppState>, id: String) -> R<()> {
    state.metadata.delete_tab(&id).await.map_err(err)
}

// ------------------------------------------------------------------ history

#[tauri::command]
async fn history_recent(state: State<'_, AppState>, limit: Option<i64>) -> R<Vec<HistoryEntry>> {
    state
        .metadata
        .recent_history(limit.unwrap_or(100))
        .await
        .map_err(err)
}

// ------------------------------------------------------------- saved queries

#[tauri::command]
async fn saved_queries_list(state: State<'_, AppState>) -> R<Vec<SavedQuery>> {
    state.metadata.list_saved_queries().await.map_err(err)
}

#[tauri::command]
async fn saved_query_save(state: State<'_, AppState>, mut query: SavedQuery) -> R<SavedQuery> {
    if query.id.is_empty() {
        query.id = new_id();
    }
    state.metadata.upsert_saved_query(&query).await.map_err(err)?;
    Ok(query)
}

#[tauri::command]
async fn saved_query_delete(state: State<'_, AppState>, id: String) -> R<()> {
    state.metadata.delete_saved_query(&id).await.map_err(err)
}

#[tauri::command]
fn secrets_status() -> SecretsBackend {
    SecretsStore::backend()
}

// Generic app-level secret storage for things that aren't a DB connection
// password but still shouldn't sit in the plain metadata SQLite file —
// a fetched OAuth2 access token, a Basic-auth user:pass, an API key
// typed into a saved request's Auth tab. The frontend writes a `key`
// here and stores only a `{{secret:<key>}}` placeholder in the actual
// request/header text; `request_send` (and the MCP server's own send
// path) resolve that placeholder from here at send time, the same way
// `{{var}}` environment substitution already works. `authhdr:` prefixes
// every key so this can never collide with a connection id (also keyed
// directly, with no prefix, in the same underlying store).
#[tauri::command]
fn secret_set(key: String, value: String) -> R<()> {
    SecretsStore::set(&format!("authhdr:{key}"), &value).map_err(err)
}
#[tauri::command]
fn secret_get(key: String) -> R<Option<String>> {
    SecretsStore::get(&format!("authhdr:{key}")).map_err(err)
}
#[tauri::command]
fn secret_delete(key: String) -> R<()> {
    SecretsStore::delete(&format!("authhdr:{key}")).map_err(err)
}

#[tauri::command]
async fn saved_query_folders_list(state: State<'_, AppState>) -> R<Vec<SavedQueryFolder>> {
    state.metadata.list_saved_query_folders().await.map_err(err)
}

#[tauri::command]
async fn saved_query_folder_save(
    state: State<'_, AppState>,
    mut folder: SavedQueryFolder,
) -> R<SavedQueryFolder> {
    if folder.id.is_empty() {
        folder.id = new_id();
    }
    state
        .metadata
        .upsert_saved_query_folder(&folder)
        .await
        .map_err(err)?;
    Ok(folder)
}

#[tauri::command]
async fn saved_query_folder_delete(state: State<'_, AppState>, id: String) -> R<()> {
    state
        .metadata
        .delete_saved_query_folder(&id)
        .await
        .map_err(err)
}

// -------------------------------------------------------------- saved charts

#[tauri::command]
async fn saved_charts_list(state: State<'_, AppState>) -> R<Vec<SavedChart>> {
    state.metadata.list_saved_charts().await.map_err(err)
}

#[tauri::command]
async fn saved_chart_data(state: State<'_, AppState>, id: String) -> R<Option<String>> {
    state.metadata.chart_data(&id).await.map_err(err)
}

#[tauri::command]
async fn saved_chart_save(state: State<'_, AppState>, mut chart: SavedChart) -> R<SavedChart> {
    if chart.id.is_empty() {
        chart.id = new_id();
    }
    state.metadata.upsert_saved_chart(&chart).await.map_err(err)?;
    Ok(chart)
}

#[tauri::command]
async fn saved_chart_delete(state: State<'_, AppState>, id: String) -> R<()> {
    state.metadata.delete_saved_chart(&id).await.map_err(err)
}

// -------------------------------------------------------------- collections

#[tauri::command]
async fn collections_list(state: State<'_, AppState>) -> R<Vec<RequestCollection>> {
    state.metadata.list_collections().await.map_err(err)
}

#[tauri::command]
async fn collection_save(
    state: State<'_, AppState>,
    mut collection: RequestCollection,
) -> R<RequestCollection> {
    if collection.id.is_empty() {
        collection.id = new_id();
    }
    state
        .metadata
        .upsert_collection(&collection)
        .await
        .map_err(err)?;
    Ok(collection)
}

#[tauri::command]
async fn collection_delete(state: State<'_, AppState>, id: String) -> R<()> {
    state.metadata.delete_collection(&id).await.map_err(err)
}

// ------------------------------------------------------------ saved requests

#[tauri::command]
async fn saved_requests_list(state: State<'_, AppState>) -> R<Vec<SavedRequest>> {
    state.metadata.list_saved_requests().await.map_err(err)
}

#[tauri::command]
async fn request_tabs_list(state: State<'_, AppState>) -> R<Vec<RequestTab>> {
    state.metadata.list_request_tabs().await.map_err(err)
}

#[tauri::command]
async fn request_tab_save(state: State<'_, AppState>, mut tab: RequestTab) -> R<RequestTab> {
    if tab.id.is_empty() {
        tab.id = new_id();
    }
    state.metadata.upsert_request_tab(&tab).await.map_err(err)?;
    Ok(tab)
}

#[tauri::command]
async fn request_tab_delete(state: State<'_, AppState>, id: String) -> R<()> {
    state.metadata.delete_request_tab(&id).await.map_err(err)
}

#[tauri::command]
async fn saved_request_save(
    state: State<'_, AppState>,
    mut request: SavedRequest,
) -> R<SavedRequest> {
    if request.id.is_empty() {
        request.id = new_id();
    }
    state
        .metadata
        .upsert_saved_request(&request)
        .await
        .map_err(err)?;
    Ok(request)
}

#[tauri::command]
async fn saved_request_delete(state: State<'_, AppState>, id: String) -> R<()> {
    state.metadata.delete_saved_request(&id).await.map_err(err)
}

// -------------------------------------------------------------- environments

#[tauri::command]
async fn environments_list(state: State<'_, AppState>) -> R<Vec<Environment>> {
    state.metadata.list_environments().await.map_err(err)
}

#[tauri::command]
async fn environment_save(state: State<'_, AppState>, mut environment: Environment) -> R<Environment> {
    if environment.id.is_empty() {
        environment.id = new_id();
    }
    state
        .metadata
        .upsert_environment(&environment)
        .await
        .map_err(err)?;
    Ok(environment)
}

#[tauri::command]
async fn environment_delete(state: State<'_, AppState>, id: String) -> R<()> {
    state.metadata.delete_environment(&id).await.map_err(err)
}

// ------------------------------------------------------------------ request

#[tauri::command]
async fn request_send(
    state: State<'_, AppState>,
    mut request: HttpRequest,
    apply_env: Option<bool>,
    saved_request_id: Option<String>,
    name: Option<String>,
) -> R<HttpResponse> {
    // Secret-backed auth headers (Bearer/Basic/API-key/OAuth2 tokens —
    // see resolve_secret_placeholders) resolve unconditionally, same as
    // the human's own "Send" button always needing its own auth to
    // actually go out regardless of the apply_env toggle (which only
    // ever meant "also apply globals/environment", never "auth is
    // optional").
    let secret_vars = og_testdesk_core::resolve_secret_placeholders(&request);
    if !secret_vars.is_empty() {
        apply_environment(&mut request, &secret_vars);
    }
    if apply_env.unwrap_or(true) {
        let mut vars: HashMap<String, String> = HashMap::new();
        // globals first (lowest precedence)
        if let Some(raw) = state.metadata.get_state("request_globals").await.map_err(err)? {
            if let Ok(g) = serde_json::from_str::<HashMap<String, String>>(&raw) {
                vars.extend(g);
            }
        }
        // active environment overrides globals
        let envs = state.metadata.list_environments().await.map_err(err)?;
        if let Some(active) = envs.into_iter().find(|e| e.is_active) {
            if let Ok(e) = serde_json::from_str::<HashMap<String, String>>(&active.variables_json) {
                vars.extend(e);
            }
        }
        if !vars.is_empty() {
            apply_environment(&mut request, &vars);
        }
    }

    let net = load_network_settings(&state.metadata).await;
    let result = http_requests::send_with(&request, &net).await;
    let entry = og_testdesk_core::RequestHistoryEntry {
        id: new_id(),
        saved_request_id,
        name,
        method: request.method.clone(),
        url: request.url.clone(),
        headers_json: serde_json::to_string(&request.headers).unwrap_or_default(),
        body: request.body.clone(),
        status: result.as_ref().ok().map(|r| r.status as i64),
        duration_ms: result.as_ref().ok().map(|r| r.duration_ms as i64),
        size_bytes: result.as_ref().ok().map(|r| r.size_bytes as i64),
        success: result.as_ref().map(|r| r.status < 400).unwrap_or(false),
        error: result.as_ref().err().map(|e| e.to_string()),
        response_json: result.as_ref().ok().and_then(scheduler::cache_response_json),
        has_response: false,
        sent_at: now(),
        via_mcp: false,
    };
    let _ = state.metadata.add_request_history(&entry).await;

    result.map_err(err)
}

// ----------------------------------------------------------------------- mcp

#[tauri::command]
async fn mcp_config_get(state: State<'_, AppState>) -> R<McpConfig> {
    Ok(load_mcp_config(&state.metadata).await)
}

#[tauri::command]
async fn mcp_status(state: State<'_, AppState>) -> R<serde_json::Value> {
    let guard = state.mcp.lock().await;
    Ok(serde_json::json!({
        "running": guard.is_some(),
        "port": guard.as_ref().map(|h| h.port),
    }))
}

#[tauri::command]
async fn mcp_config_set(state: State<'_, AppState>, config: McpConfig) -> R<serde_json::Value> {
    save_mcp_config(&state.metadata, &config).await?;
    // restart to apply
    {
        let mut guard = state.mcp.lock().await;
        if let Some(h) = guard.take() {
            h.stop();
        }
    }
    if config.enabled {
        start_mcp(&state).await?;
    }
    mcp_status(state).await
}

#[tauri::command]
async fn mcp_start(state: State<'_, AppState>) -> R<serde_json::Value> {
    let mut cfg = load_mcp_config(&state.metadata).await;
    cfg.enabled = true;
    save_mcp_config(&state.metadata, &cfg).await?;
    start_mcp(&state).await?;
    mcp_status(state).await
}

#[tauri::command]
async fn mcp_stop(state: State<'_, AppState>) -> R<serde_json::Value> {
    let mut cfg = load_mcp_config(&state.metadata).await;
    cfg.enabled = false;
    save_mcp_config(&state.metadata, &cfg).await?;
    if let Some(h) = state.mcp.lock().await.take() {
        h.stop();
    }
    mcp_status(state).await
}

#[tauri::command]
async fn mcp_acls_get(state: State<'_, AppState>) -> R<HashMap<String, ConnAcl>> {
    Ok(state
        .metadata
        .get_state("mcp_connections")
        .await
        .map_err(err)?
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default())
}

#[tauri::command]
async fn mcp_acl_set(
    state: State<'_, AppState>,
    connection_id: String,
    acl: ConnAcl,
) -> R<()> {
    let mut map: HashMap<String, ConnAcl> = state
        .metadata
        .get_state("mcp_connections")
        .await
        .map_err(err)?
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    map.insert(connection_id, acl);
    state
        .metadata
        .set_state("mcp_connections", &serde_json::to_string(&map).map_err(err)?)
        .await
        .map_err(err)
}

async fn start_mcp(state: &State<'_, AppState>) -> R<()> {
    let cfg = load_mcp_config(&state.metadata).await;
    let mut guard = state.mcp.lock().await;
    if guard.is_some() {
        return Ok(());
    }
    let app_handle = state
        .app_handle
        .get()
        .cloned()
        .ok_or_else(|| "app not fully started yet".to_string())?;
    let handle = mcp::start(
        state.metadata.clone(),
        cfg,
        state.exports_dir.clone(),
        state.debug_state.clone(),
        app_handle,
    )
    .await
    .map_err(err)?;
    *guard = Some(handle);
    Ok(())
}

// ----------------------------------------------------------------- schedules

#[tauri::command]
async fn schedules_list(state: State<'_, AppState>) -> R<Vec<og_testdesk_core::Schedule>> {
    state.metadata.list_schedules().await.map_err(err)
}

#[tauri::command]
async fn schedule_save(
    state: State<'_, AppState>,
    mut schedule: og_testdesk_core::Schedule,
) -> R<og_testdesk_core::Schedule> {
    if schedule.id.is_empty() {
        schedule.id = new_id();
    }
    if scheduler::next_run_after(&schedule.schedule_expr, now()).is_none() {
        return Err(format!("invalid schedule expression: {}", schedule.schedule_expr));
    }
    schedule.next_run = if schedule.enabled {
        scheduler::next_run_after(&schedule.schedule_expr, now())
    } else {
        None
    };
    state.metadata.upsert_schedule(&schedule).await.map_err(err)?;
    Ok(schedule)
}

#[tauri::command]
async fn schedule_delete(state: State<'_, AppState>, id: String) -> R<()> {
    state.metadata.delete_schedule(&id).await.map_err(err)
}

#[tauri::command]
async fn schedule_run_now(state: State<'_, AppState>, id: String) -> R<String> {
    let sched = state
        .metadata
        .get_schedule(&id)
        .await
        .map_err(err)?
        .ok_or_else(|| "no such schedule".to_string())?;
    let status = scheduler::run_one(&state.metadata, &sched).await;
    let next = scheduler::next_run_after(&sched.schedule_expr, now());
    let _ = state
        .metadata
        .mark_schedule_run(&id, now(), &status, if sched.enabled { next } else { None })
        .await;
    Ok(status)
}

// ---------------------------------------------------------------- app state

#[tauri::command]
async fn state_get(state: State<'_, AppState>, key: String) -> R<Option<String>> {
    state.metadata.get_state(&key).await.map_err(err)
}

#[tauri::command]
async fn state_set(state: State<'_, AppState>, key: String, value: String) -> R<()> {
    state.metadata.set_state(&key, &value).await.map_err(err)
}

// ---------------------------------------------------------------- error log

#[tauri::command]
async fn error_log_list(limit: Option<usize>) -> R<Vec<og_testdesk_core::ErrorLogEntry>> {
    Ok(og_testdesk_core::recent_errors(limit))
}

#[tauri::command]
async fn error_log_clear() -> R<()> {
    og_testdesk_core::clear_error_log();
    Ok(())
}

// ------------------------------------------------------------------- gRPC

#[derive(serde::Serialize)]
struct GrpcMethodOut {
    service: String,
    method: String,
    input_type: String,
    output_type: String,
    client_streaming: bool,
    server_streaming: bool,
}

#[tauri::command]
async fn grpc_list_services(url: String) -> R<Vec<String>> {
    og_testdesk_core::grpc::list_services(&url).await.map_err(err)
}

#[tauri::command]
async fn grpc_list_methods(url: String, service: String) -> R<Vec<GrpcMethodOut>> {
    let methods = og_testdesk_core::grpc::list_methods(&url, &service).await.map_err(err)?;
    Ok(methods
        .into_iter()
        .map(|m| GrpcMethodOut {
            service: m.service,
            method: m.method,
            input_type: m.input_type,
            output_type: m.output_type,
            client_streaming: m.client_streaming,
            server_streaming: m.server_streaming,
        })
        .collect())
}

#[tauri::command]
async fn grpc_call_unary(url: String, service: String, method: String, payload: String) -> R<String> {
    og_testdesk_core::grpc::call_unary(&url, &service, &method, &payload)
        .await
        .map_err(err)
}

// -------------------------------------------------------------- mock server

#[tauri::command]
async fn mock_routes_list(state: State<'_, AppState>) -> R<Vec<og_testdesk_core::MockRoute>> {
    state.metadata.list_mock_routes().await.map_err(err)
}

#[tauri::command]
async fn mock_route_save(
    state: State<'_, AppState>,
    mut route: og_testdesk_core::MockRoute,
) -> R<og_testdesk_core::MockRoute> {
    if route.id.is_empty() {
        route.id = new_id();
    }
    state.metadata.upsert_mock_route(&route).await.map_err(err)?;
    Ok(route)
}

#[tauri::command]
async fn mock_route_delete(state: State<'_, AppState>, id: String) -> R<()> {
    state.metadata.delete_mock_route(&id).await.map_err(err)
}

#[tauri::command]
async fn mock_server_status(state: State<'_, AppState>) -> R<serde_json::Value> {
    let running = state.mock.lock().await.as_ref().map(|h| h.port);
    Ok(serde_json::json!({ "running": running.is_some(), "port": running }))
}

#[tauri::command]
async fn mock_server_start(state: State<'_, AppState>, port: u16) -> R<serde_json::Value> {
    if state.mock.lock().await.is_some() {
        return mock_server_status(state).await;
    }
    let handle = mockserver::start(state.metadata.clone(), port).await.map_err(err)?;
    *state.mock.lock().await = Some(handle);
    mock_server_status(state).await
}

#[tauri::command]
async fn mock_server_stop(state: State<'_, AppState>) -> R<serde_json::Value> {
    if let Some(h) = state.mock.lock().await.take() {
        h.stop();
    }
    mock_server_status(state).await
}

// --------------------------------------------------------- network settings

pub(crate) async fn load_network_settings(metadata: &MetadataStore) -> og_testdesk_core::requests::NetworkSettings {
    metadata
        .get_state("network_settings")
        .await
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[tauri::command]
async fn network_settings_get(state: State<'_, AppState>) -> R<og_testdesk_core::requests::NetworkSettings> {
    Ok(load_network_settings(&state.metadata).await)
}

#[tauri::command]
async fn network_settings_set(
    state: State<'_, AppState>,
    settings: og_testdesk_core::requests::NetworkSettings,
) -> R<()> {
    let json = serde_json::to_string(&settings).map_err(err)?;
    state
        .metadata
        .set_state("network_settings", &json)
        .await
        .map_err(err)
}

// -------------------------------------------------------------- cookie jar

#[tauri::command]
async fn cookies_list() -> R<Vec<og_testdesk_core::requests::cookiejar::CookieRecord>> {
    Ok(og_testdesk_core::requests::cookie_jar().list())
}

#[tauri::command]
async fn cookies_clear(domain: Option<String>) -> R<()> {
    og_testdesk_core::requests::cookie_jar().clear(domain.as_deref());
    Ok(())
}

#[tauri::command]
async fn cookie_delete(domain: String, name: String) -> R<()> {
    og_testdesk_core::requests::cookie_jar().delete(&domain, &name);
    Ok(())
}

// ---------------------------------------------------------- oauth2 loopback
//
// For the Authorization Code flow: the frontend opens the system browser to
// the provider's /authorize URL with redirect_uri pointing at this loopback
// listener; the provider redirects the browser back here with ?code=...
// once the human approves. A tiny one-shot HTTP server, not a real one —
// accepts exactly one connection, pulls the query string out of the
// request line, and returns a "you can close this tab" page.

static OAUTH_LISTENERS: OnceLock<AsyncMutex<HashMap<u16, tokio::net::TcpListener>>> = OnceLock::new();

#[tauri::command]
async fn oauth_start_listener() -> R<u16> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(err)?;
    let port = listener.local_addr().map_err(err)?.port();
    OAUTH_LISTENERS
        .get_or_init(|| AsyncMutex::new(HashMap::new()))
        .lock()
        .await
        .insert(port, listener);
    Ok(port)
}

#[tauri::command]
async fn oauth_wait_callback(port: u16, timeout_secs: u64) -> R<HashMap<String, String>> {
    let listener = OAUTH_LISTENERS
        .get_or_init(|| AsyncMutex::new(HashMap::new()))
        .lock()
        .await
        .remove(&port)
        .ok_or_else(|| "no listener on that port — call oauth_start_listener first".to_string())?;

    let fut = async {
        let (mut stream, _) = listener.accept().await?;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).await?;
        let req = String::from_utf8_lossy(&buf[..n]);
        let first_line = req.lines().next().unwrap_or("");
        let path_and_query = first_line.split_whitespace().nth(1).unwrap_or("/");
        let query = path_and_query.splitn(2, '?').nth(1).unwrap_or("");
        let mut params = HashMap::new();
        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut it = pair.splitn(2, '=');
            let k = oauth_urldecode(it.next().unwrap_or(""));
            let v = oauth_urldecode(it.next().unwrap_or(""));
            params.insert(k, v);
        }
        let body = "<html><body style=\"font-family:sans-serif;padding:40px;text-align:center\">\
                     <h2>OG TestDesk</h2><p>Authorization complete \u{2014} you can close this tab.</p>\
                     </body></html>";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(resp.as_bytes()).await?;
        Ok::<_, std::io::Error>(params)
    };
    tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), fut)
        .await
        .map_err(|_| "Timed out waiting for the OAuth redirect".to_string())?
        .map_err(err)
}

fn oauth_urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(byte);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

/// So a frontend-only failure (a fetch that never reaches a Rust command,
/// a JS exception) lands in the same log as backend errors, instead of
/// only ever showing up as a toast the user has to remember and retype.
#[tauri::command]
async fn log_client_error(source: String, message: String) -> R<()> {
    og_testdesk_core::record_error(&format!("frontend:{source}"), &message);
    Ok(())
}

// ------------------------------------------------------------------- debug

/// The frontend pushes its `debugSnapshot` store here (debounced) so the
/// running app's tab/tool/group state can be read from outside the
/// webview — the in-app Activity → Debug tab reads the store directly,
/// but the `get_app_state` MCP tool has no other way to see it.
#[tauri::command]
async fn debug_state_set(state: State<'_, AppState>, json: String) -> R<()> {
    *state.debug_state.lock().await = json;
    Ok(())
}

#[tauri::command]
async fn debug_state_get(state: State<'_, AppState>) -> R<String> {
    Ok(state.debug_state.lock().await.clone())
}

// ---------------------------------------------------------------------- main

#[tokio::main]
async fn main() {
    let app_data_dir = dirs_next::data_dir()
        .expect("no app data dir")
        .join("OGTestDesk");
    std::fs::create_dir_all(&app_data_dir).expect("create app data dir");
    SecretsStore::init_fallback(app_data_dir.clone());
    og_testdesk_core::init_error_log(app_data_dir.clone());
    let db_path = std::env::var("OGTESTDESK_DB_PATH")
        .unwrap_or_else(|_| app_data_dir.join("og_testdesk.db").to_string_lossy().into());

    let metadata = MetadataStore::open(&db_path)
        .await
        .expect("open metadata store");
    let metadata = Arc::new(metadata);
    let exports_dir = app_data_dir.join("exports");
    let debug_state: Arc<AsyncMutex<String>> = Arc::new(AsyncMutex::new(String::new()));
    let mcp_slot: Arc<AsyncMutex<Option<McpHandle>>> = Arc::new(AsyncMutex::new(None));
    let mock_slot: Arc<AsyncMutex<Option<mockserver::MockHandle>>> = Arc::new(AsyncMutex::new(None));

    if let Ok(Some(s)) = metadata.get_state("query_max_rows").await {
        if let Ok(n) = s.parse::<usize>() {
            og_testdesk_core::set_max_rows(n);
        }
    }

    scheduler::spawn(metadata.clone());

    let managed = AppState {
        metadata: metadata.clone(),
        mcp: mcp_slot.clone(),
        mock: mock_slot.clone(),
        exports_dir: exports_dir.clone(),
        debug_state: debug_state.clone(),
        app_handle: OnceLock::new(),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(move |app| {
            app.manage(managed);
            // MCP needs a real AppHandle (to emit `mcp:*` events into the
            // window) which doesn't exist until right here — auto-start
            // lives in this closure instead of before Builder::default()
            // for exactly that reason. The closure itself is sync;
            // spawn the actual (async) startup onto Tauri's runtime.
            let handle = app.handle().clone();
            let _ = app.state::<AppState>().app_handle.set(handle.clone());
            let metadata2 = metadata.clone();
            let exports_dir2 = exports_dir.clone();
            let debug_state2 = debug_state.clone();
            let mcp_slot2 = mcp_slot.clone();
            tauri::async_runtime::spawn(async move {
                let cfg = load_mcp_config(&metadata2).await;
                if cfg.enabled {
                    match mcp::start(metadata2, cfg, exports_dir2, debug_state2, handle).await {
                        Ok(h) => *mcp_slot2.lock().await = Some(h),
                        Err(e) => eprintln!("[mcp] failed to auto-start: {e}"),
                    }
                }
            });
            // macOS keeps its overlaid traffic lights (tauri.conf titleBarStyle).
            // Everywhere else: frameless window, custom controls live in the top bar.
            #[cfg(not(target_os = "macos"))]
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_decorations(false);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            window_environment,
            app_capabilities,
            connections_list,
            connection_save,
            connection_delete,
            connections_reorder,
            connection_test,
            schemas_list,
            columns_list,
            foreign_keys_list,
            functions_list,
            db_time,
            query_run,
            query_count,
            tabs_list_all,
            tab_save,
            tab_delete,
            history_recent,
            history_request_recent,
            history_result,
            history_request_result,
            query_limits_get,
            query_limits_set,
            schedules_list,
            schedule_save,
            schedule_delete,
            schedule_run_now,
            saved_queries_list,
            saved_query_save,
            saved_query_delete,
            secrets_status,
            secret_set,
            secret_get,
            secret_delete,
            saved_query_folders_list,
            saved_query_folder_save,
            saved_query_folder_delete,
            saved_charts_list,
            saved_chart_data,
            saved_chart_save,
            saved_chart_delete,
            collections_list,
            collection_save,
            collection_delete,
            saved_requests_list,
            request_tabs_list,
            request_tab_save,
            request_tab_delete,
            saved_request_save,
            saved_request_delete,
            environments_list,
            environment_save,
            environment_delete,
            request_send,
            state_get,
            state_set,
            error_log_list,
            error_log_clear,
            oauth_start_listener,
            oauth_wait_callback,
            cookies_list,
            cookies_clear,
            cookie_delete,
            network_settings_get,
            network_settings_set,
            grpc_list_services,
            grpc_list_methods,
            grpc_call_unary,
            mock_routes_list,
            mock_route_save,
            mock_route_delete,
            mock_server_status,
            mock_server_start,
            mock_server_stop,
            log_client_error,
            debug_state_set,
            debug_state_get,
            mcp_config_get,
            mcp_config_set,
            mcp_status,
            mcp_start,
            mcp_stop,
            mcp_acls_get,
            mcp_acl_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
