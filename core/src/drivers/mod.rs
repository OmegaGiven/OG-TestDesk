mod decode;
mod exec;
mod mysql;
mod pool;
mod postgres;
mod sqlite;

pub(crate) use exec::run_query_body;

pub use mysql::MySqlDriverImpl;
pub use postgres::PostgresDriverImpl;
pub use sqlite::SqliteDriverImpl;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DbKind {
    Postgres,
    MySql,
    Sqlite,
}

/// A saved connection profile. Secrets (password) are NOT stored here —
/// they live in the OS keychain, keyed by `id`. See `storage::secrets`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnConfig {
    pub id: String,
    pub nickname: String,
    pub kind: DbKind,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub user: Option<String>,
    /// SQLite only — path to the .db file.
    pub file_path: Option<String>,
    /// Enable TLS for the connection (Postgres/MySQL).
    #[serde(default)]
    pub use_tls: bool,
    /// UI accent color for this connection (hex), used to tint its tabs.
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RelationKind {
    Table,
    View,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub name: String,
    pub kind: RelationKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    pub name: String,
    pub relations: Vec<Relation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
    pub default: Option<String>,
}

/// The server/session's notion of "now" — shown next to Run so it's
/// obvious when a DB isn't on the same clock/timezone as this machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTime {
    /// Server local wall-clock time (session timezone applied), as text.
    pub local_time: String,
    /// Session timezone name/setting, when the engine exposes one
    /// (Postgres/MySQL). `None` for engines with no such concept.
    pub tz_name: Option<String>,
    /// Session's offset from UTC, in seconds east of UTC.
    pub utc_offset_secs: i64,
}

/// A user-defined SQL function / procedure / aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlFunction {
    pub schema: String,
    pub name: String,
    /// "function" | "procedure" | "aggregate" | "window" — best-effort,
    /// not all engines distinguish these.
    pub kind: String,
    /// Argument list as engine-formatted text, e.g. "a integer, b text".
    pub arguments: String,
    pub return_type: Option<String>,
}

/// One foreign-key edge: `schema.table.column` references
/// `ref_schema.ref_table.ref_column`. Used to draw the table
/// relationships popup in the schema tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKey {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_column: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub kind: DbKind,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryColumn {
    pub name: String,
    pub type_name: String,
}

/// Options for a single `run_query` call.
#[derive(Debug, Clone, Copy)]
pub struct QueryOpts {
    /// Page window size. `None` = no pagination wrapper (full result,
    /// still subject to the streaming safety cap).
    pub limit: Option<usize>,
    /// Row offset for the page window.
    pub offset: usize,
    /// Also try to compute the total row count (time-boxed).
    pub count: bool,
}

impl QueryOpts {
    /// Full result, no count — used by the scheduler and MCP server.
    pub fn full() -> Self {
        Self {
            limit: None,
            offset: 0,
            count: false,
        }
    }
    pub fn page(page: usize, size: usize, count: bool) -> Self {
        Self {
            limit: Some(size),
            offset: page.saturating_mul(size),
            count,
        }
    }
}

/// Result of a statement. For non-SELECT statements `columns`/`rows` are
/// empty and `rows_affected` is populated instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<QueryColumn>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
    pub rows_affected: u64,
    pub duration_ms: u64,
    /// True when the statement returned a row set (SELECT / RETURNING / SHOW).
    pub is_select: bool,
    /// True when the row set was cut off at the streaming safety cap.
    #[serde(default)]
    pub truncated: bool,
    /// Zero-based page index of this window (paged calls only).
    #[serde(default)]
    pub page: usize,
    /// Page window size requested (0 when unpaged).
    #[serde(default)]
    pub page_size: usize,
    /// Total rows the full query would return, when it could be counted
    /// within the time budget. `None` = unknown (too slow / not countable).
    #[serde(default)]
    pub total: Option<usize>,
    /// Milliseconds spent on the COUNT(*), when attempted.
    #[serde(default)]
    pub count_ms: Option<u64>,
    /// True when a full page was returned (there may be another page).
    #[serde(default)]
    pub has_more: bool,
}

use std::sync::atomic::{AtomicUsize, Ordering};

/// Max rows any single `run_query` will pull into memory. Streamed row by
/// row and stopped at this count so a `SELECT *` on a huge table cannot
/// exhaust RAM. 0 means unlimited (not recommended).
static MAX_ROWS: AtomicUsize = AtomicUsize::new(10_000);

pub fn set_max_rows(n: usize) {
    MAX_ROWS.store(n, Ordering::Relaxed);
}
pub fn max_rows() -> usize {
    match MAX_ROWS.load(Ordering::Relaxed) {
        0 => usize::MAX,
        n => n,
    }
}

/// Implemented once per database engine. Adding a new engine (Mongo,
/// Redis, ...) means a new impl of this trait — no changes to the UI
/// or command layer.
#[async_trait]
pub trait DbDriver: Send + Sync {
    async fn test_connection(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<ServerInfo>;
    async fn list_schemas(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<Schema>>;
    async fn list_columns(
        &self,
        cfg: &ConnConfig,
        password: Option<&str>,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<Column>>;
    async fn run_query(
        &self,
        cfg: &ConnConfig,
        password: Option<&str>,
        sql: &str,
        opts: QueryOpts,
    ) -> Result<QueryResult>;
    /// Every foreign key in the database (all schemas), for the
    /// relationships popup. Best-effort — engines/versions that can't
    /// answer cheaply may just return an empty list.
    async fn list_foreign_keys(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<ForeignKey>>;
    /// User-defined functions/procedures (all schemas), for the schema
    /// tree's Functions tab. SQLite has no such catalog — its impl
    /// returns an empty list.
    async fn list_functions(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<SqlFunction>>;
    /// The server's current time + session timezone offset.
    async fn server_time(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<DbTime>;
}

/// Statements that can be safely wrapped as `SELECT * FROM (<sql>) x` for
/// pagination / counting. `SHOW`, `EXPLAIN`, `PRAGMA`, `DESCRIBE` cannot.
pub(crate) fn is_wrappable(sql: &str) -> bool {
    let head = sql
        .trim_start()
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(head.as_str(), "select" | "with" | "table" | "values")
}

pub(crate) fn strip_trailing_semi(sql: &str) -> &str {
    sql.trim().trim_end_matches(';').trim_end()
}

pub(crate) fn wrap_paged(sql: &str, limit: usize, offset: usize) -> String {
    format!(
        "SELECT * FROM (\n{}\n) AS _ogtd_q LIMIT {} OFFSET {}",
        strip_trailing_semi(sql),
        limit,
        offset
    )
}

pub(crate) fn wrap_count(sql: &str) -> String {
    format!(
        "SELECT COUNT(*) FROM (\n{}\n) AS _ogtd_q",
        strip_trailing_semi(sql)
    )
}

pub fn driver_for(kind: DbKind) -> Box<dyn DbDriver> {
    match kind {
        DbKind::Postgres => Box::new(PostgresDriverImpl),
        DbKind::MySql => Box::new(MySqlDriverImpl),
        DbKind::Sqlite => Box::new(SqliteDriverImpl),
    }
}

/// Heuristic: does this statement return rows? Used to decide between
/// reporting a row set vs. an affected-row count, and to enforce
/// read-only access (see the MCP server).
pub fn stmt_returns_rows(sql: &str) -> bool {
    let trimmed = sql.trim_start();
    // skip leading line comments / CTEs
    let head = trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        head.as_str(),
        "select" | "with" | "show" | "explain" | "table" | "values" | "pragma" | "describe" | "desc"
    ) || trimmed.to_ascii_lowercase().contains(" returning ")
}
