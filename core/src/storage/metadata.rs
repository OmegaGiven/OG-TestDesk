use crate::drivers::{ConnConfig, DbKind};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};

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

CREATE TABLE IF NOT EXISTS saved_queries (
    id            TEXT PRIMARY KEY,
    connection_id TEXT REFERENCES connections(id) ON DELETE SET NULL,
    folder        TEXT,
    name          TEXT NOT NULL,
    sql_text      TEXT NOT NULL,
    created_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS query_history (
    id            TEXT PRIMARY KEY,
    connection_id TEXT REFERENCES connections(id) ON DELETE SET NULL,
    sql_text      TEXT NOT NULL,
    duration_ms   INTEGER,
    row_count     INTEGER,
    success       INTEGER NOT NULL DEFAULT 1,
    ran_at        INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS request_collections (
    id     TEXT PRIMARY KEY,
    name   TEXT NOT NULL,
    parent_id TEXT REFERENCES request_collections(id) ON DELETE CASCADE
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
    pub folder: Option<String>,
    pub name: String,
    pub sql_text: String,
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
    pub ran_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestCollection {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
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
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await?;
        sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await?;
        sqlx::query(SCHEMA).execute(&pool).await?;
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
                (id, connection_id, sql_text, duration_ms, row_count, success, ran_at)
             VALUES (?,?,?,?,?,?,?)",
        )
        .bind(&e.id)
        .bind(&e.connection_id)
        .bind(&e.sql_text)
        .bind(e.duration_ms)
        .bind(e.row_count)
        .bind(e.success as i64)
        .bind(e.ran_at)
        .execute(&self.pool)
        .await?;
        // keep last 500
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
            "SELECT id, connection_id, sql_text, duration_ms, row_count, success, ran_at
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
                ran_at: r.get("ran_at"),
            })
            .collect())
    }

    // -------------------------------------------------------- saved queries

    pub async fn list_saved_queries(&self) -> Result<Vec<SavedQuery>> {
        let rows = sqlx::query(
            "SELECT id, connection_id, folder, name, sql_text, created_at
             FROM saved_queries ORDER BY folder, name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| SavedQuery {
                id: r.get("id"),
                connection_id: r.get("connection_id"),
                folder: r.get("folder"),
                name: r.get("name"),
                sql_text: r.get("sql_text"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    pub async fn upsert_saved_query(&self, q: &SavedQuery) -> Result<()> {
        sqlx::query(
            "INSERT INTO saved_queries (id, connection_id, folder, name, sql_text, created_at)
             VALUES (?,?,?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET
                connection_id=excluded.connection_id, folder=excluded.folder,
                name=excluded.name, sql_text=excluded.sql_text",
        )
        .bind(&q.id)
        .bind(&q.connection_id)
        .bind(&q.folder)
        .bind(&q.name)
        .bind(&q.sql_text)
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
