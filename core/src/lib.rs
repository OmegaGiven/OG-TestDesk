//! og_testdesk_core — persistence, DB driver abstraction, and HTTP request
//! runner. No UI code lives here; this crate is consumed by the Tauri
//! backend (`src-tauri`) via `tauri::command` wrappers.

pub mod drivers;
pub mod requests;
pub mod storage;

pub use drivers::{ConnConfig, DbDriver, DbKind, QueryResult, Schema};
pub use requests::{HttpRequest, HttpResponse};
pub use storage::metadata::MetadataStore;
pub use storage::secrets::SecretsStore;
