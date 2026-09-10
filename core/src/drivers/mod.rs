mod mysql;
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
    pub database: Option<String>,
    pub user: Option<String>,
    /// SQLite only — path to the .db file.
    pub file_path: Option<String>,
    /// UI accent color for this connection (hex), used to tint its tabs.
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    pub name: String,
    pub tables: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
    pub duration_ms: u64,
}

/// Implemented once per database engine. Adding a new engine (Mongo,
/// Redis, ...) means a new impl of this trait — no changes to the UI
/// or command layer.
#[async_trait]
pub trait DbDriver: Send + Sync {
    async fn test_connection(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<()>;
    async fn list_schemas(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<Schema>>;
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
