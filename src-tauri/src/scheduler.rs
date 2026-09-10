//! Background scheduler. Ticks every 20s, runs any schedule whose
//! `next_run` is due, records the outcome to SQL / request history, and
//! computes the following `next_run`.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use cron::Schedule as CronSchedule;
use og_testdesk_core::{
    apply_environment, drivers, requests as http_requests, HistoryEntry, HttpRequest, MetadataStore,
    RequestHistoryEntry, Schedule, SecretsStore,
};

const MAX_CACHED_ROWS: usize = 5000;
const MAX_CACHED_BYTES: usize = 4 * 1024 * 1024;

/// Serialize a query result for history only when it is small enough that
/// keeping it on disk (and later loading one such blob) is cheap. A
/// truncated result is never cached — it is not the full answer.
pub fn cache_result_json(r: &og_testdesk_core::QueryResult) -> Option<String> {
    if r.truncated || r.row_count > MAX_CACHED_ROWS {
        return None;
    }
    let s = serde_json::to_string(r).ok()?;
    (s.len() <= MAX_CACHED_BYTES).then_some(s)
}

pub fn cache_response_json(r: &og_testdesk_core::HttpResponse) -> Option<String> {
    let s = serde_json::to_string(r).ok()?;
    (s.len() <= MAX_CACHED_BYTES).then_some(s)
}

/// Next fire time (unix seconds) at or after `from_ts`, or None if the
/// expression is malformed.
pub fn next_run_after(expr: &str, from_ts: i64) -> Option<i64> {
    let expr = expr.trim();
    if let Some(rest) = expr.strip_prefix("every:") {
        let secs: i64 = rest.trim().parse().ok()?;
        if secs <= 0 {
            return None;
        }
        return Some(from_ts + secs);
    }
    // Accept 5-field cron by prepending a seconds field.
    let normalized = match expr.split_whitespace().count() {
        5 => format!("0 {expr}"),
        6 | 7 => expr.to_string(),
        _ => return None,
    };
    let sched = CronSchedule::from_str(&normalized).ok()?;
    let from = Utc.timestamp_opt(from_ts, 0).single()?;
    sched.after(&from).next().map(|dt| dt.timestamp())
}

pub fn spawn(metadata: Arc<MetadataStore>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let now = Utc::now().timestamp();
            let due = match metadata.due_schedules(now).await {
                Ok(d) => d,
                Err(_) => continue,
            };
            for s in due {
                let status = run_one(&metadata, &s).await;
                let next = next_run_after(&s.schedule_expr, Utc::now().timestamp());
                let _ = metadata
                    .mark_schedule_run(&s.id, Utc::now().timestamp(), &status, next)
                    .await;
            }
        }
    });
}

async fn active_vars(metadata: &MetadataStore) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    if let Ok(Some(raw)) = metadata.get_state("request_globals").await {
        if let Ok(g) = serde_json::from_str::<HashMap<String, String>>(&raw) {
            vars.extend(g);
        }
    }
    if let Ok(envs) = metadata.list_environments().await {
        if let Some(active) = envs.into_iter().find(|e| e.is_active) {
            if let Ok(e) = serde_json::from_str::<HashMap<String, String>>(&active.variables_json) {
                vars.extend(e);
            }
        }
    }
    vars
}

/// Runs a schedule and records history. Returns a short status string.
pub async fn run_one(metadata: &MetadataStore, s: &Schedule) -> String {
    let now = Utc::now().timestamp();
    match s.kind.as_str() {
        "sql" => {
            let (Some(conn_id), Some(sql)) = (s.connection_id.as_deref(), s.sql_text.as_deref())
            else {
                return "misconfigured".into();
            };
            let conns = match metadata.list_connections().await {
                Ok(c) => c,
                Err(e) => return format!("error: {e}"),
            };
            let Some(conn) = conns.into_iter().find(|c| c.id == conn_id) else {
                return "connection missing".into();
            };
            let pw = SecretsStore::get(&conn.id).ok().flatten();
            let res = drivers::driver_for(conn.kind)
                .run_query(&conn, pw.as_deref(), sql)
                .await;
            let entry = match &res {
                Ok(r) => HistoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    connection_id: Some(conn.id.clone()),
                    sql_text: sql.to_string(),
                    duration_ms: Some(r.duration_ms as i64),
                    row_count: Some(r.row_count as i64),
                    success: true,
                    error: None,
                    result_json: cache_result_json(r),
                    has_result: false,
                    ran_at: now,
                },
                Err(e) => HistoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    connection_id: Some(conn.id.clone()),
                    sql_text: sql.to_string(),
                    duration_ms: None,
                    row_count: None,
                    success: false,
                    error: Some(e.to_string()),
                    result_json: None,
                    has_result: false,
                    ran_at: now,
                },
            };
            let _ = metadata.add_history(&entry).await;
            match res {
                Ok(r) => format!("ok · {} rows", r.row_count),
                Err(e) => format!("error: {e}"),
            }
        }
        "request" => {
            let mut req = match resolve_request(metadata, s).await {
                Some(r) => r,
                None => return "misconfigured".into(),
            };
            apply_environment(&mut req, &active_vars(metadata).await);
            let res = http_requests::send(&req).await;
            let entry = match &res {
                Ok(r) => RequestHistoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    saved_request_id: s.saved_request_id.clone(),
                    name: Some(s.name.clone()),
                    method: req.method.clone(),
                    url: req.url.clone(),
                    headers_json: serde_json::to_string(&req.headers).unwrap_or_default(),
                    body: req.body.clone(),
                    status: Some(r.status as i64),
                    duration_ms: Some(r.duration_ms as i64),
                    size_bytes: Some(r.size_bytes as i64),
                    success: r.status < 400,
                    error: None,
                    response_json: cache_response_json(r),
                    has_response: false,
                    sent_at: now,
                },
                Err(e) => RequestHistoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    saved_request_id: s.saved_request_id.clone(),
                    name: Some(s.name.clone()),
                    method: req.method.clone(),
                    url: req.url.clone(),
                    headers_json: serde_json::to_string(&req.headers).unwrap_or_default(),
                    body: req.body.clone(),
                    status: None,
                    duration_ms: None,
                    size_bytes: None,
                    success: false,
                    error: Some(e.to_string()),
                    response_json: None,
                    has_response: false,
                    sent_at: now,
                },
            };
            let _ = metadata.add_request_history(&entry).await;
            match res {
                Ok(r) => format!("ok · {}", r.status),
                Err(e) => format!("error: {e}"),
            }
        }
        _ => "unknown kind".into(),
    }
}

async fn resolve_request(metadata: &MetadataStore, s: &Schedule) -> Option<HttpRequest> {
    if let Some(sid) = &s.saved_request_id {
        let saved = metadata
            .list_saved_requests()
            .await
            .ok()?
            .into_iter()
            .find(|r| &r.id == sid)?;
        let headers = serde_json::from_str(&saved.headers_json).unwrap_or_default();
        return Some(HttpRequest {
            method: saved.method,
            url: saved.url,
            headers,
            body: saved.body,
            timeout_secs: Some(60),
        });
    }
    let headers = s
        .request_headers_json
        .as_deref()
        .and_then(|h| serde_json::from_str(h).ok())
        .unwrap_or_default();
    Some(HttpRequest {
        method: s.request_method.clone()?,
        url: s.request_url.clone()?,
        headers,
        body: s.request_body.clone(),
        timeout_secs: Some(60),
    })
}
