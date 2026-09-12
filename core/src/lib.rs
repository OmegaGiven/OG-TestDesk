//! og_testdesk_core — persistence, DB driver abstraction, and HTTP request
//! runner. No UI code lives here; this crate is consumed by the Tauri
//! backend (`src-tauri`) via `tauri::command` wrappers.

pub mod drivers;
pub mod requests;
pub mod storage;

pub use drivers::{
    driver_for, max_rows, set_max_rows, stmt_returns_rows, Column, ConnConfig, DbDriver, DbKind,
    DbTime, ForeignKey, QueryColumn, QueryOpts, QueryResult, Relation, RelationKind, Schema,
    ServerInfo, SqlFunction,
};
pub use requests::{apply_environment, HttpRequest, HttpResponse};
pub use storage::errorlog::{clear_error_log, init_error_log, record_error, recent_errors, ErrorLogEntry};
pub use storage::metadata::{
    Environment, HistoryEntry, MetadataStore, QueryTab, RequestCollection, RequestHistoryEntry,
    RequestTab, SavedChart, SavedQuery, SavedQueryFolder, SavedRequest, Schedule,
};
pub use storage::secrets::{SecretsBackend, SecretsStore};
