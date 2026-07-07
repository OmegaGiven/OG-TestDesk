use iced::widget::{column, scrollable, text, text_input};
use iced::{Element, Task};
use serde_json::Value;

use og_testdesk_core::app_db::{get_json, put_json};

const COLLECTION: &str = "inspector";
const KEY: &str = "last_payload";

#[derive(Debug, Clone)]
pub enum InspectorMessage {
    Loaded(String),
    TextChanged(String),
    Saved,
}

pub struct InspectorTab {
    raw: String,
}

impl InspectorTab {
    pub fn new() -> (Self, Task<InspectorMessage>) {
        let task = Task::perform(
            async move { get_json::<String>(COLLECTION, KEY).await.unwrap_or_default() },
            InspectorMessage::Loaded,
        );
        (Self { raw: String::new() }, task)
    }

    pub fn update(&mut self, message: InspectorMessage) -> Task<InspectorMessage> {
        match message {
            InspectorMessage::Loaded(raw) => {
                self.raw = raw;
            }
            InspectorMessage::TextChanged(raw) => {
                self.raw = raw;
                let raw = self.raw.clone();
                return Task::perform(
                    async move {
                        let _ = put_json(COLLECTION, KEY, &raw).await;
                    },
                    |_| InspectorMessage::Saved,
                );
            }
            InspectorMessage::Saved => {}
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, InspectorMessage> {
        let (summary, pretty) = inspect(&self.raw);

        column![
            text("JSON Inspector").size(16),
            text_input("paste JSON here...", &self.raw).on_input(InspectorMessage::TextChanged),
            text(summary),
            scrollable(text(pretty)),
        ]
        .spacing(8)
        .padding(16)
        .into()
    }
}

/// Parse `raw` as JSON and return a one-line summary plus a pretty-printed
/// (or error) body for display.
fn inspect(raw: &str) -> (String, String) {
    if raw.trim().is_empty() {
        return (String::new(), String::new());
    }

    match serde_json::from_str::<Value>(raw) {
        Ok(value) => {
            let summary = summarize(&value);
            let pretty = serde_json::to_string_pretty(&value)
                .unwrap_or_else(|err| format!("Failed to pretty-print: {err}"));
            (summary, pretty)
        }
        Err(err) => (format!("Invalid JSON: {err}"), String::new()),
    }
}

fn summarize(value: &Value) -> String {
    match value {
        Value::Object(map) => format!(
            "Object with {} top-level key{}",
            map.len(),
            if map.len() == 1 { "" } else { "s" }
        ),
        Value::Array(items) => format!(
            "Array with {} item{}",
            items.len(),
            if items.len() == 1 { "" } else { "s" }
        ),
        Value::String(_) => "String".to_string(),
        Value::Number(_) => "Number".to_string(),
        Value::Bool(_) => "Boolean".to_string(),
        Value::Null => "Null".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_object() {
        let (summary, pretty) = inspect(r#"{"a": 1, "b": 2}"#);
        assert_eq!(summary, "Object with 2 top-level keys");
        assert!(pretty.contains("\"a\": 1"));
    }

    #[test]
    fn summarizes_array() {
        let (summary, _) = inspect(r#"[1, 2, 3]"#);
        assert_eq!(summary, "Array with 3 items");
    }

    #[test]
    fn reports_invalid_json() {
        let (summary, pretty) = inspect("{not json");
        assert!(summary.starts_with("Invalid JSON:"));
        assert!(pretty.is_empty());
    }

    #[test]
    fn empty_input_is_blank() {
        let (summary, pretty) = inspect("   ");
        assert!(summary.is_empty());
        assert!(pretty.is_empty());
    }
}
