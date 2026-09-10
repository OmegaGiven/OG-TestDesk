mod decode;
mod mysql;
mod pool;
mod postgres;
mod sqlite;

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
    ) -> Result<QueryResult>;
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
