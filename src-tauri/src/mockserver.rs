//! A local mock HTTP server — configured method+path routes return a
//! canned status/headers/body, no real backend involved. Basic on
//! purpose: exact method+path matching only, no wildcards/path params/
//! templating. Routes are read fresh from the metadata store on every
//! request rather than cached, which is plenty fast for a dev-time mock
//! server and means edits in the UI take effect immediately with no
//! reload/restart step.

use anyhow::Result;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use og_testdesk_core::MetadataStore;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;

pub struct MockHandle {
    shutdown: Option<oneshot::Sender<()>>,
    pub port: u16,
}

impl MockHandle {
    pub fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

#[derive(Clone)]
struct Ctx {
    metadata: Arc<MetadataStore>,
}

pub async fn start(metadata: Arc<MetadataStore>, port: u16) -> Result<MockHandle> {
    let ctx = Ctx { metadata };
    let app = Router::new().fallback(handler).with_state(ctx);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let (tx, rx) = oneshot::channel();

    tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async {
            let _ = rx.await;
        });
        if let Err(e) = server.await {
            eprintln!("[mock] server error: {e}");
        }
    });

    Ok(MockHandle {
        shutdown: Some(tx),
        port,
    })
}

async fn handler(State(ctx): State<Ctx>, method: Method, uri: Uri) -> Response {
    let routes = ctx.metadata.list_mock_routes().await.unwrap_or_default();
    let path = uri.path();
    let matched = routes
        .iter()
        .find(|r| r.enabled && r.method.eq_ignore_ascii_case(method.as_str()) && r.path == path);

    let Some(r) = matched else {
        return (
            StatusCode::NOT_FOUND,
            format!("No mock route configured for {method} {path}"),
        )
            .into_response();
    };

    let status = StatusCode::from_u16(r.status).unwrap_or(StatusCode::OK);
    let mut headers = HeaderMap::new();
    if let Ok(map) = serde_json::from_str::<std::collections::HashMap<String, String>>(&r.headers_json) {
        for (k, v) in map {
            if let (Ok(name), Ok(val)) = (HeaderName::try_from(k), HeaderValue::from_str(&v)) {
                headers.insert(name, val);
            }
        }
    }
    (status, headers, r.body.clone()).into_response()
}
