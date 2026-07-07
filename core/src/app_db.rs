use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use std::{path::Path, str::FromStr, sync::OnceLock};

use crate::theme::Theme;

const DEFAULT_DB_PATH: &str = "og_testdesk.db";
const DEFAULT_USER_ID: &str = "default";

static APP_DB: OnceLock<SqlitePool> = OnceLock::new();

pub fn db_path() -> String {
    std::env::var("OGTESTDESK_DB_PATH").unwrap_or_else(|_| DEFAULT_DB_PATH.to_string())
}

pub async fn init() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let db_path = db_path();
    if let Some(parent) = Path::new(&db_path)
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }

    let options =
        SqliteConnectOptions::from_str(&format!("sqlite://{db_path}"))?.create_if_missing(true);
    let pool = SqlitePool::connect_with(options).await?;

    create_schema(&pool).await?;
    let _ = APP_DB.set(pool);
    Ok(())
}

async fn create_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    // Legacy fallback table kept only for unknown document-style data.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS app_documents (
            collection TEXT NOT NULL,
            user_id TEXT NOT NULL,
            key TEXT NOT NULL,
            value_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
            PRIMARY KEY (collection, user_id, key)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS app_data_sets (
            collection TEXT NOT NULL,
            key TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
            PRIMARY KEY (collection, key)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sql_saved_queries (
            connection TEXT NOT NULL,
            name TEXT NOT NULL,
            folder TEXT,
            sql TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
            PRIMARY KEY (connection, name)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sql_query_folders (
            connection TEXT NOT NULL,
            name TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
            PRIMARY KEY (connection, name)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sql_run_history (
            id TEXT NOT NULL PRIMARY KEY,
            connection TEXT NOT NULL,
            tab_id TEXT NOT NULL DEFAULT '',
            sql TEXT NOT NULL,
            query_name TEXT NOT NULL DEFAULT '',
            query_folder TEXT NOT NULL DEFAULT '',
            run_source TEXT NOT NULL DEFAULT 'manual',
            cron_task_id TEXT NOT NULL DEFAULT '',
            cron_task_name TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            completed_at TEXT,
            row_count_text TEXT,
            result_json TEXT,
            error TEXT,
            alert_triggered INTEGER NOT NULL DEFAULT 0,
            alert_message TEXT,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_sql_run_history_connection_tab ON sql_run_history(connection, tab_id, updated_at DESC)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS saved_requests (
            folder TEXT NOT NULL DEFAULT '',
            name TEXT NOT NULL,
            method TEXT NOT NULL,
            url TEXT NOT NULL,
            headers TEXT NOT NULL DEFAULT '',
            body TEXT NOT NULL DEFAULT '',
            auth_type TEXT,
            oauth_token_url TEXT,
            oauth_client_id TEXT,
            oauth_client_secret TEXT,
            oauth_scope TEXT,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
            PRIMARY KEY (folder, name)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS request_folders (
            name TEXT NOT NULL PRIMARY KEY,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS request_variable_sets (
            name TEXT NOT NULL PRIMARY KEY,
            is_active INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS request_variables (
            set_name TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT NOT NULL DEFAULT '',
            updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
            PRIMARY KEY (set_name, key),
            FOREIGN KEY (set_name) REFERENCES request_variable_sets(name) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS request_history (
            id TEXT NOT NULL PRIMARY KEY,
            position INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            name TEXT,
            method TEXT,
            url TEXT,
            final_url TEXT,
            status INTEGER,
            duration_ms INTEGER,
            size_kb TEXT,
            curl TEXT,
            request_json TEXT NOT NULL,
            response_json TEXT NOT NULL,
            entry_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS scratchpads (
            id TEXT NOT NULL PRIMARY KEY,
            position INTEGER NOT NULL DEFAULT 0,
            title TEXT NOT NULL,
            text TEXT NOT NULL DEFAULT '',
            updated_at_text TEXT,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS ai_provider_settings (
            id TEXT NOT NULL PRIMARY KEY,
            name TEXT NOT NULL DEFAULT '',
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            base_url TEXT NOT NULL DEFAULT '',
            encrypted_api_key TEXT NOT NULL DEFAULT '',
            is_active INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        )
        "#,
    )
    .execute(pool)
    .await?;

    ensure_ai_provider_active_profile(pool).await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS themes (
            name TEXT NOT NULL PRIMARY KEY,
            theme_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS current_theme (
            id INTEGER NOT NULL PRIMARY KEY CHECK (id = 1),
            theme_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        )
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn ensure_ai_provider_active_profile(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let active_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ai_provider_settings WHERE is_active = 1")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
    if active_count > 0 {
        return Ok(());
    }

    sqlx::query(
        r#"
        UPDATE ai_provider_settings
        SET is_active = 1
        WHERE id = (
            SELECT id
            FROM ai_provider_settings
            ORDER BY CASE WHEN id = 'default' THEN 0 ELSE 1 END, updated_at DESC
            LIMIT 1
        )
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

fn pool() -> Option<&'static SqlitePool> {
    APP_DB.get()
}

pub async fn get_json<T>(collection: &str, key: &str) -> Option<T>
where
    T: DeserializeOwned,
{
    let pool = pool()?;
    let value = get_typed_json(pool, collection, key).await?;
    serde_json::from_value(value).ok()
}

pub async fn put_json<T>(collection: &str, key: &str, value: &T) -> Result<(), sqlx::Error>
where
    T: Serialize,
{
    let Some(pool) = pool() else {
        return Ok(());
    };
    let value =
        serde_json::to_value(value).map_err(|err| sqlx::Error::Protocol(err.to_string()))?;
    put_json_value(pool, collection, key, value).await
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AiProviderSettingsRecord {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub encrypted_api_key: String,
    pub is_active: bool,
}

pub async fn get_ai_provider_settings() -> Option<AiProviderSettingsRecord> {
    let pool = pool()?;
    let row = sqlx::query(
        r#"
        SELECT id, name, provider, model, base_url, encrypted_api_key, is_active
        FROM ai_provider_settings
        ORDER BY is_active DESC, CASE WHEN id = 'default' THEN 0 ELSE 1 END, updated_at DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await
    .ok()??;

    ai_provider_settings_from_row(&row)
}

pub async fn get_ai_provider_settings_by_id(id: &str) -> Option<AiProviderSettingsRecord> {
    let pool = pool()?;
    let row = sqlx::query(
        r#"
        SELECT id, name, provider, model, base_url, encrypted_api_key, is_active
        FROM ai_provider_settings
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .ok()??;

    ai_provider_settings_from_row(&row)
}

pub async fn list_ai_provider_settings() -> Vec<AiProviderSettingsRecord> {
    let Some(pool) = pool() else {
        return Vec::new();
    };
    let rows = sqlx::query(
        r#"
        SELECT id, name, provider, model, base_url, encrypted_api_key, is_active
        FROM ai_provider_settings
        ORDER BY is_active DESC, name COLLATE NOCASE ASC, updated_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.iter()
        .filter_map(ai_provider_settings_from_row)
        .collect()
}

fn ai_provider_settings_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Option<AiProviderSettingsRecord> {
    Some(AiProviderSettingsRecord {
        id: row.try_get("id").ok()?,
        name: row.try_get("name").unwrap_or_default(),
        provider: row.try_get("provider").ok()?,
        model: row.try_get("model").ok()?,
        base_url: row.try_get("base_url").unwrap_or_default(),
        encrypted_api_key: row.try_get("encrypted_api_key").unwrap_or_default(),
        is_active: row.try_get::<i64, _>("is_active").unwrap_or(0) == 1,
    })
}

pub async fn save_ai_provider_settings(
    settings: &AiProviderSettingsRecord,
) -> Result<(), sqlx::Error> {
    let Some(pool) = pool() else {
        return Ok(());
    };

    if settings.is_active {
        sqlx::query("UPDATE ai_provider_settings SET is_active = 0")
            .execute(pool)
            .await?;
    }

    sqlx::query(
        r#"
        INSERT INTO ai_provider_settings (
            id, name, provider, model, base_url, encrypted_api_key, is_active, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, unixepoch())
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            provider = excluded.provider,
            model = excluded.model,
            base_url = excluded.base_url,
            encrypted_api_key = excluded.encrypted_api_key,
            is_active = excluded.is_active,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&settings.id)
    .bind(&settings.name)
    .bind(&settings.provider)
    .bind(&settings.model)
    .bind(&settings.base_url)
    .bind(&settings.encrypted_api_key)
    .bind(if settings.is_active { 1 } else { 0 })
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn set_active_ai_provider_settings(id: &str) -> Result<(), sqlx::Error> {
    let Some(pool) = pool() else {
        return Ok(());
    };

    sqlx::query("UPDATE ai_provider_settings SET is_active = 0")
        .execute(pool)
        .await?;
    sqlx::query(
        "UPDATE ai_provider_settings SET is_active = 1, updated_at = unixepoch() WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct SqlRunHistoryRecord {
    pub id: String,
    pub connection: String,
    pub tab_id: String,
    pub sql: String,
    pub query_name: String,
    pub query_folder: String,
    pub run_source: String,
    pub cron_task_id: String,
    pub cron_task_name: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub row_count_text: Option<String>,
    pub result_json: Option<String>,
    pub error: Option<String>,
    pub alert_triggered: bool,
    pub alert_message: Option<String>,
}

pub async fn upsert_sql_run_history(record: &SqlRunHistoryRecord) -> Result<(), sqlx::Error> {
    let Some(pool) = pool() else {
        return Ok(());
    };

    sqlx::query(
        r#"
        INSERT INTO sql_run_history (
            id, connection, tab_id, sql, query_name, query_folder, status, created_at,
            completed_at, row_count_text, result_json, error, run_source, cron_task_id,
            cron_task_name, alert_triggered, alert_message, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch())
        ON CONFLICT(id) DO UPDATE SET
            connection = excluded.connection,
            tab_id = excluded.tab_id,
            sql = excluded.sql,
            query_name = excluded.query_name,
            query_folder = excluded.query_folder,
            status = excluded.status,
            created_at = excluded.created_at,
            completed_at = excluded.completed_at,
            row_count_text = excluded.row_count_text,
            result_json = excluded.result_json,
            error = excluded.error,
            run_source = excluded.run_source,
            cron_task_id = excluded.cron_task_id,
            cron_task_name = excluded.cron_task_name,
            alert_triggered = excluded.alert_triggered,
            alert_message = excluded.alert_message,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&record.id)
    .bind(&record.connection)
    .bind(&record.tab_id)
    .bind(&record.sql)
    .bind(&record.query_name)
    .bind(&record.query_folder)
    .bind(&record.status)
    .bind(&record.created_at)
    .bind(&record.completed_at)
    .bind(&record.row_count_text)
    .bind(&record.result_json)
    .bind(&record.error)
    .bind(&record.run_source)
    .bind(&record.cron_task_id)
    .bind(&record.cron_task_name)
    .bind(if record.alert_triggered { 1 } else { 0 })
    .bind(&record.alert_message)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_sql_run_history(
    connection: &str,
    tab_id: Option<&str>,
    limit: i64,
) -> Vec<SqlRunHistoryRecord> {
    let Some(pool) = pool() else {
        return Vec::new();
    };

    let rows = if let Some(tab_id) = tab_id {
        sqlx::query(
            r#"
            SELECT id, connection, tab_id, sql, query_name, query_folder, status, created_at,
                   completed_at, row_count_text, result_json, error, run_source, cron_task_id,
                   cron_task_name, alert_triggered, alert_message
            FROM sql_run_history
            WHERE connection = ? AND tab_id = ?
            ORDER BY updated_at DESC, created_at DESC
            LIMIT ?
            "#,
        )
        .bind(connection)
        .bind(tab_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query(
            r#"
            SELECT id, connection, tab_id, sql, query_name, query_folder, status, created_at,
                   completed_at, row_count_text, result_json, error, run_source, cron_task_id,
                   cron_task_name, alert_triggered, alert_message
            FROM sql_run_history
            WHERE connection = ?
            ORDER BY updated_at DESC, created_at DESC
            LIMIT ?
            "#,
        )
        .bind(connection)
        .bind(limit)
        .fetch_all(pool)
        .await
    };

    rows.unwrap_or_default()
        .into_iter()
        .filter_map(sql_run_history_record_from_row)
        .collect()
}

pub async fn get_sql_run_history_summaries(
    connection: &str,
    tab_id: Option<&str>,
    limit: i64,
) -> Vec<SqlRunHistoryRecord> {
    let Some(pool) = pool() else {
        return Vec::new();
    };

    let rows = if let Some(tab_id) = tab_id {
        sqlx::query(
            r#"
            SELECT id, connection, tab_id, sql, query_name, query_folder, status, created_at,
                   completed_at, row_count_text, NULL AS result_json, NULL AS error, run_source,
                   cron_task_id, cron_task_name, alert_triggered, alert_message
            FROM sql_run_history
            WHERE connection = ? AND tab_id = ?
            ORDER BY updated_at DESC, created_at DESC
            LIMIT ?
            "#,
        )
        .bind(connection)
        .bind(tab_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query(
            r#"
            SELECT id, connection, tab_id, sql, query_name, query_folder, status, created_at,
                   completed_at, row_count_text, NULL AS result_json, NULL AS error, run_source,
                   cron_task_id, cron_task_name, alert_triggered, alert_message
            FROM sql_run_history
            WHERE connection = ?
            ORDER BY updated_at DESC, created_at DESC
            LIMIT ?
            "#,
        )
        .bind(connection)
        .bind(limit)
        .fetch_all(pool)
        .await
    };

    rows.unwrap_or_default()
        .into_iter()
        .filter_map(sql_run_history_record_from_row)
        .collect()
}

pub async fn get_sql_run_history_by_id(id: &str) -> Option<SqlRunHistoryRecord> {
    let pool = pool()?;
    let row = sqlx::query(
        r#"
        SELECT id, connection, tab_id, sql, query_name, query_folder, status, created_at,
               completed_at, row_count_text, result_json, error, run_source, cron_task_id,
               cron_task_name, alert_triggered, alert_message
        FROM sql_run_history
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .ok()??;

    sql_run_history_record_from_row(row)
}

pub async fn delete_sql_run_history(id: &str) -> Result<(), sqlx::Error> {
    let Some(pool) = pool() else {
        return Ok(());
    };

    sqlx::query("DELETE FROM sql_run_history WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn clear_sql_run_history(
    connection: &str,
    tab_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    let Some(pool) = pool() else {
        return Ok(());
    };

    if let Some(tab_id) = tab_id {
        sqlx::query("DELETE FROM sql_run_history WHERE connection = ? AND tab_id = ?")
            .bind(connection)
            .bind(tab_id)
            .execute(pool)
            .await?;
    } else {
        sqlx::query("DELETE FROM sql_run_history WHERE connection = ?")
            .bind(connection)
            .execute(pool)
            .await?;
    }
    Ok(())
}

fn sql_run_history_record_from_row(row: sqlx::sqlite::SqliteRow) -> Option<SqlRunHistoryRecord> {
    Some(SqlRunHistoryRecord {
        id: row.try_get("id").ok()?,
        connection: row.try_get("connection").ok()?,
        tab_id: row.try_get("tab_id").ok()?,
        sql: row.try_get("sql").ok()?,
        query_name: row.try_get("query_name").ok()?,
        query_folder: row.try_get("query_folder").ok()?,
        run_source: row
            .try_get("run_source")
            .unwrap_or_else(|_| "manual".to_string()),
        cron_task_id: row.try_get("cron_task_id").unwrap_or_default(),
        cron_task_name: row.try_get("cron_task_name").unwrap_or_default(),
        status: row.try_get("status").ok()?,
        created_at: row.try_get("created_at").ok()?,
        completed_at: row.try_get("completed_at").ok(),
        row_count_text: row.try_get("row_count_text").ok(),
        result_json: row.try_get("result_json").ok(),
        error: row.try_get("error").ok(),
        alert_triggered: row.try_get::<i64, _>("alert_triggered").unwrap_or(0) == 1,
        alert_message: row.try_get("alert_message").ok(),
    })
}

async fn put_json_value(
    pool: &SqlitePool,
    collection: &str,
    key: &str,
    value: Value,
) -> Result<(), sqlx::Error> {
    if put_typed_json(pool, collection, key, &value).await? {
        return Ok(());
    }

    let raw =
        serde_json::to_string(&value).map_err(|err| sqlx::Error::Protocol(err.to_string()))?;
    sqlx::query(
        r#"
        INSERT INTO app_documents (collection, user_id, key, value_json, updated_at)
        VALUES (?, ?, ?, ?, unixepoch())
        ON CONFLICT(collection, user_id, key) DO UPDATE SET
            value_json = excluded.value_json,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(collection)
    .bind(DEFAULT_USER_ID)
    .bind(key)
    .bind(raw)
    .execute(pool)
    .await?;
    Ok(())
}

async fn get_typed_json(pool: &SqlitePool, collection: &str, key: &str) -> Option<Value> {
    let value = match (collection, key) {
        ("sql", "queries") => get_sql_queries(pool).await,
        ("sql", "query_folders") => get_sql_query_folders(pool).await,
        ("requests", "saved") => get_saved_requests(pool).await,
        ("requests", "folders") => get_request_folders(pool).await,
        ("requests", "variables") => get_request_variables(pool).await,
        ("requests", "history") => get_request_history(pool).await,
        ("scratchpads", "pads") => get_scratchpads(pool).await,
        ("themes", "all") => get_themes(pool).await,
        ("themes", "current") => get_current_theme(pool).await,
        _ => None,
    };

    if value.is_some() {
        return value;
    }

    if !is_known_typed_store(collection, key) || !has_data_set(pool, collection, key).await {
        return get_legacy_json(pool, collection, key).await;
    }

    Some(empty_typed_value(collection, key))
}

async fn put_typed_json(
    pool: &SqlitePool,
    collection: &str,
    key: &str,
    value: &Value,
) -> Result<bool, sqlx::Error> {
    match (collection, key) {
        ("sql", "queries") => {
            put_sql_queries(pool, value).await?;
            mark_data_set(pool, collection, key).await?;
            Ok(true)
        }
        ("sql", "query_folders") => {
            put_sql_query_folders(pool, value).await?;
            mark_data_set(pool, collection, key).await?;
            Ok(true)
        }
        ("requests", "saved") => {
            put_saved_requests(pool, value).await?;
            mark_data_set(pool, collection, key).await?;
            Ok(true)
        }
        ("requests", "folders") => {
            put_request_folders(pool, value).await?;
            mark_data_set(pool, collection, key).await?;
            Ok(true)
        }
        ("requests", "variables") => {
            put_request_variables(pool, value).await?;
            mark_data_set(pool, collection, key).await?;
            Ok(true)
        }
        ("requests", "history") => {
            put_request_history(pool, value).await?;
            mark_data_set(pool, collection, key).await?;
            Ok(true)
        }
        ("scratchpads", "pads") => {
            put_scratchpads(pool, value).await?;
            mark_data_set(pool, collection, key).await?;
            Ok(true)
        }
        ("themes", "all") => {
            put_themes(pool, value).await?;
            mark_data_set(pool, collection, key).await?;
            Ok(true)
        }
        ("themes", "current") => {
            put_current_theme(pool, value).await?;
            mark_data_set(pool, collection, key).await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn is_known_typed_store(collection: &str, key: &str) -> bool {
    matches!(
        (collection, key),
        ("sql", "queries")
            | ("sql", "query_folders")
            | ("requests", "saved")
            | ("requests", "folders")
            | ("requests", "variables")
            | ("requests", "history")
            | ("scratchpads", "pads")
            | ("themes", "all")
            | ("themes", "current")
    )
}

fn empty_typed_value(collection: &str, key: &str) -> Value {
    match (collection, key) {
        ("requests", "variables") => json!({
            "active_set": "",
            "sets": [],
            "global": {},
        }),
        ("themes", "all") => Value::Object(Map::new()),
        _ => Value::Array(Vec::new()),
    }
}

async fn has_data_set(pool: &SqlitePool, collection: &str, key: &str) -> bool {
    sqlx::query("SELECT 1 FROM app_data_sets WHERE collection = ? AND key = ?")
        .bind(collection)
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .is_some()
}

async fn mark_data_set(pool: &SqlitePool, collection: &str, key: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO app_data_sets (collection, key, updated_at)
        VALUES (?, ?, unixepoch())
        ON CONFLICT(collection, key) DO UPDATE SET updated_at = excluded.updated_at
        "#,
    )
    .bind(collection)
    .bind(key)
    .execute(pool)
    .await?;
    Ok(())
}

async fn get_legacy_json(pool: &SqlitePool, collection: &str, key: &str) -> Option<Value> {
    let row = sqlx::query(
        "SELECT value_json FROM app_documents WHERE collection = ? AND user_id = ? AND key = ?",
    )
    .bind(collection)
    .bind(DEFAULT_USER_ID)
    .bind(key)
    .fetch_optional(pool)
    .await
    .ok()??;

    let raw: String = row.try_get("value_json").ok()?;
    serde_json::from_str(&raw).ok()
}

async fn get_sql_queries(pool: &SqlitePool) -> Option<Value> {
    let rows = sqlx::query(
        "SELECT connection, name, folder, sql FROM sql_saved_queries ORDER BY connection, COALESCE(folder, ''), name",
    )
    .fetch_all(pool)
    .await
    .ok()?;
    if rows.is_empty() {
        return None;
    }
    let queries = rows
        .into_iter()
        .map(|row| {
            json!({
                "name": row.try_get::<String, _>("name").unwrap_or_default(),
                "sql": row.try_get::<String, _>("sql").unwrap_or_default(),
                "folder": row.try_get::<Option<String>, _>("folder").ok().flatten(),
                "connection": row.try_get::<String, _>("connection").unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    Some(Value::Array(queries))
}

async fn put_sql_queries(pool: &SqlitePool, value: &Value) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sql_saved_queries")
        .execute(pool)
        .await?;
    if let Some(items) = value.as_array() {
        for item in items {
            let connection = string_field(item, "connection");
            let name = string_field(item, "name");
            if connection.is_empty() || name.is_empty() {
                continue;
            }
            sqlx::query(
                r#"
                INSERT INTO sql_saved_queries (connection, name, folder, sql, updated_at)
                VALUES (?, ?, ?, ?, unixepoch())
                ON CONFLICT(connection, name) DO UPDATE SET
                    folder = excluded.folder,
                    sql = excluded.sql,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(connection)
            .bind(name)
            .bind(optional_string_field(item, "folder"))
            .bind(string_field(item, "sql"))
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

async fn get_sql_query_folders(pool: &SqlitePool) -> Option<Value> {
    let rows =
        sqlx::query("SELECT connection, name FROM sql_query_folders ORDER BY connection, name")
            .fetch_all(pool)
            .await
            .ok()?;
    if rows.is_empty() {
        return None;
    }
    let folders = rows
        .into_iter()
        .map(|row| {
            let connection = row.try_get::<String, _>("connection").unwrap_or_default();
            let name = row.try_get::<String, _>("name").unwrap_or_default();
            if connection.is_empty() {
                Value::String(name)
            } else {
                Value::String(format!("{connection}::{name}"))
            }
        })
        .collect::<Vec<_>>();
    Some(Value::Array(folders))
}

async fn put_sql_query_folders(pool: &SqlitePool, value: &Value) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sql_query_folders")
        .execute(pool)
        .await?;
    if let Some(items) = value.as_array() {
        for item in items {
            let raw = value_as_string(item);
            let (connection, name) = raw
                .split_once("::")
                .map(|(connection, name)| (connection.to_string(), name.to_string()))
                .unwrap_or_else(|| (String::new(), raw));
            if name.trim().is_empty() {
                continue;
            }
            sqlx::query(
                r#"
                INSERT INTO sql_query_folders (connection, name, updated_at)
                VALUES (?, ?, unixepoch())
                ON CONFLICT(connection, name) DO UPDATE SET updated_at = excluded.updated_at
                "#,
            )
            .bind(connection)
            .bind(name)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

async fn get_saved_requests(pool: &SqlitePool) -> Option<Value> {
    let rows = sqlx::query(
        r#"
        SELECT folder, name, method, url, headers, body, auth_type, oauth_token_url,
               oauth_client_id, oauth_client_secret, oauth_scope
        FROM saved_requests
        ORDER BY folder, name
        "#,
    )
    .fetch_all(pool)
    .await
    .ok()?;
    if rows.is_empty() {
        return None;
    }
    let requests = rows
        .into_iter()
        .map(|row| {
            let folder = row.try_get::<String, _>("folder").unwrap_or_default();
            json!({
                "name": row.try_get::<String, _>("name").unwrap_or_default(),
                "method": row.try_get::<String, _>("method").unwrap_or_else(|_| "GET".to_string()),
                "url": row.try_get::<String, _>("url").unwrap_or_default(),
                "headers": row.try_get::<String, _>("headers").unwrap_or_default(),
                "body": row.try_get::<String, _>("body").unwrap_or_default(),
                "auth_type": row.try_get::<Option<String>, _>("auth_type").ok().flatten(),
                "oauth_token_url": row.try_get::<Option<String>, _>("oauth_token_url").ok().flatten(),
                "oauth_client_id": row.try_get::<Option<String>, _>("oauth_client_id").ok().flatten(),
                "oauth_client_secret": row.try_get::<Option<String>, _>("oauth_client_secret").ok().flatten(),
                "oauth_scope": row.try_get::<Option<String>, _>("oauth_scope").ok().flatten(),
                "folder": if folder.is_empty() { Value::Null } else { Value::String(folder) },
            })
        })
        .collect::<Vec<_>>();
    Some(Value::Array(requests))
}

async fn put_saved_requests(pool: &SqlitePool, value: &Value) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM saved_requests")
        .execute(pool)
        .await?;
    if let Some(items) = value.as_array() {
        for item in items {
            let name = string_field(item, "name");
            if name.is_empty() {
                continue;
            }
            sqlx::query(
                r#"
                INSERT INTO saved_requests (
                    folder, name, method, url, headers, body, auth_type, oauth_token_url,
                    oauth_client_id, oauth_client_secret, oauth_scope, updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch())
                ON CONFLICT(folder, name) DO UPDATE SET
                    method = excluded.method,
                    url = excluded.url,
                    headers = excluded.headers,
                    body = excluded.body,
                    auth_type = excluded.auth_type,
                    oauth_token_url = excluded.oauth_token_url,
                    oauth_client_id = excluded.oauth_client_id,
                    oauth_client_secret = excluded.oauth_client_secret,
                    oauth_scope = excluded.oauth_scope,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(optional_string_field(item, "folder").unwrap_or_default())
            .bind(name)
            .bind(nonempty_string_field(item, "method", "GET"))
            .bind(string_field(item, "url"))
            .bind(string_field(item, "headers"))
            .bind(string_field(item, "body"))
            .bind(optional_string_field(item, "auth_type"))
            .bind(optional_string_field(item, "oauth_token_url"))
            .bind(optional_string_field(item, "oauth_client_id"))
            .bind(optional_string_field(item, "oauth_client_secret"))
            .bind(optional_string_field(item, "oauth_scope"))
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

async fn get_request_folders(pool: &SqlitePool) -> Option<Value> {
    let rows = sqlx::query("SELECT name FROM request_folders ORDER BY name")
        .fetch_all(pool)
        .await
        .ok()?;
    if rows.is_empty() {
        return None;
    }
    Some(Value::Array(
        rows.into_iter()
            .filter_map(|row| row.try_get::<String, _>("name").ok())
            .map(Value::String)
            .collect(),
    ))
}

async fn put_request_folders(pool: &SqlitePool, value: &Value) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM request_folders")
        .execute(pool)
        .await?;
    if let Some(items) = value.as_array() {
        for item in items {
            let name = value_as_string(item);
            if name.trim().is_empty() {
                continue;
            }
            sqlx::query(
                r#"
                INSERT INTO request_folders (name, updated_at)
                VALUES (?, unixepoch())
                ON CONFLICT(name) DO UPDATE SET updated_at = excluded.updated_at
                "#,
            )
            .bind(name)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

async fn get_request_variables(pool: &SqlitePool) -> Option<Value> {
    let set_rows = sqlx::query("SELECT name, is_active FROM request_variable_sets ORDER BY name")
        .fetch_all(pool)
        .await
        .ok()?;
    if set_rows.is_empty() {
        return None;
    }

    let mut active_set = String::new();
    let mut sets = Vec::new();
    for row in set_rows {
        let name = row.try_get::<String, _>("name").unwrap_or_default();
        if row.try_get::<i64, _>("is_active").unwrap_or(0) == 1 {
            active_set = name.clone();
        }
        let variable_rows =
            sqlx::query("SELECT key, value FROM request_variables WHERE set_name = ? ORDER BY key")
                .bind(&name)
                .fetch_all(pool)
                .await
                .unwrap_or_default();
        let mut values = Map::new();
        for variable_row in variable_rows {
            values.insert(
                variable_row.try_get::<String, _>("key").unwrap_or_default(),
                Value::String(
                    variable_row
                        .try_get::<String, _>("value")
                        .unwrap_or_default(),
                ),
            );
        }
        sets.push(json!({ "name": name, "values": values }));
    }

    if active_set.is_empty() {
        active_set = sets
            .first()
            .and_then(|set| set.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
    }

    Some(json!({
        "active_set": active_set,
        "sets": sets,
        "global": {},
    }))
}

async fn put_request_variables(pool: &SqlitePool, value: &Value) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM request_variables")
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM request_variable_sets")
        .execute(pool)
        .await?;

    let active_set = string_field(value, "active_set");
    if let Some(sets) = value.get("sets").and_then(Value::as_array) {
        for set in sets {
            let name = string_field(set, "name");
            if name.is_empty() {
                continue;
            }
            sqlx::query(
                r#"
                INSERT INTO request_variable_sets (name, is_active, updated_at)
                VALUES (?, ?, unixepoch())
                "#,
            )
            .bind(&name)
            .bind(if name == active_set { 1 } else { 0 })
            .execute(pool)
            .await?;

            if let Some(values) = set.get("values").and_then(Value::as_object) {
                for (key, variable_value) in values {
                    if key.trim().is_empty() {
                        continue;
                    }
                    sqlx::query(
                        r#"
                        INSERT INTO request_variables (set_name, key, value, updated_at)
                        VALUES (?, ?, ?, unixepoch())
                        "#,
                    )
                    .bind(&name)
                    .bind(key)
                    .bind(value_as_string(variable_value))
                    .execute(pool)
                    .await?;
                }
            }
        }
    }
    Ok(())
}

async fn get_request_history(pool: &SqlitePool) -> Option<Value> {
    let rows = sqlx::query("SELECT entry_json FROM request_history ORDER BY position")
        .fetch_all(pool)
        .await
        .ok()?;
    if rows.is_empty() {
        return None;
    }
    Some(Value::Array(
        rows.into_iter()
            .filter_map(|row| row.try_get::<String, _>("entry_json").ok())
            .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
            .collect(),
    ))
}

async fn put_request_history(pool: &SqlitePool, value: &Value) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM request_history")
        .execute(pool)
        .await?;
    if let Some(entries) = value.as_array() {
        for (position, entry) in entries.iter().enumerate() {
            let id = string_field(entry, "id");
            if id.is_empty() {
                continue;
            }
            let request = entry.get("request").cloned().unwrap_or_else(|| json!({}));
            let response = entry.get("response").cloned().unwrap_or_else(|| json!({}));
            sqlx::query(
                r#"
                INSERT INTO request_history (
                    id, position, created_at, name, method, url, final_url, status,
                    duration_ms, size_kb, curl, request_json, response_json, entry_json, updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch())
                "#,
            )
            .bind(id)
            .bind(position as i64)
            .bind(nonempty_string_field(entry, "createdAt", ""))
            .bind(optional_string_field(&request, "name"))
            .bind(optional_string_field(&request, "method"))
            .bind(optional_string_field(&request, "url"))
            .bind(optional_string_field(&request, "finalUrl"))
            .bind(response.get("status").and_then(Value::as_i64))
            .bind(response.get("duration_ms").and_then(Value::as_i64))
            .bind(optional_string_field(&response, "size_kb"))
            .bind(optional_string_field(&response, "curl"))
            .bind(serde_json::to_string(&request).unwrap_or_else(|_| "{}".to_string()))
            .bind(serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string()))
            .bind(serde_json::to_string(entry).unwrap_or_else(|_| "{}".to_string()))
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

async fn get_scratchpads(pool: &SqlitePool) -> Option<Value> {
    let rows =
        sqlx::query("SELECT id, title, text, updated_at_text FROM scratchpads ORDER BY position")
            .fetch_all(pool)
            .await
            .ok()?;
    if rows.is_empty() {
        return None;
    }
    Some(Value::Array(
        rows.into_iter()
            .map(|row| {
                json!({
                    "id": row.try_get::<String, _>("id").unwrap_or_default(),
                    "title": row.try_get::<String, _>("title").unwrap_or_else(|_| "Untitled".to_string()),
                    "text": row.try_get::<String, _>("text").unwrap_or_default(),
                    "updatedAt": row.try_get::<Option<String>, _>("updated_at_text").ok().flatten(),
                })
            })
            .collect(),
    ))
}

async fn put_scratchpads(pool: &SqlitePool, value: &Value) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM scratchpads").execute(pool).await?;
    if let Some(pads) = value.as_array() {
        for (position, pad) in pads.iter().enumerate() {
            let id = string_field(pad, "id");
            if id.is_empty() {
                continue;
            }
            sqlx::query(
                r#"
                INSERT INTO scratchpads (id, position, title, text, updated_at_text, updated_at)
                VALUES (?, ?, ?, ?, ?, unixepoch())
                ON CONFLICT(id) DO UPDATE SET
                    position = excluded.position,
                    title = excluded.title,
                    text = excluded.text,
                    updated_at_text = excluded.updated_at_text,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(id)
            .bind(position as i64)
            .bind(nonempty_string_field(pad, "title", "Untitled"))
            .bind(string_field(pad, "text"))
            .bind(optional_string_field(pad, "updatedAt"))
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

async fn get_themes(pool: &SqlitePool) -> Option<Value> {
    let rows = sqlx::query("SELECT theme_json FROM themes")
        .fetch_all(pool)
        .await
        .ok()?;
    if rows.is_empty() {
        return None;
    }
    let mut map = Map::new();
    for row in rows {
        let raw: String = row.try_get("theme_json").ok()?;
        let theme: Theme = serde_json::from_str(&raw).ok()?;
        map.insert(theme.name.clone(), serde_json::to_value(&theme).ok()?);
    }
    Some(Value::Object(map))
}

async fn put_themes(pool: &SqlitePool, value: &Value) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM themes").execute(pool).await?;
    if let Some(map) = value.as_object() {
        for (name, theme) in map {
            let raw = serde_json::to_string(theme)
                .map_err(|err| sqlx::Error::Protocol(err.to_string()))?;
            sqlx::query(
                r#"
                INSERT INTO themes (name, theme_json, updated_at)
                VALUES (?, ?, unixepoch())
                ON CONFLICT(name) DO UPDATE SET
                    theme_json = excluded.theme_json,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(name)
            .bind(raw)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

async fn get_current_theme(pool: &SqlitePool) -> Option<Value> {
    let row = sqlx::query("SELECT theme_json FROM current_theme WHERE id = 1")
        .fetch_optional(pool)
        .await
        .ok()??;
    let raw: String = row.try_get("theme_json").ok()?;
    serde_json::from_str(&raw).ok()
}

async fn put_current_theme(pool: &SqlitePool, value: &Value) -> Result<(), sqlx::Error> {
    let raw = serde_json::to_string(value).map_err(|err| sqlx::Error::Protocol(err.to_string()))?;
    sqlx::query(
        r#"
        INSERT INTO current_theme (id, theme_json, updated_at)
        VALUES (1, ?, unixepoch())
        ON CONFLICT(id) DO UPDATE SET
            theme_json = excluded.theme_json,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(raw)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_saved_themes() -> std::collections::HashMap<String, Theme> {
    get_json("themes", "all").await.unwrap_or_default()
}

pub async fn save_themes(themes: &std::collections::HashMap<String, Theme>) -> Result<(), sqlx::Error> {
    put_json("themes", "all", themes).await
}

pub async fn get_current_theme_value() -> Option<Theme> {
    get_json("themes", "current").await
}

pub async fn save_current_theme_value(theme: &Theme) -> Result<(), sqlx::Error> {
    put_json("themes", "current", theme).await
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .map(value_as_string)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn nonempty_string_field(value: &Value, key: &str, fallback: &str) -> String {
    let value = string_field(value, key);
    if value.is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

fn optional_string_field(value: &Value, key: &str) -> Option<String> {
    let value = string_field(value, key);
    if value.is_empty() { None } else { Some(value) }
}

fn value_as_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}
