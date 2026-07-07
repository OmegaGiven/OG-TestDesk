//! Pure string-manipulation helpers for the Requests builder's URL bar:
//! bidirectional query-param <-> row sync, and `{path}`-style path variable
//! detection/substitution (distinct from the app's `{{env}}` double-brace
//! variable syntax used elsewhere).

use crate::request_kv_editor::KvRow;

/// Splits a URL into its base (scheme/host/path, no `?...`) and query rows.
pub fn parse_query_params(url: &str) -> (String, Vec<KvRow>) {
    match url.split_once('?') {
        None => (url.to_string(), Vec::new()),
        Some((base, query)) => {
            let rows = query
                .split('&')
                .filter(|pair| !pair.is_empty())
                .map(|pair| match pair.split_once('=') {
                    Some((k, v)) => KvRow::new(urldecode(k), urldecode(v)),
                    None => KvRow::new(urldecode(pair), ""),
                })
                .collect();
            (base.to_string(), rows)
        }
    }
}

/// Rebuilds a full URL from a base and the editor's rows, keeping only
/// enabled rows with a non-empty key.
pub fn build_url_with_params(base: &str, rows: &[KvRow]) -> String {
    let pairs: Vec<String> = rows
        .iter()
        .filter(|r| r.enabled && !r.key.trim().is_empty())
        .map(|r| format!("{}={}", urlencode(&r.key), urlencode(&r.value)))
        .collect();
    if pairs.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", pairs.join("&"))
    }
}

pub(crate) fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Scans for single-brace `{name}` path-variable tokens, skipping
/// double-brace `{{name}}` environment-variable spans entirely. Returns
/// distinct names in first-seen order.
pub fn scan_path_variables(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'{' {
            if i + 1 < len && bytes[i + 1] == b'{' {
                match s[i + 2..].find("}}") {
                    Some(rel) => {
                        i = i + 2 + rel + 2;
                        continue;
                    }
                    None => break,
                }
            }
            match s[i + 1..].find('}') {
                Some(rel) => {
                    let name = &s[i + 1..i + 1 + rel];
                    if !name.is_empty() && !name.contains('{') && !result.contains(&name.to_string())
                    {
                        result.push(name.to_string());
                    }
                    i = i + 1 + rel + 1;
                    continue;
                }
                None => break,
            }
        }
        i += 1;
    }
    result
}

/// Replaces single-brace `{name}` tokens with their resolved value, leaving
/// double-brace `{{name}}` spans untouched for the separate env-var
/// substitution pass. Missing values are left as the literal `{name}` text.
pub fn substitute_path_variables(s: &str, values: &std::collections::HashMap<String, String>) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'{' {
            if i + 1 < len && bytes[i + 1] == b'{' {
                match s[i + 2..].find("}}") {
                    Some(rel) => {
                        out.push_str(&s[i..i + 2 + rel + 2]);
                        i = i + 2 + rel + 2;
                        continue;
                    }
                    None => {
                        out.push_str(&s[i..]);
                        break;
                    }
                }
            }
            match s[i + 1..].find('}') {
                Some(rel) => {
                    let name = &s[i + 1..i + 1 + rel];
                    match values.get(name) {
                        Some(v) => out.push_str(v),
                        None => out.push_str(&s[i..i + 1 + rel + 1]),
                    }
                    i = i + 1 + rel + 1;
                    continue;
                }
                None => {
                    out.push_str(&s[i..]);
                    break;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn parse_query_params_splits_base_and_rows() {
        let (base, rows) = parse_query_params("https://x.com/a?foo=1&bar=two");
        assert_eq!(base, "https://x.com/a");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key, "foo");
        assert_eq!(rows[0].value, "1");
        assert_eq!(rows[1].key, "bar");
        assert_eq!(rows[1].value, "two");
    }

    #[test]
    fn parse_query_params_no_query_returns_empty_rows() {
        let (base, rows) = parse_query_params("https://x.com/a");
        assert_eq!(base, "https://x.com/a");
        assert!(rows.is_empty());
    }

    #[test]
    fn build_url_with_params_round_trips() {
        let rows = vec![KvRow::new("foo", "1"), KvRow::new("bar", "two")];
        let url = build_url_with_params("https://x.com/a", &rows);
        assert_eq!(url, "https://x.com/a?foo=1&bar=two");
    }

    #[test]
    fn build_url_with_params_skips_disabled_and_empty_key() {
        let rows = vec![
            KvRow::new("foo", "1"),
            KvRow { enabled: false, key: "bar".into(), value: "2".into() },
            KvRow::new("", "3"),
        ];
        let url = build_url_with_params("https://x.com/a", &rows);
        assert_eq!(url, "https://x.com/a?foo=1");
    }

    #[test]
    fn build_url_with_params_no_rows_returns_base() {
        assert_eq!(build_url_with_params("https://x.com/a", &[]), "https://x.com/a");
    }

    #[test]
    fn scan_path_variables_ignores_double_brace_env_vars() {
        let names = scan_path_variables("{{host}}/users/{id}");
        assert_eq!(names, vec!["id".to_string()]);
    }

    #[test]
    fn scan_path_variables_finds_multiple_in_order_deduped() {
        let names = scan_path_variables("/a/{id}/b/{slug}/c/{id}");
        assert_eq!(names, vec!["id".to_string(), "slug".to_string()]);
    }

    #[test]
    fn scan_path_variables_no_braces_returns_empty() {
        assert!(scan_path_variables("/a/b/c").is_empty());
    }

    #[test]
    fn scan_path_variables_only_env_vars_returns_empty() {
        assert!(scan_path_variables("{{host}}/{{path}}").is_empty());
    }

    #[test]
    fn substitute_path_variables_replaces_known_leaves_env_vars() {
        let mut values = HashMap::new();
        values.insert("id".to_string(), "42".to_string());
        let result = substitute_path_variables("{{host}}/users/{id}", &values);
        assert_eq!(result, "{{host}}/users/42");
    }

    #[test]
    fn substitute_path_variables_leaves_missing_value_as_literal() {
        let values = HashMap::new();
        let result = substitute_path_variables("/users/{id}", &values);
        assert_eq!(result, "/users/{id}");
    }
}
