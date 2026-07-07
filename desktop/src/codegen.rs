//! Request-equivalent code-snippet generation (curl already lives in
//! `curl_import.rs::generate_curl_command` — this covers the other
//! languages referenced in the design doc: Rust `reqwest` and JS `fetch`).
//! Each function takes the same shape as `generate_curl_command` for
//! consistency: method, resolved URL, headers, optional body.

/// Generates a blocking `reqwest` snippet. Blocking (not async) keeps the
/// generated snippet self-contained — no runtime/executor ceremony needed
/// for a copy-pasted example.
pub fn generate_rust_reqwest(method: &str, url: &str, headers: &[(String, String)], body: Option<&str>) -> String {
    let mut lines = vec![
        "let client = reqwest::blocking::Client::new();".to_string(),
        format!(
            "let mut request = client.request(reqwest::Method::{}, \"{}\");",
            method.to_uppercase(),
            escape_rust_string(url)
        ),
    ];
    for (k, v) in headers {
        lines.push(format!(
            "request = request.header(\"{}\", \"{}\");",
            escape_rust_string(k),
            escape_rust_string(v)
        ));
    }
    if let Some(b) = body.filter(|b| !b.is_empty()) {
        lines.push(format!("request = request.body(\"{}\");", escape_rust_string(b)));
    }
    lines.push("let response = request.send()?;".to_string());
    lines.join("\n")
}

/// Generates a browser/Node `fetch` snippet.
pub fn generate_js_fetch(method: &str, url: &str, headers: &[(String, String)], body: Option<&str>) -> String {
    let mut options = vec![format!("  method: \"{}\"", method.to_uppercase())];

    if !headers.is_empty() {
        let header_lines = headers
            .iter()
            .map(|(k, v)| format!("    \"{}\": \"{}\"", escape_js_string(k), escape_js_string(v)))
            .collect::<Vec<_>>()
            .join(",\n");
        options.push(format!("  headers: {{\n{header_lines}\n  }}"));
    }

    if let Some(b) = body.filter(|b| !b.is_empty()) {
        options.push(format!("  body: \"{}\"", escape_js_string(b)));
    }

    format!(
        "fetch(\"{}\", {{\n{}\n}})\n  .then(response => response.text())\n  .then(console.log);",
        escape_js_string(url),
        options.join(",\n")
    )
}

fn escape_rust_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

fn escape_js_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// A single cookie parsed from a Netscape-format cookie jar file: tab-
/// separated `domain, include-subdomains-flag, path, secure-flag,
/// expiration (unix seconds), name, value`. Lines starting with `#`
/// (comments, including curl's `#HttpOnly_` prefix variant) and blank
/// lines are skipped, except `#HttpOnly_<domain>` lines which curl uses to
/// mark a cookie HttpOnly by prefixing the domain field itself — treated
/// like a normal entry with the prefix stripped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieEntry {
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub expires: i64,
    pub name: String,
    pub value: String,
}

pub fn parse_netscape_cookie_jar(contents: &str) -> Vec<CookieEntry> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim_end();
            if line.is_empty() {
                return None;
            }
            let (domain_field, is_http_only) = if let Some(stripped) = line.strip_prefix("#HttpOnly_") {
                (stripped, true)
            } else if line.starts_with('#') {
                return None;
            } else {
                (line, false)
            };
            let _ = is_http_only;
            let fields: Vec<&str> = domain_field.split('\t').collect();
            if fields.len() < 7 {
                return None;
            }
            Some(CookieEntry {
                domain: fields[0].to_string(),
                path: fields[2].to_string(),
                secure: fields[3].eq_ignore_ascii_case("TRUE"),
                expires: fields[4].parse().unwrap_or(0),
                name: fields[5].to_string(),
                value: fields[6].to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_snippet_includes_method_url_headers_body() {
        let headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        let snippet = generate_rust_reqwest("POST", "https://api.example.com/x", &headers, Some(r#"{"a":1}"#));
        assert!(snippet.contains("reqwest::Method::POST"));
        assert!(snippet.contains("https://api.example.com/x"));
        assert!(snippet.contains("Content-Type"));
        assert!(snippet.contains(r#"{\"a\":1}"#));
        assert!(snippet.contains(".send()?;"));
    }

    #[test]
    fn rust_snippet_omits_body_when_absent() {
        let snippet = generate_rust_reqwest("GET", "https://example.com", &[], None);
        assert!(!snippet.contains(".body("));
    }

    #[test]
    fn rust_snippet_omits_body_when_empty() {
        let snippet = generate_rust_reqwest("GET", "https://example.com", &[], Some(""));
        assert!(!snippet.contains(".body("));
    }

    #[test]
    fn js_snippet_includes_method_url_headers_body() {
        let headers = vec![("X-Api-Key".to_string(), "abc123".to_string())];
        let snippet = generate_js_fetch("PUT", "https://api.example.com/y", &headers, Some("payload"));
        assert!(snippet.contains("method: \"PUT\""));
        assert!(snippet.contains("https://api.example.com/y"));
        assert!(snippet.contains("X-Api-Key"));
        assert!(snippet.contains("body: \"payload\""));
    }

    #[test]
    fn js_snippet_omits_headers_object_when_no_headers() {
        let snippet = generate_js_fetch("GET", "https://example.com", &[], None);
        assert!(!snippet.contains("headers:"));
        assert!(!snippet.contains("body:"));
    }

    #[test]
    fn escapes_embedded_quotes_and_backslashes() {
        let snippet = generate_rust_reqwest("GET", "https://example.com", &[], Some(r#"say "hi""#));
        assert!(snippet.contains(r#"say \"hi\""#));
    }

    #[test]
    fn parses_simple_netscape_jar() {
        let jar = "# Netscape HTTP Cookie File\n\
                   # https://curl.se/docs/http-cookies.html\n\
                   \n\
                   httpbin.org\tFALSE\t/\tFALSE\t0\ttestcookie\t1\n";
        let cookies = parse_netscape_cookie_jar(jar);
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].domain, "httpbin.org");
        assert_eq!(cookies[0].path, "/");
        assert!(!cookies[0].secure);
        assert_eq!(cookies[0].name, "testcookie");
        assert_eq!(cookies[0].value, "1");
    }

    #[test]
    fn parses_multiple_cookies_and_secure_flag() {
        let jar = "example.com\tTRUE\t/app\tTRUE\t1999999999\tsession\tabc123\n\
                   other.com\tFALSE\t/\tFALSE\t0\tfoo\tbar\n";
        let cookies = parse_netscape_cookie_jar(jar);
        assert_eq!(cookies.len(), 2);
        assert!(cookies[0].secure);
        assert_eq!(cookies[0].expires, 1999999999);
        assert_eq!(cookies[1].name, "foo");
    }

    #[test]
    fn empty_jar_parses_to_no_cookies() {
        assert!(parse_netscape_cookie_jar("").is_empty());
        assert!(parse_netscape_cookie_jar("# just a comment\n").is_empty());
    }

    #[test]
    fn strips_http_only_prefix_from_domain() {
        let jar = "#HttpOnly_example.com\tFALSE\t/\tFALSE\t0\tsecure_cookie\tval\n";
        let cookies = parse_netscape_cookie_jar(jar);
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].domain, "example.com");
    }
}
