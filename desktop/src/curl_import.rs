//! Curl command <-> request-builder conversion: parsing a pasted `curl ...`
//! command into method/URL/headers/body, and generating an equivalent
//! command from the current builder state ("View as curl").

/// Result of parsing a `curl ...` command string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCurl {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// Tokenizes a shell-like command line: whitespace-separated words, with
/// single-quoted spans taken literally and double-quoted spans supporting
/// `\"` / `\\` escapes. A bare backslash outside quotes escapes the next
/// character. Not a full shell parser — enough for typical copy-pasted
/// curl commands.
fn tokenize_shell_like(input: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut has_current = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if has_current {
                    tokens.push(std::mem::take(&mut current));
                    has_current = false;
                }
            }
            '\'' => {
                has_current = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(inner) => current.push(inner),
                        None => return Err("Unterminated single-quoted string.".to_string()),
                    }
                }
            }
            '"' => {
                has_current = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(escaped @ ('"' | '\\' | '$' | '`')) => current.push(escaped),
                            Some(other) => {
                                current.push('\\');
                                current.push(other);
                            }
                            None => return Err("Unterminated double-quoted string.".to_string()),
                        },
                        Some(inner) => current.push(inner),
                        None => return Err("Unterminated double-quoted string.".to_string()),
                    }
                }
            }
            '\\' => {
                has_current = true;
                match chars.next() {
                    Some(escaped) => current.push(escaped),
                    None => return Err("Trailing backslash in command.".to_string()),
                }
            }
            other => {
                has_current = true;
                current.push(other);
            }
        }
    }
    if has_current {
        tokens.push(current);
    }
    Ok(tokens)
}

/// Parses a pasted `curl ...` command into its method/URL/headers/body.
/// Covers `-X`/`--request`, `-H`/`--header`, `-d`/`--data`/`--data-raw`/
/// `--data-binary`, `--url`, and a bare positional URL argument. Unknown
/// flags are skipped conservatively (without consuming the next token, so
/// an unrecognized flag can't accidentally swallow the URL). Defaults to
/// `POST` when a data flag is present with no explicit method, `GET`
/// otherwise, matching curl's own behavior.
pub fn parse_curl_command(input: &str) -> Result<ParsedCurl, String> {
    let normalized = input.replace("\\\r\n", " ").replace("\\\n", " ");
    let tokens = tokenize_shell_like(&normalized)?;
    if tokens.is_empty() {
        return Err("Empty curl command.".to_string());
    }

    let mut iter = tokens.into_iter().peekable();
    if iter.peek().map(|t| t.eq_ignore_ascii_case("curl")).unwrap_or(false) {
        iter.next();
    }

    let mut method: Option<String> = None;
    let mut url: Option<String> = None;
    let mut headers = Vec::new();
    let mut body: Option<String> = None;
    let mut had_data = false;

    while let Some(tok) = iter.next() {
        match tok.as_str() {
            "-X" | "--request" => method = iter.next(),
            "-H" | "--header" => {
                if let Some(h) = iter.next() {
                    if let Some((k, v)) = h.split_once(':') {
                        headers.push((k.trim().to_string(), v.trim().to_string()));
                    }
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii" => {
                if let Some(d) = iter.next() {
                    body = Some(d);
                    had_data = true;
                }
            }
            "--url" => url = iter.next(),
            t if t.starts_with('-') => {
                // Unknown flag (e.g. -s, -sS, -k, --compressed, -v). Skip
                // it without consuming the next token, since we don't know
                // whether it takes a value — safer to risk leaving a flag
                // unhandled than to accidentally eat the URL.
            }
            _ => {
                if url.is_none() {
                    url = Some(tok);
                }
            }
        }
    }

    let url = url.ok_or_else(|| "Could not find a URL in the curl command.".to_string())?;
    let method = method
        .unwrap_or_else(|| if had_data { "POST".to_string() } else { "GET".to_string() })
        .to_uppercase();

    Ok(ParsedCurl { method, url, headers, body })
}

/// Wraps a value in single quotes for safe shell embedding, escaping any
/// embedded single quotes as `'\''` (standard POSIX shell technique: close
/// the quote, emit an escaped quote, reopen the quote).
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Generates an equivalent `curl` command for the given request state.
pub fn generate_curl_command(method: &str, url: &str, headers: &[(String, String)], body: Option<&str>) -> String {
    let mut parts = vec!["curl".to_string(), "-X".to_string(), method.to_string(), shell_quote(url)];
    for (k, v) in headers {
        parts.push("-H".to_string());
        parts.push(shell_quote(&format!("{k}: {v}")));
    }
    if let Some(b) = body {
        if !b.is_empty() {
            parts.push("--data".to_string());
            parts.push(shell_quote(b));
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_get() {
        let parsed = parse_curl_command("curl https://example.com/api").unwrap();
        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.url, "https://example.com/api");
        assert!(parsed.headers.is_empty());
        assert!(parsed.body.is_none());
    }

    #[test]
    fn parses_post_with_header_and_data() {
        let parsed = parse_curl_command(
            r#"curl -X POST https://api.example.com/x -H 'Content-Type: application/json' -d '{"a":1}'"#,
        )
        .unwrap();
        assert_eq!(parsed.method, "POST");
        assert_eq!(parsed.url, "https://api.example.com/x");
        assert_eq!(parsed.headers, vec![("Content-Type".to_string(), "application/json".to_string())]);
        assert_eq!(parsed.body.as_deref(), Some(r#"{"a":1}"#));
    }

    #[test]
    fn defaults_to_post_when_data_present_without_explicit_method() {
        let parsed = parse_curl_command("curl https://api.example.com -d 'a=1'").unwrap();
        assert_eq!(parsed.method, "POST");
    }

    #[test]
    fn defaults_to_get_when_no_data_and_no_method() {
        let parsed = parse_curl_command("curl https://api.example.com").unwrap();
        assert_eq!(parsed.method, "GET");
    }

    #[test]
    fn handles_double_quoted_arguments_with_spaces() {
        let parsed = parse_curl_command(
            r#"curl "https://api.example.com/search?q=hello world" -H "Authorization: Bearer abc def""#,
        )
        .unwrap();
        assert_eq!(parsed.url, "https://api.example.com/search?q=hello world");
        assert_eq!(parsed.headers, vec![("Authorization".to_string(), "Bearer abc def".to_string())]);
    }

    #[test]
    fn skips_unknown_no_value_flags_without_eating_url() {
        let parsed = parse_curl_command("curl -sS -k https://api.example.com").unwrap();
        assert_eq!(parsed.url, "https://api.example.com");
    }

    #[test]
    fn errors_on_missing_url() {
        assert!(parse_curl_command("curl -X POST").is_err());
    }

    #[test]
    fn errors_on_empty_input() {
        assert!(parse_curl_command("").is_err());
        assert!(parse_curl_command("   ").is_err());
    }

    #[test]
    fn errors_on_unterminated_quote() {
        assert!(parse_curl_command("curl 'https://example.com").is_err());
    }

    #[test]
    fn shell_quote_escapes_embedded_single_quote() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn round_trip_generate_then_parse() {
        let headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("X-Api-Key".to_string(), "abc123".to_string()),
        ];
        let generated = generate_curl_command("POST", "https://api.example.com/things", &headers, Some(r#"{"x":1}"#));
        let parsed = parse_curl_command(&generated).unwrap();
        assert_eq!(parsed.method, "POST");
        assert_eq!(parsed.url, "https://api.example.com/things");
        assert_eq!(parsed.headers, headers);
        assert_eq!(parsed.body.as_deref(), Some(r#"{"x":1}"#));
    }

    #[test]
    fn round_trip_handles_embedded_single_quote_in_body() {
        let generated = generate_curl_command("POST", "https://api.example.com", &[], Some("it's a test"));
        let parsed = parse_curl_command(&generated).unwrap();
        assert_eq!(parsed.body.as_deref(), Some("it's a test"));
    }

    #[test]
    fn generate_omits_data_flag_for_empty_body() {
        let cmd = generate_curl_command("GET", "https://example.com", &[], Some(""));
        assert!(!cmd.contains("--data"));
    }
}
