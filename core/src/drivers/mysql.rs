use super::{ConnConfig, DbDriver, QueryResult, Schema};
use anyhow::Result;
use async_trait::async_trait;

pub struct MySqlDriverImpl;

#[async_trait]
impl DbDriver for MySqlDriverImpl {
    async fn test_connection(&self, _cfg: &ConnConfig, _password: Option<&str>) -> Result<()> {
        todo!("wire sqlx MySql pool")
    }

    async fn list_schemas(
        &self,
        _cfg: &ConnConfig,
        _password: Option<&str>,
    ) -> Result<Vec<Schema>> {
        todo!("query information_schema.tables")
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
