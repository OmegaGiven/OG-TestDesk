//! A minimal, enumerable cookie jar. reqwest's own `cookie::Jar` handles
//! sending/receiving cookies correctly but doesn't let you list or delete
//! individual cookies — needed here for an actual Cookie Manager UI, the
//! way Postman has one. Not aiming for full RFC 6265 compliance (no
//! expiry/Max-Age handling — every cookie is treated as a session cookie
//! that lives until cleared or the app restarts, and this jar is in-memory
//! only, not persisted to disk): enough to make login-flow cookies and
//! session tracking actually work across requests in one run of the app.

use reqwest::cookie::CookieStore;
use reqwest::header::HeaderValue;
use reqwest::Url;
use serde::Serialize;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
pub struct CookieRecord {
    pub domain: String,
    pub path: String,
    pub name: String,
    pub value: String,
    pub secure: bool,
    pub http_only: bool,
}

#[derive(Default)]
pub struct SharedCookieJar {
    store: Mutex<Vec<CookieRecord>>,
}

impl SharedCookieJar {
    pub fn list(&self) -> Vec<CookieRecord> {
        let mut v = self.store.lock().unwrap().clone();
        v.sort_by(|a, b| (a.domain.as_str(), a.name.as_str()).cmp(&(b.domain.as_str(), b.name.as_str())));
        v
    }
    pub fn clear(&self, domain: Option<&str>) {
        let mut s = self.store.lock().unwrap();
        match domain {
            Some(d) => s.retain(|c| c.domain != d),
            None => s.clear(),
        }
    }
    pub fn delete(&self, domain: &str, name: &str) {
        self.store.lock().unwrap().retain(|c| !(c.domain == domain && c.name == name));
    }
}

fn domain_matches(cookie_domain: &str, host: &str) -> bool {
    host == cookie_domain || host.ends_with(&format!(".{cookie_domain}"))
}

fn parse_set_cookie(s: &str, default_host: &str) -> Option<CookieRecord> {
    let mut parts = s.split(';').map(|p| p.trim());
    let first = parts.next()?;
    let (name, value) = first.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let mut domain = default_host.to_string();
    let mut path = "/".to_string();
    let mut secure = false;
    let mut http_only = false;
    for attr in parts {
        let lower = attr.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("domain=") {
            domain = v.trim_start_matches('.').to_string();
        } else if lower.starts_with("path=") {
            // Re-split the original-case attr (not `lower`) so the path
            // value itself keeps its case — only the "path=" prefix match
            // needed lowercasing.
            path = attr.splitn(2, '=').nth(1).unwrap_or("/").to_string();
        } else if lower == "secure" {
            secure = true;
        } else if lower == "httponly" {
            http_only = true;
        }
    }
    Some(CookieRecord {
        domain,
        path,
        name: name.to_string(),
        value: value.trim().to_string(),
        secure,
        http_only,
    })
}

impl CookieStore for SharedCookieJar {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        let host = url.host_str().unwrap_or("").to_string();
        let mut store = self.store.lock().unwrap();
        for header in cookie_headers {
            let Ok(s) = header.to_str() else { continue };
            let Some(rec) = parse_set_cookie(s, &host) else { continue };
            store.retain(|c| !(c.domain == rec.domain && c.path == rec.path && c.name == rec.name));
            store.push(rec);
        }
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        let host = url.host_str().unwrap_or("");
        let path = url.path();
        let store = self.store.lock().unwrap();
        let matched: Vec<String> = store
            .iter()
            .filter(|c| {
                domain_matches(&c.domain, host)
                    && path.starts_with(&c.path)
                    && (!c.secure || url.scheme() == "https")
            })
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        if matched.is_empty() {
            return None;
        }
        HeaderValue::from_str(&matched.join("; ")).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn stores_and_replays_a_simple_cookie() {
        let jar = SharedCookieJar::default();
        let header = HeaderValue::from_static("session=abc123; Path=/; HttpOnly");
        jar.set_cookies(&mut std::iter::once(&header), &url("https://api.example.com/login"));

        let sent = jar.cookies(&url("https://api.example.com/dashboard"));
        assert_eq!(sent.unwrap().to_str().unwrap(), "session=abc123");
    }

    #[test]
    fn respects_domain_scoping() {
        let jar = SharedCookieJar::default();
        let header = HeaderValue::from_static("token=xyz; Domain=example.com");
        jar.set_cookies(&mut std::iter::once(&header), &url("https://sub.example.com/x"));

        assert!(jar.cookies(&url("https://sub.example.com/y")).is_some());
        assert!(jar.cookies(&url("https://other.com/y")).is_none());
    }

    #[test]
    fn respects_secure_flag() {
        let jar = SharedCookieJar::default();
        let header = HeaderValue::from_static("s=1; Secure");
        jar.set_cookies(&mut std::iter::once(&header), &url("https://example.com/"));

        assert!(jar.cookies(&url("https://example.com/")).is_some());
        assert!(jar.cookies(&url("http://example.com/")).is_none());
    }

    #[test]
    fn list_clear_and_delete_work() {
        let jar = SharedCookieJar::default();
        let h1 = HeaderValue::from_static("a=1");
        let h2 = HeaderValue::from_static("b=2");
        jar.set_cookies(&mut std::iter::once(&h1), &url("https://one.com/"));
        jar.set_cookies(&mut std::iter::once(&h2), &url("https://two.com/"));
        assert_eq!(jar.list().len(), 2);

        jar.delete("one.com", "a");
        assert_eq!(jar.list().len(), 1);

        jar.clear(None);
        assert_eq!(jar.list().len(), 0);
    }
}
