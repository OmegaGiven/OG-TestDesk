//! A small, always-on error log so a human (or an AI looking through the
//! MCP server) can see what actually went wrong recently without having
//! to reproduce it live. Every `Err` that reaches a Tauri command
//! boundary lands here automatically (see `src-tauri/main.rs`'s `err()`
//! helper) — nothing to opt into, nothing sensitive is captured (error
//! messages in this app never include passwords, only things like host/
//! database names that already show up in the UI unencrypted).
//!
//! Storage: append-only JSONL under `<app_data_dir>/logs/errors.jsonl`,
//! trimmed to the most recent `CAP` entries on write so it can't grow
//! unbounded. Best-effort throughout — a logging failure never becomes
//! a user-facing error itself.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const CAP: usize = 500;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
/// Serializes writes so concurrent errors don't interleave/corrupt the
/// trim-and-rewrite step.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorLogEntry {
    /// Unix seconds.
    pub ts: i64,
    /// Where this came from — a `file:line` for backend errors, or
    /// `mcp:<tool_name>` / `frontend:<component>` for the rest.
    pub source: String,
    pub message: String,
}

/// Point the log at the app data dir. Call once at startup.
pub fn init_error_log(app_data_dir: PathBuf) {
    let _ = LOG_PATH.set(app_data_dir.join("logs").join("errors.jsonl"));
}

/// Record one error. Silently does nothing if `init_error_log` was never
/// called or the write fails — logging must never itself cause a crash
/// or mask the original error.
pub fn record_error(source: &str, message: &str) {
    let Some(path) = LOG_PATH.get() else { return };
    let entry = ErrorLogEntry {
        ts: now(),
        source: source.to_string(),
        message: message.to_string(),
    };

    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(path));
    let mut entries = read_all(path);
    entries.push(entry);
    if entries.len() > CAP {
        let drop_n = entries.len() - CAP;
        entries.drain(0..drop_n);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path) {
        for e in &entries {
            if let Ok(l) = serde_json::to_string(e) {
                let _ = writeln!(f, "{l}");
            }
        }
    }
}

/// Most recent entries, newest last. `limit` caps how many; `None` = all
/// (up to `CAP`).
pub fn recent_errors(limit: Option<usize>) -> Vec<ErrorLogEntry> {
    let Some(path) = LOG_PATH.get() else { return vec![] };
    let mut entries = read_all(path);
    if let Some(n) = limit {
        if entries.len() > n {
            let drop_n = entries.len() - n;
            entries.drain(0..drop_n);
        }
    }
    entries
}

pub fn clear_error_log() {
    if let Some(path) = LOG_PATH.get() {
        let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = std::fs::write(path, "");
    }
}

fn read_all(path: &PathBuf) -> Vec<ErrorLogEntry> {
    let Ok(text) = std::fs::read_to_string(path) else { return vec![] };
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
