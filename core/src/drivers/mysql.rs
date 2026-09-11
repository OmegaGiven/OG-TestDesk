use super::decode::my_value;
use super::pool::mysql_pool;
use super::{
    run_query_body, Column, ConnConfig, DbDriver, DbKind, ForeignKey, QueryOpts, QueryResult,
    Relation, RelationKind, Schema, ServerInfo, SqlFunction,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::Row;

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
        opts: QueryOpts,
    ) -> Result<QueryResult> {
        run_query_body!(
            mysql_pool(&conn_url(cfg, password)).await?,
            sql,
            opts,
            my_value
        )
    }

    async fn list_foreign_keys(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<ForeignKey>> {
        let pool = mysql_pool(&conn_url(cfg, password)).await?;
        let rows = sqlx::query(
            r#"
            SELECT table_schema, table_name, column_name,
                   referenced_table_schema, referenced_table_name, referenced_column_name
            FROM information_schema.key_column_usage
            WHERE referenced_table_name IS NOT NULL
              AND table_schema NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys')
            ORDER BY table_schema, table_name, column_name
            "#,
        )
        .fetch_all(&pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ForeignKey {
                schema: r.get("table_schema"),
                table: r.get("table_name"),
                column: r.get("column_name"),
                ref_schema: r.get("referenced_table_schema"),
                ref_table: r.get("referenced_table_name"),
                ref_column: r.get("referenced_column_name"),
            })
            .collect())
    }

    async fn list_functions(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<SqlFunction>> {
        let pool = mysql_pool(&conn_url(cfg, password)).await?;
        let rows = sqlx::query(
            r#"
            SELECT
                r.ROUTINE_SCHEMA AS schema_name,
                r.ROUTINE_NAME AS name,
                LOWER(r.ROUTINE_TYPE) AS kind,
                COALESCE((
                    SELECT GROUP_CONCAT(CONCAT(p.PARAMETER_NAME, ' ', p.DTD_IDENTIFIER)
                                         ORDER BY p.ORDINAL_POSITION SEPARATOR ', ')
                    FROM information_schema.parameters p
                    WHERE p.SPECIFIC_SCHEMA = r.ROUTINE_SCHEMA
                      AND p.SPECIFIC_NAME = r.ROUTINE_NAME
                      AND p.PARAMETER_MODE IS NOT NULL
                ), '') AS arguments,
                r.DTD_IDENTIFIER AS return_type
            FROM information_schema.routines r
            WHERE r.ROUTINE_SCHEMA NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys')
            ORDER BY r.ROUTINE_SCHEMA, r.ROUTINE_NAME
            "#,
        )
        .fetch_all(&pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| SqlFunction {
                schema: r.get("schema_name"),
                name: r.get("name"),
                kind: r.get("kind"),
                arguments: r.get("arguments"),
                return_type: r.get("return_type"),
            })
            .collect())
    }
}
