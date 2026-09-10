use super::decode::sqlite_value;
use super::pool::sqlite_pool;
use super::{
    stmt_returns_rows, Column, ConnConfig, DbDriver, DbKind, QueryColumn, QueryResult, Relation,
    RelationKind, Schema, ServerInfo,
};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use sqlx::{Column as _, Row, TypeInfo};
use std::time::Instant;

pub struct SqliteDriverImpl;

fn path_of(cfg: &ConnConfig) -> Result<&str> {
    cfg.file_path
        .as_deref()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| anyhow!("SQLite connection has no file path"))
}

#[async_trait]
impl DbDriver for SqliteDriverImpl {
    async fn test_connection(&self, cfg: &ConnConfig, _password: Option<&str>) -> Result<ServerInfo> {
        let pool = sqlite_pool(path_of(cfg)?)
            .await
            .context("opening SQLite database")?;
        let version: String = sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&pool)
            .await?;
        Ok(ServerInfo {
            kind: DbKind::Sqlite,
            version,
        })
    }

    async fn list_schemas(&self, cfg: &ConnConfig, _password: Option<&str>) -> Result<Vec<Schema>> {
        let pool = sqlite_pool(path_of(cfg)?).await?;
        let rows = sqlx::query(
            "SELECT name, type FROM sqlite_master
             WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )
        .fetch_all(&pool)
        .await?;

        let relations = rows
            .into_iter()
            .map(|r| {
                let ttype: String = r.get("type");
                Relation {
                    name: r.get("name"),
                    kind: if ttype == "view" {
                        RelationKind::View
                    } else {
                        RelationKind::Table
                    },
                }
            })
            .collect();

        Ok(vec![Schema {
            name: "main".to_string(),
            relations,
        }])
    }

    async fn list_columns(
        &self,
        cfg: &ConnConfig,
        _password: Option<&str>,
        _schema: &str,
        relation: &str,
    ) -> Result<Vec<Column>> {
        let pool = sqlite_pool(path_of(cfg)?).await?;
        // table_info() is safe here — identifier comes from our own schema list.
        let sql = format!("PRAGMA table_info(\"{}\")", relation.replace('"', "\"\""));
        let rows = sqlx::query(&sql).fetch_all(&pool).await?;
        Ok(rows
            .into_iter()
            .map(|r| Column {
                name: r.get("name"),
                data_type: r.get::<Option<String>, _>("type").unwrap_or_default(),
                nullable: r.get::<i64, _>("notnull") == 0,
                primary_key: r.get::<i64, _>("pk") > 0,
                default: r.get::<Option<String>, _>("dflt_value"),
            })
            .collect())
    }

    async fn run_query(
        &self,
        cfg: &ConnConfig,
        _password: Option<&str>,
        sql: &str,
    ) -> Result<QueryResult> {
        let pool = sqlite_pool(path_of(cfg)?).await?;
        let start = Instant::now();

        if stmt_returns_rows(sql) {
            let rows = sqlx::query(sql).fetch_all(&pool).await?;
            let duration_ms = start.elapsed().as_millis() as u64;
            let columns: Vec<QueryColumn> = rows
                .first()
                .map(|r| {
                    r.columns()
                        .iter()
                        .map(|c| QueryColumn {
                            name: c.name().to_string(),
                            type_name: c.type_info().name().to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let data: Vec<Vec<serde_json::Value>> = rows
                .iter()
                .map(|row| (0..row.len()).map(|i| sqlite_value(row, i)).collect())
                .collect();
            Ok(QueryResult {
                row_count: data.len(),
                rows_affected: 0,
                columns,
                rows: data,
                duration_ms,
                is_select: true,
            })
        } else {
            let res = sqlx::query(sql).execute(&pool).await?;
            Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                row_count: 0,
                rows_affected: res.rows_affected(),
                duration_ms: start.elapsed().as_millis() as u64,
                is_select: false,
            })
        }
    }
}
