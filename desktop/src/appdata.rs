use std::path::PathBuf;

/// Resolve the OS-appropriate app-data directory, create it if missing,
/// and point `OGTESTDESK_DB_PATH` at the database file inside it (unless
/// already set by the caller/environment).
pub fn configure() {
    if std::env::var("OGTESTDESK_DB_PATH").is_ok() {
        return;
    }

    let dir = app_data_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("failed to create app-data dir {}: {err}", dir.display());
    }

    let db_path = dir.join("og_testdesk.db");
    // SAFETY: called once at startup before any other thread reads env vars.
    unsafe {
        std::env::set_var("OGTESTDESK_DB_PATH", db_path);
    }
}

fn app_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(base).join("OGTestDesk")
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("OGTestDesk")
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let base = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            format!("{home}/.local/share")
        });
        PathBuf::from(base).join("og-testdesk")
    }
}
