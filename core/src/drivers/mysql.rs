use super::decode::my_value;
use super::pool::mysql_pool;
use super::{
    run_query_body, Column, ConnConfig, DbDriver, DbKind, DbTime, ForeignKey, QueryOpts,
    QueryResult, Relation, RelationKind, Schema, ServerInfo, SqlFunction,
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

async fn conn_url(cfg: &ConnConfig, password: Option<&str>) -> Result<String> {
    let (host, port) = super::tunnel::effective_host_port(cfg).await?;
    let port = if port == 0 { 3306 } else { port };
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
    Ok(url)
}

#[async_trait]
impl DbDriver for MySqlDriverImpl {
    async fn test_connection(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<ServerInfo> {
        let url = conn_url(cfg, password).await?;
        let pool = mysql_pool(&url).await.context("connecting to MySQL")?;
        let version: String = sqlx::query_scalar("SELECT VERSION()").fetch_one(&pool).await?;
        Ok(ServerInfo {
            kind: DbKind::MySql,
            version,
        })
    }

    // MySQL 8 returns information_schema columns as VARBINARY with UPPERCASE
    // names (5.7/MariaDB: VARCHAR, lowercase). sqlx won't decode VARBINARY
    // into String and row.get("name") panics on a name miss, so every
    // information_schema column is CAST to CHAR under an explicit alias.
    async fn list_schemas(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<Schema>> {
        let url = conn_url(cfg, password).await?;
        let pool = mysql_pool(&url).await?;
        let rows = sqlx::query(
            r#"
            SELECT CAST(table_schema AS CHAR) AS table_schema, CAST(table_name AS CHAR) AS table_name, CAST(table_type AS CHAR) AS table_type
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
        let url = conn_url(cfg, password).await?;
        let pool = mysql_pool(&url).await?;
        let rows = sqlx::query(
            r#"
            SELECT CAST(column_name AS CHAR) AS column_name, CAST(column_type AS CHAR) AS column_type, CAST(is_nullable AS CHAR) AS is_nullable,
                   CAST(column_default AS CHAR) AS column_default, CAST(column_key AS CHAR) AS column_key
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
            cfg,
            mysql_pool(&conn_url(cfg, password).await?).await?,
            sql,
            opts,
            my_value
        )
    }

    async fn list_foreign_keys(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<ForeignKey>> {
        let url = conn_url(cfg, password).await?;
        let pool = mysql_pool(&url).await?;
        let rows = sqlx::query(
            r#"
            SELECT CAST(table_schema AS CHAR) AS table_schema, CAST(table_name AS CHAR) AS table_name, CAST(column_name AS CHAR) AS column_name,
                   CAST(referenced_table_schema AS CHAR) AS referenced_table_schema,
                   CAST(referenced_table_name AS CHAR) AS referenced_table_name,
                   CAST(referenced_column_name AS CHAR) AS referenced_column_name
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
        let url = conn_url(cfg, password).await?;
        let pool = mysql_pool(&url).await?;
        let rows = sqlx::query(
            r#"
            SELECT
                CAST(r.ROUTINE_SCHEMA AS CHAR) AS schema_name,
                CAST(r.ROUTINE_NAME AS CHAR) AS name,
                CAST(LOWER(r.ROUTINE_TYPE) AS CHAR) AS kind,
                CAST(COALESCE((
                    SELECT GROUP_CONCAT(CONCAT(p.PARAMETER_NAME, ' ', p.DTD_IDENTIFIER)
                                         ORDER BY p.ORDINAL_POSITION SEPARATOR ', ')
                    FROM information_schema.parameters p
                    WHERE p.SPECIFIC_SCHEMA = r.ROUTINE_SCHEMA
                      AND p.SPECIFIC_NAME = r.ROUTINE_NAME
                      AND p.PARAMETER_MODE IS NOT NULL
                ), '') AS CHAR) AS arguments,
                CAST(r.DTD_IDENTIFIER AS CHAR) AS return_type
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

    async fn server_time(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<DbTime> {
        let url = conn_url(cfg, password).await?;
        let pool = mysql_pool(&url).await?;
        let row = sqlx::query(
            r#"
            SELECT
                DATE_FORMAT(NOW(), '%Y-%m-%d %H:%i:%s') AS local_time,
                @@session.time_zone AS tz_name,
                TIMESTAMPDIFF(SECOND, UTC_TIMESTAMP(), NOW()) AS utc_offset_secs
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
