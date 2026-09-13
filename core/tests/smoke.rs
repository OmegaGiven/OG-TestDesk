use og_testdesk_core::{driver_for, ConnConfig, DbKind, MetadataStore};

fn sqlite_cfg(path: &str) -> ConnConfig {
    ConnConfig {
        id: "test".into(),
        nickname: "test".into(),
        kind: DbKind::Sqlite,
        host: None,
        port: None,
        database: None,
        user: None,
        file_path: Some(path.into()),
        use_tls: false,
        color: None,
        read_only: false,
        pre_connect_cmd: None,
        ssh_host: None,
        ssh_port: None,
        ssh_user: None,
        ssh_key_path: None,
    }
}

#[tokio::test]
async fn sqlite_end_to_end() {
    let dir = std::env::temp_dir().join(format!("ogtd-{}.db", uuid::Uuid::new_v4()));
    let path = dir.to_string_lossy().to_string();
    let cfg = sqlite_cfg(&path);
    let drv = driver_for(DbKind::Sqlite);

    let info = drv.test_connection(&cfg, None).await.unwrap();
    assert_eq!(info.kind, DbKind::Sqlite);

    let opts = og_testdesk_core::QueryOpts::full();
    drv.run_query(
        &cfg,
        None,
        "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, score REAL, ok BOOLEAN)",
        opts,
    )
    .await
    .unwrap();
    let ins = drv
        .run_query(
            &cfg,
            None,
            "INSERT INTO t (name, score, ok) VALUES ('a', 1.5, 1), ('b', 2.0, 0)",
            opts,
        )
        .await
        .unwrap();
    assert_eq!(ins.rows_affected, 2);
    assert!(!ins.is_select);

    let res = drv
        .run_query(
            &cfg,
            None,
            "SELECT id, name, score, ok FROM t ORDER BY id",
            og_testdesk_core::QueryOpts::page(0, 100, true),
        )
        .await
        .unwrap();
    assert!(res.is_select);
    assert_eq!(res.total, Some(2));
    assert_eq!(res.row_count, 2);
    assert_eq!(res.columns.len(), 4);
    assert_eq!(res.rows[0][1], serde_json::json!("a"));
    assert_eq!(res.rows[1][2], serde_json::json!(2.0));

    let schemas = drv.list_schemas(&cfg, None).await.unwrap();
    assert!(schemas[0].relations.iter().any(|r| r.name == "t"));
    let cols = drv.list_columns(&cfg, None, "main", "t").await.unwrap();
    assert!(cols.iter().find(|c| c.name == "id").unwrap().primary_key);

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn metadata_crud() {
    let path = std::env::temp_dir().join(format!("ogtd-meta-{}.db", uuid::Uuid::new_v4()));
    let store = MetadataStore::open(&path.to_string_lossy()).await.unwrap();

    let mut cfg = sqlite_cfg("/tmp/x.db");
    cfg.id = uuid::Uuid::new_v4().to_string();
    cfg.nickname = "PROD".into();
    store.upsert_connection(&cfg).await.unwrap();
    let list = store.list_connections().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].nickname, "PROD");

    store.set_state("k", "v").await.unwrap();
    assert_eq!(store.get_state("k").await.unwrap().as_deref(), Some("v"));

    store.delete_connection(&cfg.id).await.unwrap();
    assert_eq!(store.list_connections().await.unwrap().len(), 0);

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn read_only_connection_blocks_writes_but_not_reads() {
    let dir = std::env::temp_dir().join(format!("ogtd-ro-{}.db", uuid::Uuid::new_v4()));
    let path = dir.to_string_lossy().to_string();
    let drv = driver_for(DbKind::Sqlite);

    // Set the table up while still writable.
    let mut cfg = sqlite_cfg(&path);
    drv.run_query(&cfg, None, "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)", og_testdesk_core::QueryOpts::full())
        .await
        .unwrap();

    cfg.read_only = true;
    let err = drv
        .run_query(&cfg, None, "INSERT INTO t (v) VALUES ('x')", og_testdesk_core::QueryOpts::full())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("read-only"), "unexpected error: {err}");

    // Reads still work on a read-only connection.
    let res = drv
        .run_query(&cfg, None, "SELECT * FROM t", og_testdesk_core::QueryOpts::full())
        .await
        .unwrap();
    assert!(res.is_select);
    assert_eq!(res.row_count, 0);

    let _ = std::fs::remove_file(&path);
}
