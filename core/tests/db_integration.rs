//! End-to-end driver checks against real Postgres and MySQL servers —
//! the SQLite equivalent lives in smoke.rs and runs everywhere. These
//! need servers, so they're #[ignore]'d for plain `cargo test`; CI runs
//! them with service containers. Locally:
//!   docker run -d --rm -e POSTGRES_PASSWORD=RealSecretPass123 -p 55432:5432 postgres:16-alpine
//!   docker run -d --rm -e MYSQL_ROOT_PASSWORD=RealSecretPass123 -e MYSQL_DATABASE=ogtd -p 53306:3306 mysql:8
//!   cargo test -p og_testdesk_core --test db_integration -- --ignored

use og_testdesk_core::{driver_for, ConnConfig, DbKind, QueryOpts};
use serde_json::Value;

const PASSWORD: &str = "RealSecretPass123";

fn cfg(kind: DbKind, port: u16, database: &str, user: &str) -> ConnConfig {
    ConnConfig {
        id: format!("it-{}", uuid::Uuid::new_v4()),
        nickname: "it".into(),
        kind,
        host: Some("127.0.0.1".into()),
        port: Some(port),
        database: Some(database.into()),
        user: Some(user.into()),
        use_tls: false,
        read_only: false,
        pre_connect_cmd: None,
        ssh_host: None,
        ssh_port: None,
        ssh_user: None,
        ssh_key_path: None,
        file_path: None,
        color: None,
    }
}

fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap(),
        Value::String(s) => s.parse().unwrap_or_else(|_| panic!("not numeric: {v}")),
        other => panic!("not numeric: {other}"),
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_i64() == Some(1),
        other => panic!("not a boolean: {other}"),
    }
}

/// Same scenario for every engine; only the DDL dialect and schema name differ.
async fn exercise(cfg: ConnConfig, schema: &str, create_sql: &str, table: &str, create_fn: &str) {
    let drv = driver_for(cfg.kind);
    let pw = Some(PASSWORD);
    let full = QueryOpts::full();

    let info = drv.test_connection(&cfg, pw).await.expect("connect");
    assert_eq!(info.kind, cfg.kind);

    drv.run_query(&cfg, pw, &format!("DROP TABLE IF EXISTS {table}"), full)
        .await
        .unwrap();
    drv.run_query(&cfg, pw, create_sql, full)
        .await
        .expect("create table");

    let ins = drv
        .run_query(
            &cfg,
            pw,
            &format!(
                "INSERT INTO {table} (id, name, price, active, meta, note) VALUES \
                 (1, 'Keyboard', 79.99, true, '{{\"sku\":\"K-1\"}}', NULL), \
                 (2, 'Monitor', 124.50, false, '{{\"sku\":\"M-2\"}}', 'returned'), \
                 (3, 'Mouse', 9.99, true, '{{\"sku\":\"S-3\"}}', NULL)"
            ),
            full,
        )
        .await
        .expect("insert");
    assert_eq!(ins.rows_affected, 3);
    assert!(!ins.is_select);

    let page = drv
        .run_query(
            &cfg,
            pw,
            &format!(
                "SELECT id, name, price, active, created, meta, note FROM {table} ORDER BY id"
            ),
            QueryOpts::page(0, 2, true),
        )
        .await
        .expect("select page");
    assert!(page.is_select);
    assert_eq!(page.row_count, 2);
    assert_eq!(page.total, Some(3));
    assert!(page.has_more);
    let cols: Vec<&str> = page.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        cols,
        ["id", "name", "price", "active", "created", "meta", "note"]
    );

    let r0 = &page.rows[0];
    assert_eq!(num(&r0[0]), 1.0);
    assert_eq!(r0[1], Value::from("Keyboard"));
    assert!(
        (num(&r0[2]) - 79.99).abs() < 1e-9,
        "decimal decoded as {}",
        r0[2]
    );
    assert!(truthy(&r0[3]));
    assert!(
        matches!(&r0[4], Value::String(s) if !s.is_empty()),
        "timestamp decoded as {}",
        r0[4]
    );
    let meta = match &r0[5] {
        Value::String(s) => serde_json::from_str::<Value>(s).unwrap(),
        v => v.clone(),
    };
    assert_eq!(meta["sku"], Value::from("K-1"));
    assert_eq!(r0[6], Value::Null);
    assert!(!truthy(&page.rows[1][3]));

    let second = drv
        .run_query(
            &cfg,
            pw,
            &format!("SELECT id FROM {table} ORDER BY id"),
            QueryOpts::page(1, 2, false),
        )
        .await
        .unwrap();
    assert_eq!(second.row_count, 1);
    assert_eq!(num(&second.rows[0][0]), 3.0);

    let upd = drv
        .run_query(
            &cfg,
            pw,
            &format!("UPDATE {table} SET note = 'restocked' WHERE id = 3"),
            full,
        )
        .await
        .unwrap();
    assert_eq!(upd.rows_affected, 1);

    let schemas = drv.list_schemas(&cfg, pw).await.expect("list schemas");
    let s = schemas
        .iter()
        .find(|s| s.name == schema)
        .unwrap_or_else(|| {
            panic!(
                "schema {schema} missing from {:?}",
                schemas.iter().map(|s| &s.name).collect::<Vec<_>>()
            )
        });
    assert!(
        s.relations.iter().any(|r| r.name == table),
        "{table} missing from schema tree"
    );

    let cols = drv
        .list_columns(&cfg, pw, schema, table)
        .await
        .expect("list columns");
    assert!(
        cols.iter()
            .find(|c| c.name == "id")
            .expect("id column")
            .primary_key
    );
    assert_eq!(cols.len(), 7);

    // Foreign keys + functions feed the FK picker and the Functions tab.
    let child = format!("{table}_child");
    drv.run_query(&cfg, pw, &format!("DROP TABLE IF EXISTS {child}"), full)
        .await
        .unwrap();
    drv.run_query(
        &cfg,
        pw,
        &format!("CREATE TABLE {child} (id INT PRIMARY KEY, widget_id INT, FOREIGN KEY (widget_id) REFERENCES {table}(id))"),
        full,
    )
    .await
    .expect("create child table");
    let fks = drv
        .list_foreign_keys(&cfg, pw)
        .await
        .expect("list foreign keys");
    assert!(
        fks.iter().any(|f| f.table == child
            && f.column == "widget_id"
            && f.ref_table == table
            && f.ref_column == "id"),
        "FK {child}.widget_id -> {table}.id missing from {fks:?}"
    );

    let func = format!("{table}_double");
    setup_exec(&cfg, &create_fn.replace("{fn}", &func)).await;
    let funcs = drv.list_functions(&cfg, pw).await.expect("list functions");
    let f = funcs
        .iter()
        .find(|f| f.name == func)
        .unwrap_or_else(|| panic!("{func} missing from functions list"));
    assert!(
        f.arguments.contains('x'),
        "arguments decoded as {:?}",
        f.arguments
    );
    setup_exec(&cfg, &format!("DROP FUNCTION {func}")).await;
    drv.run_query(&cfg, pw, &format!("DROP TABLE {child}"), full)
        .await
        .unwrap();

    drv.server_time(&cfg, pw).await.expect("server time");

    let mut ro = cfg.clone();
    ro.read_only = true;
    let err = drv
        .run_query(&ro, pw, &format!("DELETE FROM {table}"), full)
        .await
        .expect_err("read-only connection must block writes");
    assert!(
        err.to_string().contains("read-only"),
        "unexpected error: {err}"
    );
    assert_eq!(
        drv.run_query(&ro, pw, &format!("SELECT * FROM {table}"), full)
            .await
            .unwrap()
            .row_count,
        3
    );

    drv.run_query(&cfg, pw, &format!("DROP TABLE {table}"), full)
        .await
        .unwrap();
}

/// Fixture DDL over the plain text protocol. The app's own run_query uses
/// prepared statements, which MySQL refuses for CREATE FUNCTION (error 1295).
async fn setup_exec(cfg: &ConnConfig, sql: &str) {
    let host = cfg.host.as_deref().unwrap();
    let (port, db, user) = (
        cfg.port.unwrap(),
        cfg.database.as_deref().unwrap(),
        cfg.user.as_deref().unwrap(),
    );
    match cfg.kind {
        DbKind::Postgres => {
            let pool =
                sqlx::PgPool::connect(&format!("postgres://{user}:{PASSWORD}@{host}:{port}/{db}"))
                    .await
                    .unwrap();
            sqlx::raw_sql(sql)
                .execute(&pool)
                .await
                .expect("fixture sql");
        }
        DbKind::MySql => {
            let pool =
                sqlx::MySqlPool::connect(&format!("mysql://{user}:{PASSWORD}@{host}:{port}/{db}"))
                    .await
                    .unwrap();
            sqlx::raw_sql(sql)
                .execute(&pool)
                .await
                .expect("fixture sql");
        }
        DbKind::Sqlite => unreachable!(),
    }
}

fn table_name() -> String {
    format!(
        "it_widgets_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    )
}

#[tokio::test]
#[ignore = "needs Postgres on 127.0.0.1:55432"]
async fn postgres_end_to_end() {
    let t = table_name();
    let ddl = format!(
        "CREATE TABLE {t} (id INTEGER PRIMARY KEY, name TEXT NOT NULL, price NUMERIC(10,2), \
         active BOOLEAN, created TIMESTAMPTZ NOT NULL DEFAULT now(), meta JSONB, note TEXT)"
    );
    let func = "CREATE FUNCTION {fn}(x integer) RETURNS integer LANGUAGE sql AS 'SELECT x * 2'";
    exercise(
        cfg(DbKind::Postgres, 55432, "postgres", "postgres"),
        "public",
        &ddl,
        &t,
        func,
    )
    .await;
}

#[tokio::test]
#[ignore = "needs MySQL on 127.0.0.1:53306"]
async fn mysql_end_to_end() {
    let t = table_name();
    let ddl = format!(
        "CREATE TABLE {t} (id INT PRIMARY KEY, name VARCHAR(100) NOT NULL, price DECIMAL(10,2), \
         active BOOLEAN, created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, meta JSON, note TEXT)"
    );
    let func = "CREATE FUNCTION {fn}(x INT) RETURNS INT DETERMINISTIC RETURN x * 2";
    exercise(
        cfg(DbKind::MySql, 53306, "ogtd", "root"),
        "ogtd",
        &ddl,
        &t,
        func,
    )
    .await;
}
