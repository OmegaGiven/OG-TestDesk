#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod mcp;
mod scheduler;

use std::collections::HashMap;
use std::sync::Arc;

use mcp::{ConnAcl, McpConfig, McpHandle};
use tokio::sync::Mutex as AsyncMutex;

use og_testdesk_core::{
    apply_environment, drivers, requests as http_requests, Column, ConnConfig, Environment,
    HistoryEntry, HttpRequest, HttpResponse, MetadataStore, QueryResult, QueryTab,
    RequestCollection, RequestTab, SavedQuery, SavedRequest, Schema, SecretsStore, ServerInfo,
};
use tauri::{Manager, State};

struct AppState {
    metadata: Arc<MetadataStore>,
    mcp: Arc<AsyncMutex<Option<McpHandle>>>,
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

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
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
    state.metadata.upsert_connection(&config).await.map_err(err)?;
    Ok(config)
}

#[tauri::command]
async fn connection_delete(state: State<'_, AppState>, id: String) -> R<()> {
    let _ = SecretsStore::delete(&id);
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
        None => SecretsStore::get(&config.id).map_err(err)?,
    };
    drivers::driver_for(config.kind)
        .test_connection(&config, pw.as_deref())
        .await
        .map_err(err)
}

// ------------------------------------------------------------------- schema

#[tauri::command]
async fn schemas_list(config: ConnConfig) -> R<Vec<Schema>> {
    let pw = SecretsStore::get(&config.id).map_err(err)?;
    drivers::driver_for(config.kind)
        .list_schemas(&config, pw.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
async fn columns_list(config: ConnConfig, schema: String, relation: String) -> R<Vec<Column>> {
    let pw = SecretsStore::get(&config.id).map_err(err)?;
    drivers::driver_for(config.kind)
        .list_columns(&config, pw.as_deref(), &schema, &relation)
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
    let pw = SecretsStore::get(&config.id).map_err(err)?;
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
        };
        let _ = state.metadata.add_history(&entry).await;
    }

    result.map_err(err)
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

    let result = http_requests::send(&request).await;
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
    let handle = mcp::start(state.metadata.clone(), cfg).await.map_err(err)?;
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

// ---------------------------------------------------------------------- main

#[tokio::main]
async fn main() {
    let app_data_dir = dirs_next::data_dir()
        .expect("no app data dir")
        .join("OGTestDesk");
    std::fs::create_dir_all(&app_data_dir).expect("create app data dir");
    let db_path = std::env::var("OGTESTDESK_DB_PATH")
        .unwrap_or_else(|_| app_data_dir.join("og_testdesk.db").to_string_lossy().into());

    let metadata = MetadataStore::open(&db_path)
        .await
        .expect("open metadata store");
    let metadata = Arc::new(metadata);

    // Auto-start the MCP server if it was left enabled.
    let mcp_slot: Arc<AsyncMutex<Option<McpHandle>>> = Arc::new(AsyncMutex::new(None));
    {
        let cfg = load_mcp_config(&metadata).await;
        if cfg.enabled {
            match mcp::start(metadata.clone(), cfg).await {
                Ok(h) => *mcp_slot.lock().await = Some(h),
                Err(e) => eprintln!("[mcp] failed to auto-start: {e}"),
            }
        }
    }

    if let Ok(Some(s)) = metadata.get_state("query_max_rows").await {
        if let Ok(n) = s.parse::<usize>() {
            og_testdesk_core::set_max_rows(n);
        }
    }

    scheduler::spawn(metadata.clone());

    let managed = AppState {
        metadata: metadata.clone(),
        mcp: mcp_slot,
    };

    tauri::Builder::default()
        .setup(move |app| {
            app.manage(managed);
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
            connections_list,
            connection_save,
            connection_delete,
            connections_reorder,
            connection_test,
            schemas_list,
            columns_list,
            query_run,
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
