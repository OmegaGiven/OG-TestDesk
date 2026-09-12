//! Local MCP server. Lets on-device AI tools use the connections and
//! saved requests stored in OG TestDesk **without ever seeing the
//! secrets** — the server executes queries / requests itself, holding
//! passwords only long enough to open a pool.
//!
//! Transport: HTTP + SSE (the 2024-11-05 MCP transport), bound to
//! 127.0.0.1 only, bearer-token gated. Safety rails:
//!   * a connection is invisible until explicitly exposed (per-connection
//!     toggle, persisted in app_state `mcp_connections`)
//!   * exposed connections are read-only unless both the per-connection
//!     "allow writes" and the server-wide `allow_write` are on
//!   * HTTP tools (`send_request`, `run_saved_request`) are off unless
//!     `allow_http` is on
//!   * "populate" tools (`open_sql_tab`, `save_query`, `save_sql_file`,
//!     `save_request`) let an AI put content in front of the human to
//!     read/run themselves — off unless `allow_populate` is on. They
//!     never execute anything against a database or the network.
//!   * `add_connection` creates a real, persistent DB credential (writes
//!     to the OS keychain) and is gated by its own `allow_manage_connections`
//!     flag, separate from `allow_populate`, since it's meaningfully more
//!     sensitive than "write some text into the app". A connection made
//!     this way is not automatically exposed to MCP — the human still has
//!     to flip that on in Settings before it's queryable.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use og_testdesk_core::{
    drivers, requests as http_requests, stmt_returns_rows, ConnConfig, DbKind, HttpRequest,
    MetadataStore, QueryTab, SavedQuery, SavedRequest, SecretsStore,
};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

pub const DEFAULT_PORT: u16 = 7788;
const PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub enabled: bool,
    pub port: u16,
    pub token: String,
    /// Permit non-SELECT statements through `run_query` (still also needs
    /// the per-connection "allow writes" flag).
    pub allow_write: bool,
    /// Permit `send_request` / `run_saved_request`.
    pub allow_http: bool,
    /// Permit `open_sql_tab` / `save_query` / `save_sql_file` / `save_request`
    /// — an AI putting content in front of the human, nothing executed.
    #[serde(default)]
    pub allow_populate: bool,
    /// Permit `add_connection` — creates a real DB credential in the OS
    /// keychain. Separate from `allow_populate` since it's more sensitive.
    #[serde(default)]
    pub allow_manage_connections: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            token: uuid::Uuid::new_v4().simple().to_string(),
            allow_write: false,
            allow_http: false,
            allow_populate: false,
            allow_manage_connections: false,
        }
    }
}

#[derive(Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct ConnAcl {
    pub exposed: bool,
    #[serde(default)]
    pub allow_writes: bool,
}

type Sessions = Arc<Mutex<HashMap<String, mpsc::Sender<String>>>>;

#[derive(Clone)]
struct AppCtx {
    metadata: Arc<MetadataStore>,
    cfg: McpConfig,
    sessions: Sessions,
    /// Where `save_sql_file` is allowed to write — a fixed subdirectory,
    /// never an AI-chosen path, so this tool can't touch arbitrary
    /// locations on disk.
    exports_dir: PathBuf,
}

pub struct McpHandle {
    shutdown: Option<oneshot::Sender<()>>,
    pub port: u16,
}

impl McpHandle {
    pub fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

pub async fn start(
    metadata: Arc<MetadataStore>,
    cfg: McpConfig,
    exports_dir: PathBuf,
) -> Result<McpHandle> {
    let ctx = AppCtx {
        metadata,
        cfg: cfg.clone(),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        exports_dir,
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/sse", get(sse_handler))
        .route("/message", post(message_handler))
        .with_state(ctx);

    let addr = SocketAddr::from(([127, 0, 0, 1], cfg.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let (tx, rx) = oneshot::channel();

    tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async {
            let _ = rx.await;
        });
        if let Err(e) = server.await {
            eprintln!("[mcp] server error: {e}");
        }
    });

    Ok(McpHandle {
        shutdown: Some(tx),
        port: cfg.port,
    })
}

// ---------------------------------------------------------------- transport

#[derive(serde::Deserialize)]
struct TokenQ {
    token: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

fn authed(cfg: &McpConfig, headers: &HeaderMap, q: &TokenQ) -> bool {
    if let Some(t) = &q.token {
        if t == &cfg.token {
            return true;
        }
    }
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim_start_matches("Bearer ").trim() == cfg.token)
        .unwrap_or(false)
}

async fn sse_handler(
    State(ctx): State<AppCtx>,
    headers: HeaderMap,
    Query(q): Query<TokenQ>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>>, StatusCode>
{
    if !authed(&ctx.cfg, &headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let session = uuid::Uuid::new_v4().simple().to_string();
    let (tx, rx) = mpsc::channel::<String>(32);
    ctx.sessions.lock().await.insert(session.clone(), tx);

    let endpoint = format!("/message?sessionId={session}&token={}", ctx.cfg.token);
    let init = tokio_stream::once(Ok(Event::default().event("endpoint").data(endpoint)));
    let body = ReceiverStream::new(rx).map(|msg| Ok(Event::default().event("message").data(msg)));

    Ok(Sse::new(init.chain(body)))
}

async fn message_handler(
    State(ctx): State<AppCtx>,
    headers: HeaderMap,
    Query(q): Query<TokenQ>,
    Json(req): Json<Value>,
) -> StatusCode {
    if !authed(&ctx.cfg, &headers, &q) {
        return StatusCode::UNAUTHORIZED;
    }
    let Some(session) = q.session_id.clone() else {
        return StatusCode::BAD_REQUEST;
    };
    let sender = { ctx.sessions.lock().await.get(&session).cloned() };
    let Some(sender) = sender else {
        return StatusCode::NOT_FOUND;
    };

    // Notifications (no `id`) get no reply.
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    if id.is_none() {
        return StatusCode::ACCEPTED;
    }
    let id = id.unwrap();

    let response = match dispatch(&ctx, method, params).await {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(e) => json!({
            "jsonrpc": "2.0", "id": id,
            "error": { "code": -32000, "message": e.to_string() }
        }),
    };
    let _ = sender.send(response.to_string()).await;
    StatusCode::ACCEPTED
}

// ------------------------------------------------------------------ methods

async fn dispatch(ctx: &AppCtx, method: &str, params: Value) -> Result<Value> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "og-testdesk", "version": env!("CARGO_PKG_VERSION") }
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_defs(&ctx.cfg) })),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match call_tool(ctx, name, args).await {
                Ok(text) => Ok(json!({ "content": [{ "type": "text", "text": text }] })),
                Err(e) => Ok(json!({
                    "content": [{ "type": "text", "text": format!("Error: {e}") }],
                    "isError": true
                })),
            }
        }
        other => Err(anyhow::anyhow!("unknown method: {other}")),
    }
}

fn tool_defs(cfg: &McpConfig) -> Vec<Value> {
    let mut tools = vec![
        json!({
            "name": "list_connections",
            "description": "List the database connections exposed to MCP (id, name, engine, host). Never returns passwords.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "list_schemas",
            "description": "List schemas and their tables/views for an exposed connection.",
            "inputSchema": { "type": "object", "properties": {
                "connection": { "type": "string", "description": "connection id or name" }
            }, "required": ["connection"] }
        }),
        json!({
            "name": "list_columns",
            "description": "List columns (name, type, nullable, primary key) of a table/view.",
            "inputSchema": { "type": "object", "properties": {
                "connection": { "type": "string" },
                "schema": { "type": "string" },
                "relation": { "type": "string" }
            }, "required": ["connection", "schema", "relation"] }
        }),
        json!({
            "name": "run_query",
            "description": "Run SQL against an exposed connection and return the rows as JSON. Read-only (SELECT/EXPLAIN/…) unless the connection and server both allow writes.",
            "inputSchema": { "type": "object", "properties": {
                "connection": { "type": "string" },
                "sql": { "type": "string" }
            }, "required": ["connection", "sql"] }
        }),
    ];
    if cfg.allow_http {
        tools.push(json!({
            "name": "list_saved_requests",
            "description": "List saved HTTP requests (name, method, url).",
            "inputSchema": { "type": "object", "properties": {} }
        }));
        tools.push(json!({
            "name": "run_saved_request",
            "description": "Send a saved HTTP request by name. The active environment + globals are applied.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string" }
            }, "required": ["name"] }
        }));
        tools.push(json!({
            "name": "send_request",
            "description": "Send an ad-hoc HTTP request. The active environment + globals are applied to {{vars}}.",
            "inputSchema": { "type": "object", "properties": {
                "method": { "type": "string" },
                "url": { "type": "string" },
                "headers": { "type": "object" },
                "body": { "type": "string" }
            }, "required": ["method", "url"] }
        }));
    }
    if cfg.allow_populate {
        tools.push(json!({
            "name": "open_sql_tab",
            "description": "Open a new SQL tab in the app with the given query, for the human to review and run themselves. Does not execute anything.",
            "inputSchema": { "type": "object", "properties": {
                "connection": { "type": "string", "description": "connection id or name" },
                "sql": { "type": "string" },
                "title": { "type": "string", "description": "tab title (default: \"MCP query\")" }
            }, "required": ["connection", "sql"] }
        }));
        tools.push(json!({
            "name": "save_query",
            "description": "Save a query into the app's Saved Queries tree (optionally into a named folder), for the human to find and run later.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string" },
                "sql": { "type": "string" },
                "connection": { "type": "string", "description": "connection id or name (optional)" },
                "folder": { "type": "string", "description": "existing saved-query folder name (optional)" }
            }, "required": ["name", "sql"] }
        }));
        tools.push(json!({
            "name": "save_sql_file",
            "description": "Write a .sql file to disk for the human, under this app's exports folder (not an arbitrary path).",
            "inputSchema": { "type": "object", "properties": {
                "filename": { "type": "string" },
                "sql": { "type": "string" }
            }, "required": ["filename", "sql"] }
        }));
        tools.push(json!({
            "name": "save_request",
            "description": "Save an HTTP request into the app's Requests sidebar (optionally into a named collection), for the human to find and send later.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string" },
                "method": { "type": "string" },
                "url": { "type": "string" },
                "headers": { "type": "object" },
                "body": { "type": "string" },
                "collection": { "type": "string", "description": "existing collection name (optional)" }
            }, "required": ["name", "url"] }
        }));
    }
    if cfg.allow_manage_connections {
        tools.push(json!({
            "name": "add_connection",
            "description": "Create a new DB connection profile in the app. It is NOT automatically exposed to MCP — the human must still turn that on in Settings before it can be queried by run_query/list_schemas/etc.",
            "inputSchema": { "type": "object", "properties": {
                "nickname": { "type": "string" },
                "kind": { "type": "string", "enum": ["postgres", "mysql", "sqlite"] },
                "host": { "type": "string" },
                "port": { "type": "integer" },
                "database": { "type": "string" },
                "user": { "type": "string" },
                "password": { "type": "string" },
                "file_path": { "type": "string", "description": "sqlite only" },
                "use_tls": { "type": "boolean" },
                "color": { "type": "string", "description": "hex accent color, e.g. #4f8cff" }
            }, "required": ["nickname", "kind"] }
        }));
    }
    tools
}

async fn acl_for(meta: &MetadataStore, conn_id: &str) -> ConnAcl {
    let map: HashMap<String, ConnAcl> = meta
        .get_state("mcp_connections")
        .await
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    map.get(conn_id).copied().unwrap_or_default()
}

/// Find a connection by id or nickname without requiring it be exposed —
/// for the "populate" tools, which never touch the database itself, only
/// reference its id/name so the human's tab points at the right place.
async fn resolve_any(ctx: &AppCtx, key: &str) -> Result<ConnConfig> {
    ctx.metadata
        .list_connections()
        .await?
        .into_iter()
        .find(|c| c.id == key || c.nickname.eq_ignore_ascii_case(key))
        .ok_or_else(|| anyhow::anyhow!("no connection '{key}'"))
}

async fn resolve_exposed(ctx: &AppCtx, key: &str) -> Result<(og_testdesk_core::ConnConfig, ConnAcl)> {
    let conns = ctx.metadata.list_connections().await?;
    let conn = conns
        .into_iter()
        .find(|c| c.id == key || c.nickname.eq_ignore_ascii_case(key))
        .ok_or_else(|| anyhow::anyhow!("no connection '{key}'"))?;
    let acl = acl_for(&ctx.metadata, &conn.id).await;
    if !acl.exposed {
        return Err(anyhow::anyhow!(
            "connection '{}' is not exposed to MCP",
            conn.nickname
        ));
    }
    Ok((conn, acl))
}

async fn call_tool(ctx: &AppCtx, name: &str, args: Value) -> Result<String> {
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::to_string);

    match name {
        "list_connections" => {
            let conns = ctx.metadata.list_connections().await?;
            let mut out = vec![];
            for c in conns {
                let acl = acl_for(&ctx.metadata, &c.id).await;
                if !acl.exposed {
                    continue;
                }
                out.push(json!({
                    "id": c.id, "name": c.nickname, "engine": format!("{:?}", c.kind).to_lowercase(),
                    "host": c.host, "database": c.database,
                    "read_only": !(acl.allow_writes && ctx.cfg.allow_write)
                }));
            }
            Ok(serde_json::to_string_pretty(&out)?)
        }
        "list_schemas" => {
            let (conn, _) = resolve_exposed(ctx, &s("connection").unwrap_or_default()).await?;
            let pw = SecretsStore::get(&conn.id)?;
            let schemas = drivers::driver_for(conn.kind)
                .list_schemas(&conn, pw.as_deref())
                .await?;
            Ok(serde_json::to_string_pretty(&schemas)?)
        }
        "list_columns" => {
            let (conn, _) = resolve_exposed(ctx, &s("connection").unwrap_or_default()).await?;
            let pw = SecretsStore::get(&conn.id)?;
            let cols = drivers::driver_for(conn.kind)
                .list_columns(
                    &conn,
                    pw.as_deref(),
                    &s("schema").unwrap_or_default(),
                    &s("relation").unwrap_or_default(),
                )
                .await?;
            Ok(serde_json::to_string_pretty(&cols)?)
        }
        "run_query" => {
            let (conn, acl) = resolve_exposed(ctx, &s("connection").unwrap_or_default()).await?;
            let sql = s("sql").ok_or_else(|| anyhow::anyhow!("missing 'sql'"))?;
            let writes_ok = acl.allow_writes && ctx.cfg.allow_write;
            if !writes_ok && !stmt_returns_rows(&sql) {
                return Err(anyhow::anyhow!(
                    "connection '{}' is read-only for MCP — only row-returning statements allowed",
                    conn.nickname
                ));
            }
            let pw = SecretsStore::get(&conn.id)?;
            let result = drivers::driver_for(conn.kind)
                .run_query(&conn, pw.as_deref(), &sql, og_testdesk_core::QueryOpts::full())
                .await?;
            Ok(serde_json::to_string_pretty(&result)?)
        }
        "list_saved_requests" if ctx.cfg.allow_http => {
            let reqs = ctx.metadata.list_saved_requests().await?;
            let out: Vec<Value> = reqs
                .into_iter()
                .map(|r| json!({ "name": r.name, "method": r.method, "url": r.url }))
                .collect();
            Ok(serde_json::to_string_pretty(&out)?)
        }
        "run_saved_request" if ctx.cfg.allow_http => {
            let want = s("name").unwrap_or_default();
            let req = ctx
                .metadata
                .list_saved_requests()
                .await?
                .into_iter()
                .find(|r| r.name.eq_ignore_ascii_case(&want))
                .ok_or_else(|| anyhow::anyhow!("no saved request '{want}'"))?;
            let headers: HashMap<String, String> =
                serde_json::from_str(&req.headers_json).unwrap_or_default();
            let mut http = HttpRequest {
                method: req.method,
                url: req.url,
                headers,
                body: req.body,
                timeout_secs: Some(60),
            };
            apply_vars(&ctx.metadata, &mut http).await;
            let resp = http_requests::send(&http).await?;
            Ok(serde_json::to_string_pretty(&resp)?)
        }
        "send_request" if ctx.cfg.allow_http => {
            let headers: HashMap<String, String> = args
                .get("headers")
                .and_then(|h| serde_json::from_value(h.clone()).ok())
                .unwrap_or_default();
            let mut http = HttpRequest {
                method: s("method").unwrap_or_else(|| "GET".into()),
                url: s("url").ok_or_else(|| anyhow::anyhow!("missing 'url'"))?,
                headers,
                body: s("body"),
                timeout_secs: Some(60),
            };
            apply_vars(&ctx.metadata, &mut http).await;
            let resp = http_requests::send(&http).await?;
            Ok(serde_json::to_string_pretty(&resp)?)
        }
        "open_sql_tab" if ctx.cfg.allow_populate => {
            let conn = resolve_any(ctx, &s("connection").unwrap_or_default()).await?;
            let sql = s("sql").ok_or_else(|| anyhow::anyhow!("missing 'sql'"))?;
            let title = s("title").unwrap_or_else(|| "MCP query".to_string());
            let position = ctx.metadata.list_tabs(&conn.id).await?.len() as i64;
            let tab = QueryTab {
                id: uuid::Uuid::new_v4().to_string(),
                connection_id: conn.id.clone(),
                title,
                sql_text: sql,
                position,
                is_active: false,
            };
            ctx.metadata.upsert_tab(&tab).await?;
            Ok(format!(
                "Opened a SQL tab \"{}\" on {} — switch to the app to see it.",
                tab.title, conn.nickname
            ))
        }
        "save_query" if ctx.cfg.allow_populate => {
            let name = s("name").ok_or_else(|| anyhow::anyhow!("missing 'name'"))?;
            let sql = s("sql").ok_or_else(|| anyhow::anyhow!("missing 'sql'"))?;
            let connection_id = match s("connection") {
                Some(key) => Some(resolve_any(ctx, &key).await?.id),
                None => None,
            };
            let folder_id = match s("folder") {
                Some(name) => Some(
                    ctx.metadata
                        .list_saved_query_folders()
                        .await?
                        .into_iter()
                        .find(|f| f.name.eq_ignore_ascii_case(&name))
                        .ok_or_else(|| anyhow::anyhow!("no saved-query folder '{name}'"))?
                        .id,
                ),
                None => None,
            };
            let q = SavedQuery {
                id: uuid::Uuid::new_v4().to_string(),
                connection_id,
                folder_id,
                name: name.clone(),
                sql_text: sql,
                sort_order: 0,
                created_at: chrono::Utc::now().timestamp(),
            };
            ctx.metadata.upsert_saved_query(&q).await?;
            Ok(format!("Saved query \"{name}\" — find it in Saved Queries."))
        }
        "save_sql_file" if ctx.cfg.allow_populate => {
            let filename = s("filename").ok_or_else(|| anyhow::anyhow!("missing 'filename'"))?;
            let sql = s("sql").ok_or_else(|| anyhow::anyhow!("missing 'sql'"))?;
            // never let the AI pick a path — just a bare filename under a
            // fixed exports directory
            let safe: String = filename
                .chars()
                .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
                .collect();
            let safe = safe.trim();
            if safe.is_empty() {
                return Err(anyhow::anyhow!("filename has no usable characters"));
            }
            let safe = if safe.to_lowercase().ends_with(".sql") {
                safe.to_string()
            } else {
                format!("{safe}.sql")
            };
            tokio::fs::create_dir_all(&ctx.exports_dir).await?;
            let path = ctx.exports_dir.join(&safe);
            tokio::fs::write(&path, sql).await?;
            Ok(format!("Wrote {}", path.display()))
        }
        "save_request" if ctx.cfg.allow_populate => {
            let name = s("name").ok_or_else(|| anyhow::anyhow!("missing 'name'"))?;
            let url = s("url").ok_or_else(|| anyhow::anyhow!("missing 'url'"))?;
            let headers: HashMap<String, String> = args
                .get("headers")
                .and_then(|h| serde_json::from_value(h.clone()).ok())
                .unwrap_or_default();
            let collection_id = match s("collection") {
                Some(cname) => Some(
                    ctx.metadata
                        .list_collections()
                        .await?
                        .into_iter()
                        .find(|c| c.name.eq_ignore_ascii_case(&cname))
                        .ok_or_else(|| anyhow::anyhow!("no request collection '{cname}'"))?
                        .id,
                ),
                None => None,
            };
            let r = SavedRequest {
                id: uuid::Uuid::new_v4().to_string(),
                collection_id,
                name: name.clone(),
                method: s("method").unwrap_or_else(|| "GET".to_string()),
                url,
                headers_json: serde_json::to_string(&headers)?,
                body: s("body"),
                sort_order: 0,
                created_at: chrono::Utc::now().timestamp(),
            };
            ctx.metadata.upsert_saved_request(&r).await?;
            Ok(format!("Saved request \"{name}\" — find it in the Requests sidebar."))
        }
        "add_connection" if ctx.cfg.allow_manage_connections => {
            let nickname = s("nickname").ok_or_else(|| anyhow::anyhow!("missing 'nickname'"))?;
            let kind = match s("kind").unwrap_or_default().to_lowercase().as_str() {
                "postgres" | "pg" | "postgresql" => DbKind::Postgres,
                "mysql" => DbKind::MySql,
                "sqlite" => DbKind::Sqlite,
                other => return Err(anyhow::anyhow!("unknown kind '{other}' — use postgres/mysql/sqlite")),
            };
            let id = uuid::Uuid::new_v4().to_string();
            if let Some(pw) = s("password") {
                if !pw.is_empty() {
                    SecretsStore::set(&id, &pw)?;
                }
            }
            let conn = ConnConfig {
                id,
                nickname: nickname.clone(),
                kind,
                host: s("host"),
                port: args.get("port").and_then(|v| v.as_u64()).map(|n| n as u16),
                database: s("database"),
                user: s("user"),
                file_path: s("file_path"),
                use_tls: args.get("use_tls").and_then(|v| v.as_bool()).unwrap_or(false),
                color: s("color"),
            };
            ctx.metadata.upsert_connection(&conn).await?;
            Ok(format!(
                "Created connection \"{nickname}\" — it is NOT yet exposed to MCP; turn that on in Settings if you want it queryable here."
            ))
        }
        _ => Err(anyhow::anyhow!("unknown or disabled tool: {name}")),
    }
}

async fn apply_vars(meta: &MetadataStore, req: &mut HttpRequest) {
    let mut vars: HashMap<String, String> = HashMap::new();
    if let Ok(Some(raw)) = meta.get_state("request_globals").await {
        if let Ok(g) = serde_json::from_str::<HashMap<String, String>>(&raw) {
            vars.extend(g);
        }
    }
    if let Ok(envs) = meta.list_environments().await {
        if let Some(active) = envs.into_iter().find(|e| e.is_active) {
            if let Ok(e) = serde_json::from_str::<HashMap<String, String>>(&active.variables_json) {
                vars.extend(e);
            }
        }
    }
    if !vars.is_empty() {
        og_testdesk_core::apply_environment(req, &vars);
    }
}
