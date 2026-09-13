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
    /// Blocks every non-row-returning statement at the driver level —
    /// UPDATE/INSERT/DELETE/DDL/etc. — regardless of caller (query editor,
    /// MCP, scheduler). A human's own accident-proofing on a connection
    /// they never intend to write through (e.g. a prod replica), distinct
    /// from the per-connection MCP write ACL, which only governs AI access.
    #[serde(default)]
    pub read_only: bool,
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
    /// Overrides the global `max_rows()` streaming cap for this call only,
    /// ignoring the human's UI row-limit preference entirely (including its
    /// "unlimited" setting). Used by callers that run SQL unattended — MCP
    /// tool calls and the scheduler — where nobody is watching to notice a
    /// multi-million-row `SELECT *` and cancel it, unlike the human-driven
    /// query editor, which respects whatever risk the human chose for
    /// their own foreground query.
    pub row_cap_override: Option<usize>,
}

impl QueryOpts {
    /// Full result, no count — used by the scheduler and MCP server.
    pub fn full() -> Self {
        Self {
            limit: None,
            offset: 0,
            count: false,
            row_cap_override: None,
        }
    }
    pub fn page(page: usize, size: usize, count: bool) -> Self {
        Self {
            limit: Some(size),
            offset: page.saturating_mul(size),
            count,
            row_cap_override: None,
        }
    }
    /// Like `full()`, but never subject to the human's UI row-limit
    /// preference — always capped at `n` regardless of the persisted
    /// `max_rows()` setting (even if that's 0/"unlimited"). For unattended
    /// callers (MCP, scheduler) that promise their own hard ceiling.
    pub fn full_capped(n: usize) -> Self {
        Self {
            limit: None,
            offset: 0,
            count: false,
            row_cap_override: Some(n),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_wrappable_accepts_row_producing_statements() {
        assert!(is_wrappable("SELECT * FROM orders"));
        assert!(is_wrappable("  with x as (select 1) select * from x"));
        assert!(is_wrappable("TABLE orders"));
        assert!(is_wrappable("VALUES (1), (2)"));
    }

    #[test]
    fn is_wrappable_rejects_statements_that_cant_be_subqueried() {
        assert!(!is_wrappable("INSERT INTO orders VALUES (1)"));
        assert!(!is_wrappable("PRAGMA table_info(orders)"));
        assert!(!is_wrappable("EXPLAIN SELECT 1"));
        assert!(!is_wrappable("SHOW TABLES"));
        assert!(!is_wrappable(""));
    }

    #[test]
    fn strip_trailing_semi_trims_trailing_semicolons_and_whitespace() {
        assert_eq!(strip_trailing_semi("SELECT 1;"), "SELECT 1");
        assert_eq!(strip_trailing_semi("SELECT 1;;  "), "SELECT 1");
        assert_eq!(strip_trailing_semi("  SELECT 1  "), "SELECT 1");
        assert_eq!(strip_trailing_semi("SELECT 1"), "SELECT 1");
    }

    #[test]
    fn wrap_paged_produces_a_bounded_subquery() {
        let wrapped = wrap_paged("SELECT * FROM t;", 50, 100);
        assert!(wrapped.contains("SELECT * FROM t"));
        assert!(wrapped.contains("LIMIT 50"));
        assert!(wrapped.contains("OFFSET 100"));
        assert!(!wrapped.contains(';'), "the inner statement's semicolon must be stripped");
    }

    #[test]
    fn wrap_count_produces_a_count_subquery() {
        let wrapped = wrap_count("SELECT * FROM t;");
        assert!(wrapped.starts_with("SELECT COUNT(*) FROM"));
        assert!(wrapped.contains("SELECT * FROM t"));
    }

    #[test]
    fn stmt_returns_rows_detects_row_producing_statements() {
        for sql in [
            "SELECT 1",
            "  with x as (select 1) select * from x",
            "show tables",
            "explain select 1",
            "pragma table_info(t)",
        ] {
            assert!(stmt_returns_rows(sql), "expected {sql:?} to return rows");
        }
    }

    #[test]
    fn full_capped_ignores_unlimited_max_rows_setting() {
        // Regression test: MCP/scheduler must stay hard-capped even when a
        // human has set the UI's global row limit to 0 ("unlimited
        // (risky)") for their own interactive queries — see max_rows()'s
        // 0-means-unlimited escape hatch above. A 5M-row `SELECT *` run
        // unattended through this path once froze the whole app because
        // full() alone just inherits that global setting.
        let original = max_rows();
        set_max_rows(0);
        let opts = QueryOpts::full_capped(10_000);
        assert_eq!(opts.row_cap_override, Some(10_000));
        assert_eq!(max_rows(), usize::MAX); // confirms the risky global is in effect
        set_max_rows(original);
    }

    #[test]
    fn stmt_returns_rows_detects_dml_returning() {
        assert!(stmt_returns_rows(
            "INSERT INTO t (a) VALUES (1) RETURNING id"
        ));
        assert!(stmt_returns_rows(
            "UPDATE t SET a = 1 WHERE id = 1 returning a"
        ));
    }

    #[test]
    fn stmt_returns_rows_rejects_plain_dml() {
        assert!(!stmt_returns_rows("INSERT INTO t (a) VALUES (1)"));
        assert!(!stmt_returns_rows("UPDATE t SET a = 1"));
        assert!(!stmt_returns_rows("DELETE FROM t"));
        assert!(!stmt_returns_rows("CREATE TABLE t (a int)"));
    }

    #[test]
    fn max_rows_round_trips_through_set_max_rows() {
        let original = max_rows();
        set_max_rows(1234);
        assert_eq!(max_rows(), 1234);
        set_max_rows(original); // don't leak state into other tests in this binary
    }
}
