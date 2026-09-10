#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::sync::Arc;

use og_testdesk_core::{
    apply_environment, drivers, requests as http_requests, Column, ConnConfig, Environment,
    HistoryEntry, HttpRequest, HttpResponse, MetadataStore, QueryResult, QueryTab,
    RequestCollection, SavedQuery, SavedRequest, Schema, SecretsStore, ServerInfo,
};
use tauri::{Manager, State};

struct AppState {
    metadata: Arc<MetadataStore>,
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
) -> R<QueryResult> {
    let pw = SecretsStore::get(&config.id).map_err(err)?;
    let result = drivers::driver_for(config.kind)
        .run_query(&config, pw.as_deref(), &sql)
        .await;

    let entry = HistoryEntry {
        id: new_id(),
        connection_id: Some(config.id.clone()),
        sql_text: sql.clone(),
        duration_ms: result.as_ref().ok().map(|r| r.duration_ms as i64),
        row_count: result.as_ref().ok().map(|r| r.row_count as i64),
        success: result.is_ok(),
        ran_at: now(),
    };
    let _ = state.metadata.add_history(&entry).await;

    result.map_err(err)
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
    http_requests::send(&request).await.map_err(err)
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

    tauri::Builder::default()
        .setup(move |app| {
            app.manage(AppState {
                metadata: metadata.clone(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            saved_queries_list,
            saved_query_save,
            saved_query_delete,
            collections_list,
            collection_save,
            collection_delete,
            saved_requests_list,
            saved_request_save,
            saved_request_delete,
            environments_list,
            environment_save,
            environment_delete,
            request_send,
            state_get,
            state_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
