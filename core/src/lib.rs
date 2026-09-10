//! og_testdesk_core — persistence, DB driver abstraction, and HTTP request
//! runner. No UI code lives here; this crate is consumed by the Tauri
//! backend (`src-tauri`) via `tauri::command` wrappers.

pub mod drivers;
pub mod requests;
pub mod storage;

pub use drivers::{
    driver_for, Column, ConnConfig, DbDriver, DbKind, QueryColumn, QueryResult, Relation,
    RelationKind, Schema, ServerInfo,
};
pub use requests::{apply_environment, HttpRequest, HttpResponse};
pub use storage::metadata::{
    Environment, HistoryEntry, MetadataStore, QueryTab, RequestCollection, SavedQuery, SavedRequest,
};
pub use storage::secrets::SecretsStore;
