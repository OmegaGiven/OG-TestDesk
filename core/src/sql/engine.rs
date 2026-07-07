use crate::app_db;
use crate::sql::crypto::{encrypt_and_save, load_and_decrypt};
use crate::sql::helpers::find_connection;
use crate::sql::models::*;
use futures_util::TryStreamExt;
use serde_json::Value;
use sqlx::{
    Column, Row, TypeInfo, ValueRef,
    postgres::{PgConnectOptions, PgPool, PgPoolOptions},
    sqlite::{SqlitePool, SqlitePoolOptions},
    types::JsonValue,
};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration as StdDuration, SystemTime, UNIX_EPOCH},
};

const APP_DB_CONNECTION_NICKNAME: &str = "app_db";
const SQLITE_POOL_MAX_CONNECTIONS: u32 = 4;
const POSTGRES_POOL_MAX_CONNECTIONS: u32 = 12;
const SQL_POOL_ACQUIRE_TIMEOUT_SECS: u64 = 90;

fn sql_pool_acquire_timeout() -> StdDuration {
    StdDuration::from_secs(SQL_POOL_ACQUIRE_TIMEOUT_SECS)
}

/// Owns SQL connection pools, cached connection list, background job state, and
/// last-run results for CSV export. One instance is shared by the desktop app.
pub struct SqlEngineState {
    pub connections: Mutex<Option<Vec<DbConnection>>>,
    pub last_results: Mutex<HashMap<String, Vec<HashMap<String, String>>>>,
    pub jobs: Mutex<HashMap<String, SqlJob>>,
    pub sqlite_pools: Mutex<HashMap<String, SqlitePool>>,
    pub pg_pools: Mutex<HashMap<String, PgPool>>,
}

impl Default for SqlEngineState {
    fn default() -> Self {
        Self {
            connections: Mutex::new(None),
            last_results: Mutex::new(HashMap::new()),
            jobs: Mutex::new(HashMap::new()),
            sqlite_pools: Mutex::new(HashMap::new()),
            pg_pools: Mutex::new(HashMap::new()),
        }
    }
}

impl SqlEngineState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Connections list, including the built-in `app_db` entry, loading and
    /// decrypting from disk on first access.
    pub fn connections(&self) -> Vec<DbConnection> {
        let mut conns_opt = self.connections.lock().unwrap();
        if conns_opt.is_none() {
            *conns_opt = Some(load_and_decrypt());
        }
        include_app_db_connection(conns_opt.as_mut().unwrap());
        conns_opt.clone().unwrap()
    }
}

// --- Folder path helpers (shared by the query-folder tree) ---

pub fn normalize_folder_path(folder: &str) -> String {
    folder
        .replace(" / ", "/")
        .split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn folder_basename(folder: &str) -> String {
    normalize_folder_path(folder)
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

pub fn add_folder_path(folders: &mut Vec<String>, folder: &str) {
    let folder = normalize_folder_path(folder);
    if folder.is_empty() {
        return;
    }
    let parts = folder.split('/').collect::<Vec<_>>();
    for index in 1..=parts.len() {
        let path = parts[..index].join("/");
        if !folders
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&path))
        {
            folders.push(path);
        }
    }
}

fn is_same_or_child_folder(folder: &str, parent: &str) -> bool {
    folder == parent
        || folder
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn move_folder_path(value: &str, old_folder: &str, new_folder: &str) -> String {
    if value == old_folder {
        return new_folder.to_string();
    }
    if let Some(rest) = value.strip_prefix(old_folder) {
        if rest.starts_with('/') {
            return format!("{new_folder}{rest}");
        }
    }
    value.to_string()
}

fn query_matches_connection(query: &SavedQuery, connection: &str) -> bool {
    query.connection.as_deref() == Some(connection)
}

fn query_identity_matches(query: &SavedQuery, name: &str, connection: &str) -> bool {
    query.name == name && query_matches_connection(query, connection)
}

fn folder_matches_connection(folder: &str, connection: &str) -> bool {
    folder
        .split_once("::")
        .map(|(prefix, _)| prefix == connection)
        .unwrap_or(false)
}

fn stored_folder_name(folder: &str, connection: &str) -> String {
    format!("{connection}::{folder}")
}

fn display_folder_name(folder: &str) -> &str {
    folder
        .split_once("::")
        .map(|(_, name)| name)
        .unwrap_or(folder)
}

fn stored_folder_exists(folders: &[String], display_name: &str, connection: &str) -> bool {
    let display_name = normalize_folder_path(display_name);
    let stored = stored_folder_name(&display_name, connection);
    folders.iter().any(|folder| {
        folder.eq_ignore_ascii_case(&stored)
            || (folder_matches_connection(folder, connection)
                && display_folder_name(folder).eq_ignore_ascii_case(&display_name))
    })
}

fn unique_query_name(existing_queries: &[SavedQuery], base_name: &str, connection: &str) -> String {
    if !existing_queries
        .iter()
        .any(|query| query.name == base_name && query.connection.as_deref() == Some(connection))
    {
        return base_name.to_string();
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{base_name} ({suffix})");
        if !existing_queries
            .iter()
            .any(|query| query.name == candidate && query.connection.as_deref() == Some(connection))
        {
            return candidate;
        }
        suffix += 1;
    }
}

// --- Saved query / folder persistence (backed by app_db, replacing the
// original flat JSON files) ---

async fn load_queries() -> Vec<SavedQuery> {
    app_db::get_json("sql", "queries").await.unwrap_or_default()
}

async fn save_queries(queries: &[SavedQuery]) -> Result<(), sqlx::Error> {
    app_db::put_json("sql", "queries", &queries).await
}

async fn load_query_folders() -> Vec<String> {
    app_db::get_json("sql", "query_folders")
        .await
        .unwrap_or_default()
}

async fn save_query_folders(folders: &[String]) -> Result<(), sqlx::Error> {
    app_db::put_json("sql", "query_folders", &folders).await
}

pub async fn list_saved_queries(connection: &str) -> Vec<SavedQuery> {
    load_queries()
        .await
        .into_iter()
        .filter(|query| query_matches_connection(query, connection))
        .collect()
}

pub async fn list_saved_query_folders(connection: &str) -> Vec<String> {
    load_query_folders()
        .await
        .into_iter()
        .filter(|folder| folder_matches_connection(folder, connection))
        .map(|folder| display_folder_name(&folder).to_string())
        .collect()
}

pub async fn save_query(
    connection: &str,
    query_name: &str,
    sql: &str,
    folder: Option<&str>,
) -> Result<(), sqlx::Error> {
    let mut queries = load_queries().await;
    let folder = folder
        .map(normalize_folder_path)
        .filter(|value| !value.is_empty());

    if let Some(idx) = queries
        .iter()
        .position(|q| q.name == query_name && q.connection.as_deref() == Some(connection))
    {
        queries[idx].sql = sql.to_string();
        queries[idx].folder = folder;
        queries[idx].connection = Some(connection.to_string());
    } else {
        queries.push(SavedQuery {
            name: query_name.to_string(),
            sql: sql.to_string(),
            folder,
            connection: Some(connection.to_string()),
        });
    }
    save_queries(&queries).await
}

pub async fn delete_query(connection: &str, query_name: &str) -> Result<(), sqlx::Error> {
    let mut queries = load_queries().await;
    if let Some(pos) = queries
        .iter()
        .position(|query| query_identity_matches(query, query_name, connection))
    {
        queries.remove(pos);
        save_queries(&queries).await?;
    }
    Ok(())
}

pub async fn rename_query(
    connection: &str,
    query_name: &str,
    new_name: &str,
) -> Result<(), sqlx::Error> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Ok(());
    }
    let mut queries = load_queries().await;
    let duplicate_exists = queries
        .iter()
        .any(|query| query.name == new_name && query.connection.as_deref() == Some(connection));
    if duplicate_exists {
        return Ok(());
    }
    if let Some(query) = queries
        .iter_mut()
        .find(|query| query_identity_matches(query, query_name, connection))
    {
        query.name = new_name.to_string();
        query.connection = Some(connection.to_string());
        save_queries(&queries).await?;
    }
    Ok(())
}

pub async fn create_query_folder(connection: &str, folder_name: &str) -> Result<(), sqlx::Error> {
    let folder_name = normalize_folder_path(folder_name);
    if folder_name.is_empty() {
        return Ok(());
    }
    let mut folders = load_query_folders().await;
    if !stored_folder_exists(&folders, &folder_name, connection) {
        folders.push(stored_folder_name(&folder_name, connection));
        folders.sort_by_key(|folder| folder.to_lowercase());
        save_query_folders(&folders).await?;
    }
    Ok(())
}

pub async fn delete_query_folder(connection: &str, folder_name: &str) -> Result<(), sqlx::Error> {
    let folder_name = normalize_folder_path(folder_name);
    if folder_name.is_empty() {
        return Ok(());
    }
    let mut folders = load_query_folders().await;
    folders.retain(|folder| {
        !folder_matches_connection(folder, connection)
            || !is_same_or_child_folder(
                &normalize_folder_path(display_folder_name(folder)),
                &folder_name,
            )
    });
    save_query_folders(&folders).await?;

    let mut queries = load_queries().await;
    queries.retain(|query| {
        if !query_matches_connection(query, connection) {
            return true;
        }
        let folder = query
            .folder
            .as_deref()
            .map(normalize_folder_path)
            .unwrap_or_default();
        !is_same_or_child_folder(&folder, &folder_name)
    });
    save_queries(&queries).await
}

pub async fn move_query(
    connection: &str,
    query_name: &str,
    new_folder: Option<&str>,
) -> Result<(), sqlx::Error> {
    let new_folder = new_folder
        .map(normalize_folder_path)
        .filter(|folder| !folder.is_empty());

    let mut queries = load_queries().await;
    if let Some(query) = queries
        .iter_mut()
        .find(|query| query_identity_matches(query, query_name, connection))
    {
        query.folder = new_folder.clone();
        query.connection = Some(connection.to_string());
        save_queries(&queries).await?;
    }

    if let Some(folder) = new_folder {
        let mut folders = load_query_folders().await;
        if !stored_folder_exists(&folders, &folder, connection) {
            folders.push(stored_folder_name(&folder, connection));
            folders.sort_by_key(|folder| folder.to_lowercase());
            save_query_folders(&folders).await?;
        }
    }
    Ok(())
}

pub async fn move_query_folder(
    connection: &str,
    folder_name: &str,
    new_parent: Option<&str>,
) -> Result<(), String> {
    let old_folder = normalize_folder_path(folder_name);
    let new_parent = new_parent.map(normalize_folder_path).unwrap_or_default();

    if old_folder.is_empty() || is_same_or_child_folder(&new_parent, &old_folder) {
        return Err("Invalid folder move".to_string());
    }
    let basename = folder_basename(&old_folder);
    if basename.is_empty() {
        return Err("Invalid folder name".to_string());
    }
    let new_folder = if new_parent.is_empty() {
        basename
    } else {
        format!("{new_parent}/{basename}")
    };
    if old_folder == new_folder {
        return Ok(());
    }

    let mut folders = load_query_folders().await;
    let mut changed = false;
    for folder in folders.iter_mut() {
        if !folder_matches_connection(folder, connection) {
            continue;
        }
        let display = normalize_folder_path(display_folder_name(folder));
        if is_same_or_child_folder(&display, &old_folder) {
            *folder = stored_folder_name(
                &move_folder_path(&display, &old_folder, &new_folder),
                connection,
            );
            changed = true;
        }
    }
    if !folders.iter().any(|folder| {
        folder_matches_connection(folder, connection)
            && display_folder_name(folder).eq_ignore_ascii_case(&new_folder)
    }) {
        folders.push(stored_folder_name(&new_folder, connection));
        changed = true;
    }
    if changed {
        folders.sort_by_key(|folder| folder.to_lowercase());
        save_query_folders(&folders)
            .await
            .map_err(|err| err.to_string())?;
    }

    let mut queries = load_queries().await;
    let mut queries_changed = false;
    for query in queries
        .iter_mut()
        .filter(|query| query_matches_connection(query, connection))
    {
        let Some(folder) = query.folder.as_ref() else {
            continue;
        };
        let folder = normalize_folder_path(folder);
        if is_same_or_child_folder(&folder, &old_folder) {
            query.folder = Some(move_folder_path(&folder, &old_folder, &new_folder));
            queries_changed = true;
        }
    }
    if queries_changed {
        save_queries(&queries).await.map_err(|err| err.to_string())?;
    }
    Ok(())
}

pub async fn export_queries(connection: &str) -> SavedQueryExport {
    let queries = load_queries()
        .await
        .into_iter()
        .filter(|query| query_matches_connection(query, connection))
        .map(|mut query| {
            query.connection = None;
            query
        })
        .collect::<Vec<_>>();
    let folders = load_query_folders()
        .await
        .into_iter()
        .filter(|folder| folder_matches_connection(folder, connection))
        .map(|folder| display_folder_name(&folder).to_string())
        .chain(
            queries
                .iter()
                .filter_map(|query| query.folder.as_ref())
                .map(|folder| folder.trim().to_string())
                .filter(|folder| !folder.is_empty()),
        )
        .fold(Vec::<String>::new(), |mut folders, folder| {
            if !folders
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&folder))
            {
                folders.push(folder);
            }
            folders
        });
    SavedQueryExport {
        version: 1,
        source_connection: connection.to_string(),
        queries,
        folders,
    }
}

pub async fn import_queries(
    connection: &str,
    payload: &str,
    duplicate_mode: &str,
) -> Result<(), String> {
    let parsed = serde_json::from_str::<SavedQueryExport>(payload).or_else(|_| {
        serde_json::from_str::<Vec<SavedQuery>>(payload).map(|queries| SavedQueryExport {
            version: 1,
            source_connection: String::new(),
            queries,
            folders: Vec::new(),
        })
    });

    let export = parsed.map_err(|err| err.to_string())?;
    let mut existing_queries = load_queries().await;
    for mut query in export.queries {
        let base_name = query.name.trim();
        if base_name.is_empty() {
            continue;
        }
        query.name = base_name.to_string();
        query.connection = Some(connection.to_string());
        if let Some(folder) = query
            .folder
            .as_ref()
            .map(|folder| folder.trim())
            .filter(|folder| !folder.is_empty())
        {
            query.folder = Some(folder.to_string());
        }

        if let Some(idx) = existing_queries
            .iter()
            .position(|existing| existing.name == query.name && existing.connection.as_deref() == Some(connection))
        {
            if duplicate_mode == "overwrite" {
                existing_queries[idx] = query;
            } else {
                query.name = unique_query_name(&existing_queries, &query.name, connection);
                existing_queries.push(query);
            }
        } else {
            existing_queries.push(query);
        }
    }

    let mut folders = load_query_folders().await;
    for folder in export.folders {
        let folder = folder.trim();
        if folder.is_empty() {
            continue;
        }
        let stored = stored_folder_name(folder, connection);
        if !folders
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&stored))
        {
            folders.push(stored);
        }
    }
    for query in &existing_queries {
        if query.connection.as_deref() != Some(connection) {
            continue;
        }
        if let Some(folder) = query
            .folder
            .as_ref()
            .map(|folder| folder.trim())
            .filter(|folder| !folder.is_empty())
        {
            if !stored_folder_exists(&folders, folder, connection) {
                folders.push(stored_folder_name(folder, connection));
            }
        }
    }
    folders.sort_by_key(|folder| folder.to_lowercase());
    save_queries(&existing_queries)
        .await
        .map_err(|err| err.to_string())?;
    save_query_folders(&folders).await.map_err(|err| err.to_string())
}

// --- Connection management ---

fn app_db_connection() -> DbConnection {
    DbConnection {
        db_type: "sqlite".to_string(),
        host: app_db::db_path(),
        db_name: String::new(),
        user: String::new(),
        password: String::new(),
        nickname: APP_DB_CONNECTION_NICKNAME.to_string(),
    }
}

fn include_app_db_connection(conns: &mut Vec<DbConnection>) {
    conns.retain(|conn| conn.nickname != APP_DB_CONNECTION_NICKNAME);
    conns.insert(0, app_db_connection());
}

fn user_saved_connections(conns: &[DbConnection]) -> Vec<DbConnection> {
    conns
        .iter()
        .filter(|conn| conn.nickname != APP_DB_CONNECTION_NICKNAME)
        .cloned()
        .collect()
}

fn save_user_connections(conns: &[DbConnection]) {
    let user_conns = user_saved_connections(conns);
    if let Err(e) = encrypt_and_save(&user_conns) {
        eprintln!("Failed to save encrypted connections: {e}");
    }
}

fn parse_pg_host_port(host: &str) -> (String, Option<u16>) {
    let trimmed = host.trim();
    if trimmed.starts_with('[') {
        if let Some((host_part, port_part)) = trimmed.rsplit_once("]:") {
            if let Ok(port) = port_part.parse::<u16>() {
                return (host_part.trim_start_matches('[').to_string(), Some(port));
            }
        }
        return (
            trimmed
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string(),
            None,
        );
    }

    if let Some((host_part, port_part)) = trimmed.rsplit_once(':') {
        if !host_part.contains(':') {
            if let Ok(port) = port_part.parse::<u16>() {
                return (host_part.to_string(), Some(port));
            }
        }
    }

    (trimmed.to_string(), None)
}

fn pg_pool_key(conn: &DbConnection) -> String {
    format!(
        "postgres|{}|{}|{}|{}",
        conn.host, conn.db_name, conn.user, conn.password
    )
}

fn pg_connect_options(conn: &DbConnection) -> PgConnectOptions {
    let (host, port) = parse_pg_host_port(&conn.host);
    let mut options = PgConnectOptions::new()
        .host(&host)
        .username(&conn.user)
        .password(&conn.password)
        .database(&conn.db_name);
    if let Some(port) = port {
        options = options.port(port);
    }
    options
}

pub fn list_connections(state: &SqlEngineState) -> Vec<DbConnection> {
    state.connections()
}

pub fn add_connection(state: &SqlEngineState, form: AddConnForm) {
    let new_conn = DbConnection {
        db_type: form.db_type.unwrap_or_else(|| "postgres".to_string()),
        host: form.host,
        db_name: form.db_name,
        user: form.user,
        password: form.password,
        nickname: form.nickname,
    };
    let mut conns_opt = state.connections.lock().unwrap();
    if conns_opt.is_none() {
        *conns_opt = Some(load_and_decrypt());
    }
    let conns = conns_opt.as_mut().unwrap();
    if let Some(idx) = conns.iter().position(|c| c.nickname == new_conn.nickname) {
        conns[idx] = new_conn;
    } else {
        conns.push(new_conn);
    }
    save_user_connections(conns);
}

pub fn update_connection(state: &SqlEngineState, form: EditConnectionForm) -> Result<(), String> {
    let mut conns_opt = state.connections.lock().unwrap();
    if conns_opt.is_none() {
        *conns_opt = Some(load_and_decrypt());
    }
    let conns = conns_opt.as_mut().unwrap();
    let Some(idx) = conns
        .iter()
        .position(|c| c.nickname == form.original_nickname)
    else {
        return Err("Connection not found".to_string());
    };

    let existing_password = conns[idx].password.clone();
    let next_password = if form.password.is_empty() {
        existing_password
    } else {
        form.password
    };

    conns[idx] = DbConnection {
        db_type: form.db_type.unwrap_or_else(|| "postgres".to_string()),
        host: form.host,
        db_name: form.db_name,
        user: form.user,
        password: next_password,
        nickname: form.nickname,
    };
    save_user_connections(conns);
    Ok(())
}

pub fn delete_connection(state: &SqlEngineState, nickname: &str) {
    let mut conns_opt = state.connections.lock().unwrap();
    if conns_opt.is_none() {
        *conns_opt = Some(load_and_decrypt());
    }
    let conns = conns_opt.as_mut().unwrap();
    if let Some(idx) = conns.iter().position(|c| c.nickname == nickname) {
        conns.remove(idx);
        save_user_connections(conns);
    }
}

pub async fn disconnect_connection(state: &SqlEngineState, nickname: &str) {
    let conn_opt = {
        let mut conns_opt = state.connections.lock().unwrap();
        if conns_opt.is_none() {
            *conns_opt = Some(load_and_decrypt());
        }
        include_app_db_connection(conns_opt.as_mut().unwrap());
        conns_opt
            .as_ref()
            .and_then(|conns| conns.iter().find(|conn| conn.nickname == nickname).cloned())
    };

    let Some(conn) = conn_opt else { return };
    if conn.db_type == "sqlite" {
        let dsn = format!("sqlite:{}?mode=rwc", conn.host);
        let pool = {
            let mut pools = state.sqlite_pools.lock().unwrap();
            pools.remove(&dsn)
        };
        if let Some(pool) = pool {
            pool.close().await;
        }
    } else {
        let pool_key = pg_pool_key(&conn);
        let pool = {
            let mut pools = state.pg_pools.lock().unwrap();
            pools.remove(&pool_key)
        };
        if let Some(pool) = pool {
            pool.close().await;
        }
    }
}

// --- Timestamp / alert helpers ---

fn format_ts(seconds: i64) -> String {
    const SECONDS_IN_MINUTE: i64 = 60;
    const SECONDS_IN_HOUR: i64 = 3600;
    const SECONDS_IN_DAY: i64 = 86400;
    const DAYS_IN_400_YEARS: i64 = 146097;
    const DAYS_IN_100_YEARS: i64 = 36524;

    let days_since_epoch = seconds / SECONDS_IN_DAY;
    let mut second_of_day = seconds % SECONDS_IN_DAY;
    if second_of_day < 0 {
        second_of_day += SECONDS_IN_DAY;
    }

    let h = second_of_day / SECONDS_IN_HOUR;
    let m = (second_of_day % SECONDS_IN_HOUR) / SECONDS_IN_MINUTE;
    let s = second_of_day % SECONDS_IN_MINUTE;

    let days = days_since_epoch + 719468;
    let era = if days >= 0 { days } else { days - 146096 } / DAYS_IN_400_YEARS;
    let doe = days - era * DAYS_IN_400_YEARS;
    let yoe = (doe - doe / DAYS_IN_100_YEARS + doe / DAYS_IN_400_YEARS - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if mp < 10 { y } else { y + 1 };

    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", yr, mo, d, h, m, s)
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn now_isoish() -> String {
    format_ts((now_millis() / 1000) as i64)
}

fn row_count_text(rows: usize) -> String {
    if rows == 0 {
        "0 rows".to_string()
    } else {
        format!("{rows} rows")
    }
}

fn row_count_from_text(row_count_text: &str) -> i64 {
    row_count_text
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0)
}

fn normalize_alert_comparator(comparator: &str) -> &str {
    match comparator.trim().to_lowercase().as_str() {
        "=" | "==" | "eq" => "=",
        "!=" | "<>" | "not_eq" | "neq" => "!=",
        "<" | "lt" => "<",
        "<=" | "lte" | "le" => "<=",
        ">" | "gt" => ">",
        ">=" | "gte" | "ge" => ">=",
        _ => ">",
    }
}

pub fn alert_trigger_message(
    rule: Option<&SqlAlertRule>,
    row_count_text: &str,
    task_name: &str,
) -> Option<String> {
    let rule = rule?;
    let row_count = row_count_from_text(row_count_text);
    let comparator = normalize_alert_comparator(&rule.comparator);
    let matches = match comparator {
        "=" => row_count == rule.value,
        "!=" => row_count != rule.value,
        "<" => row_count < rule.value,
        "<=" => row_count <= rule.value,
        ">" => row_count > rule.value,
        ">=" => row_count >= rule.value,
        _ => false,
    };

    if matches {
        let subject = if task_name.trim().is_empty() {
            "SQL cron".to_string()
        } else {
            task_name.trim().to_string()
        };
        Some(format!(
            "{subject}: returned {row_count} rows, matching {comparator} {}",
            rule.value
        ))
    } else {
        None
    }
}

// --- SQL identifier/statement builders ---

fn quote_sql_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn quote_sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn table_reference(conn: &DbConnection, table: &str) -> String {
    if conn.db_type == "postgres" {
        format!("public.{}", quote_sql_identifier(table))
    } else {
        quote_sql_identifier(table)
    }
}

fn filter_sql_for_column(column_expr: &str, op: &str, value: &str) -> Option<String> {
    let value_literal = quote_sql_literal(value);
    match op {
        "is_null" => Some(format!("{column_expr} IS NULL")),
        "not_null" => Some(format!("{column_expr} IS NOT NULL")),
        "eq" => Some(format!("{column_expr} = {value_literal}")),
        "not_eq" => Some(format!("{column_expr} <> {value_literal}")),
        "contains" => Some(format!(
            "CAST({column_expr} AS TEXT) LIKE {}",
            quote_sql_literal(&format!("%{value}%"))
        )),
        "begins_with" => Some(format!(
            "CAST({column_expr} AS TEXT) LIKE {}",
            quote_sql_literal(&format!("{value}%"))
        )),
        "ends_with" => Some(format!(
            "CAST({column_expr} AS TEXT) LIKE {}",
            quote_sql_literal(&format!("%{value}"))
        )),
        "like" => Some(format!("CAST({column_expr} AS TEXT) LIKE {value_literal}")),
        _ => None,
    }
}

pub fn build_table_filter_sql(
    filters: &[TableBrowseFilter],
    columns: &[String],
) -> Result<String, String> {
    let mut clauses = Vec::new();
    for filter in filters {
        let op = filter.op.as_str();
        let value = filter.value.as_deref().unwrap_or("");
        let is_null_op = op == "is_null" || op == "not_null";
        if !is_null_op && value.is_empty() {
            continue;
        }

        let column = filter.column.as_deref().unwrap_or("");
        if column.is_empty() {
            let any_clauses = columns
                .iter()
                .filter_map(|col| filter_sql_for_column(&quote_sql_identifier(col), op, value))
                .collect::<Vec<_>>();
            if !any_clauses.is_empty() {
                clauses.push(format!("({})", any_clauses.join(" OR ")));
            }
            continue;
        }

        if !columns.iter().any(|col| col == column) {
            return Err(format!("Unknown column: {column}"));
        }
        if let Some(clause) = filter_sql_for_column(&quote_sql_identifier(column), op, value) {
            clauses.push(clause);
        }
    }

    if clauses.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!(" WHERE {}", clauses.join(" AND ")))
    }
}

pub fn build_table_update_sql(
    conn: &DbConnection,
    table: &str,
    columns: &[String],
    changes: &[TableUpdateChange],
) -> Result<Vec<String>, String> {
    let table_sql = table_reference(conn, table);
    let mut statements = Vec::new();

    for change in changes {
        let mut set_clauses = Vec::new();
        let mut where_clauses = Vec::new();

        for column in columns {
            let original = change.original.get(column).cloned().unwrap_or_default();
            let current = change.current.get(column).cloned().unwrap_or_default();
            let column_sql = quote_sql_identifier(column);

            where_clauses.push(format!("{column_sql} = {}", quote_sql_literal(&original)));

            if original != current {
                set_clauses.push(format!("{column_sql} = {}", quote_sql_literal(&current)));
            }
        }

        if set_clauses.is_empty() {
            continue;
        }
        if where_clauses.is_empty() {
            return Err("Cannot update table without row identity columns".to_string());
        }

        statements.push(format!(
            "UPDATE {table_sql}\nSET {}\nWHERE {};",
            set_clauses.join(",\n    "),
            where_clauses.join("\n  AND ")
        ));
    }

    Ok(statements)
}

pub fn build_table_insert_sql(
    conn: &DbConnection,
    table: &str,
    columns: &[String],
    rows: &[HashMap<String, String>],
) -> Result<Vec<String>, String> {
    let table_sql = table_reference(conn, table);
    let mut statements = Vec::new();

    for row in rows {
        for key in row.keys() {
            if !columns.iter().any(|column| column == key) {
                return Err(format!("Unknown column: {key}"));
            }
        }

        let filled_columns = columns
            .iter()
            .filter(|column| {
                row.get(*column)
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();

        if filled_columns.is_empty() {
            return Err("Add entry needs at least one filled value.".to_string());
        }

        let column_sql = filled_columns
            .iter()
            .map(|column| quote_sql_identifier(column))
            .collect::<Vec<_>>()
            .join(", ");
        let value_sql = filled_columns
            .iter()
            .map(|column| {
                quote_sql_literal(row.get(*column).map(|value| value.trim()).unwrap_or(""))
            })
            .collect::<Vec<_>>()
            .join(", ");

        statements.push(format!(
            "INSERT INTO {table_sql} ({column_sql})\nVALUES ({value_sql});"
        ));
    }

    Ok(statements)
}

fn finalize_sql_variables(form: &SqlForm) -> String {
    let mut final_sql = form.sql.clone();
    if let Some(vars) = &form.variables {
        for (key, val) in vars {
            final_sql = final_sql.replace(&format!("{{{{{}}}}}", key), val);
        }
    }
    final_sql
}

struct SqlRunContext {
    conn: DbConnection,
    final_sql: String,
}

fn resolve_sql_run_context(form: &SqlForm, state: &SqlEngineState) -> Result<SqlRunContext, String> {
    let conns = state.connections();
    let Some(conn) = find_connection(&form.connection, &conns).cloned() else {
        return Err(format!("Connection '{}' not found.", form.connection));
    };
    Ok(SqlRunContext {
        conn,
        final_sql: finalize_sql_variables(form),
    })
}

fn sqlite_row_to_values(row: &sqlx::sqlite::SqliteRow) -> (Vec<String>, HashMap<String, String>) {
    let mut ordered_row_data = Vec::new();
    let mut map_for_export = HashMap::new();
    for (idx, col) in row.columns().iter().enumerate() {
        let name = col.name().to_string();
        let val_str = if let Ok(s) = row.try_get::<String, _>(idx) {
            s
        } else if let Ok(i) = row.try_get::<i64, _>(idx) {
            i.to_string()
        } else if let Ok(f) = row.try_get::<f64, _>(idx) {
            f.to_string()
        } else if let Ok(b) = row.try_get::<Vec<u8>, _>(idx) {
            format!("<blob len={}>", b.len())
        } else if row.try_get_raw(idx).map(|r| r.is_null()).unwrap_or(true) {
            String::new()
        } else {
            "?".to_string()
        };
        ordered_row_data.push(val_str.clone());
        map_for_export.insert(name, val_str);
    }
    (ordered_row_data, map_for_export)
}

fn postgres_row_to_values(row: &sqlx::postgres::PgRow) -> (Vec<String>, HashMap<String, String>) {
    use std::convert::TryInto;

    let mut ordered_row_data = Vec::new();
    let mut map_for_export = HashMap::new();

    for (idx, col) in row.columns().iter().enumerate() {
        let name = col.name().to_string();
        let type_name = col.type_info().name();
        let display_val = if let Ok(s) = row.try_get::<String, usize>(idx) {
            s
        } else if let Ok(i) = row.try_get::<i32, usize>(idx) {
            i.to_string()
        } else if let Ok(i) = row.try_get::<i16, usize>(idx) {
            i.to_string()
        } else if let Ok(i) = row.try_get::<i64, usize>(idx) {
            i.to_string()
        } else if let Ok(f) = row.try_get::<f64, usize>(idx) {
            f.to_string()
        } else if let Ok(f) = row.try_get::<f32, usize>(idx) {
            f.to_string()
        } else if let Ok(b) = row.try_get::<bool, usize>(idx) {
            b.to_string()
        } else if let Ok(json) = row.try_get::<JsonValue, usize>(idx) {
            json.to_string().trim_matches('"').to_string()
        } else if let Ok(raw_val) = row.try_get_raw(idx) {
            if raw_val.is_null() {
                String::new()
            } else if let Ok(bytes) = raw_val.as_bytes() {
                match type_name {
                    "TIMESTAMPTZ" | "TIMESTAMP" if bytes.len() == 8 => {
                        let micros = i64::from_be_bytes(bytes.try_into().unwrap_or([0; 8]));
                        format_ts((micros / 1_000_000) + 946_684_800)
                    }
                    "DATE" if bytes.len() == 4 => {
                        let days = i32::from_be_bytes(bytes.try_into().unwrap_or([0; 4]));
                        format_ts((days as i64) * 86400 + 946_684_800)
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .to_string()
                    }
                    "UUID" if bytes.len() == 16 => {
                        let b = bytes;
                        format!(
                            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
                        )
                    }
                    "MONEY" if bytes.len() == 8 => {
                        let cents = i64::from_be_bytes(bytes.try_into().unwrap_or([0; 8]));
                        format!("${:.2}", cents as f64 / 100.0)
                    }
                    _ => std::str::from_utf8(bytes)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|_| format!("[Complex: {}]", type_name)),
                }
            } else {
                format!("[Complex: {}]", type_name)
            }
        } else {
            format!("[Complex: {}]", type_name)
        };
        ordered_row_data.push(display_val.clone());
        map_for_export.insert(name, display_val);
    }

    (ordered_row_data, map_for_export)
}

async fn get_or_create_sqlite_pool(
    state: &SqlEngineState,
    dsn: &str,
) -> Result<SqlitePool, String> {
    let existing = {
        let pools = state.sqlite_pools.lock().unwrap();
        pools.get(dsn).cloned()
    };
    if let Some(pool) = existing {
        return Ok(pool);
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(SQLITE_POOL_MAX_CONNECTIONS)
        .acquire_timeout(sql_pool_acquire_timeout())
        .connect(dsn)
        .await
        .map_err(|e| format!("SQLite Connect Error: {e}"))?;
    let mut pools = state.sqlite_pools.lock().unwrap();
    Ok(pools.entry(dsn.to_string()).or_insert(pool).clone())
}

async fn get_or_create_pg_pool(state: &SqlEngineState, conn: &DbConnection) -> Result<PgPool, String> {
    let pool_key = pg_pool_key(conn);
    let existing = {
        let pools = state.pg_pools.lock().unwrap();
        pools.get(&pool_key).cloned()
    };
    if let Some(pool) = existing {
        return Ok(pool);
    }
    let pool = PgPoolOptions::new()
        .max_connections(POSTGRES_POOL_MAX_CONNECTIONS)
        .acquire_timeout(sql_pool_acquire_timeout())
        .connect_with(pg_connect_options(conn))
        .await
        .map_err(|e| format!("Postgres Connect Error: {e}"))?;
    let mut pools = state.pg_pools.lock().unwrap();
    Ok(pools.entry(pool_key).or_insert(pool).clone())
}

/// Runs one SQL statement to completion and returns the full result set.
pub async fn execute_sql(form: SqlForm, state: &SqlEngineState) -> SqlExecution {
    let context = match resolve_sql_run_context(&form, state) {
        Ok(context) => context,
        Err(err) => return SqlExecution { error: Some(err), ..Default::default() },
    };
    let conn = context.conn;
    let final_sql = context.final_sql;
    let mut execution = SqlExecution::default();

    if conn.db_type == "sqlite" {
        let dsn = format!("sqlite:{}?mode=rwc", conn.host);
        let pool = match get_or_create_sqlite_pool(state, &dsn).await {
            Ok(pool) => pool,
            Err(err) => return SqlExecution { error: Some(err), ..Default::default() },
        };
        let rows = match sqlx::query(&final_sql).fetch_all(&pool).await {
            Ok(rows) => rows,
            Err(e) => {
                return SqlExecution {
                    error: Some(format!("Query Error: {e}")),
                    ..Default::default()
                };
            }
        };
        if let Some(first_row) = rows.first() {
            execution.headers = first_row
                .columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect();
        }
        for row in rows {
            let (ordered, mapped) = sqlite_row_to_values(&row);
            execution.rows.push(ordered);
            execution.results.push(mapped);
        }
    } else {
        let pool = match get_or_create_pg_pool(state, &conn).await {
            Ok(pool) => pool,
            Err(err) => return SqlExecution { error: Some(err), ..Default::default() },
        };
        let rows = match sqlx::query(&final_sql).fetch_all(&pool).await {
            Ok(rows) => rows,
            Err(e) => {
                return SqlExecution {
                    error: Some(format!("Query error: {e}")),
                    ..Default::default()
                };
            }
        };
        if let Some(first_row) = rows.first() {
            execution.headers = first_row
                .columns()
                .iter()
                .map(|col| col.name().to_string())
                .collect();
        }
        for row in rows {
            let (ordered, mapped) = postgres_row_to_values(&row);
            execution.rows.push(ordered);
            execution.results.push(mapped);
        }
    }

    execution
}

const STREAM_BATCH_ROWS: usize = 100;

fn append_sql_job_stream_batch(
    state: &SqlEngineState,
    job_id: &str,
    headers: &[String],
    new_rows: &[Vec<String>],
    new_results: &[HashMap<String, String>],
    total_rows: usize,
) {
    let mut jobs = state.jobs.lock().unwrap();
    if let Some(job) = jobs.get_mut(job_id) {
        if job.stream_headers.is_empty() && !headers.is_empty() {
            job.stream_headers = headers.to_vec();
        }
        job.stream_rows.extend(new_rows.iter().cloned());
        job.results.extend(new_results.iter().cloned());
        job.row_count_text = Some(format!("{total_rows} rows (streaming)"));
    }
}

/// Runs one SQL statement, streaming batches of rows into the job's state as
/// they arrive so a UI can show partial results for long-running queries.
pub async fn execute_sql_streaming_job(
    form: SqlForm,
    state: &SqlEngineState,
    job_id: &str,
) -> SqlExecution {
    let context = match resolve_sql_run_context(&form, state) {
        Ok(context) => context,
        Err(err) => return SqlExecution { error: Some(err), ..Default::default() },
    };
    let conn = context.conn;
    let final_sql = context.final_sql;
    let mut execution = SqlExecution::default();
    let mut rows_since_update = 0usize;
    let mut batch_rows: Vec<Vec<String>> = Vec::new();
    let mut batch_results: Vec<HashMap<String, String>> = Vec::new();

    macro_rules! stream_rows {
        ($rows:expr, $to_values:expr) => {
            loop {
                match $rows.try_next().await {
                    Ok(Some(row)) => {
                        if execution.headers.is_empty() {
                            execution.headers = row
                                .columns()
                                .iter()
                                .map(|c| c.name().to_string())
                                .collect();
                        }
                        let (ordered, mapped) = $to_values(&row);
                        execution.rows.push(ordered.clone());
                        execution.results.push(mapped.clone());
                        batch_rows.push(ordered);
                        batch_results.push(mapped);
                        rows_since_update += 1;
                        if rows_since_update >= STREAM_BATCH_ROWS {
                            append_sql_job_stream_batch(
                                state,
                                job_id,
                                &execution.headers,
                                &batch_rows,
                                &batch_results,
                                execution.rows.len(),
                            );
                            batch_rows.clear();
                            batch_results.clear();
                            rows_since_update = 0;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        return SqlExecution {
                            error: Some(format!("Query Error: {e}")),
                            ..Default::default()
                        };
                    }
                }
            }
        };
    }

    if conn.db_type == "sqlite" {
        let dsn = format!("sqlite:{}?mode=rwc", conn.host);
        let pool = match get_or_create_sqlite_pool(state, &dsn).await {
            Ok(pool) => pool,
            Err(err) => return SqlExecution { error: Some(err), ..Default::default() },
        };
        let mut rows = sqlx::query(&final_sql).fetch(&pool);
        stream_rows!(rows, sqlite_row_to_values);
    } else {
        let pool = match get_or_create_pg_pool(state, &conn).await {
            Ok(pool) => pool,
            Err(err) => return SqlExecution { error: Some(err), ..Default::default() },
        };
        let mut rows = sqlx::query(&final_sql).fetch(&pool);
        stream_rows!(rows, postgres_row_to_values);
    }

    if !batch_rows.is_empty() || !batch_results.is_empty() {
        append_sql_job_stream_batch(
            state,
            job_id,
            &execution.headers,
            &batch_rows,
            &batch_results,
            execution.rows.len(),
        );
    }

    execution
}

/// Runs a statement and remembers its results for later CSV export via
/// [`export_results_csv`].
pub async fn run_and_remember(form: SqlForm, state: &SqlEngineState) -> SqlExecution {
    let connection = form.connection.clone();
    let execution = execute_sql(form, state).await;
    let mut last = state.last_results.lock().unwrap();
    last.insert(connection, execution.results.clone());
    execution
}

pub async fn fetch_timezone(state: &SqlEngineState, connection: &str) -> Result<SqlTimezoneInfo, String> {
    let conns = state.connections();
    let Some(conn) = find_connection(connection, &conns).cloned() else {
        return Err("SQL connection not found".to_string());
    };

    if conn.db_type == "sqlite" {
        let dsn = format!("sqlite:{}?mode=rwc", conn.host);
        let pool = get_or_create_sqlite_pool(state, &dsn).await?;
        let db_time = sqlx::query("SELECT datetime('now') AS db_time")
            .fetch_one(&pool)
            .await
            .ok()
            .and_then(|row| row.try_get::<String, _>("db_time").ok())
            .unwrap_or_default();
        return Ok(SqlTimezoneInfo {
            timezone: "UTC".to_string(),
            db_time,
            utc_offset: "+00:00".to_string(),
            note: Some(
                "SQLite date/time functions return UTC unless localtime is requested.".to_string(),
            ),
        });
    }

    let pool = get_or_create_pg_pool(state, &conn).await?;
    let row = sqlx::query(
        "SELECT current_setting('TimeZone') AS timezone, \
                to_char(now(), 'YYYY-MM-DD HH24:MI:SS OF') AS db_time, \
                to_char(now(), 'OF') AS utc_offset",
    )
    .fetch_one(&pool)
    .await
    .map_err(|err| format!("Failed to read database timezone: {err}"))?;

    Ok(SqlTimezoneInfo {
        timezone: row.try_get::<String, _>("timezone").unwrap_or_default(),
        db_time: row.try_get::<String, _>("db_time").unwrap_or_default(),
        utc_offset: row.try_get::<String, _>("utc_offset").unwrap_or_default(),
        note: None,
    })
}

#[derive(Debug, Clone)]
pub struct TableBrowseResult {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub row_count_text: String,
    pub page: u32,
    pub page_size: u32,
    pub has_next: bool,
}

pub async fn browse_table(
    state: &SqlEngineState,
    connection: &str,
    tab_id: Option<&str>,
    table: &str,
    page: Option<u32>,
    page_size: Option<u32>,
    filters: &[TableBrowseFilter],
) -> Result<TableBrowseResult, String> {
    let conns = state.connections();
    let Some(conn) = find_connection(connection, &conns).cloned() else {
        return Err("SQL connection not found".to_string());
    };

    let schema = fetch_schema_map(&conn, state).await?;
    let Some(columns) = schema.get(table) else {
        return Err("Unknown table".to_string());
    };

    let page = page.unwrap_or(1).max(1);
    let page_size = page_size.unwrap_or(100).clamp(10, 500);
    let offset = (page - 1) * page_size;
    let where_sql = build_table_filter_sql(filters, columns)?;
    let table_sql = table_reference(&conn, table);
    let sql = format!(
        "SELECT * FROM {table_sql}{where_sql} LIMIT {} OFFSET {}",
        page_size + 1,
        offset
    );

    let execution = execute_sql(
        SqlForm {
            connection: connection.to_string(),
            sql,
            variables: None,
            tab_id: tab_id.map(String::from),
            query_name: Some(format!("Browse {table}")),
            query_folder: None,
            run_source: None,
            cron_task_id: None,
            cron_task_name: None,
            alert: None,
        },
        state,
    )
    .await;
    if let Some(err) = execution.error {
        return Err(err);
    }

    let returned_rows = execution.rows.len();
    let has_next = returned_rows > page_size as usize;
    let (headers, mut rows) = (execution.headers, execution.rows);
    if has_next {
        rows.truncate(page_size as usize);
    }
    let visible_rows = rows.len();

    Ok(TableBrowseResult {
        headers,
        rows,
        row_count_text: format!("{visible_rows} rows"),
        page,
        page_size,
        has_next,
    })
}

#[derive(Debug, Clone)]
pub struct TableWriteResult {
    pub status: String,
    pub message: String,
    pub sql: String,
}

async fn run_statements_and_record_history(
    state: &SqlEngineState,
    connection: &str,
    tab_id: Option<&str>,
    query_name: &str,
    statements: &[String],
    id_prefix: &str,
) -> TableWriteResult {
    let created_at = now_isoish();
    let preview_sql = statements.join("\n\n");
    let mut status = "completed".to_string();
    let mut error_message = None;

    for statement in statements {
        let execution = execute_sql(
            SqlForm {
                connection: connection.to_string(),
                sql: statement.clone(),
                variables: None,
                tab_id: tab_id.map(String::from),
                query_name: Some(query_name.to_string()),
                query_folder: None,
                run_source: None,
                cron_task_id: None,
                cron_task_name: None,
                alert: None,
            },
            state,
        )
        .await;

        if let Some(err) = execution.error {
            status = "error".to_string();
            error_message = Some(err);
            break;
        }
    }

    let completed_at = now_isoish();
    let message = if status == "completed" {
        format!("Ran {} statement(s).", statements.len())
    } else {
        "Statement failed. See error for database details.".to_string()
    };

    if let Err(err) = app_db::upsert_sql_run_history(&app_db::SqlRunHistoryRecord {
        id: format!("{id_prefix}-{}-{}", now_millis(), std::process::id()),
        connection: connection.to_string(),
        tab_id: tab_id.unwrap_or_default().to_string(),
        sql: preview_sql.clone(),
        query_name: query_name.to_string(),
        query_folder: String::new(),
        run_source: "manual".to_string(),
        cron_task_id: String::new(),
        cron_task_name: String::new(),
        status: status.clone(),
        created_at,
        completed_at: Some(completed_at),
        row_count_text: Some(message.clone()),
        result_json: None,
        error: error_message,
        alert_triggered: false,
        alert_message: None,
    })
    .await
    {
        eprintln!("Failed to persist SQL run history: {err}");
    }

    TableWriteResult {
        status,
        message,
        sql: preview_sql,
    }
}

pub async fn update_table_rows(
    state: &SqlEngineState,
    connection: &str,
    tab_id: Option<&str>,
    table: &str,
    changes: &[TableUpdateChange],
) -> Result<TableWriteResult, String> {
    let conns = state.connections();
    let Some(conn) = find_connection(connection, &conns).cloned() else {
        return Err("SQL connection not found".to_string());
    };
    let schema = fetch_schema_map(&conn, state).await?;
    let Some(columns) = schema.get(table) else {
        return Err("Unknown table".to_string());
    };
    let statements = build_table_update_sql(&conn, table, columns, changes)?;
    if statements.is_empty() {
        return Ok(TableWriteResult {
            status: "noop".to_string(),
            message: "No changes to save.".to_string(),
            sql: String::new(),
        });
    }
    Ok(run_statements_and_record_history(
        state,
        connection,
        tab_id,
        &format!("Edit {table}"),
        &statements,
        "sql-edit",
    )
    .await)
}

pub async fn insert_table_rows(
    state: &SqlEngineState,
    connection: &str,
    tab_id: Option<&str>,
    table: &str,
    rows: &[HashMap<String, String>],
) -> Result<TableWriteResult, String> {
    let conns = state.connections();
    let Some(conn) = find_connection(connection, &conns).cloned() else {
        return Err("SQL connection not found".to_string());
    };
    let schema = fetch_schema_map(&conn, state).await?;
    let Some(columns) = schema.get(table) else {
        return Err("Unknown table".to_string());
    };
    let statements = build_table_insert_sql(&conn, table, columns, rows)?;
    if statements.is_empty() {
        return Ok(TableWriteResult {
            status: "noop".to_string(),
            message: "No entries to add.".to_string(),
            sql: String::new(),
        });
    }
    Ok(run_statements_and_record_history(
        state,
        connection,
        tab_id,
        &format!("Add entry {table}"),
        &statements,
        "sql-insert",
    )
    .await)
}

/// Starts a SQL statement running in the background (via `tokio::spawn`),
/// returning immediately with a job id that can be polled via
/// [`get_job`]/[`get_job_delta`].
pub async fn run_background(
    state: std::sync::Arc<SqlEngineState>,
    form: SqlForm,
) -> String {
    let job_id = format!("sql-{}-{}", now_millis(), std::process::id());
    let created_at = now_isoish();
    let tab_id = form.tab_id.clone().unwrap_or_default();
    let query_name = form.query_name.clone().unwrap_or_default();
    let query_folder = form.query_folder.clone().unwrap_or_default();
    let run_source = form.run_source.clone().unwrap_or_else(|| "manual".to_string());
    let cron_task_id = form.cron_task_id.clone().unwrap_or_default();
    let cron_task_name = form.cron_task_name.clone().unwrap_or_default();

    let job = SqlJob {
        id: job_id.clone(),
        connection: form.connection.clone(),
        tab_id: tab_id.clone(),
        sql: form.sql.clone(),
        query_name: query_name.clone(),
        query_folder: query_folder.clone(),
        run_source: run_source.clone(),
        cron_task_id: cron_task_id.clone(),
        cron_task_name: cron_task_name.clone(),
        status: "running".to_string(),
        created_at: created_at.clone(),
        completed_at: None,
        row_count_text: None,
        error: None,
        alert_triggered: false,
        alert_message: None,
        results: Vec::new(),
        stream_headers: Vec::new(),
        stream_rows: Vec::new(),
    };
    {
        let mut jobs = state.jobs.lock().unwrap();
        jobs.insert(job_id.clone(), job);
    }
    if let Err(err) = app_db::upsert_sql_run_history(&app_db::SqlRunHistoryRecord {
        id: job_id.clone(),
        connection: form.connection.clone(),
        tab_id: tab_id.clone(),
        sql: form.sql.clone(),
        query_name: query_name.clone(),
        query_folder: query_folder.clone(),
        run_source: run_source.clone(),
        cron_task_id: cron_task_id.clone(),
        cron_task_name: cron_task_name.clone(),
        status: "running".to_string(),
        created_at: created_at.clone(),
        completed_at: None,
        row_count_text: None,
        result_json: None,
        error: None,
        alert_triggered: false,
        alert_message: None,
    })
    .await
    {
        eprintln!("Failed to persist SQL running job: {err}");
    }

    let job_id_for_task = job_id.clone();
    tokio::spawn(async move {
        let execution = execute_sql_streaming_job(form.clone(), &state, &job_id_for_task).await;
        let rows_text = row_count_text(execution.rows.len());
        let status = if execution.error.is_some() {
            "error".to_string()
        } else {
            "completed".to_string()
        };
        let alert_message = if status == "completed" {
            alert_trigger_message(form.alert.as_ref(), &rows_text, &cron_task_name)
        } else {
            None
        };
        let alert_triggered = alert_message.is_some();
        let results = execution.results.clone();
        let completed_at = now_isoish();

        let history_record = {
            let mut jobs = state.jobs.lock().unwrap();
            let mut history_record = None;
            if let Some(job) = jobs.get_mut(&job_id_for_task) {
                job.status = status.clone();
                job.completed_at = Some(completed_at.clone());
                job.row_count_text = Some(rows_text.clone());
                job.results = execution.results;
                job.error = execution.error.clone();
                job.alert_triggered = alert_triggered;
                job.alert_message = alert_message.clone();
                let mut last = state.last_results.lock().unwrap();
                last.insert(job.connection.clone(), results);
                history_record = Some(app_db::SqlRunHistoryRecord {
                    id: job.id.clone(),
                    connection: job.connection.clone(),
                    tab_id: job.tab_id.clone(),
                    sql: job.sql.clone(),
                    query_name: job.query_name.clone(),
                    query_folder: job.query_folder.clone(),
                    run_source: job.run_source.clone(),
                    cron_task_id: job.cron_task_id.clone(),
                    cron_task_name: job.cron_task_name.clone(),
                    status: job.status.clone(),
                    created_at: job.created_at.clone(),
                    completed_at: job.completed_at.clone(),
                    row_count_text: job.row_count_text.clone(),
                    result_json: None,
                    error: job.error.clone(),
                    alert_triggered: job.alert_triggered,
                    alert_message: job.alert_message.clone(),
                });
            }
            history_record
        };
        let history_record = history_record.unwrap_or_else(|| app_db::SqlRunHistoryRecord {
            id: job_id_for_task,
            connection: form.connection,
            tab_id: form.tab_id.unwrap_or_default(),
            sql: form.sql,
            query_name: form.query_name.unwrap_or_default(),
            query_folder: form.query_folder.unwrap_or_default(),
            run_source,
            cron_task_id,
            cron_task_name,
            status,
            created_at,
            completed_at: Some(completed_at),
            row_count_text: Some(rows_text),
            result_json: None,
            error: execution.error,
            alert_triggered,
            alert_message,
        });
        if let Err(err) = app_db::upsert_sql_run_history(&history_record).await {
            eprintln!("Failed to persist completed SQL job: {err}");
        }
    });

    job_id
}

fn sql_job_from_history_record(record: app_db::SqlRunHistoryRecord) -> SqlJob {
    SqlJob {
        id: record.id,
        connection: record.connection,
        tab_id: record.tab_id,
        sql: record.sql,
        query_name: record.query_name,
        query_folder: record.query_folder,
        run_source: record.run_source,
        cron_task_id: record.cron_task_id,
        cron_task_name: record.cron_task_name,
        status: record.status,
        created_at: record.created_at,
        completed_at: record.completed_at,
        row_count_text: record.row_count_text,
        error: record.error,
        alert_triggered: record.alert_triggered,
        alert_message: record.alert_message,
        results: Vec::new(),
        stream_headers: Vec::new(),
        stream_rows: Vec::new(),
    }
}

pub async fn list_jobs(state: &SqlEngineState, connection: &str) -> Vec<SqlJob> {
    let mut jobs = app_db::get_sql_run_history_summaries(connection, None, 25)
        .await
        .into_iter()
        .map(sql_job_from_history_record)
        .collect::<Vec<_>>();

    let memory_jobs = {
        let job_map = state.jobs.lock().unwrap();
        job_map
            .values()
            .filter(|job| job.connection == connection)
            .cloned()
            .collect::<Vec<_>>()
    };
    for job in memory_jobs {
        if let Some(existing) = jobs.iter_mut().find(|existing| existing.id == job.id) {
            *existing = job;
        } else {
            jobs.push(job);
        }
    }
    jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    jobs.truncate(25);
    jobs
}

pub async fn get_job(state: &SqlEngineState, job_id: &str) -> Option<SqlJob> {
    let job = {
        let jobs = state.jobs.lock().unwrap();
        jobs.get(job_id).cloned()
    };
    match job {
        Some(job) => Some(job),
        None => app_db::get_sql_run_history_by_id(job_id)
            .await
            .map(sql_job_from_history_record),
    }
}

pub struct SqlJobDelta {
    pub id: String,
    pub status: String,
    pub row_count_text: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub next_offset: usize,
    pub error: Option<String>,
}

pub async fn get_job_delta(state: &SqlEngineState, job_id: &str, offset: usize) -> Option<SqlJobDelta> {
    let memory_job = {
        let jobs = state.jobs.lock().unwrap();
        jobs.get(job_id).cloned()
    };

    if let Some(job) = memory_job {
        let total_rows = job.stream_rows.len();
        let safe_offset = offset.min(total_rows);
        return Some(SqlJobDelta {
            id: job.id,
            status: job.status,
            row_count_text: job.row_count_text,
            created_at: job.created_at,
            completed_at: job.completed_at,
            headers: if safe_offset == 0 { job.stream_headers } else { Vec::new() },
            rows: job.stream_rows[safe_offset..].to_vec(),
            next_offset: total_rows,
            error: job.error,
        });
    }

    app_db::get_sql_run_history_by_id(job_id)
        .await
        .map(|record| SqlJobDelta {
            id: record.id,
            status: record.status,
            row_count_text: record.row_count_text,
            created_at: record.created_at,
            completed_at: record.completed_at,
            headers: Vec::new(),
            rows: Vec::new(),
            next_offset: offset,
            error: record.error,
        })
}

pub fn activate_job(state: &SqlEngineState, job_id: &str) -> bool {
    let results = {
        let jobs = state.jobs.lock().unwrap();
        jobs.get(job_id)
            .map(|job| (job.connection.clone(), job.results.clone()))
    };
    match results {
        Some((connection, results)) => {
            let mut last = state.last_results.lock().unwrap();
            last.insert(connection, results);
            true
        }
        None => false,
    }
}

/// Renders the last results run for a connection (see [`run_and_remember`])
/// as CSV bytes.
pub fn export_results_csv(state: &SqlEngineState, connection: &str) -> String {
    let results = {
        let last_results = state.last_results.lock().unwrap();
        last_results.get(connection).cloned().unwrap_or_default()
    };
    let mut wtr = csv::Writer::from_writer(vec![]);
    if !results.is_empty() {
        let mut headers: Vec<String> = results[0].keys().cloned().collect();
        headers.sort();
        wtr.write_record(&headers).ok();
        for row in results.iter() {
            let record: Vec<String> = headers
                .iter()
                .map(|h| row.get(h).cloned().unwrap_or_default())
                .collect();
            wtr.write_record(&record).ok();
        }
    }
    match wtr.into_inner() {
        Ok(buf) => String::from_utf8(buf).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

// --- Schema / relationship / function introspection ---

pub async fn fetch_schema_map(
    conn: &DbConnection,
    state: &SqlEngineState,
) -> Result<HashMap<String, Vec<String>>, String> {
    let mut schema_map: HashMap<String, Vec<String>> = HashMap::new();

    if conn.db_type == "sqlite" {
        let dsn = format!("sqlite:{}?mode=rwc", conn.host);
        let pool = get_or_create_sqlite_pool(state, &dsn).await?;

        let table_query =
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'";
        let rows = sqlx::query(table_query)
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("Failed to fetch tables: {e}"))?;

        for row in rows {
            let table_name: String = row.get("name");
            schema_map.insert(table_name.clone(), Vec::new());

            let col_query = format!("PRAGMA table_info(\"{table_name}\")");
            if let Ok(cols) = sqlx::query(&col_query).fetch_all(&pool).await {
                for col_row in cols {
                    let col_name: String = col_row.get("name");
                    if let Some(vec) = schema_map.get_mut(&table_name) {
                        vec.push(col_name);
                    }
                }
            }
        }
    } else {
        let pool = get_or_create_pg_pool(state, conn).await?;

        let schema_query = r#"
            SELECT table_name, column_name
            FROM information_schema.columns
            WHERE table_schema = 'public'
            ORDER BY table_name, ordinal_position
        "#;

        let rows = sqlx::query(schema_query)
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("Failed to fetch schema: {e}"))?;

        for row in rows {
            let table: String = row.get("table_name");
            let col: String = row.get("column_name");
            schema_map.entry(table).or_default().push(col);
        }
    }

    Ok(schema_map)
}

fn quote_sqlite_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

pub async fn fetch_relationship_schema(
    conn: &DbConnection,
    state: &SqlEngineState,
) -> Result<SqlRelationshipSchema, String> {
    if conn.db_type == "sqlite" {
        let dsn = format!("sqlite:{}?mode=rwc", conn.host);
        let pool = get_or_create_sqlite_pool(state, &dsn).await?;

        let table_rows = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("Failed to fetch SQLite tables: {e}"))?;

        let mut tables = Vec::new();
        let mut relationships = Vec::new();

        for row in table_rows {
            let table_name: String = row.get("name");
            let table_ref = quote_sqlite_identifier(&table_name);
            let col_query = format!("PRAGMA table_info({table_ref})");
            let col_rows = sqlx::query(&col_query)
                .fetch_all(&pool)
                .await
                .map_err(|e| format!("Failed to fetch SQLite columns for {table_name}: {e}"))?;
            let columns = col_rows
                .into_iter()
                .map(|col| SqlColumnInfo {
                    name: col.get::<String, _>("name"),
                    data_type: col.get::<String, _>("type"),
                    nullable: col.get::<i64, _>("notnull") == 0,
                    default: col.try_get::<String, _>("dflt_value").ok(),
                    primary_key: col.get::<i64, _>("pk") > 0,
                })
                .collect::<Vec<_>>();

            let fk_query = format!("PRAGMA foreign_key_list({table_ref})");
            if let Ok(fk_rows) = sqlx::query(&fk_query).fetch_all(&pool).await {
                for fk in fk_rows {
                    let id = fk.try_get::<i64, _>("id").unwrap_or(0);
                    let seq = fk.try_get::<i64, _>("seq").unwrap_or(0);
                    relationships.push(SqlForeignKeyInfo {
                        name: format!("fk_{table_name}_{id}_{seq}"),
                        from_table: table_name.clone(),
                        from_column: fk.try_get::<String, _>("from").unwrap_or_default(),
                        to_table: fk.try_get::<String, _>("table").unwrap_or_default(),
                        to_column: fk.try_get::<String, _>("to").unwrap_or_default(),
                    });
                }
            }

            tables.push(SqlTableInfo {
                name: table_name,
                columns,
            });
        }

        return Ok(SqlRelationshipSchema {
            tables,
            relationships,
        });
    }

    let pool = get_or_create_pg_pool(state, conn).await?;

    let column_rows = sqlx::query(
        r#"
        SELECT
            c.table_name,
            c.column_name,
            c.data_type,
            c.is_nullable,
            c.column_default,
            EXISTS (
                SELECT 1
                FROM information_schema.table_constraints tc
                JOIN information_schema.key_column_usage kcu
                  ON tc.constraint_name = kcu.constraint_name
                 AND tc.table_schema = kcu.table_schema
                 AND tc.table_name = kcu.table_name
                WHERE tc.constraint_type = 'PRIMARY KEY'
                  AND tc.table_schema = c.table_schema
                  AND tc.table_name = c.table_name
                  AND kcu.column_name = c.column_name
            ) AS primary_key
        FROM information_schema.columns c
        WHERE c.table_schema = 'public'
        ORDER BY c.table_name, c.ordinal_position
        "#,
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("Failed to fetch Postgres structure: {e}"))?;

    let mut table_map: HashMap<String, Vec<SqlColumnInfo>> = HashMap::new();
    for row in column_rows {
        let table_name: String = row.get("table_name");
        table_map
            .entry(table_name)
            .or_default()
            .push(SqlColumnInfo {
                name: row.get("column_name"),
                data_type: row.get("data_type"),
                nullable: row.get::<String, _>("is_nullable") == "YES",
                default: row.try_get::<String, _>("column_default").ok(),
                primary_key: row.get("primary_key"),
            });
    }

    let fk_rows = sqlx::query(
        r#"
        SELECT
            tc.constraint_name,
            kcu.table_name AS from_table,
            kcu.column_name AS from_column,
            ccu.table_name AS to_table,
            ccu.column_name AS to_column
        FROM information_schema.table_constraints tc
        JOIN information_schema.key_column_usage kcu
          ON tc.constraint_name = kcu.constraint_name
         AND tc.table_schema = kcu.table_schema
         AND tc.table_name = kcu.table_name
        JOIN information_schema.constraint_column_usage ccu
          ON ccu.constraint_name = tc.constraint_name
         AND ccu.table_schema = tc.table_schema
        WHERE tc.constraint_type = 'FOREIGN KEY'
          AND tc.table_schema = 'public'
        ORDER BY kcu.table_name, tc.constraint_name, kcu.ordinal_position
        "#,
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("Failed to fetch Postgres relationships: {e}"))?;

    let relationships = fk_rows
        .into_iter()
        .map(|row| SqlForeignKeyInfo {
            name: row.get("constraint_name"),
            from_table: row.get("from_table"),
            from_column: row.get("from_column"),
            to_table: row.get("to_table"),
            to_column: row.get("to_column"),
        })
        .collect::<Vec<_>>();

    let mut tables = table_map
        .into_iter()
        .map(|(name, columns)| SqlTableInfo { name, columns })
        .collect::<Vec<_>>();
    tables.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(SqlRelationshipSchema {
        tables,
        relationships,
    })
}

pub async fn fetch_function_list(
    conn: &DbConnection,
    state: &SqlEngineState,
) -> Result<Vec<DbFunctionInfo>, String> {
    if conn.db_type == "sqlite" {
        return Ok(Vec::new());
    }

    let pool = get_or_create_pg_pool(state, conn).await?;

    let function_query = r#"
        SELECT
            n.nspname AS schema_name,
            p.proname AS function_name,
            pg_get_function_identity_arguments(p.oid) AS arguments,
            pg_get_function_result(p.oid) AS return_type,
            pg_get_functiondef(p.oid) AS definition
        FROM pg_proc p
        JOIN pg_namespace n ON n.oid = p.pronamespace
        WHERE n.nspname NOT IN ('pg_catalog', 'information_schema')
        ORDER BY n.nspname, p.proname, pg_get_function_identity_arguments(p.oid)
    "#;

    let rows = sqlx::query(function_query)
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("Failed to fetch functions: {e}"))?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let schema: String = row.get("schema_name");
            let name: String = row.get("function_name");
            let arguments: String = row.get("arguments");
            let return_type: String = row.get("return_type");
            let definition: String = row.get("definition");
            let signature = format!("{schema}.{name}({arguments})");

            DbFunctionInfo {
                name,
                schema,
                signature,
                arguments,
                return_type,
                definition,
            }
        })
        .collect())
}

/// Cron-scheduled SQL tasks: thin JSON blob storage, kept generic since the
/// desktop UI defines the task shape.
pub async fn get_cron_tasks() -> Vec<Value> {
    app_db::get_json("sql", "cron_tasks").await.unwrap_or_default()
}

pub async fn save_cron_tasks(tasks: &[Value]) -> Result<(), sqlx::Error> {
    app_db::put_json("sql", "cron_tasks", &tasks).await
}
