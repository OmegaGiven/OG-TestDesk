use serde::{Deserialize, Serialize};

/// Cap far above the original app's 12-entry `<select>` dropdown — that
/// was an explicit UX gap called out in the design doc, not a limit worth
/// preserving.
pub const HISTORY_CAP: usize = 150;

/// Snapshot bodies are truncated at this many bytes so a handful of huge
/// responses can't bloat the persisted history blob indefinitely.
const MAX_SNAPSHOT_BYTES: usize = 200_000;

#[derive(Clone, Serialize, Deserialize)]
pub struct RequestSnapshot {
    pub method: String,
    pub url: String,
    pub headers: String,
    pub body: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ResponseSnapshot {
    pub status: u16,
    pub headers: String,
    pub body: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub timestamp_ms: u64,
    pub request: RequestSnapshot,
    pub response: Option<ResponseSnapshot>,
    pub error: Option<String>,
}

fn truncate_snapshot_text(text: &str) -> String {
    if text.len() <= MAX_SNAPSHOT_BYTES {
        return text.to_string();
    }
    let mut end = MAX_SNAPSHOT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}... [truncated]", &text[..end])
}

impl RequestSnapshot {
    pub fn new(method: &str, url: &str, headers: &str, body: &str) -> Self {
        Self {
            method: method.to_string(),
            url: url.to_string(),
            headers: headers.to_string(),
            body: truncate_snapshot_text(body),
        }
    }
}

impl ResponseSnapshot {
    pub fn new(status: u16, headers: &str, body: &str) -> Self {
        Self {
            status,
            headers: headers.to_string(),
            body: truncate_snapshot_text(body),
        }
    }
}

/// Parses a history blob loaded via `core::requests::get_request_history`.
/// Returns an empty list for anything that doesn't deserialize (e.g. the
/// very first run, when the stored value is `Value::Null`).
pub fn entries_from_value(value: &serde_json::Value) -> Vec<HistoryEntry> {
    serde_json::from_value::<Vec<HistoryEntry>>(value.clone()).unwrap_or_default()
}

/// Serializes a history list for `core::requests::save_request_history`.
pub fn entries_to_value(entries: &[HistoryEntry]) -> serde_json::Value {
    serde_json::to_value(entries).unwrap_or_else(|_| serde_json::Value::Array(Vec::new()))
}

/// Inserts `entry` as the newest (front of the list), evicting the oldest
/// entries once the list exceeds `cap`.
pub fn push_capped(entries: &mut Vec<HistoryEntry>, entry: HistoryEntry, cap: usize) {
    entries.insert(0, entry);
    if entries.len() > cap {
        entries.truncate(cap);
    }
}

/// Case-insensitive substring match against method or URL. An empty
/// filter matches everything.
pub fn matches_filter(entry: &HistoryEntry, filter: &str) -> bool {
    if filter.trim().is_empty() {
        return true;
    }
    let filter = filter.to_lowercase();
    entry.request.method.to_lowercase().contains(&filter) || entry.request.url.to_lowercase().contains(&filter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> HistoryEntry {
        HistoryEntry {
            id: id.to_string(),
            timestamp_ms: 0,
            request: RequestSnapshot::new("GET", "https://example.com/users", "Accept: application/json", ""),
            response: Some(ResponseSnapshot::new(200, "Content-Type: application/json", "{\"ok\":true}")),
            error: None,
        }
    }

    #[test]
    fn round_trips_through_value() {
        let entries = vec![sample("a"), sample("b")];
        let value = entries_to_value(&entries);
        let reloaded = entries_from_value(&value);
        assert_eq!(reloaded.len(), 2);
        assert_eq!(reloaded[0].id, "a");
        assert_eq!(reloaded[1].request.method, "GET");
        assert_eq!(reloaded[0].response.as_ref().unwrap().status, 200);
    }

    #[test]
    fn entries_from_value_on_null_is_empty() {
        assert!(entries_from_value(&serde_json::Value::Null).is_empty());
    }

    #[test]
    fn push_capped_evicts_oldest_when_over_cap() {
        let mut entries = Vec::new();
        for i in 0..5 {
            push_capped(&mut entries, sample(&i.to_string()), 3);
        }
        // Newest-first: the 3 most recently pushed (2, 3, 4) survive; the
        // oldest (0, 1) are evicted.
        assert_eq!(entries.len(), 3);
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["4", "3", "2"]);
    }

    #[test]
    fn push_capped_under_cap_keeps_all() {
        let mut entries = Vec::new();
        push_capped(&mut entries, sample("a"), 10);
        push_capped(&mut entries, sample("b"), 10);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn matches_filter_empty_matches_everything() {
        assert!(matches_filter(&sample("a"), ""));
        assert!(matches_filter(&sample("a"), "   "));
    }

    #[test]
    fn matches_filter_matches_method_or_url_case_insensitively() {
        let entry = sample("a");
        assert!(matches_filter(&entry, "get"));
        assert!(matches_filter(&entry, "USERS"));
        assert!(!matches_filter(&entry, "nonexistent"));
    }

    #[test]
    fn truncate_snapshot_text_leaves_short_text_untouched() {
        let short = "a short body";
        assert_eq!(RequestSnapshot::new("GET", "url", "", short).body, short);
    }

    #[test]
    fn truncate_snapshot_text_truncates_long_text() {
        let long = "x".repeat(MAX_SNAPSHOT_BYTES + 100);
        let snapshot = RequestSnapshot::new("GET", "url", "", &long);
        assert!(snapshot.body.len() < long.len());
        assert!(snapshot.body.ends_with("... [truncated]"));
    }
}
