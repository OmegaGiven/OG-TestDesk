use super::{ConnConfig, DbDriver, QueryResult, Schema};
use anyhow::Result;
use async_trait::async_trait;

pub struct SqliteDriverImpl;

#[async_trait]
impl DbDriver for SqliteDriverImpl {
    async fn test_connection(&self, _cfg: &ConnConfig, _password: Option<&str>) -> Result<()> {
        todo!("open sqlx SqlitePool at cfg.file_path, create file if missing")
    }

    async fn list_schemas(
        &self,
        _cfg: &ConnConfig,
        _password: Option<&str>,
    ) -> Result<Vec<Schema>> {
        todo!("query sqlite_master for tables")
    }

    async fn run_query(
        &self,
        _cfg: &ConnConfig,
        _password: Option<&str>,
        _sql: &str,
    ) -> Result<QueryResult> {
        todo!("execute via sqlx, map rows into QueryResult")
    }
}
