use super::decode::my_value;
use super::pool::mysql_pool;
use super::{
    stmt_returns_rows, Column, ConnConfig, DbDriver, DbKind, QueryColumn, QueryResult, Relation,
    RelationKind, Schema, ServerInfo,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::{Column as _, Row, TypeInfo};
use std::time::Instant;

pub struct MySqlDriverImpl;

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn conn_url(cfg: &ConnConfig, password: Option<&str>) -> String {
    let host = cfg.host.as_deref().unwrap_or("localhost");
    let port = cfg.port.unwrap_or(3306);
    let user = cfg.user.as_deref().unwrap_or("root");
    let db = cfg.database.as_deref().unwrap_or("");
    let mut url = String::from("mysql://");
    url.push_str(&urlencode(user));
    if let Some(pw) = password {
        if !pw.is_empty() {
            url.push(':');
            url.push_str(&urlencode(pw));
        }
    }
    url.push_str(&format!("@{host}:{port}/{}", urlencode(db)));
    if cfg.use_tls {
        url.push_str("?ssl-mode=REQUIRED");
    }
    url
}

#[async_trait]
impl DbDriver for MySqlDriverImpl {
    async fn test_connection(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<ServerInfo> {
        let pool = mysql_pool(&conn_url(cfg, password))
            .await
            .context("connecting to MySQL")?;
        let version: String = sqlx::query_scalar("SELECT VERSION()").fetch_one(&pool).await?;
        Ok(ServerInfo {
            kind: DbKind::MySql,
            version,
        })
    }

    async fn list_schemas(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<Schema>> {
        let pool = mysql_pool(&conn_url(cfg, password)).await?;
        let rows = sqlx::query(
            r#"
            SELECT table_schema, table_name, table_type
            FROM information_schema.tables
            WHERE table_schema NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys')
            ORDER BY table_schema, table_name
            "#,
        )
        .fetch_all(&pool)
        .await?;

        let mut schemas: Vec<Schema> = Vec::new();
        for row in rows {
            let schema: String = row.get("table_schema");
            let name: String = row.get("table_name");
            let ttype: String = row.get("table_type");
            let kind = if ttype.eq_ignore_ascii_case("VIEW") {
                RelationKind::View
            } else {
                RelationKind::Table
            };
            match schemas.last_mut() {
                Some(s) if s.name == schema => s.relations.push(Relation { name, kind }),
                _ => schemas.push(Schema {
                    name: schema,
                    relations: vec![Relation { name, kind }],
                }),
            }
        }
        Ok(schemas)
    }

    async fn list_columns(
        &self,
        cfg: &ConnConfig,
        password: Option<&str>,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<Column>> {
        let pool = mysql_pool(&conn_url(cfg, password)).await?;
        let rows = sqlx::query(
            r#"
            SELECT column_name, column_type, is_nullable, column_default, column_key
            FROM information_schema.columns
            WHERE table_schema = ? AND table_name = ?
            ORDER BY ordinal_position
            "#,
        )
        .bind(schema)
        .bind(relation)
        .fetch_all(&pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Column {
                name: r.get("column_name"),
                data_type: r.get("column_type"),
                nullable: r.get::<String, _>("is_nullable").eq_ignore_ascii_case("YES"),
                primary_key: r.get::<String, _>("column_key") == "PRI",
                default: r.try_get("column_default").ok(),
            })
            .collect())
    }

    async fn run_query(
        &self,
        cfg: &ConnConfig,
        password: Option<&str>,
        sql: &str,
    ) -> Result<QueryResult> {
        let pool = mysql_pool(&conn_url(cfg, password)).await?;
        let start = Instant::now();

        if stmt_returns_rows(sql) {
            use futures_util::TryStreamExt;
            let cap = super::max_rows();
            let mut stream = sqlx::query(sql).fetch(&pool);
            let mut columns: Vec<QueryColumn> = Vec::new();
            let mut data: Vec<Vec<serde_json::Value>> = Vec::new();
            let mut truncated = false;
            while let Some(row) = stream.try_next().await? {
                if columns.is_empty() {
                    columns = row
                        .columns()
                        .iter()
                        .map(|c| QueryColumn {
                            name: c.name().to_string(),
                            type_name: c.type_info().name().to_string(),
                        })
                        .collect();
                }
                if data.len() >= cap {
                    truncated = true;
                    break;
                }
                data.push((0..row.len()).map(|i| my_value(&row, i)).collect());
            }
            Ok(QueryResult {
                row_count: data.len(),
                rows_affected: 0,
                columns,
                rows: data,
                duration_ms: start.elapsed().as_millis() as u64,
                is_select: true,
                truncated,
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
                truncated: false,
            })
        }
    }
}
