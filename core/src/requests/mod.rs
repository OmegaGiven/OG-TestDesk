use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
    /// Seconds; defaults to 60 when absent.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub content_type: Option<String>,
    pub is_json: bool,
    pub duration_ms: u64,
    pub size_bytes: usize,
}

/// Substitutes `{{var}}` tokens in url/headers/body against an
/// environment's variable set before sending.
pub fn apply_environment(req: &mut HttpRequest, vars: &HashMap<String, String>) {
    let sub = |s: &str| -> String {
        let mut out = s.to_string();
        for (k, v) in vars {
            out = out.replace(&format!("{{{{{k}}}}}"), v);
        }
        out
    };
    req.url = sub(&req.url);
    if let Some(b) = &req.body {
        req.body = Some(sub(b));
    }
    let headers = std::mem::take(&mut req.headers);
    req.headers = headers
        .into_iter()
        .map(|(k, v)| (sub(&k), sub(&v)))
        .collect();
}

pub async fn send(req: &HttpRequest) -> Result<HttpResponse> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(req.timeout_secs.unwrap_or(60)))
        .build()?;
    let method = reqwest::Method::from_bytes(req.method.to_ascii_uppercase().as_bytes())?;
    let mut builder = client.request(method, &req.url);
    for (k, v) in &req.headers {
        if !k.trim().is_empty() {
            builder = builder.header(k, v);
        }
    }
    if let Some(body) = &req.body {
        if !body.is_empty() {
            builder = builder.body(body.clone());
        }
    }

    let start = Instant::now();
    let resp = builder.send().await?;
    let status = resp.status();
    let status_text = status
        .canonical_reason()
        .unwrap_or("")
        .to_string();
    let headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
        .collect();
    let content_type = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone());
    let body = resp.text().await?;
    let duration_ms = start.elapsed().as_millis() as u64;
    let size_bytes = body.len();
    let looks_json = content_type
        .as_deref()
        .map(|c| c.contains("json"))
        .unwrap_or(false);
    let is_json = looks_json && serde_json::from_str::<serde_json::Value>(body.trim()).is_ok();

    Ok(HttpResponse {
        status: status.as_u16(),
        status_text,
        headers,
        body,
        content_type,
        is_json,
        duration_ms,
        size_bytes,
    })
}
