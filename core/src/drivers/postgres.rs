use super::decode::pg_value;
use super::pool::pg_pool;
use super::{
    stmt_returns_rows, Column, ConnConfig, DbDriver, DbKind, QueryColumn, QueryResult, Relation,
    RelationKind, Schema, ServerInfo,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::{Column as _, Row, TypeInfo};
use std::time::Instant;

pub struct PostgresDriverImpl;

fn conn_url(cfg: &ConnConfig, password: Option<&str>) -> String {
    let host = cfg.host.as_deref().unwrap_or("localhost");
    let port = cfg.port.unwrap_or(5432);
    let user = cfg.user.as_deref().unwrap_or("postgres");
    let db = cfg.database.as_deref().unwrap_or("postgres");
    let mut url = String::from("postgres://");
    url.push_str(&urlencode(user));
    if let Some(pw) = password {
        if !pw.is_empty() {
            url.push(':');
            url.push_str(&urlencode(pw));
        }
    }
    url.push_str(&format!("@{host}:{port}/{}", urlencode(db)));
    url.push_str(if cfg.use_tls {
        "?sslmode=require"
    } else {
        "?sslmode=prefer"
    });
    url
}

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

#[async_trait]
impl DbDriver for PostgresDriverImpl {
    async fn test_connection(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<ServerInfo> {
        let pool = pg_pool(&conn_url(cfg, password))
            .await
            .context("connecting to Postgres")?;
        let version: String = sqlx::query_scalar("SELECT version()").fetch_one(&pool).await?;
        Ok(ServerInfo {
            kind: DbKind::Postgres,
            version,
        })
    }

    async fn list_schemas(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<Schema>> {
        let pool = pg_pool(&conn_url(cfg, password)).await?;
        let rows = sqlx::query(
            r#"
            SELECT table_schema, table_name, table_type
            FROM information_schema.tables
            WHERE table_schema NOT IN ('pg_catalog', 'information_schema')
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
        let pool = pg_pool(&conn_url(cfg, password)).await?;
        let rows = sqlx::query(
            r#"
            SELECT
                c.column_name,
                c.data_type,
                c.is_nullable,
                c.column_default,
                COALESCE(pk.is_pk, false) AS is_pk
            FROM information_schema.columns c
            LEFT JOIN (
                SELECT kcu.column_name, true AS is_pk
                FROM information_schema.table_constraints tc
                JOIN information_schema.key_column_usage kcu
                  ON kcu.constraint_name = tc.constraint_name
                 AND kcu.table_schema = tc.table_schema
                WHERE tc.constraint_type = 'PRIMARY KEY'
                  AND tc.table_schema = $1 AND tc.table_name = $2
            ) pk ON pk.column_name = c.column_name
            WHERE c.table_schema = $1 AND c.table_name = $2
            ORDER BY c.ordinal_position
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
                data_type: r.get("data_type"),
                nullable: r.get::<String, _>("is_nullable").eq_ignore_ascii_case("YES"),
                primary_key: r.get("is_pk"),
                default: r.get("column_default"),
            })
            .collect())
    }

    async fn run_query(
        &self,
        cfg: &ConnConfig,
        password: Option<&str>,
        sql: &str,
    ) -> Result<QueryResult> {
        let pool = pg_pool(&conn_url(cfg, password)).await?;
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
                data.push((0..row.len()).map(|i| pg_value(&row, i)).collect());
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
