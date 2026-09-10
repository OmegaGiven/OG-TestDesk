use anyhow::Result;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

/// Wraps the app's own local SQLite DB (connections, tabs, history,
/// saved queries/requests). Separate from any DB the user connects to.
pub struct MetadataStore {
    pool: SqlitePool,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS connections (
    id          TEXT PRIMARY KEY,
    nickname    TEXT NOT NULL,
    kind        TEXT NOT NULL,          -- postgres | mysql | sqlite
    host        TEXT,
    database    TEXT,
    user        TEXT,
    file_path   TEXT,                   -- sqlite only
    color       TEXT,                   -- hex, tints this connection's tabs
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
    created_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS environments (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    variables_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS app_state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

impl MetadataStore {
    /// `path` — OS app-data dir, e.g. `~/Library/Application Support/OGTestDesk/og_testdesk.db`.
    /// Override via `OGTESTDESK_DB_PATH` env var at the call site if needed.
    pub async fn open(path: &str) -> Result<Self> {
        let url = format!("sqlite://{path}?mode=rwc");
        let pool = SqlitePoolOptions::new().max_connections(5).connect(&url).await?;
        sqlx::query(SCHEMA).execute(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
