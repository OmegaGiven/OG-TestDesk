use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub duration_ms: u64,
    pub size_bytes: usize,
}

/// Substitutes `{{var}}` tokens in url/headers/body against an
/// environment's variable set before sending.
pub fn apply_environment(req: &mut HttpRequest, vars: &HashMap<String, String>) {
    let sub = |s: &str, vars: &HashMap<String, String>| -> String {
        let mut out = s.to_string();
        for (k, v) in vars {
            out = out.replace(&format!("{{{{{k}}}}}"), v);
        }
        out
    };
    req.url = sub(&req.url, vars);
    if let Some(b) = &req.body {
        req.body = Some(sub(b, vars));
    }
    for v in req.headers.values_mut() {
        *v = sub(v, vars);
    }
}

pub async fn send(req: &HttpRequest) -> Result<HttpResponse> {
    let client = reqwest::Client::new();
    let method = reqwest::Method::from_bytes(req.method.as_bytes())?;
    let mut builder = client.request(method, &req.url);
    for (k, v) in &req.headers {
        builder = builder.header(k, v);
    }
    if let Some(body) = &req.body {
        builder = builder.body(body.clone());
    }

    let start = Instant::now();
    let resp = builder.send().await?;
    let status = resp.status().as_u16();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
        .collect();
    let body = resp.text().await?;
    let duration_ms = start.elapsed().as_millis() as u64;
    let size_bytes = body.len();

    Ok(HttpResponse { status, headers, body, duration_ms, size_bytes })
}
