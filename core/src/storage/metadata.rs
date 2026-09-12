use crate::drivers::{ConnConfig, DbKind};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;
use std::time::Duration;

/// Wraps the app's own local SQLite DB (connections, tabs, history,
/// saved queries/requests). Separate from any DB the user connects to.
pub struct MetadataStore {
    pool: SqlitePool,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS connections (
    id          TEXT PRIMARY KEY,
    nickname    TEXT NOT NULL,
    kind        TEXT NOT NULL,
    host        TEXT,
    port        INTEGER,
    database    TEXT,
    user        TEXT,
    file_path   TEXT,
    use_tls     INTEGER NOT NULL DEFAULT 0,
    color       TEXT,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS query_tabs (
    id            TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
    title         TEXT NOT NULL,
    sql_text      TEXT NOT NULL DEFAULT '',
    position      INTEGER NOT NULL,
    is_active     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS saved_query_folders (
    id        TEXT PRIMARY KEY,
    name      TEXT NOT NULL,
    parent_id TEXT REFERENCES saved_query_folders(id) ON DELETE CASCADE,
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS saved_queries (
    id            TEXT PRIMARY KEY,
    connection_id TEXT REFERENCES connections(id) ON DELETE SET NULL,
    folder        TEXT,
    folder_id     TEXT REFERENCES saved_query_folders(id) ON DELETE SET NULL,
    name          TEXT NOT NULL,
    sql_text      TEXT NOT NULL,
    sort_order    INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS query_history (
    id            TEXT PRIMARY KEY,
    connection_id TEXT REFERENCES connections(id) ON DELETE SET NULL,
    sql_text      TEXT NOT NULL,
    duration_ms   INTEGER,
    row_count     INTEGER,
    success       INTEGER NOT NULL DEFAULT 1,
    error         TEXT,
    result_json   TEXT,          -- serialized QueryResult, only when small
    ran_at        INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS request_history (
    id               TEXT PRIMARY KEY,
    saved_request_id TEXT,
    name             TEXT,
    method           TEXT NOT NULL,
    url              TEXT NOT NULL,
    headers_json     TEXT NOT NULL DEFAULT '{}',
    body             TEXT,
    status           INTEGER,
    duration_ms      INTEGER,
    size_bytes       INTEGER,
    success          INTEGER NOT NULL DEFAULT 1,
    error            TEXT,
    response_json    TEXT,        -- serialized HttpResponse, only when small
    sent_at          INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS schedules (
    id                   TEXT PRIMARY KEY,
    name                 TEXT NOT NULL,
    kind                 TEXT NOT NULL,           -- 'sql' | 'request'
    connection_id        TEXT,
    sql_text             TEXT,
    saved_request_id     TEXT,
    request_method       TEXT,
    request_url          TEXT,
    request_headers_json TEXT,
    request_body         TEXT,
    schedule_expr        TEXT NOT NULL,           -- cron, or 'every:<seconds>'
    enabled              INTEGER NOT NULL DEFAULT 1,
    last_run             INTEGER,
    last_status          TEXT,
    next_run             INTEGER,
    created_at           INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS request_collections (
    id     TEXT PRIMARY KEY,
    name   TEXT NOT NULL,
    parent_id TEXT REFERENCES request_collections(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS request_tabs (
    id             TEXT PRIMARY KEY,
    saved_request_id TEXT,
    title          TEXT NOT NULL,
    method         TEXT NOT NULL DEFAULT 'GET',
    url            TEXT NOT NULL DEFAULT '',
    headers_json   TEXT NOT NULL DEFAULT '{}',
    body           TEXT,
    position       INTEGER NOT NULL DEFAULT 0,
    is_active      INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS saved_requests (
    id            TEXT PRIMARY KEY,
    collection_id TEXT REFERENCES request_collections(id) ON DELETE SET NULL,
    name          TEXT NOT NULL,
    method        TEXT NOT NULL,
    url           TEXT NOT NULL,
    headers_json  TEXT NOT NULL DEFAULT '{}',
    body          TEXT,
    sort_order    INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS environments (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    variables_json TEXT NOT NULL DEFAULT '{}',
    is_active     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS app_state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Saved data visualizations (Inspector "Chart" mode). `data_json` is a
-- cached snapshot of the last-plotted rows so reopening one is instant;
-- it's excluded from list_saved_charts (same disk-not-RAM pattern as
-- query/request history) and fetched separately on demand. When
-- connection_id + sql_text are both set the chart can be re-run to
-- refresh that snapshot (the seed for a future scheduled dashboard).
CREATE TABLE IF NOT EXISTS saved_charts (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    connection_id  TEXT REFERENCES connections(id) ON DELETE SET NULL,
    saved_query_id TEXT REFERENCES saved_queries(id) ON DELETE SET NULL,
    sql_text       TEXT,
    chart_type     TEXT NOT NULL,
    x_field        TEXT,
    y_fields_json  TEXT NOT NULL DEFAULT '[]',
    options_json   TEXT NOT NULL DEFAULT '{}',
    data_json      TEXT,
    row_count      INTEGER,
    last_run_at    INTEGER,
    sort_order     INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL
);
"#;

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn kind_str(k: DbKind) -> &'static str {
    match k {
        DbKind::Postgres => "postgres",
        DbKind::MySql => "mysql",
        DbKind::Sqlite => "sqlite",
    }
}

fn parse_kind(s: &str) -> DbKind {
    match s {
        "mysql" => DbKind::MySql,
        "sqlite" => DbKind::Sqlite,
        _ => DbKind::Postgres,
    }
}

// ------------------------------------------------------------------ models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryTab {
    pub id: String,
    pub connection_id: String,
    pub title: String,
    pub sql_text: String,
    pub position: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedQuery {
    pub id: String,
    pub connection_id: Option<String>,
    #[serde(default)]
    pub folder_id: Option<String>,
    pub name: String,
    pub sql_text: String,
    #[serde(default)]
    pub sort_order: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedQueryFolder {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub sort_order: i64,
}

/// A saved data visualization (Inspector "Chart" mode). `data_json` is
/// write-only from `list_saved_charts`'s point of view — never returned
/// by the list, only by `chart_data(id)` — same pattern as query/request
/// history, so a big cached result set doesn't sit in every list render.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedChart {
    pub id: String,
    pub name: String,
    pub connection_id: Option<String>,
    pub saved_query_id: Option<String>,
    pub sql_text: Option<String>,
    pub chart_type: String,
    pub x_field: Option<String>,
    #[serde(default)]
    pub y_fields_json: String,
    #[serde(default)]
    pub options_json: String,
    /// Cached last-plotted rows (JSON array of objects). Write-only: sent
    /// in on save/refresh, never populated by `list_saved_charts`.
    #[serde(default)]
    pub data_json: Option<String>,
    #[serde(default)]
    pub has_data: bool,
    pub row_count: Option<i64>,
    pub last_run_at: Option<i64>,
    #[serde(default)]
    pub sort_order: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub connection_id: Option<String>,
    pub sql_text: String,
    pub duration_ms: Option<i64>,
    pub row_count: Option<i64>,
    pub success: bool,
    #[serde(default)]
    pub error: Option<String>,
    /// Serialized `QueryResult` — write-only. Never returned by the list
    /// query (would blow up memory); fetch on demand with `history_result`.
    #[serde(default, skip_serializing)]
    pub result_json: Option<String>,
    /// True when a cached result exists on disk for this entry.
    #[serde(default)]
    pub has_result: bool,
    pub ran_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestHistoryEntry {
    pub id: String,
    #[serde(default)]
    pub saved_request_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    pub headers_json: String,
    pub body: Option<String>,
    pub status: Option<i64>,
    pub duration_ms: Option<i64>,
    pub size_bytes: Option<i64>,
    pub success: bool,
    #[serde(default)]
    pub error: Option<String>,
    /// Serialized `HttpResponse` — write-only; fetch on demand with
    /// `request_history_result`.
    #[serde(default, skip_serializing)]
    pub response_json: Option<String>,
    #[serde(default)]
    pub has_response: bool,
    pub sent_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub id: String,
    pub name: String,
    /// "sql" | "request"
    pub kind: String,
    pub connection_id: Option<String>,
    pub sql_text: Option<String>,
    pub saved_request_id: Option<String>,
    pub request_method: Option<String>,
    pub request_url: Option<String>,
    pub request_headers_json: Option<String>,
    pub request_body: Option<String>,
    /// cron expression, or "every:<seconds>"
    pub schedule_expr: String,
    pub enabled: bool,
    pub last_run: Option<i64>,
    pub last_status: Option<String>,
    pub next_run: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestCollection {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestTab {
    pub id: String,
    pub saved_request_id: Option<String>,
    pub title: String,
    pub method: String,
    pub url: String,
    pub headers_json: String,
    pub body: Option<String>,
    pub position: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedRequest {
    pub id: String,
    pub collection_id: Option<String>,
    pub name: String,
    pub method: String,
    pub url: String,
    /// JSON object string of header name -> value.
    pub headers_json: String,
    pub body: Option<String>,
    pub sort_order: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub id: String,
    pub name: String,
    /// JSON object string of variable name -> value.
    pub variables_json: String,
    pub is_active: bool,
}

impl MetadataStore {
    pub async fn open(path: &str) -> Result<Self> {
        let url = format!("sqlite://{path}?mode=rwc");
        // WAL + a busy timeout, applied to every connection the pool
        // opens (not just a one-off PRAGMA on whichever connection ran
        // first) — the default rollback-journal mode serializes writers
        // and errors out immediately as "database is locked" under any
        // real concurrency, which this app has plenty of: rapid tab
        // closes, debounced autosaves, the scheduler, and MCP calls can
        // all hit this same file at once. WAL lets readers and a writer
        // overlap, and the busy timeout makes a genuinely-contended
        // write wait a few seconds and retry instead of failing outright
        // — a failed tab-close silently leaving the row behind (which
        // then reappears on the next full refetch) was exactly this.
        let opts = SqliteConnectOptions::from_str(&url)?
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new().max_connections(5).connect_with(opts).await?;
        sqlx::query(SCHEMA).execute(&pool).await?;
        // Additive migrations for databases created before a column existed.
        for stmt in [
            "ALTER TABLE query_history ADD COLUMN error TEXT",
            "ALTER TABLE query_history ADD COLUMN result_json TEXT",
            "ALTER TABLE saved_queries ADD COLUMN folder_id TEXT",
            "ALTER TABLE saved_queries ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0",
        ] {
            let _ = sqlx::query(stmt).execute(&pool).await; // ignore "duplicate column"
        }
        // One-time migration of the old flat `folder` string into real folders.
        let legacy: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, folder FROM saved_queries WHERE folder IS NOT NULL AND folder <> '' AND folder_id IS NULL",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        if !legacy.is_empty() {
            let mut by_name: std::collections::HashMap<String, String> = Default::default();
            for (qid, folder) in legacy {
                let fid = if let Some(f) = by_name.get(&folder) {
                    f.clone()
                } else {
                    let f = uuid::Uuid::new_v4().to_string();
                    let _ = sqlx::query(
                        "INSERT INTO saved_query_folders (id, name, parent_id) VALUES (?,?,NULL)",
                    )
                    .bind(&f)
                    .bind(&folder)
                    .execute(&pool)
                    .await;
                    by_name.insert(folder.clone(), f.clone());
                    f
                };
                let _ = sqlx::query("UPDATE saved_queries SET folder_id = ? WHERE id = ?")
                    .bind(&fid)
                    .bind(&qid)
                    .execute(&pool)
                    .await;
            }
        }
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    // --------------------------------------------------------- connections

    pub async fn list_connections(&self) -> Result<Vec<ConnConfig>> {
        let rows = sqlx::query(
            "SELECT id, nickname, kind, host, port, database, user, file_path, use_tls, color
             FROM connections ORDER BY sort_order, created_at",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ConnConfig {
                id: r.get("id"),
                nickname: r.get("nickname"),
                kind: parse_kind(&r.get::<String, _>("kind")),
                host: r.get("host"),
                port: r.get::<Option<i64>, _>("port").map(|p| p as u16),
                database: r.get("database"),
                user: r.get("user"),
                file_path: r.get("file_path"),
                use_tls: r.get::<i64, _>("use_tls") != 0,
                color: r.get("color"),
            })
            .collect())
    }

    pub async fn upsert_connection(&self, c: &ConnConfig) -> Result<()> {
        sqlx::query(
            "INSERT INTO connections
                (id, nickname, kind, host, port, database, user, file_path, use_tls, color,
                 sort_order, created_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,
                 COALESCE((SELECT sort_order FROM connections WHERE id = ?),
                          (SELECT COALESCE(MAX(sort_order)+1,0) FROM connections)),
                 COALESCE((SELECT created_at FROM connections WHERE id = ?), ?))
             ON CONFLICT(id) DO UPDATE SET
                nickname=excluded.nickname, kind=excluded.kind, host=excluded.host,
                port=excluded.port, database=excluded.database, user=excluded.user,
                file_path=excluded.file_path, use_tls=excluded.use_tls, color=excluded.color",
        )
        .bind(&c.id)
        .bind(&c.nickname)
        .bind(kind_str(c.kind))
        .bind(&c.host)
        .bind(c.port.map(|p| p as i64))
        .bind(&c.database)
        .bind(&c.user)
        .bind(&c.file_path)
        .bind(c.use_tls as i64)
        .bind(&c.color)
        .bind(&c.id)
        .bind(&c.id)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_connection(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM connections WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn reorder_connections(&self, ids: &[String]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for (i, id) in ids.iter().enumerate() {
            sqlx::query("UPDATE connections SET sort_order = ? WHERE id = ?")
                .bind(i as i64)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    // ----------------------------------------------------------- query tabs

    pub async fn list_tabs(&self, connection_id: &str) -> Result<Vec<QueryTab>> {
        let rows = sqlx::query(
            "SELECT id, connection_id, title, sql_text, position, is_active
             FROM query_tabs WHERE connection_id = ? ORDER BY position",
        )
        .bind(connection_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_tab).collect())
    }

    pub async fn list_all_tabs(&self) -> Result<Vec<QueryTab>> {
        let rows = sqlx::query(
            "SELECT id, connection_id, title, sql_text, position, is_active
             FROM query_tabs ORDER BY connection_id, position",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_tab).collect())
    }

    pub async fn upsert_tab(&self, t: &QueryTab) -> Result<()> {
        sqlx::query(
            "INSERT INTO query_tabs (id, connection_id, title, sql_text, position, is_active)
             VALUES (?,?,?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET
                title=excluded.title, sql_text=excluded.sql_text,
                position=excluded.position, is_active=excluded.is_active",
        )
        .bind(&t.id)
        .bind(&t.connection_id)
        .bind(&t.title)
        .bind(&t.sql_text)
        .bind(t.position)
        .bind(t.is_active as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_tab(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM query_tabs WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ------------------------------------------------------------- history

    pub async fn add_history(&self, e: &HistoryEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO query_history
                (id, connection_id, sql_text, duration_ms, row_count, success, error,
                 result_json, ran_at)
             VALUES (?,?,?,?,?,?,?,?,?)",
        )
        .bind(&e.id)
        .bind(&e.connection_id)
        .bind(&e.sql_text)
        .bind(e.duration_ms)
        .bind(e.row_count)
        .bind(e.success as i64)
        .bind(&e.error)
        .bind(&e.result_json)
        .bind(e.ran_at)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "DELETE FROM query_history WHERE id NOT IN
                (SELECT id FROM query_history ORDER BY ran_at DESC LIMIT 500)",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn recent_history(&self, limit: i64) -> Result<Vec<HistoryEntry>> {
        let rows = sqlx::query(
            "SELECT id, connection_id, sql_text, duration_ms, row_count, success, error,
                    (result_json IS NOT NULL) AS has_result, ran_at
             FROM query_history ORDER BY ran_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| HistoryEntry {
                id: r.get("id"),
                connection_id: r.get("connection_id"),
                sql_text: r.get("sql_text"),
                duration_ms: r.get("duration_ms"),
                row_count: r.get("row_count"),
                success: r.get::<i64, _>("success") != 0,
                error: r.get("error"),
                result_json: None,
                has_result: r.get::<i64, _>("has_result") != 0,
                ran_at: r.get("ran_at"),
            })
            .collect())
    }

    /// Fetch the cached result set for one history entry, on demand.
    pub async fn history_result(&self, id: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT result_json FROM query_history WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .flatten(),
        )
    }

    // ------------------------------------------------------- request history

    pub async fn add_request_history(&self, e: &RequestHistoryEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO request_history
                (id, saved_request_id, name, method, url, headers_json, body, status,
                 duration_ms, size_bytes, success, error, response_json, sent_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&e.id)
        .bind(&e.saved_request_id)
        .bind(&e.name)
        .bind(&e.method)
        .bind(&e.url)
        .bind(&e.headers_json)
        .bind(&e.body)
        .bind(e.status)
        .bind(e.duration_ms)
        .bind(e.size_bytes)
        .bind(e.success as i64)
        .bind(&e.error)
        .bind(&e.response_json)
        .bind(e.sent_at)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "DELETE FROM request_history WHERE id NOT IN
                (SELECT id FROM request_history ORDER BY sent_at DESC LIMIT 500)",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn recent_request_history(&self, limit: i64) -> Result<Vec<RequestHistoryEntry>> {
        let rows = sqlx::query(
            "SELECT id, saved_request_id, name, method, url, headers_json, body, status,
                    duration_ms, size_bytes, success, error,
                    (response_json IS NOT NULL) AS has_response, sent_at
             FROM request_history ORDER BY sent_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| RequestHistoryEntry {
                id: r.get("id"),
                saved_request_id: r.get("saved_request_id"),
                name: r.get("name"),
                method: r.get("method"),
                url: r.get("url"),
                headers_json: r.get("headers_json"),
                body: r.get("body"),
                status: r.get("status"),
                duration_ms: r.get("duration_ms"),
                size_bytes: r.get("size_bytes"),
                success: r.get::<i64, _>("success") != 0,
                error: r.get("error"),
                response_json: None,
                has_response: r.get::<i64, _>("has_response") != 0,
                sent_at: r.get("sent_at"),
            })
            .collect())
    }

    pub async fn request_history_result(&self, id: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT response_json FROM request_history WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .flatten(),
        )
    }

    // ---------------------------------------------------------- schedules

    pub async fn list_schedules(&self) -> Result<Vec<Schedule>> {
        let rows = sqlx::query(
            "SELECT id, name, kind, connection_id, sql_text, saved_request_id, request_method,
                    request_url, request_headers_json, request_body, schedule_expr, enabled,
                    last_run, last_status, next_run, created_at
             FROM schedules ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_schedule).collect())
    }

    pub async fn get_schedule(&self, id: &str) -> Result<Option<Schedule>> {
        let row = sqlx::query(
            "SELECT id, name, kind, connection_id, sql_text, saved_request_id, request_method,
                    request_url, request_headers_json, request_body, schedule_expr, enabled,
                    last_run, last_status, next_run, created_at
             FROM schedules WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(row_to_schedule))
    }

    pub async fn due_schedules(&self, now_ts: i64) -> Result<Vec<Schedule>> {
        let rows = sqlx::query(
            "SELECT id, name, kind, connection_id, sql_text, saved_request_id, request_method,
                    request_url, request_headers_json, request_body, schedule_expr, enabled,
                    last_run, last_status, next_run, created_at
             FROM schedules WHERE enabled = 1 AND next_run IS NOT NULL AND next_run <= ?",
        )
        .bind(now_ts)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_schedule).collect())
    }

    pub async fn upsert_schedule(&self, s: &Schedule) -> Result<()> {
        sqlx::query(
            "INSERT INTO schedules
                (id, name, kind, connection_id, sql_text, saved_request_id, request_method,
                 request_url, request_headers_json, request_body, schedule_expr, enabled,
                 last_run, last_status, next_run, created_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,
                 COALESCE((SELECT created_at FROM schedules WHERE id = ?), ?))
             ON CONFLICT(id) DO UPDATE SET
                name=excluded.name, kind=excluded.kind, connection_id=excluded.connection_id,
                sql_text=excluded.sql_text, saved_request_id=excluded.saved_request_id,
                request_method=excluded.request_method, request_url=excluded.request_url,
                request_headers_json=excluded.request_headers_json,
                request_body=excluded.request_body, schedule_expr=excluded.schedule_expr,
                enabled=excluded.enabled, next_run=excluded.next_run",
        )
        .bind(&s.id)
        .bind(&s.name)
        .bind(&s.kind)
        .bind(&s.connection_id)
        .bind(&s.sql_text)
        .bind(&s.saved_request_id)
        .bind(&s.request_method)
        .bind(&s.request_url)
        .bind(&s.request_headers_json)
        .bind(&s.request_body)
        .bind(&s.schedule_expr)
        .bind(s.enabled as i64)
        .bind(s.last_run)
        .bind(&s.last_status)
        .bind(s.next_run)
        .bind(&s.id)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_schedule_run(
        &self,
        id: &str,
        ran_at: i64,
        status: &str,
        next_run: Option<i64>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE schedules SET last_run = ?, last_status = ?, next_run = ? WHERE id = ?",
        )
        .bind(ran_at)
        .bind(status)
        .bind(next_run)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_schedule(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM schedules WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -------------------------------------------------------- saved queries

    pub async fn list_saved_queries(&self) -> Result<Vec<SavedQuery>> {
        let rows = sqlx::query(
            "SELECT id, connection_id, folder_id, name, sql_text, sort_order, created_at
             FROM saved_queries ORDER BY sort_order, name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| SavedQuery {
                id: r.get("id"),
                connection_id: r.get("connection_id"),
                folder_id: r.get("folder_id"),
                name: r.get("name"),
                sql_text: r.get("sql_text"),
                sort_order: r.get("sort_order"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    pub async fn upsert_saved_query(&self, q: &SavedQuery) -> Result<()> {
        sqlx::query(
            "INSERT INTO saved_queries (id, connection_id, folder_id, name, sql_text, sort_order, created_at)
             VALUES (?,?,?,?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET
                connection_id=excluded.connection_id, folder_id=excluded.folder_id,
                name=excluded.name, sql_text=excluded.sql_text, sort_order=excluded.sort_order",
        )
        .bind(&q.id)
        .bind(&q.connection_id)
        .bind(&q.folder_id)
        .bind(&q.name)
        .bind(&q.sql_text)
        .bind(q.sort_order)
        .bind(if q.created_at == 0 { now() } else { q.created_at })
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_saved_query(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM saved_queries WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -------------------------------------------------- saved query folders

    pub async fn list_saved_query_folders(&self) -> Result<Vec<SavedQueryFolder>> {
        let rows = sqlx::query(
            "SELECT id, name, parent_id, sort_order FROM saved_query_folders ORDER BY sort_order, name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| SavedQueryFolder {
                id: r.get("id"),
                name: r.get("name"),
                parent_id: r.get("parent_id"),
                sort_order: r.get("sort_order"),
            })
            .collect())
    }

    pub async fn upsert_saved_query_folder(&self, f: &SavedQueryFolder) -> Result<()> {
        // Guard against parent cycles: walk up from the requested parent.
        if let Some(mut pid) = f.parent_id.clone() {
            let all = self.list_saved_query_folders().await?;
            loop {
                if pid == f.id {
                    return Err(anyhow::anyhow!("cannot move a folder into its own descendant"));
                }
                match all.iter().find(|x| x.id == pid).and_then(|x| x.parent_id.clone()) {
                    Some(next) => pid = next,
                    None => break,
                }
            }
        }
        sqlx::query(
            "INSERT INTO saved_query_folders (id, name, parent_id, sort_order)
             VALUES (?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET
                name=excluded.name, parent_id=excluded.parent_id, sort_order=excluded.sort_order",
        )
        .bind(&f.id)
        .bind(&f.name)
        .bind(&f.parent_id)
        .bind(f.sort_order)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_saved_query_folder(&self, id: &str) -> Result<()> {
        // FK cascade drops child folders; ON DELETE SET NULL frees the queries.
        sqlx::query("DELETE FROM saved_query_folders WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ------------------------------------------------------- saved charts

    pub async fn list_saved_charts(&self) -> Result<Vec<SavedChart>> {
        let rows = sqlx::query(
            "SELECT id, name, connection_id, saved_query_id, sql_text, chart_type, x_field,
                    y_fields_json, options_json, row_count, last_run_at, sort_order, created_at,
                    (data_json IS NOT NULL) AS has_data
             FROM saved_charts ORDER BY sort_order, name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_chart).collect())
    }

    /// Fetch just the cached snapshot rows for one chart (never held in
    /// the list above — pulled on demand, same as history results).
    pub async fn chart_data(&self, id: &str) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT data_json FROM saved_charts WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.and_then(|(d,)| d))
    }

    /// Upsert a chart's config. `data_json` (and `row_count`/`last_run_at`)
    /// are only overwritten when `Some` — `None` leaves the existing
    /// cached snapshot alone, so renaming a chart doesn't drop its data.
    pub async fn upsert_saved_chart(&self, c: &SavedChart) -> Result<()> {
        sqlx::query(
            "INSERT INTO saved_charts
                (id, name, connection_id, saved_query_id, sql_text, chart_type, x_field,
                 y_fields_json, options_json, data_json, row_count, last_run_at, sort_order, created_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET
                name=excluded.name, connection_id=excluded.connection_id,
                saved_query_id=excluded.saved_query_id, sql_text=excluded.sql_text,
                chart_type=excluded.chart_type, x_field=excluded.x_field,
                y_fields_json=excluded.y_fields_json, options_json=excluded.options_json,
                data_json=COALESCE(excluded.data_json, saved_charts.data_json),
                row_count=COALESCE(excluded.row_count, saved_charts.row_count),
                last_run_at=COALESCE(excluded.last_run_at, saved_charts.last_run_at),
                sort_order=excluded.sort_order",
        )
        .bind(&c.id)
        .bind(&c.name)
        .bind(&c.connection_id)
        .bind(&c.saved_query_id)
        .bind(&c.sql_text)
        .bind(&c.chart_type)
        .bind(&c.x_field)
        .bind(&c.y_fields_json)
        .bind(&c.options_json)
        .bind(&c.data_json)
        .bind(c.row_count)
        .bind(c.last_run_at)
        .bind(c.sort_order)
        .bind(if c.created_at == 0 { now() } else { c.created_at })
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_saved_chart(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM saved_charts WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // --------------------------------------------------- request collections

    pub async fn list_collections(&self) -> Result<Vec<RequestCollection>> {
        let rows = sqlx::query("SELECT id, name, parent_id FROM request_collections ORDER BY name")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| RequestCollection {
                id: r.get("id"),
                name: r.get("name"),
                parent_id: r.get("parent_id"),
            })
            .collect())
    }

    pub async fn upsert_collection(&self, c: &RequestCollection) -> Result<()> {
        sqlx::query(
            "INSERT INTO request_collections (id, name, parent_id) VALUES (?,?,?)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, parent_id=excluded.parent_id",
        )
        .bind(&c.id)
        .bind(&c.name)
        .bind(&c.parent_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_collection(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM request_collections WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // --------------------------------------------------------- request tabs

    pub async fn list_request_tabs(&self) -> Result<Vec<RequestTab>> {
        let rows = sqlx::query(
            "SELECT id, saved_request_id, title, method, url, headers_json, body, position, is_active
             FROM request_tabs ORDER BY position",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| RequestTab {
                id: r.get("id"),
                saved_request_id: r.get("saved_request_id"),
                title: r.get("title"),
                method: r.get("method"),
                url: r.get("url"),
                headers_json: r.get("headers_json"),
                body: r.get("body"),
                position: r.get("position"),
                is_active: r.get::<i64, _>("is_active") != 0,
            })
            .collect())
    }

    pub async fn upsert_request_tab(&self, t: &RequestTab) -> Result<()> {
        sqlx::query(
            "INSERT INTO request_tabs
                (id, saved_request_id, title, method, url, headers_json, body, position, is_active)
             VALUES (?,?,?,?,?,?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET
                saved_request_id=excluded.saved_request_id, title=excluded.title,
                method=excluded.method, url=excluded.url, headers_json=excluded.headers_json,
                body=excluded.body, position=excluded.position, is_active=excluded.is_active",
        )
        .bind(&t.id)
        .bind(&t.saved_request_id)
        .bind(&t.title)
        .bind(&t.method)
        .bind(&t.url)
        .bind(&t.headers_json)
        .bind(&t.body)
        .bind(t.position)
        .bind(t.is_active as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_request_tab(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM request_tabs WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ------------------------------------------------------- saved requests

    pub async fn list_saved_requests(&self) -> Result<Vec<SavedRequest>> {
        let rows = sqlx::query(
            "SELECT id, collection_id, name, method, url, headers_json, body, sort_order, created_at
             FROM saved_requests ORDER BY sort_order, created_at",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| SavedRequest {
                id: r.get("id"),
                collection_id: r.get("collection_id"),
                name: r.get("name"),
                method: r.get("method"),
                url: r.get("url"),
                headers_json: r.get("headers_json"),
                body: r.get("body"),
                sort_order: r.get("sort_order"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    pub async fn upsert_saved_request(&self, s: &SavedRequest) -> Result<()> {
        sqlx::query(
            "INSERT INTO saved_requests
                (id, collection_id, name, method, url, headers_json, body, sort_order, created_at)
             VALUES (?,?,?,?,?,?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET
                collection_id=excluded.collection_id, name=excluded.name, method=excluded.method,
                url=excluded.url, headers_json=excluded.headers_json, body=excluded.body,
                sort_order=excluded.sort_order",
        )
        .bind(&s.id)
        .bind(&s.collection_id)
        .bind(&s.name)
        .bind(&s.method)
        .bind(&s.url)
        .bind(&s.headers_json)
        .bind(&s.body)
        .bind(s.sort_order)
        .bind(if s.created_at == 0 { now() } else { s.created_at })
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_saved_request(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM saved_requests WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -------------------------------------------------------- environments

    pub async fn list_environments(&self) -> Result<Vec<Environment>> {
        let rows = sqlx::query(
            "SELECT id, name, variables_json, is_active FROM environments ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| Environment {
                id: r.get("id"),
                name: r.get("name"),
                variables_json: r.get("variables_json"),
                is_active: r.get::<i64, _>("is_active") != 0,
            })
            .collect())
    }

    pub async fn upsert_environment(&self, e: &Environment) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        if e.is_active {
            sqlx::query("UPDATE environments SET is_active = 0")
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query(
            "INSERT INTO environments (id, name, variables_json, is_active) VALUES (?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET
                name=excluded.name, variables_json=excluded.variables_json,
                is_active=excluded.is_active",
        )
        .bind(&e.id)
        .bind(&e.name)
        .bind(&e.variables_json)
        .bind(e.is_active as i64)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_environment(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM environments WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------- app state

    pub async fn get_state(&self, key: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT value FROM app_state WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn set_state(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO app_state (key, value) VALUES (?,?)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn row_to_schedule(r: sqlx::sqlite::SqliteRow) -> Schedule {
    Schedule {
        id: r.get("id"),
        name: r.get("name"),
        kind: r.get("kind"),
        connection_id: r.get("connection_id"),
        sql_text: r.get("sql_text"),
        saved_request_id: r.get("saved_request_id"),
        request_method: r.get("request_method"),
        request_url: r.get("request_url"),
        request_headers_json: r.get("request_headers_json"),
        request_body: r.get("request_body"),
        schedule_expr: r.get("schedule_expr"),
        enabled: r.get::<i64, _>("enabled") != 0,
        last_run: r.get("last_run"),
        last_status: r.get("last_status"),
        next_run: r.get("next_run"),
        created_at: r.get("created_at"),
    }
}

fn row_to_chart(r: sqlx::sqlite::SqliteRow) -> SavedChart {
    SavedChart {
        id: r.get("id"),
        name: r.get("name"),
        connection_id: r.get("connection_id"),
        saved_query_id: r.get("saved_query_id"),
        sql_text: r.get("sql_text"),
        chart_type: r.get("chart_type"),
        x_field: r.get("x_field"),
        y_fields_json: r.get("y_fields_json"),
        options_json: r.get("options_json"),
        data_json: None,
        has_data: r.get::<i64, _>("has_data") != 0,
        row_count: r.get("row_count"),
        last_run_at: r.get("last_run_at"),
        sort_order: r.get("sort_order"),
        created_at: r.get("created_at"),
    }
}

fn row_to_tab(r: sqlx::sqlite::SqliteRow) -> QueryTab {
    QueryTab {
        id: r.get("id"),
        connection_id: r.get("connection_id"),
        title: r.get("title"),
        sql_text: r.get("sql_text"),
        position: r.get("position"),
        is_active: r.get::<i64, _>("is_active") != 0,
    }
}
