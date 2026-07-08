use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_db_type() -> String {
    "postgres".to_string()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DbConnection {
    #[serde(default = "default_db_type")]
    pub db_type: String, // "postgres" or "sqlite"
    pub host: String,     // For sqlite, this is the filename/path
    pub db_name: String,  // Postgres only
    pub user: String,     // Postgres only
    pub password: String, // Postgres only
    pub nickname: String,
}

#[derive(Deserialize, Clone)]
pub struct AddConnForm {
    pub db_type: Option<String>,
    pub host: String,
    pub db_name: String,
    pub user: String,
    pub password: String,
    pub nickname: String,
}

#[derive(Deserialize, Clone)]
pub struct EditConnectionForm {
    pub original_nickname: String,
    pub db_type: Option<String>,
    pub host: String,
    pub db_name: String,
    pub user: String,
    pub password: String,
    pub nickname: String,
}

#[derive(Deserialize, Clone)]
pub struct SqlForm {
    pub sql: String,
    pub connection: String,
    pub variables: Option<HashMap<String, String>>,
    pub tab_id: Option<String>,
    pub query_name: Option<String>,
    pub query_folder: Option<String>,
    pub run_source: Option<String>,
    pub cron_task_id: Option<String>,
    pub cron_task_name: Option<String>,
    pub alert: Option<SqlAlertRule>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct SqlAlertRule {
    pub comparator: String,
    pub value: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SavedQuery {
    pub name: String,
    pub sql: String,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub connection: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedQueryExport {
    pub version: u32,
    pub source_connection: String,
    pub queries: Vec<SavedQuery>,
    pub folders: Vec<String>,
}

/// In-memory record of a running or completed background SQL query, mirroring
/// `sql_run_history` rows while a job is still executing (streamed results are
/// not yet persisted).
#[derive(Serialize, Deserialize, Clone)]
pub struct SqlJob {
    pub id: String,
    pub connection: String,
    pub tab_id: String,
    pub sql: String,
    pub query_name: String,
    pub query_folder: String,
    #[serde(default)]
    pub run_source: String,
    #[serde(default)]
    pub cron_task_id: String,
    #[serde(default)]
    pub cron_task_name: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub row_count_text: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub alert_triggered: bool,
    pub alert_message: Option<String>,
    #[serde(skip)]
    pub results: Vec<HashMap<String, String>>,
    #[serde(skip)]
    pub stream_headers: Vec<String>,
    #[serde(skip)]
    pub stream_rows: Vec<Vec<String>>,
}

/// Result of running one SQL statement: column headers plus row data in both
/// ordered (for grid display) and keyed (for CSV export) forms.
#[derive(Clone, Default, Debug)]
pub struct SqlExecution {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub results: Vec<HashMap<String, String>>,
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SqlColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub primary_key: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct SqlForeignKeyInfo {
    pub name: String,
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct SqlTableInfo {
    pub name: String,
    pub columns: Vec<SqlColumnInfo>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SqlRelationshipSchema {
    pub tables: Vec<SqlTableInfo>,
    pub relationships: Vec<SqlForeignKeyInfo>,
}

#[derive(Serialize, Clone, Debug)]
pub struct DbFunctionInfo {
    pub name: String,
    pub schema: String,
    pub signature: String,
    pub arguments: String,
    pub return_type: String,
    pub definition: String,
}

#[derive(Deserialize, Clone)]
pub struct TableBrowseFilter {
    pub column: Option<String>,
    pub op: String,
    pub value: Option<String>,
}

#[derive(Deserialize)]
pub struct TableUpdateChange {
    pub original: HashMap<String, String>,
    pub current: HashMap<String, String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SqlTimezoneInfo {
    pub timezone: String,
    pub db_time: String,
    pub utc_offset: String,
    pub note: Option<String>,
}
