use super::decode::sqlite_value;
use super::pool::sqlite_pool;
use super::{
    run_query_body, Column, ConnConfig, DbDriver, DbKind, ForeignKey, QueryOpts, QueryResult,
    Relation, RelationKind, Schema, ServerInfo, SqlFunction,
};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use sqlx::Row;

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
        opts: QueryOpts,
    ) -> Result<QueryResult> {
        run_query_body!(sqlite_pool(path_of(cfg)?).await?, sql, opts, sqlite_value)
    }

    async fn list_foreign_keys(&self, cfg: &ConnConfig, _password: Option<&str>) -> Result<Vec<ForeignKey>> {
        let pool = sqlite_pool(path_of(cfg)?).await?;
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_all(&pool)
        .await?;

        let mut out = Vec::new();
        for table in tables {
            // table_info()-safe: identifier comes from our own schema list.
            let sql = format!("PRAGMA foreign_key_list(\"{}\")", table.replace('"', "\"\""));
            let rows = sqlx::query(&sql).fetch_all(&pool).await?;
            for r in rows {
                let ref_table: String = r.get("table");
                let column: String = r.get("from");
                let ref_column: Option<String> = r.get("to");
                out.push(ForeignKey {
                    schema: "main".to_string(),
                    table: table.clone(),
                    column,
                    ref_schema: "main".to_string(),
                    ref_table,
                    ref_column: ref_column.unwrap_or_default(),
                });
            }
        }
        Ok(out)
    }

    async fn list_functions(&self, _cfg: &ConnConfig, _password: Option<&str>) -> Result<Vec<SqlFunction>> {
        // SQLite has no CREATE FUNCTION / catalog of user-defined functions
        // to introspect — scalar functions are loaded as native extensions,
        // not SQL objects. Nothing to list.
        Ok(Vec::new())
    }
}
