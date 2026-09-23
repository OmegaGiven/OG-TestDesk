//! Broader MetadataStore coverage beyond the original smoke test:
//! saved-query folders (incl. the parent-cycle guard), saved charts
//! (incl. the "don't clobber cached data on a config-only save" COALESCE
//! behavior), schedules, request collections/tabs/saved requests,
//! environments, and both history tables.

use og_testdesk_core::{
    ConnConfig, DbKind, Environment, HistoryEntry, MetadataStore, MockRoute, RequestCollection,
    RequestHistoryEntry, RequestTab, SavedChart, SavedQuery, SavedQueryFolder, SavedRequest,
    Schedule,
};

async fn store() -> MetadataStore {
    let path = std::env::temp_dir().join(format!("ogtd-extra-{}.db", uuid::Uuid::new_v4()));
    MetadataStore::open(&path.to_string_lossy()).await.unwrap()
}

/// Several tables FK-reference connections(id) — seed one so those rows
/// can be inserted without it being the thing under test.
async fn seed_connection(db: &MetadataStore, id: &str) {
    db.upsert_connection(&ConnConfig {
        id: id.into(),
        nickname: id.into(),
        kind: DbKind::Sqlite,
        host: None,
        port: None,
        database: None,
        user: None,
        file_path: Some("/tmp/does-not-matter.db".into()),
        use_tls: false,
        color: None,
        read_only: false,
        pre_connect_cmd: None,
        ssh_host: None,
        ssh_port: None,
        ssh_user: None,
        ssh_key_path: None,
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn saved_query_folders_nest_and_reject_cycles() {
    let db = store().await;

    let root = SavedQueryFolder {
        id: "f-root".into(),
        name: "Reports".into(),
        parent_id: None,
        sort_order: 0,
    };
    db.upsert_saved_query_folder(&root).await.unwrap();

    let child = SavedQueryFolder {
        id: "f-child".into(),
        name: "Daily".into(),
        parent_id: Some("f-root".into()),
        sort_order: 0,
    };
    db.upsert_saved_query_folder(&child).await.unwrap();

    let folders = db.list_saved_query_folders().await.unwrap();
    assert_eq!(folders.len(), 2);

    // Moving root under its own descendant must be rejected.
    let cycle = SavedQueryFolder {
        id: "f-root".into(),
        name: "Reports".into(),
        parent_id: Some("f-child".into()),
        sort_order: 0,
    };
    assert!(db.upsert_saved_query_folder(&cycle).await.is_err());

    // A saved query in the child folder should show up with folder_id set.
    let q = SavedQuery {
        id: "q1".into(),
        connection_id: None,
        folder_id: Some("f-child".into()),
        name: "Top customers".into(),
        sql_text: "SELECT 1;".into(),
        sort_order: 0,
        created_at: 0,
    };
    db.upsert_saved_query(&q).await.unwrap();
    let queries = db.list_saved_queries().await.unwrap();
    assert_eq!(queries[0].folder_id.as_deref(), Some("f-child"));

    // Deleting the child folder cascades to the query's folder_id (ON
    // DELETE SET NULL), it doesn't delete the query itself.
    db.delete_saved_query_folder("f-child").await.unwrap();
    let queries = db.list_saved_queries().await.unwrap();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].folder_id, None);
}

#[tokio::test]
async fn saved_charts_cache_and_coalesce() {
    let db = store().await;
    seed_connection(&db, "conn-1").await;

    let chart = SavedChart {
        id: "".into(),
        name: "Revenue by status".into(),
        connection_id: Some("conn-1".into()),
        saved_query_id: None,
        sql_text: Some("SELECT status, revenue FROM orders;".into()),
        chart_type: "bar".into(),
        x_field: Some("status".into()),
        y_fields_json: "[\"revenue\"]".into(),
        options_json: "{}".into(),
        data_json: Some("[{\"status\":\"paid\",\"revenue\":10}]".into()),
        has_data: false, // ignored on write; server computes it
        row_count: Some(1),
        last_run_at: Some(1000),
        sort_order: 0,
        created_at: 0,
    };
    let mut chart = chart;
    chart.id = "c1".into();
    db.upsert_saved_chart(&chart).await.unwrap();

    // list_saved_charts must never carry data_json, only has_data.
    let list = db.list_saved_charts().await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].data_json.is_none());
    assert!(list[0].has_data);

    let cached = db.chart_data("c1").await.unwrap();
    assert!(cached.unwrap().contains("paid"));

    // A config-only update (rename, no data_json) must not clobber the
    // cached snapshot — that's the whole point of the COALESCE in the
    // upsert.
    let mut renamed = list[0].clone();
    renamed.data_json = None;
    renamed.row_count = None;
    renamed.last_run_at = None;
    renamed.name = "Revenue by status (renamed)".into();
    db.upsert_saved_chart(&renamed).await.unwrap();

    let still_cached = db.chart_data("c1").await.unwrap();
    assert!(still_cached.unwrap().contains("paid"));
    let list = db.list_saved_charts().await.unwrap();
    assert_eq!(list[0].name, "Revenue by status (renamed)");
    assert_eq!(list[0].row_count, Some(1)); // preserved, not wiped to NULL

    db.delete_saved_chart("c1").await.unwrap();
    assert!(db.list_saved_charts().await.unwrap().is_empty());
}

#[tokio::test]
async fn schedules_crud_and_mark_run() {
    let db = store().await;

    let s = Schedule {
        id: "".into(),
        name: "Hourly".into(),
        kind: "sql".into(),
        connection_id: Some("conn-1".into()),
        sql_text: Some("SELECT 1;".into()),
        saved_request_id: None,
        request_method: None,
        request_url: None,
        request_headers_json: None,
        request_body: None,
        schedule_expr: "every:3600".into(),
        enabled: true,
        last_run: None,
        last_status: None,
        next_run: Some(100),
        created_at: 0,
    };
    let mut s = s;
    s.id = "sch1".into();
    db.upsert_schedule(&s).await.unwrap();

    let due = db.due_schedules(200).await.unwrap();
    assert_eq!(due.len(), 1);
    let none_due = db.due_schedules(50).await.unwrap();
    assert!(none_due.is_empty());

    db.mark_schedule_run("sch1", 150, "ok · 3 rows", Some(3750))
        .await
        .unwrap();
    let got = db.get_schedule("sch1").await.unwrap().unwrap();
    assert_eq!(got.last_run, Some(150));
    assert_eq!(got.last_status.as_deref(), Some("ok · 3 rows"));
    assert_eq!(got.next_run, Some(3750));

    db.delete_schedule("sch1").await.unwrap();
    assert!(db.get_schedule("sch1").await.unwrap().is_none());
}

#[tokio::test]
async fn request_collections_tabs_saved_requests_environments() {
    let db = store().await;

    let col = RequestCollection {
        id: "col1".into(),
        name: "Playground".into(),
        parent_id: None,
    };
    db.upsert_collection(&col).await.unwrap();
    assert_eq!(db.list_collections().await.unwrap().len(), 1);

    let tab = RequestTab {
        id: "".into(),
        saved_request_id: None,
        title: "New request".into(),
        method: "GET".into(),
        url: "{{baseUrl}}/posts".into(),
        headers_json: "{}".into(),
        body: None,
        position: 0,
        is_active: true,
        pre_request_script: None,
        test_script: None,
        body_mode_json: None,
    };
    let mut tab = tab;
    tab.id = "rt1".into();
    db.upsert_request_tab(&tab).await.unwrap();
    assert_eq!(db.list_request_tabs().await.unwrap().len(), 1);
    db.delete_request_tab("rt1").await.unwrap();
    assert!(db.list_request_tabs().await.unwrap().is_empty());

    let saved = SavedRequest {
        id: "".into(),
        collection_id: Some("col1".into()),
        name: "List posts".into(),
        method: "GET".into(),
        url: "{{baseUrl}}/posts".into(),
        headers_json: "{}".into(),
        body: None,
        sort_order: 0,
        created_at: 0,
        pre_request_script: None,
        test_script: None,
        body_mode_json: None,
    };
    let mut saved = saved;
    saved.id = "sr1".into();
    db.upsert_saved_request(&saved).await.unwrap();
    let requests = db.list_saved_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].collection_id.as_deref(), Some("col1"));

    db.delete_collection("col1").await.unwrap();
    assert!(db.list_collections().await.unwrap().is_empty());

    let env = Environment {
        id: "".into(),
        name: "Demo".into(),
        variables_json: "{\"baseUrl\":\"https://example.com\"}".into(),
        is_active: true,
    };
    let mut env = env;
    env.id = "env1".into();
    db.upsert_environment(&env).await.unwrap();
    assert_eq!(db.list_environments().await.unwrap().len(), 1);
    db.delete_environment("env1").await.unwrap();
    assert!(db.list_environments().await.unwrap().is_empty());

    db.delete_saved_request("sr1").await.unwrap();
    assert!(db.list_saved_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn sql_and_request_history_keep_blobs_off_the_list() {
    let db = store().await;
    seed_connection(&db, "conn-1").await;

    let h = HistoryEntry {
        id: "h1".into(),
        connection_id: Some("conn-1".into()),
        sql_text: "SELECT * FROM orders;".into(),
        duration_ms: Some(12),
        row_count: Some(3),
        success: true,
        error: None,
        result_json: Some("{\"rows\":[1,2,3]}".into()),
        has_result: false,
        ran_at: 1000,
    };
    db.add_history(&h).await.unwrap();

    let list = db.recent_history(10).await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].result_json.is_none(), "list must not carry the cached blob");
    assert!(list[0].has_result);
    let blob = db.history_result("h1").await.unwrap().unwrap();
    assert!(blob.contains("rows"));

    let rh = RequestHistoryEntry {
        id: "rh1".into(),
        saved_request_id: None,
        name: Some("List posts".into()),
        method: "GET".into(),
        url: "{{baseUrl}}/posts".into(),
        headers_json: "{}".into(),
        body: None,
        status: Some(200),
        duration_ms: Some(88),
        size_bytes: Some(512),
        success: true,
        error: None,
        response_json: Some("{\"status\":200}".into()),
        has_response: false,
        sent_at: 2000,
    };
    db.add_request_history(&rh).await.unwrap();
    let rlist = db.recent_request_history(10).await.unwrap();
    assert_eq!(rlist.len(), 1);
    assert!(rlist[0].response_json.is_none());
    assert!(rlist[0].has_response);
    let rblob = db.request_history_result("rh1").await.unwrap().unwrap();
    assert!(rblob.contains("200"));
}

#[tokio::test]
async fn mock_routes_crud() {
    let db = store().await;
    let r = MockRoute {
        id: "".into(),
        method: "GET".into(),
        path: "/api/ping".into(),
        status: 200,
        headers_json: r#"{"Content-Type":"application/json"}"#.into(),
        body: r#"{"ok":true}"#.into(),
        enabled: true,
        sort_order: 0,
    };
    let mut r = r;
    r.id = "mr1".into();
    db.upsert_mock_route(&r).await.unwrap();

    let list = db.list_mock_routes().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].path, "/api/ping");
    assert_eq!(list[0].status, 200);

    let mut updated = r.clone();
    updated.status = 404;
    updated.enabled = false;
    db.upsert_mock_route(&updated).await.unwrap();
    let list2 = db.list_mock_routes().await.unwrap();
    assert_eq!(list2.len(), 1, "upsert on the same id should update, not duplicate");
    assert_eq!(list2[0].status, 404);
    assert!(!list2[0].enabled);

    db.delete_mock_route("mr1").await.unwrap();
    assert!(db.list_mock_routes().await.unwrap().is_empty());
}

#[tokio::test]
async fn delete_collection_deletes_its_requests_not_just_ungroups_them() {
    // Regression test: unlike SQL saved-query folders (where deleting a
    // folder ungroups its queries), deleting a Requests collection is
    // meant to delete the requests inside it too — a plain `DELETE FROM
    // request_collections` alone only fires the schema's ON DELETE SET
    // NULL, leaving the requests behind as "ungrouped" instead of gone.
    // Also covers a nested sub-collection: its requests must go too.
    let db = store().await;

    db.upsert_collection(&RequestCollection { id: "parent".into(), name: "Parent".into(), parent_id: None })
        .await
        .unwrap();
    db.upsert_collection(&RequestCollection {
        id: "child".into(),
        name: "Child".into(),
        parent_id: Some("parent".into()),
    })
    .await
    .unwrap();

    let mk = |id: &str, collection_id: &str| SavedRequest {
        id: id.into(),
        collection_id: Some(collection_id.into()),
        name: id.into(),
        method: "GET".into(),
        url: "{{baseUrl}}/x".into(),
        headers_json: "{}".into(),
        body: None,
        sort_order: 0,
        created_at: 0,
        pre_request_script: None,
        test_script: None,
        body_mode_json: None,
    };
    db.upsert_saved_request(&mk("req-in-parent", "parent")).await.unwrap();
    db.upsert_saved_request(&mk("req-in-child", "child")).await.unwrap();

    assert_eq!(db.list_saved_requests().await.unwrap().len(), 2);

    db.delete_collection("parent").await.unwrap();

    assert!(
        db.list_saved_requests().await.unwrap().is_empty(),
        "requests in the deleted collection and its sub-collection should be gone, not just ungrouped"
    );
    assert!(db.list_collections().await.unwrap().is_empty());
}
