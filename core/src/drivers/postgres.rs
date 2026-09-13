use super::decode::pg_value;
use super::pool::pg_pool;
use super::{
    run_query_body, Column, ConnConfig, DbDriver, DbKind, DbTime, ForeignKey, QueryOpts,
    QueryResult, Relation, RelationKind, Schema, ServerInfo, SqlFunction,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::Row;

pub struct PostgresDriverImpl;

async fn conn_url(cfg: &ConnConfig, password: Option<&str>) -> Result<String> {
    let (host, port) = super::tunnel::effective_host_port(cfg).await?;
    let port = if port == 0 { 5432 } else { port };
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
    Ok(url)
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
        let url = conn_url(cfg, password).await?;
        let pool = pg_pool(&url).await.context("connecting to Postgres")?;
        let version: String = sqlx::query_scalar("SELECT version()").fetch_one(&pool).await?;
        Ok(ServerInfo {
            kind: DbKind::Postgres,
            version,
        })
    }

    async fn list_schemas(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<Schema>> {
        let url = conn_url(cfg, password).await?;
        let pool = pg_pool(&url).await?;
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
        let url = conn_url(cfg, password).await?;
        let pool = pg_pool(&url).await?;
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
        opts: QueryOpts,
    ) -> Result<QueryResult> {
        run_query_body!(cfg, pg_pool(&conn_url(cfg, password).await?).await?, sql, opts, pg_value)
    }

    async fn list_foreign_keys(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<ForeignKey>> {
        let url = conn_url(cfg, password).await?;
        let pool = pg_pool(&url).await?;
        let rows = sqlx::query(
            r#"
            SELECT
                tc.table_schema, tc.table_name, kcu.column_name,
                ccu.table_schema AS ref_schema, ccu.table_name AS ref_table, ccu.column_name AS ref_column
            FROM information_schema.table_constraints tc
            JOIN information_schema.key_column_usage kcu
              ON kcu.constraint_name = tc.constraint_name AND kcu.table_schema = tc.table_schema
            JOIN information_schema.constraint_column_usage ccu
              ON ccu.constraint_name = tc.constraint_name AND ccu.table_schema = tc.table_schema
            WHERE tc.constraint_type = 'FOREIGN KEY'
              AND tc.table_schema NOT IN ('pg_catalog', 'information_schema')
            ORDER BY tc.table_schema, tc.table_name, kcu.column_name
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
                ref_schema: r.get("ref_schema"),
                ref_table: r.get("ref_table"),
                ref_column: r.get("ref_column"),
            })
            .collect())
    }

    async fn list_functions(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<SqlFunction>> {
        let url = conn_url(cfg, password).await?;
        let pool = pg_pool(&url).await?;
        let rows = sqlx::query(
            r#"
            SELECT
                n.nspname AS schema,
                p.proname AS name,
                CASE p.prokind
                    WHEN 'p' THEN 'procedure'
                    WHEN 'a' THEN 'aggregate'
                    WHEN 'w' THEN 'window'
                    ELSE 'function'
                END AS kind,
                pg_get_function_arguments(p.oid) AS arguments,
                pg_get_function_result(p.oid) AS return_type
            FROM pg_proc p
            JOIN pg_namespace n ON n.oid = p.pronamespace
            WHERE n.nspname NOT IN ('pg_catalog', 'information_schema')
            ORDER BY n.nspname, p.proname
            "#,
        )
        .fetch_all(&pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| SqlFunction {
                schema: r.get("schema"),
                name: r.get("name"),
                kind: r.get("kind"),
                arguments: r.get::<Option<String>, _>("arguments").unwrap_or_default(),
                return_type: r.get("return_type"),
            })
            .collect())
    }

    async fn server_time(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<DbTime> {
        let url = conn_url(cfg, password).await?;
        let pool = pg_pool(&url).await?;
        let row = sqlx::query(
            r#"
            SELECT
                to_char(now(), 'YYYY-MM-DD HH24:MI:SS') AS local_time,
                current_setting('TIMEZONE') AS tz_name,
                EXTRACT(TIMEZONE FROM now())::bigint AS utc_offset_secs
            "#,
        )
        .fetch_one(&pool)
        .await?;
        Ok(DbTime {
            local_time: row.get("local_time"),
            tz_name: row.get("tz_name"),
            utc_offset_secs: row.get("utc_offset_secs"),
        })
    }
}
