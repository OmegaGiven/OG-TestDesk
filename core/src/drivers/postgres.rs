use super::{ConnConfig, DbDriver, QueryResult, Schema};
use anyhow::Result;
use async_trait::async_trait;

pub struct PostgresDriverImpl;

#[async_trait]
impl DbDriver for PostgresDriverImpl {
    async fn test_connection(&self, _cfg: &ConnConfig, _password: Option<&str>) -> Result<()> {
        // TODO: sqlx::postgres::PgPoolOptions::connect(&conn_string(cfg, password)).await
        todo!("wire sqlx Postgres pool")
    }

    async fn list_schemas(
        &self,
        _cfg: &ConnConfig,
        _password: Option<&str>,
    ) -> Result<Vec<Schema>> {
        todo!("query information_schema.tables grouped by table_schema")
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
