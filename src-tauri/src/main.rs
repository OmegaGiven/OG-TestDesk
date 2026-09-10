#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use og_testdesk_core::{
    drivers, requests as http_requests, ConnConfig, HttpRequest, HttpResponse, MetadataStore,
    QueryResult, SecretsStore,
};
use std::sync::Arc;
use tauri::State;

struct AppState {
    metadata: Arc<MetadataStore>,
}

#[tauri::command]
async fn list_connections(state: State<'_, AppState>) -> Result<Vec<ConnConfig>, String> {
    // TODO: SELECT * FROM connections via state.metadata.pool()
    let _ = state;
    Ok(vec![])
}

#[tauri::command]
async fn save_connection(
    state: State<'_, AppState>,
    config: ConnConfig,
    password: Option<String>,
) -> Result<(), String> {
    if let Some(pw) = password {
        SecretsStore::set(&config.id, &pw).map_err(|e| e.to_string())?;
    }
    // TODO: upsert into connections table via state.metadata.pool()
    let _ = state;
    Ok(())
}

#[tauri::command]
async fn run_query(config: ConnConfig, sql: String) -> Result<QueryResult, String> {
    let password = SecretsStore::get(&config.id).map_err(|e| e.to_string())?;
    let driver = drivers::driver_for(config.kind);
    driver
        .run_query(&config, password.as_deref(), &sql)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn send_request(req: HttpRequest) -> Result<HttpResponse, String> {
    http_requests::send(&req).await.map_err(|e| e.to_string())
}

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

    tauri::Builder::default()
        .manage(AppState { metadata: Arc::new(metadata) })
        .invoke_handler(tauri::generate_handler![
            list_connections,
            save_connection,
            run_query,
            send_request
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
