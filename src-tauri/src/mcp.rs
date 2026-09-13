//! Local MCP server. Lets on-device AI tools use the connections and
//! saved requests stored in OG TestDesk **without ever seeing the
//! secrets** — the server executes queries / requests itself, holding
//! passwords only long enough to open a pool.
//!
//! Transport: HTTP + SSE (the 2024-11-05 MCP transport), bound to
//! 127.0.0.1 only. Two ways in, both accepted side by side:
//!   * the original static bearer token (`?token=` or `Authorization`),
//!     shown in Settings — what Claude Code's `claude mcp add` uses today.
//!   * a minimal OAuth 2.0 layer (metadata discovery, dynamic client
//!     registration, authorization-code + PKCE, refresh tokens) so
//!     clients that require OAuth for remote connectors (ChatGPT's
//!     connector framework, notably) can add this server by URL alone.
//!     Since this is a single-user local app, "authorize" is just a
//!     plain local "Approve access?" page — no account system to speak
//!     of. OAuth state (registered clients, codes, tokens) lives only in
//!     memory; it resets on restart, which every MCP OAuth client
//!     already handles by silently redoing discovery + registration.
//!
//! Safety rails:
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
//!
//! `open_sql_tab`/`open_request_tab`/`save_query`/`save_request`/
//! `add_connection` each emit an `mcp:*` event into the window right
//! after writing (see `ctx.app_handle.emit`) — the frontend merges just
//! that one new thing in live (+page.svelte's `listen(...)` calls), so
//! an AI opening a tab shows up on screen immediately instead of
//! waiting for a reload. This is the actual point of the MCP server:
//! not a headless API, but "watch the AI work and correct it" — the
//! human always sees exactly what got opened, on the connection it was
//! opened on, before anything runs.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::{
    extract::{Form, Query, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, Sse},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use og_testdesk_core::{
    drivers, requests as http_requests, stmt_returns_rows, ConnConfig, DbKind, HttpRequest,
    MetadataStore, QueryTab, RequestTab, SavedQuery, SavedRequest, SecretsStore,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::Emitter;
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

// ------------------------------------------------------------------- OAuth
// A minimal in-memory authorization server, just enough of RFC 7591
// (dynamic client registration), RFC 8414 / RFC 9728 (metadata discovery)
// and RFC 7636 (PKCE) for MCP clients that require OAuth for a remote
// connector. No accounts: there's exactly one human at this URL, so
// "authorize" is a plain approve/deny page, not a login form.

#[derive(Clone)]
struct OAuthClient {
    redirect_uris: Vec<String>,
    client_name: String,
}

struct PendingAuth {
    redirect_uri: String,
    code_challenge: String,
    code_challenge_method: String, // "S256" | "plain" | "" (no PKCE)
    scope: Option<String>,
    expires_at: Instant,
}

struct IssuedToken {
    expires_at: Instant,
}

#[derive(Default)]
struct OAuthState {
    clients: HashMap<String, OAuthClient>,
    codes: HashMap<String, PendingAuth>,
    access_tokens: HashMap<String, IssuedToken>,
    refresh_tokens: HashMap<String, ()>, // just presence; no rotation needed at this scale
}

type OAuth = Arc<Mutex<OAuthState>>;

#[derive(Clone)]
struct AppCtx {
    metadata: Arc<MetadataStore>,
    cfg: McpConfig,
    sessions: Sessions,
    oauth: OAuth,
    /// Where `save_sql_file` is allowed to write — a fixed subdirectory,
    /// never an AI-chosen path, so this tool can't touch arbitrary
    /// locations on disk.
    exports_dir: PathBuf,
    /// The frontend's last-reported `debugSnapshot` (raw JSON, opaque
    /// here) — see the `get_app_state` tool.
    debug_state: Arc<Mutex<String>>,
    /// Used to emit `mcp:*` events straight into the window, so a tab
    /// this server opens shows up live in the UI instead of waiting for
    /// the next reload — the whole point of "watch the AI work" being
    /// more than "reload occasionally and hope".
    app_handle: tauri::AppHandle,
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
    debug_state: Arc<Mutex<String>>,
    app_handle: tauri::AppHandle,
) -> Result<McpHandle> {
    let ctx = AppCtx {
        metadata,
        cfg: cfg.clone(),
        app_handle,
        sessions: Arc::new(Mutex::new(HashMap::new())),
        oauth: Arc::new(Mutex::new(OAuthState::default())),
        exports_dir,
        debug_state,
    };

    // Permissive CORS: this server only ever binds 127.0.0.1, and every
    // route that does anything is already gated by the bearer token or
    // an OAuth access token regardless of origin — CORS only controls
    // whether a *browser* lets its own page JS read the response, so
    // this doesn't widen who can actually call these endpoints, just
    // lets a browser-hosted MCP client (rather than one whose own
    // backend makes the request) work at all.
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/sse", get(sse_handler))
        .route("/message", post(message_handler))
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth_as_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            get(oauth_resource_metadata),
        )
        .route("/register", post(oauth_register))
        .route("/authorize", get(oauth_authorize_get).post(oauth_authorize_post))
        .route("/token", post(oauth_token))
        .layer(tower_http::cors::CorsLayer::permissive())
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

/// Pull whatever bearer credential the request carries, from `?token=`
/// or an `Authorization: Bearer …` header — either the static config
/// token or an OAuth-issued access token is accepted here.
fn bearer_of(headers: &HeaderMap, q: &TokenQ) -> Option<String> {
    if let Some(t) = &q.token {
        return Some(t.clone());
    }
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim_start_matches("Bearer ").trim().to_string())
}

async fn authed(ctx: &AppCtx, headers: &HeaderMap, q: &TokenQ) -> bool {
    let Some(token) = bearer_of(headers, q) else {
        return false;
    };
    if token == ctx.cfg.token {
        return true;
    }
    let mut st = ctx.oauth.lock().await;
    match st.access_tokens.get(&token) {
        Some(t) if t.expires_at > Instant::now() => true,
        Some(_) => {
            st.access_tokens.remove(&token);
            false
        }
        None => false,
    }
}

/// A 401 that also tells OAuth-aware clients where to go discover this
/// server's authorization metadata and redo the auth flow, per the MCP
/// Authorization spec.
fn unauthorized(ctx: &AppCtx) -> Response {
    let hint = format!(
        "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\"",
        base_url(ctx)
    );
    (
        StatusCode::UNAUTHORIZED,
        [("WWW-Authenticate", hint)],
        "unauthorized",
    )
        .into_response()
}

fn base_url(ctx: &AppCtx) -> String {
    format!("http://127.0.0.1:{}", ctx.cfg.port)
}

async fn sse_handler(State(ctx): State<AppCtx>, headers: HeaderMap, Query(q): Query<TokenQ>) -> Response {
    if !authed(&ctx, &headers, &q).await {
        return unauthorized(&ctx);
    }
    let session = uuid::Uuid::new_v4().simple().to_string();
    let (tx, rx) = mpsc::channel::<String>(32);
    ctx.sessions.lock().await.insert(session.clone(), tx);

    let endpoint = format!("/message?sessionId={session}&token={}", ctx.cfg.token);
    let init = tokio_stream::once(Ok::<_, std::convert::Infallible>(
        Event::default().event("endpoint").data(endpoint),
    ));
    let body = ReceiverStream::new(rx)
        .map(|msg| Ok::<_, std::convert::Infallible>(Event::default().event("message").data(msg)));

    Sse::new(init.chain(body)).into_response()
}

async fn message_handler(
    State(ctx): State<AppCtx>,
    headers: HeaderMap,
    Query(q): Query<TokenQ>,
    Json(req): Json<Value>,
) -> Response {
    if !authed(&ctx, &headers, &q).await {
        return unauthorized(&ctx);
    }
    let Some(session) = q.session_id.clone() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let sender = { ctx.sessions.lock().await.get(&session).cloned() };
    let Some(sender) = sender else {
        return StatusCode::NOT_FOUND.into_response();
    };

    // Notifications (no `id`) get no reply.
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    if id.is_none() {
        return StatusCode::ACCEPTED.into_response();
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
    StatusCode::ACCEPTED.into_response()
}

// --------------------------------------------------------------- OAuth 2.0

async fn oauth_as_metadata(State(ctx): State<AppCtx>) -> Json<Value> {
    let base = base_url(&ctx);
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "registration_endpoint": format!("{base}/register"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256", "plain"],
        "token_endpoint_auth_methods_supported": ["none"],
        "scopes_supported": ["mcp"]
    }))
}

/// RFC 9728 protected-resource metadata — what a well-behaved MCP client
/// fetches first on a 401 to find the authorization server above.
async fn oauth_resource_metadata(State(ctx): State<AppCtx>) -> Json<Value> {
    let base = base_url(&ctx);
    Json(json!({
        "resource": base,
        "authorization_servers": [base]
    }))
}

#[derive(serde::Deserialize)]
struct RegisterReq {
    redirect_uris: Vec<String>,
    #[serde(default)]
    client_name: Option<String>,
}

/// Dynamic client registration (RFC 7591). Public client, no secret —
/// PKCE is what actually secures the authorization-code exchange.
async fn oauth_register(State(ctx): State<AppCtx>, Json(req): Json<RegisterReq>) -> Response {
    if req.redirect_uris.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_redirect_uri"}))).into_response();
    }
    let client_id = uuid::Uuid::new_v4().to_string();
    ctx.oauth.lock().await.clients.insert(
        client_id.clone(),
        OAuthClient {
            redirect_uris: req.redirect_uris.clone(),
            client_name: req.client_name.clone().unwrap_or_else(|| "MCP client".to_string()),
        },
    );
    Json(json!({
        "client_id": client_id,
        "redirect_uris": req.redirect_uris,
        "token_endpoint_auth_method": "none",
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"]
    }))
    .into_response()
}

#[derive(serde::Deserialize)]
struct AuthorizeQ {
    client_id: String,
    redirect_uri: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    code_challenge: Option<String>,
    #[serde(default)]
    code_challenge_method: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

/// The "approve access" page. No login — there's one human, and they're
/// sitting at this machine, looking at this window.
async fn oauth_authorize_get(State(ctx): State<AppCtx>, Query(q): Query<AuthorizeQ>) -> Html<String> {
    let client_name = {
        let st = ctx.oauth.lock().await;
        match st.clients.get(&q.client_id) {
            Some(c) if c.redirect_uris.contains(&q.redirect_uri) => c.client_name.clone(),
            _ => {
                return Html(
                    "<h3>Unknown client or redirect URI.</h3><p>This client needs to register with \
                     this server (POST /register) before requesting authorization.</p>"
                        .to_string(),
                )
            }
        }
    };
    Html(format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>OG TestDesk — Authorize</title>
<style>body{{font-family:-apple-system,sans-serif;max-width:420px;margin:80px auto;color:#222}}
h1{{font-size:18px}}p{{color:#555;font-size:14px}}button{{padding:9px 16px;border-radius:6px;
border:1px solid #ccc;font-size:14px;cursor:pointer;margin-right:8px}}
button.approve{{background:#111;color:#fff;border-color:#111}}</style></head><body>
<h1>OG TestDesk MCP server</h1>
<p><b>{name}</b> wants to access your database connections and requests through this app.</p>
<form method="post" action="/authorize">
<input type="hidden" name="client_id" value="{client_id}">
<input type="hidden" name="redirect_uri" value="{redirect_uri}">
<input type="hidden" name="state" value="{state}">
<input type="hidden" name="code_challenge" value="{challenge}">
<input type="hidden" name="code_challenge_method" value="{method}">
<input type="hidden" name="scope" value="{scope}">
<button class="approve" name="decision" value="approve" type="submit">Approve</button>
<button name="decision" value="deny" type="submit">Deny</button>
</form></body></html>"#,
        name = html_escape(&client_name),
        client_id = html_escape(&q.client_id),
        redirect_uri = html_escape(&q.redirect_uri),
        state = html_escape(q.state.as_deref().unwrap_or("")),
        challenge = html_escape(q.code_challenge.as_deref().unwrap_or("")),
        method = html_escape(q.code_challenge_method.as_deref().unwrap_or("")),
        scope = html_escape(q.scope.as_deref().unwrap_or(""))
    ))
}

#[derive(serde::Deserialize)]
struct AuthorizeForm {
    client_id: String,
    redirect_uri: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    code_challenge: String,
    #[serde(default)]
    code_challenge_method: String,
    #[serde(default)]
    scope: Option<String>,
    decision: String,
}

async fn oauth_authorize_post(State(ctx): State<AppCtx>, Form(f): Form<AuthorizeForm>) -> Redirect {
    let known = {
        let st = ctx.oauth.lock().await;
        st.clients
            .get(&f.client_id)
            .map(|c| c.redirect_uris.contains(&f.redirect_uri))
            .unwrap_or(false)
    };
    if !known {
        // Not a redirect a registered client asked for — refuse to send
        // a code anywhere rather than trust an unvalidated redirect_uri.
        return Redirect::to("about:blank");
    }
    let sep = if f.redirect_uri.contains('?') { "&" } else { "?" };
    if f.decision != "approve" {
        let mut url = format!("{}{sep}error=access_denied", f.redirect_uri);
        if !f.state.is_empty() {
            url.push_str(&format!("&state={}", percent_encode(&f.state)));
        }
        return Redirect::to(&url);
    }
    let code = format!("ogtd_code_{}", uuid::Uuid::new_v4().simple());
    ctx.oauth.lock().await.codes.insert(
        code.clone(),
        PendingAuth {
            redirect_uri: f.redirect_uri.clone(),
            code_challenge: f.code_challenge,
            code_challenge_method: if f.code_challenge_method.is_empty() {
                "plain".to_string()
            } else {
                f.code_challenge_method
            },
            scope: f.scope,
            expires_at: Instant::now() + Duration::from_secs(300),
        },
    );
    let mut url = format!("{}{sep}code={code}", f.redirect_uri);
    if !f.state.is_empty() {
        url.push_str(&format!("&state={}", percent_encode(&f.state)));
    }
    Redirect::to(&url)
}

#[derive(serde::Deserialize)]
struct TokenForm {
    grant_type: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    code_verifier: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
}

async fn oauth_token(State(ctx): State<AppCtx>, Form(f): Form<TokenForm>) -> Response {
    match f.grant_type.as_str() {
        "authorization_code" => {
            let Some(code) = f.code else {
                return oauth_err(StatusCode::BAD_REQUEST, "invalid_request", "missing code");
            };
            let mut st = ctx.oauth.lock().await;
            let Some(pending) = st.codes.remove(&code) else {
                return oauth_err(StatusCode::BAD_REQUEST, "invalid_grant", "unknown or reused code");
            };
            if pending.expires_at < Instant::now() {
                return oauth_err(StatusCode::BAD_REQUEST, "invalid_grant", "code expired");
            }
            if let Some(ruri) = &f.redirect_uri {
                if ruri != &pending.redirect_uri {
                    return oauth_err(StatusCode::BAD_REQUEST, "invalid_grant", "redirect_uri mismatch");
                }
            }
            if !pending.code_challenge.is_empty() {
                let verifier = f.code_verifier.unwrap_or_default();
                let ok = match pending.code_challenge_method.as_str() {
                    "S256" => {
                        let digest = Sha256::digest(verifier.as_bytes());
                        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest) == pending.code_challenge
                    }
                    _ => verifier == pending.code_challenge,
                };
                if !ok {
                    return oauth_err(StatusCode::BAD_REQUEST, "invalid_grant", "PKCE verification failed");
                }
            }
            issue_tokens(&mut st, pending.scope)
        }
        "refresh_token" => {
            let Some(rt) = f.refresh_token else {
                return oauth_err(StatusCode::BAD_REQUEST, "invalid_request", "missing refresh_token");
            };
            let mut st = ctx.oauth.lock().await;
            if !st.refresh_tokens.contains_key(&rt) {
                return oauth_err(StatusCode::BAD_REQUEST, "invalid_grant", "unknown refresh_token");
            }
            issue_tokens(&mut st, None)
        }
        other => oauth_err(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            &format!("'{other}' is not supported"),
        ),
    }
}

fn issue_tokens(st: &mut OAuthState, scope: Option<String>) -> Response {
    let access_token = format!("ogtd_at_{}", uuid::Uuid::new_v4().simple());
    let refresh_token = format!("ogtd_rt_{}", uuid::Uuid::new_v4().simple());
    st.access_tokens.insert(
        access_token.clone(),
        IssuedToken {
            expires_at: Instant::now() + Duration::from_secs(3600),
        },
    );
    st.refresh_tokens.insert(refresh_token.clone(), ());
    Json(json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expires_in": 3600,
        "refresh_token": refresh_token,
        "scope": scope.unwrap_or_default()
    }))
    .into_response()
}

fn oauth_err(status: StatusCode, code: &str, description: &str) -> Response {
    (status, Json(json!({ "error": code, "error_description": description }))).into_response()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Just enough percent-encoding for a `state` value in a redirect query
/// string — no extra crate needed for this one field.
fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
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
                Err(e) => {
                    og_testdesk_core::record_error(&format!("mcp:{name}"), &e.to_string());
                    Ok(json!({
                        "content": [{ "type": "text", "text": format!("Error: {e}") }],
                        "isError": true
                    }))
                }
            }
        }
        other => Err(anyhow::anyhow!("unknown method: {other}")),
    }
}

fn tool_defs(cfg: &McpConfig) -> Vec<Value> {
    let mut tools = vec![
        json!({
            "name": "get_app_state",
            "description": "Read the app's current UI state as reported by the frontend — every open SQL/request tab (id, title, dirty flag, which is active), split-screen state, Inspector open/closed, connections, and top-bar group layout. Use this to see what tabs actually exist when debugging a tab-count/visibility mismatch, instead of asking the human to describe their screen. May be empty or stale if the app hasn't reported yet or was just restarted.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "get_error_log",
            "description": "Read the app's recent error log (backend, MCP tool calls, and reported frontend errors) — for diagnosing something that just went wrong. Never contains passwords.",
            "inputSchema": { "type": "object", "properties": {
                "limit": { "type": "integer", "description": "max entries, newest last (default 50)" }
            } }
        }),
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
            "name": "open_request_tab",
            "description": "Open a new HTTP request tab in the app (method/url/headers/body filled in), for the human to review and send themselves. Does not send anything.",
            "inputSchema": { "type": "object", "properties": {
                "method": { "type": "string" },
                "url": { "type": "string" },
                "headers": { "type": "object" },
                "body": { "type": "string" },
                "title": { "type": "string", "description": "tab title (default: \"MCP request\")" }
            }, "required": ["url"] }
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
        "get_app_state" => {
            let snap = ctx.debug_state.lock().await.clone();
            if snap.is_empty() {
                Ok("(no state reported yet — the app may have just started; give it a moment)".to_string())
            } else {
                Ok(snap)
            }
        }
        "get_error_log" => {
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
            let entries = og_testdesk_core::recent_errors(Some(limit));
            Ok(serde_json::to_string_pretty(&entries)?)
        }
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
            let _ = ctx.app_handle.emit("mcp:sql-tab-opened", &tab);
            Ok(format!(
                "Opened a SQL tab \"{}\" on {} — it's live in the app now.",
                tab.title, conn.nickname
            ))
        }
        "open_request_tab" if ctx.cfg.allow_populate => {
            let url = s("url").ok_or_else(|| anyhow::anyhow!("missing 'url'"))?;
            let headers: HashMap<String, String> = args
                .get("headers")
                .and_then(|h| serde_json::from_value(h.clone()).ok())
                .unwrap_or_default();
            let title = s("title").unwrap_or_else(|| "MCP request".to_string());
            let position = ctx.metadata.list_request_tabs().await?.len() as i64;
            let tab = RequestTab {
                id: uuid::Uuid::new_v4().to_string(),
                saved_request_id: None,
                title,
                method: s("method").unwrap_or_else(|| "GET".to_string()),
                url,
                headers_json: serde_json::to_string(&headers)?,
                body: s("body"),
                position,
                is_active: false,
            };
            ctx.metadata.upsert_request_tab(&tab).await?;
            let _ = ctx.app_handle.emit("mcp:request-tab-opened", &tab);
            Ok(format!("Opened a request tab \"{}\" — it's live in the app now.", tab.title))
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
            let _ = ctx.app_handle.emit("mcp:saved-query-created", &q);
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
            let _ = ctx.app_handle.emit("mcp:saved-request-created", &r);
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
            let _ = ctx.app_handle.emit("mcp:connection-created", &conn);
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
