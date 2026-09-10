//! Per-connection pool cache. Pools are keyed by their full connection
//! string so a password or host change transparently makes a new pool.
//! Pools are never closed for the process lifetime — sqlx pools are
//! designed to be long-lived and idle connections time out on their own.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use sqlx::mysql::{MySqlPool, MySqlPoolOptions};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

macro_rules! cache {
    ($name:ident, $pool:ty, $opts:ty) => {
        pub async fn $name(url: &str) -> Result<$pool> {
            static CACHE: OnceLock<Mutex<HashMap<String, $pool>>> = OnceLock::new();
            let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
            if let Some(p) = cache.lock().unwrap().get(url).cloned() {
                return Ok(p);
            }
            let pool = <$opts>::new()
                .max_connections(5)
                .acquire_timeout(Duration::from_secs(15))
                .connect(url)
                .await?;
            cache
                .lock()
                .unwrap()
                .insert(url.to_string(), pool.clone());
            Ok(pool)
        }
    };
}

cache!(pg_pool, PgPool, PgPoolOptions);
cache!(mysql_pool, MySqlPool, MySqlPoolOptions);

pub async fn sqlite_pool(path: &str) -> Result<SqlitePool> {
    static CACHE: OnceLock<Mutex<HashMap<String, SqlitePool>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(p) = cache.lock().unwrap().get(path).cloned() {
        return Ok(p);
    }
    let url = format!("sqlite://{path}?mode=rwc");
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(15))
        .connect(&url)
        .await?;
    cache.lock().unwrap().insert(path.to_string(), pool.clone());
    Ok(pool)
}
