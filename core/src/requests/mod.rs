pub mod cookiejar;

use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// One cookie jar shared by every request this app process sends — so a
/// login response's Set-Cookie actually gets replayed on the next
/// request, the way a browser (and Postman) does. In-memory only; see
/// cookiejar.rs's module doc for what's deliberately not implemented.
static COOKIE_JAR: OnceLock<Arc<cookiejar::SharedCookieJar>> = OnceLock::new();
pub fn cookie_jar() -> Arc<cookiejar::SharedCookieJar> {
    COOKIE_JAR
        .get_or_init(|| Arc::new(cookiejar::SharedCookieJar::default()))
        .clone()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Raw string body — still how a plain-text/JSON/XML request is sent.
    /// Ignored when `body_mode` is set to anything other than `Raw`/`None`.
    pub body: Option<String>,
    /// Structured body for anything raw text can't express. `None` means
    /// "use `body` as-is" — every existing caller (MCP, saved history
    /// replay, older saved requests) keeps working unchanged.
    #[serde(default)]
    pub body_mode: Option<RequestBody>,
    /// Seconds; defaults to 60 when absent.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// Every variant here is struct-like (named fields), even the ones that
// naturally feel like a single value (Raw, Binary) — serde can't
// serialize an internally-tagged enum's newtype/tuple variant unless its
// inner type itself serializes as a map, so `Raw(String)` or
// `FormUrlEncoded(Vec<FormField>)` would compile but panic at
// serde_json::to_string time. Caught by round-tripping this through JSON
// before wiring up the frontend, not just by it compiling.
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RequestBody {
    None,
    Raw { text: String },
    FormUrlEncoded { fields: Vec<FormField> },
    Multipart { fields: Vec<FormField> },
    /// Raw bytes (base64-encoded over the wire) as the whole body — a
    /// single file upload with no multipart envelope, or any other
    /// binary payload.
    Binary { base64: String },
    GraphQl { query: String, variables: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormField {
    pub key: String,
    /// "text" or "file". A file's `value` is base64-encoded content.
    pub kind: String,
    pub value: String,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    /// UTF-8 text normally; base64-encoded raw bytes when `is_binary` —
    /// decoding an image/PDF/etc. response as UTF-8 text (the previous
    /// behavior, unconditionally) silently mangled it.
    pub body: String,
    pub content_type: Option<String>,
    pub is_json: bool,
    #[serde(default)]
    pub is_binary: bool,
    pub duration_ms: u64,
    pub size_bytes: usize,
}

/// Content-types treated as text and decoded as UTF-8 (lossily, so a
/// wrong/missing charset doesn't turn into a hard error) rather than
/// base64-encoded. Deliberately a denylist-shaped allowlist: `text/*`
/// plus the handful of `application/*` types that are actually text in
/// practice (JSON, XML, JS, form-urlencoded) — anything else (images,
/// PDF, zip, protobuf, ...) is treated as binary.
fn is_text_content_type(ct: &str) -> bool {
    let ct = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    ct.starts_with("text/")
        || ct.ends_with("+json")
        || ct.ends_with("+xml")
        || matches!(
            ct.as_str(),
            "application/json"
                | "application/xml"
                | "application/javascript"
                | "application/ecmascript"
                | "application/x-www-form-urlencoded"
                | "application/graphql"
        )
}

/// Finds every `{{secret:<key>}}` placeholder referenced in a request's
/// url/headers/body and resolves each from the OS keychain/encrypted-
/// file `SecretsStore` (namespaced `authhdr:<key>`) into a map ready to
/// merge into `apply_environment`'s `vars` — the exact same
/// `{{var}}`-substitution mechanism already used for environment
/// variables, just sourced from the secrets store instead of the plain
/// metadata DB. This is how a saved request's Bearer/Basic/API-key/
/// OAuth2 auth value round-trips without that value ever being written
/// to disk in `headers_json` — the frontend stores a placeholder there
/// and the real value only ever lives in the secrets store, resolved
/// right before the request actually goes out. A key with no matching
/// secret (deleted, or the app's data moved machines) is silently
/// skipped — `apply_environment` then leaves that one placeholder
/// untouched, same as any other unknown `{{var}}`.
pub fn resolve_secret_placeholders(req: &HttpRequest) -> HashMap<String, String> {
    let mut keys = std::collections::HashSet::new();
    let mut scan = |s: &str| {
        let mut rest = s;
        while let Some(start) = rest.find("{{secret:") {
            let after = &rest[start + "{{secret:".len()..];
            if let Some(end) = after.find("}}") {
                keys.insert(after[..end].to_string());
                rest = &after[end + 2..];
            } else {
                break;
            }
        }
    };
    scan(&req.url);
    if let Some(b) = &req.body {
        scan(b);
    }
    for (k, v) in &req.headers {
        scan(k);
        scan(v);
    }
    let mut vars = HashMap::new();
    for key in keys {
        if let Ok(Some(val)) = crate::storage::secrets::SecretsStore::get(&format!("authhdr:{key}")) {
            vars.insert(format!("secret:{key}"), val);
        }
    }
    vars
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
    match &mut req.body_mode {
        Some(RequestBody::Raw { text }) => *text = sub(text),
        Some(RequestBody::FormUrlEncoded { fields }) | Some(RequestBody::Multipart { fields }) => {
            for f in fields.iter_mut() {
                f.key = sub(&f.key);
                if f.kind == "text" {
                    f.value = sub(&f.value);
                }
            }
        }
        Some(RequestBody::GraphQl { query, variables }) => {
            *query = sub(query);
            if let Some(v) = variables {
                *v = sub(v);
            }
        }
        _ => {}
    }
}

fn apply_body(
    mut builder: reqwest::RequestBuilder,
    req: &HttpRequest,
) -> Result<reqwest::RequestBuilder> {
    let Some(mode) = &req.body_mode else {
        if let Some(body) = &req.body {
            if !body.is_empty() {
                builder = builder.body(body.clone());
            }
        }
        return Ok(builder);
    };
    match mode {
        RequestBody::None => {}
        RequestBody::Raw { text } => {
            if !text.is_empty() {
                builder = builder.body(text.clone());
            }
        }
        RequestBody::FormUrlEncoded { fields } => {
            let pairs: Vec<(&str, &str)> = fields
                .iter()
                .filter(|f| f.enabled && !f.key.is_empty())
                .map(|f| (f.key.as_str(), f.value.as_str()))
                .collect();
            builder = builder.form(&pairs);
        }
        RequestBody::Multipart { fields } => {
            let mut form = reqwest::multipart::Form::new();
            for f in fields.iter().filter(|f| f.enabled && !f.key.is_empty()) {
                if f.kind == "file" {
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(&f.value)
                        .context("multipart field is not valid base64")?;
                    let mut part = reqwest::multipart::Part::bytes(bytes);
                    if let Some(name) = &f.filename {
                        part = part.file_name(name.clone());
                    }
                    if let Some(ct) = &f.content_type {
                        part = part.mime_str(ct).context("invalid content-type on multipart field")?;
                    }
                    form = form.part(f.key.clone(), part);
                } else {
                    form = form.text(f.key.clone(), f.value.clone());
                }
            }
            builder = builder.multipart(form);
        }
        RequestBody::Binary { base64: b64 } => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .context("binary body is not valid base64")?;
            builder = builder.body(bytes);
        }
        RequestBody::GraphQl { query, variables } => {
            let vars: serde_json::Value = variables
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .map(|v| serde_json::from_str(v))
                .transpose()
                .context("GraphQL variables aren't valid JSON")?
                .unwrap_or(serde_json::Value::Null);
            let payload = serde_json::json!({ "query": query, "variables": vars });
            builder = builder
                .header("content-type", "application/json")
                .body(payload.to_string());
        }
    }
    Ok(builder)
}

/// A client certificate to present for requests to `host` (and its
/// subdomains) — for mTLS-protected APIs. `pem` is the certificate and
/// its private key concatenated in one PEM blob (what reqwest's
/// `Identity::from_pem` expects).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCertEntry {
    pub host: String,
    pub pem: String,
}

/// Network-level settings that apply to every request, resolved once by
/// the caller (who has access to the settings store) and threaded
/// through rather than read from inside `send()` itself, which stays
/// storage-agnostic. `Default` = exactly today's behavior (no proxy, no
/// custom trust/identity).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkSettings {
    pub proxy_url: Option<String>,
    /// An extra CA certificate (PEM) to trust, for internal APIs behind
    /// a self-signed or private-CA cert — added alongside, not instead
    /// of, the normal system trust roots.
    pub extra_ca_pem: Option<String>,
    #[serde(default)]
    pub client_certs: Vec<ClientCertEntry>,
}

pub async fn send(req: &HttpRequest) -> Result<HttpResponse> {
    send_with(req, &NetworkSettings::default()).await
}

pub async fn send_with(req: &HttpRequest, net: &NetworkSettings) -> Result<HttpResponse> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(req.timeout_secs.unwrap_or(60)))
        .cookie_provider(cookie_jar());
    if let Some(proxy_url) = net.proxy_url.as_deref().filter(|p| !p.trim().is_empty()) {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy URL")?);
    }
    if let Some(ca_pem) = net.extra_ca_pem.as_deref().filter(|c| !c.trim().is_empty()) {
        let cert = reqwest::Certificate::from_pem(ca_pem.as_bytes()).context("invalid CA certificate PEM")?;
        builder = builder.add_root_certificate(cert);
    }
    if let Some(host) = reqwest::Url::parse(&req.url).ok().and_then(|u| u.host_str().map(str::to_string)) {
        if let Some(entry) = net
            .client_certs
            .iter()
            .find(|c| host == c.host || host.ends_with(&format!(".{}", c.host)))
        {
            let identity =
                reqwest::Identity::from_pem(entry.pem.as_bytes()).context("invalid client certificate PEM")?;
            builder = builder.identity(identity);
        }
    }
    let client = builder.build()?;
    let method = reqwest::Method::from_bytes(req.method.to_ascii_uppercase().as_bytes())?;
    let mut builder = client.request(method, &req.url);
    for (k, v) in &req.headers {
        if !k.trim().is_empty() {
            builder = builder.header(k, v);
        }
    }
    builder = apply_body(builder, req)?;

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
    let raw = resp.bytes().await?;
    let duration_ms = start.elapsed().as_millis() as u64;
    let size_bytes = raw.len();
    let is_binary = !content_type.as_deref().map(is_text_content_type).unwrap_or(true);
    let body = if is_binary {
        base64::engine::general_purpose::STANDARD.encode(&raw)
    } else {
        String::from_utf8_lossy(&raw).into_owned()
    };
    let looks_json = content_type
        .as_deref()
        .map(|c| c.contains("json"))
        .unwrap_or(false);
    let is_json = !is_binary && looks_json && serde_json::from_str::<serde_json::Value>(body.trim()).is_ok();

    Ok(HttpResponse {
        status: status.as_u16(),
        status_text,
        headers,
        body,
        content_type,
        is_json,
        is_binary,
        duration_ms,
        size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_text_content_type_classifies_common_types() {
        assert!(is_text_content_type("text/html; charset=utf-8"));
        assert!(is_text_content_type("application/json"));
        assert!(is_text_content_type("application/vnd.api+json"));
        assert!(is_text_content_type("application/xml"));
        assert!(is_text_content_type("application/javascript"));
        assert!(!is_text_content_type("image/png"));
        assert!(!is_text_content_type("application/pdf"));
        assert!(!is_text_content_type("application/octet-stream"));
        assert!(!is_text_content_type("application/zip"));
    }

    fn req(url: &str, headers: &[(&str, &str)], body: Option<&str>) -> HttpRequest {
        HttpRequest {
            method: "GET".into(),
            url: url.into(),
            headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            body: body.map(|s| s.to_string()),
            body_mode: None,
            timeout_secs: None,
        }
    }

    #[test]
    fn apply_environment_substitutes_url_headers_and_body() {
        let mut vars = HashMap::new();
        vars.insert("baseUrl".to_string(), "https://api.example.com".to_string());
        vars.insert("token".to_string(), "secret123".to_string());

        let mut r = req(
            "{{baseUrl}}/posts",
            &[("Authorization", "Bearer {{token}}")],
            Some("{\"owner\":\"{{token}}\"}"),
        );
        apply_environment(&mut r, &vars);

        assert_eq!(r.url, "https://api.example.com/posts");
        assert_eq!(r.headers.get("Authorization").unwrap(), "Bearer secret123");
        assert_eq!(r.body.unwrap(), "{\"owner\":\"secret123\"}");
    }

    #[test]
    fn apply_environment_leaves_unknown_placeholders_untouched() {
        let vars = HashMap::new();
        let mut r = req("{{baseUrl}}/posts", &[], None);
        apply_environment(&mut r, &vars);
        assert_eq!(r.url, "{{baseUrl}}/posts");
    }

    #[test]
    fn apply_environment_substitutes_in_header_names_too() {
        let mut vars = HashMap::new();
        vars.insert("hdr".to_string(), "X-Custom".to_string());
        let mut r = req("https://example.com", &[("{{hdr}}", "value")], None);
        apply_environment(&mut r, &vars);
        assert!(r.headers.contains_key("X-Custom"));
    }

    #[test]
    fn resolve_secret_placeholders_reads_the_secrets_store_not_the_metadata_db() {
        // Regression test for storing a live Bearer/Basic/API-key/OAuth2
        // token in headers_json in plaintext: the frontend is meant to
        // write only a `{{secret:<key>}}` placeholder there, with the
        // real value living in SecretsStore — this confirms the send
        // path actually resolves that placeholder back to the real
        // value right before the request goes out.
        let dir = std::env::temp_dir().join(format!("ogtd-secrets-test-{}", uuid::Uuid::new_v4()));
        crate::storage::secrets::SecretsStore::init_fallback(dir);
        crate::storage::secrets::SecretsStore::set("authhdr:tok1", "s3cr3t-bearer-value").unwrap();

        let mut r = req(
            "https://example.com",
            &[("Authorization", "Bearer {{secret:tok1}}")],
            None,
        );
        let vars = resolve_secret_placeholders(&r);
        assert_eq!(vars.get("secret:tok1").map(String::as_str), Some("s3cr3t-bearer-value"));
        apply_environment(&mut r, &vars);
        assert_eq!(r.headers.get("Authorization").unwrap(), "Bearer s3cr3t-bearer-value");
    }

    #[test]
    fn resolve_secret_placeholders_skips_a_missing_key() {
        let vars = resolve_secret_placeholders(&req(
            "https://example.com",
            &[("Authorization", "Bearer {{secret:does-not-exist}}")],
            None,
        ));
        assert!(vars.is_empty());
    }

    fn builder() -> reqwest::RequestBuilder {
        reqwest::Client::new().post("https://example.invalid")
    }

    #[test]
    fn apply_body_form_urlencoded_sets_body_and_content_type() {
        let mut r = req("https://example.invalid", &[], None);
        r.body_mode = Some(RequestBody::FormUrlEncoded { fields: vec![
            FormField { key: "a".into(), kind: "text".into(), value: "1".into(), filename: None, content_type: None, enabled: true },
            FormField { key: "b".into(), kind: "text".into(), value: "hello world".into(), filename: None, content_type: None, enabled: true },
            FormField { key: "skip".into(), kind: "text".into(), value: "x".into(), filename: None, content_type: None, enabled: false },
        ]});
        let built = apply_body(builder(), &r).unwrap().build().unwrap();
        let ct = built.headers().get("content-type").unwrap().to_str().unwrap();
        assert_eq!(ct, "application/x-www-form-urlencoded");
        let body_bytes = built.body().unwrap().as_bytes().unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert_eq!(body_str, "a=1&b=hello+world");
    }

    #[test]
    fn apply_body_multipart_sets_multipart_content_type() {
        let mut r = req("https://example.invalid", &[], None);
        r.body_mode = Some(RequestBody::Multipart { fields: vec![
            FormField { key: "name".into(), kind: "text".into(), value: "Ada".into(), filename: None, content_type: None, enabled: true },
            FormField {
                key: "file".into(), kind: "file".into(),
                value: base64::engine::general_purpose::STANDARD.encode(b"hello"),
                filename: Some("hi.txt".into()), content_type: Some("text/plain".into()), enabled: true,
            },
        ]});
        let built = apply_body(builder(), &r).unwrap().build().unwrap();
        let ct = built.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(ct.starts_with("multipart/form-data; boundary="), "got {ct}");
    }

    #[test]
    fn apply_body_binary_decodes_base64() {
        let mut r = req("https://example.invalid", &[], None);
        r.body_mode = Some(RequestBody::Binary {
            base64: base64::engine::general_purpose::STANDARD.encode(b"raw bytes here"),
        });
        let built = apply_body(builder(), &r).unwrap().build().unwrap();
        let body_bytes = built.body().unwrap().as_bytes().unwrap();
        assert_eq!(body_bytes, b"raw bytes here");
    }

    #[test]
    fn apply_body_binary_rejects_invalid_base64() {
        let mut r = req("https://example.invalid", &[], None);
        r.body_mode = Some(RequestBody::Binary { base64: "not valid base64!!".into() });
        assert!(apply_body(builder(), &r).is_err());
    }

    #[test]
    fn apply_body_graphql_wraps_query_and_variables_as_json() {
        let mut r = req("https://example.invalid", &[], None);
        r.body_mode = Some(RequestBody::GraphQl {
            query: "query { me { id } }".into(),
            variables: Some(r#"{"id":1}"#.into()),
        });
        let built = apply_body(builder(), &r).unwrap().build().unwrap();
        let ct = built.headers().get("content-type").unwrap().to_str().unwrap();
        assert_eq!(ct, "application/json");
        let body_bytes = built.body().unwrap().as_bytes().unwrap();
        let v: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(v["query"], "query { me { id } }");
        assert_eq!(v["variables"]["id"], 1);
    }

    // Regression test: an internally-tagged enum (`#[serde(tag = "kind")]`)
    // can't serialize a newtype/tuple variant whose inner type isn't
    // itself a map — `FormUrlEncoded(Vec<FormField>)` compiles fine but
    // panics at serde_json::to_string time, which would only ever surface
    // the moment the frontend actually sent one over Tauri's JSON IPC.
    // Every RequestBody variant is struct-like specifically so this holds.
    #[test]
    fn request_body_json_round_trips_over_the_wire_shape() {
        let rb = RequestBody::FormUrlEncoded {
            fields: vec![FormField { key: "a".into(), kind: "text".into(), value: "1".into(), filename: None, content_type: None, enabled: true }],
        };
        let s = serde_json::to_string(&rb).expect("RequestBody must be serde_json-serializable");
        assert!(s.contains("\"kind\":\"form_url_encoded\""), "got {s}");
        let back: RequestBody = serde_json::from_str(&s).unwrap();
        match back {
            RequestBody::FormUrlEncoded { fields } => assert_eq!(fields[0].key, "a"),
            _ => panic!("wrong variant"),
        }
    }
}
