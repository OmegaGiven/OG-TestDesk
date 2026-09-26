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
    drv.run_query(&cfg, pw, &create_fn.replace("{fn}", &func), full)
        .await
        .expect("create function via the editor path");
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
    drv.run_query(&cfg, pw, &format!("DROP FUNCTION {func}"), full)
        .await
        .expect("drop function via the editor path");
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
    postgres_routines_via_editor().await;
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
    mysql_routines_via_editor().await;
}

/// Runs `sql` the way the SQL editor does (one run_query call per Run) and
/// returns the first cell of the first row, if any.
async fn scalar(cfg: &ConnConfig, sql: &str) -> Value {
    let res = driver_for(cfg.kind)
        .run_query(cfg, Some(PASSWORD), sql, QueryOpts::full())
        .await
        .unwrap_or_else(|e| panic!("{sql}\n-> {e}"));
    res.rows
        .first()
        .and_then(|r| r.first())
        .cloned()
        .unwrap_or(Value::Null)
}

/// Stored routines typed straight into the editor. Called from the
/// end_to_end tests rather than being their own #[tokio::test]: the pool
/// cache is global and keyed by URL, so two tests (= two runtimes) on the
/// same server would share connections from a runtime that shut down.
///: bodies full of `;`,
/// no DELIMITER (that's a client-side directive the splitter strips).
async fn mysql_routines_via_editor() {
    let c = cfg(DbKind::MySql, 53306, "ogtd", "root");
    let t = table_name();
    for sql in [
        format!("CREATE TABLE {t} (id INT PRIMARY KEY AUTO_INCREMENT, qty INT, audit INT DEFAULT 0)"),
        format!(
            "CREATE PROCEDURE {t}_add(IN q INT)\nBEGIN\n  INSERT INTO {t} (qty) VALUES (q);\n  INSERT INTO {t} (qty) VALUES (q * 10);\nEND"
        ),
        format!(
            "CREATE TRIGGER {t}_bi BEFORE INSERT ON {t} FOR EACH ROW\nBEGIN\n  SET NEW.audit = NEW.qty + 1;\nEND"
        ),
        format!(
            "CREATE FUNCTION {t}_triple(x INT) RETURNS INT DETERMINISTIC\nBEGIN\n  DECLARE r INT;\n  SET r = x * 3;\n  RETURN r;\nEND"
        ),
        format!("CALL {t}_add(2)"),
    ] {
        scalar(&c, &sql).await;
    }
    assert_eq!(
        num(&scalar(&c, &format!("SELECT COUNT(*) FROM {t}")).await),
        2.0
    );
    assert_eq!(
        num(&scalar(&c, &format!("SELECT audit FROM {t} WHERE qty = 20")).await),
        21.0
    );
    assert_eq!(
        num(&scalar(&c, &format!("SELECT {t}_triple(4)")).await),
        12.0
    );
    for sql in [
        format!("DROP TRIGGER {t}_bi"),
        format!("DROP PROCEDURE {t}_add"),
        format!("DROP FUNCTION {t}_triple"),
        format!("DROP TABLE {t}"),
    ] {
        scalar(&c, &sql).await;
    }
}

async fn postgres_routines_via_editor() {
    let c = cfg(DbKind::Postgres, 55432, "postgres", "postgres");
    let t = table_name();
    for sql in [
        format!("CREATE TABLE {t} (id SERIAL PRIMARY KEY, qty INT, audit INT DEFAULT 0)"),
        format!(
            "CREATE FUNCTION {t}_triple(x integer) RETURNS integer LANGUAGE plpgsql AS $$\nDECLARE r integer;\nBEGIN\n  r := x * 3;\n  RETURN r;\nEND\n$$"
        ),
        format!(
            "CREATE PROCEDURE {t}_add(q integer) LANGUAGE plpgsql AS $body$\nBEGIN\n  INSERT INTO {t} (qty) VALUES (q);\n  INSERT INTO {t} (qty) VALUES (q * 10);\nEND\n$body$"
        ),
        format!(
            "CREATE FUNCTION {t}_audit() RETURNS trigger LANGUAGE plpgsql AS $$\nBEGIN\n  NEW.audit := NEW.qty + 1;\n  RETURN NEW;\nEND\n$$"
        ),
        format!("CREATE TRIGGER {t}_bi BEFORE INSERT ON {t} FOR EACH ROW EXECUTE FUNCTION {t}_audit()"),
        format!("CALL {t}_add(2)"),
    ] {
        scalar(&c, &sql).await;
    }
    assert_eq!(
        num(&scalar(&c, &format!("SELECT COUNT(*) FROM {t}")).await),
        2.0
    );
    assert_eq!(
        num(&scalar(&c, &format!("SELECT audit FROM {t} WHERE qty = 20")).await),
        21.0
    );
    assert_eq!(
        num(&scalar(&c, &format!("SELECT {t}_triple(4)")).await),
        12.0
    );

    // Several non-row statements in one Run now go through together.
    let multi = driver_for(c.kind)
        .run_query(
            &c,
            Some(PASSWORD),
            &format!("INSERT INTO {t} (qty) VALUES (7); INSERT INTO {t} (qty) VALUES (8)"),
            QueryOpts::full(),
        )
        .await
        .expect("multi-statement run");
    assert_eq!(multi.rows_affected, 2);

    for sql in [
        format!("DROP TABLE {t}"),
        format!("DROP FUNCTION {t}_audit"),
        format!("DROP PROCEDURE {t}_add"),
        format!("DROP FUNCTION {t}_triple"),
    ] {
        scalar(&c, &sql).await;
    }
}
