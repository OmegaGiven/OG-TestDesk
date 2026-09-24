//! Security audit: does a failed DB connection's error text ever leak
//! the (correct or attempted) password? core::drivers::pool caches
//! pools keyed by the full connection URL (password embedded), and
//! errors bubble up through sqlx via `anyhow`'s `?` conversion, whose
//! source is sqlx::Error's own Display/Debug — if that ever echoes the
//! connection string back (some drivers' I/O errors do, for other
//! languages/libraries), the password would land in the UI and in the
//! persisted error_log table.
//!
//! Needs a real Postgres/MySQL reachable — not run by default (`cargo
//! test` skips #[ignore]'d tests). Run explicitly once you have one:
//!   docker run -d --rm -e POSTGRES_PASSWORD=RealSecretPass123 -p 55432:5432 postgres:16-alpine
//!   cargo test -p og_testdesk_core --test password_leak_audit -- --ignored --nocapture

use og_testdesk_core::{driver_for, ConnConfig, DbKind, QueryOpts};

fn pg_config(port: u16) -> ConnConfig {
    ConnConfig {
        id: "audit-pg".into(),
        nickname: "audit".into(),
        kind: DbKind::Postgres,
        host: Some("127.0.0.1".into()),
        port: Some(port),
        database: Some("postgres".into()),
        user: Some("postgres".into()),
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

fn mysql_config(port: u16) -> ConnConfig {
    ConnConfig {
        id: "audit-mysql".into(),
        nickname: "audit".into(),
        kind: DbKind::MySql,
        host: Some("127.0.0.1".into()),
        port: Some(port),
        database: Some("mysql".into()),
        user: Some("root".into()),
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

const WRONG_PASSWORD: &str = "TotallyWrongGuess456";
const RIGHT_PASSWORD: &str = "RealSecretPass123";

fn assert_no_leak(err_text: &str, label: &str) {
    assert!(
        !err_text.contains(WRONG_PASSWORD),
        "{label}: error text leaked the attempted password! {err_text}"
    );
    assert!(
        !err_text.contains(RIGHT_PASSWORD),
        "{label}: error text leaked the real password! {err_text}"
    );
}

#[tokio::test]
#[ignore]
async fn wrong_password_does_not_leak_in_test_connection_error() {
    let cfg = pg_config(55432);
    let err = driver_for(DbKind::Postgres)
        .test_connection(&cfg, Some(WRONG_PASSWORD))
        .await
        .expect_err("wrong password should fail to connect");
    let display = format!("{err}");
    let debug = format!("{err:?}");
    println!("DISPLAY: {display}");
    println!("DEBUG: {debug}");
    assert_no_leak(&display, "Display");
    assert_no_leak(&debug, "Debug");
}

#[tokio::test]
#[ignore]
async fn wrong_password_does_not_leak_in_run_query_error() {
    let cfg = pg_config(55432);
    let err = driver_for(DbKind::Postgres)
        .run_query(&cfg, Some(WRONG_PASSWORD), "SELECT 1", QueryOpts::full())
        .await
        .expect_err("wrong password should fail to connect");
    let display = format!("{err}");
    let debug = format!("{err:?}");
    println!("DISPLAY: {display}");
    println!("DEBUG: {debug}");
    assert_no_leak(&display, "Display");
    assert_no_leak(&debug, "Debug");
}

#[tokio::test]
#[ignore]
async fn wrong_password_does_not_leak_in_mysql_test_connection_error() {
    let cfg = mysql_config(53306);
    let err = driver_for(DbKind::MySql)
        .test_connection(&cfg, Some(WRONG_PASSWORD))
        .await
        .expect_err("wrong password should fail to connect");
    let display = format!("{err}");
    let debug = format!("{err:?}");
    println!("DISPLAY: {display}");
    println!("DEBUG: {debug}");
    assert_no_leak(&display, "Display");
    assert_no_leak(&debug, "Debug");
}

#[tokio::test]
#[ignore]
async fn unreachable_host_does_not_leak_password_in_error() {
    // A connection-refused/timeout path is a structurally different
    // sqlx error variant (Io, not Database) — check it separately.
    let mut cfg = pg_config(1); // nothing listens on port 1
    cfg.host = Some("127.0.0.1".into());
    let err = driver_for(DbKind::Postgres)
        .test_connection(&cfg, Some(RIGHT_PASSWORD))
        .await
        .expect_err("unreachable host should fail to connect");
    let display = format!("{err}");
    let debug = format!("{err:?}");
    println!("DISPLAY: {display}");
    println!("DEBUG: {debug}");
    assert_no_leak(&display, "Display");
    assert_no_leak(&debug, "Debug");
}
