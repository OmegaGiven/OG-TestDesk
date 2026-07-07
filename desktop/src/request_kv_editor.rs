//! Reusable enabled/key/value row editor, shared by the Params, Headers,
//! Form Data, and URL-encoded builder sections of the Requests tab.

use iced::widget::{button, checkbox, row, text, text_input};
use iced::{Element, Length};

#[derive(Debug, Clone, Default)]
pub struct KvRow {
    pub enabled: bool,
    pub key: String,
    pub value: String,
}

impl KvRow {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            enabled: true,
            key: key.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum KvEditorMessage {
    ToggleEnabled(usize),
    KeyChanged(usize, String),
    ValueChanged(usize, String),
    RemoveRow(usize),
    AddRow,
}

/// Applies a [`KvEditorMessage`] to `rows`, appending a trailing blank row
/// automatically so there's always an empty row available to type into.
pub fn update(rows: &mut Vec<KvRow>, message: KvEditorMessage) {
    match message {
        KvEditorMessage::ToggleEnabled(i) => {
            if let Some(row) = rows.get_mut(i) {
                row.enabled = !row.enabled;
            }
        }
        KvEditorMessage::KeyChanged(i, v) => {
            if let Some(row) = rows.get_mut(i) {
                row.key = v;
            }
        }
        KvEditorMessage::ValueChanged(i, v) => {
            if let Some(row) = rows.get_mut(i) {
                row.value = v;
            }
        }
        KvEditorMessage::RemoveRow(i) => {
            if i < rows.len() {
                rows.remove(i);
            }
        }
        KvEditorMessage::AddRow => rows.push(KvRow::default()),
    }
    ensure_trailing_blank_row(rows);
}

/// Keeps exactly one trailing blank (empty key+value) row at the end so the
/// editor always has somewhere new to type, without accumulating multiple
/// blank rows.
pub fn ensure_trailing_blank_row(rows: &mut Vec<KvRow>) {
    while rows.len() >= 2 {
        let last_blank = rows.last().is_some_and(|r| r.key.is_empty() && r.value.is_empty());
        let second_last_blank = rows
            .get(rows.len() - 2)
            .is_some_and(|r| r.key.is_empty() && r.value.is_empty());
        if last_blank && second_last_blank {
            rows.pop();
        } else {
            break;
        }
    }
    if rows.is_empty() || !rows.last().is_some_and(|r| r.key.is_empty() && r.value.is_empty()) {
        rows.push(KvRow::default());
    }
}

/// Rows with a non-empty key that are enabled — the set that actually
/// contributes to the outgoing request.
pub fn active_pairs(rows: &[KvRow]) -> Vec<(String, String)> {
    rows.iter()
        .filter(|r| r.enabled && !r.key.trim().is_empty())
        .map(|r| (r.key.clone(), r.value.clone()))
        .collect()
}

pub fn view<'a>(rows: &'a [KvRow]) -> Element<'a, KvEditorMessage> {
    let mut col = iced::widget::column![].spacing(4);
    for (i, kv) in rows.iter().enumerate() {
        col = col.push(
            row![
                checkbox("", kv.enabled).on_toggle(move |_| KvEditorMessage::ToggleEnabled(i)),
                text_input("key", &kv.key)
                    .on_input(move |v| KvEditorMessage::KeyChanged(i, v))
                    .width(Length::FillPortion(1)),
                text_input("value", &kv.value)
                    .on_input(move |v| KvEditorMessage::ValueChanged(i, v))
                    .width(Length::FillPortion(1)),
                button(text("x")).on_press(KvEditorMessage::RemoveRow(i)),
            ]
            .spacing(6),
        );
    }
    col.push(button(text("+ Add row")).on_press(KvEditorMessage::AddRow))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_pairs_skips_disabled_and_empty_key_rows() {
        let rows = vec![
            KvRow { enabled: true, key: "a".into(), value: "1".into() },
            KvRow { enabled: false, key: "b".into(), value: "2".into() },
            KvRow { enabled: true, key: "".into(), value: "3".into() },
        ];
        assert_eq!(active_pairs(&rows), vec![("a".to_string(), "1".to_string())]);
    }

    #[test]
    fn ensure_trailing_blank_row_adds_one_when_missing() {
        let mut rows = vec![KvRow::new("a", "1")];
        ensure_trailing_blank_row(&mut rows);
        assert_eq!(rows.len(), 2);
        assert!(rows[1].key.is_empty() && rows[1].value.is_empty());
    }

    #[test]
    fn ensure_trailing_blank_row_collapses_multiple_blanks() {
        let mut rows = vec![KvRow::new("a", "1"), KvRow::default(), KvRow::default()];
        ensure_trailing_blank_row(&mut rows);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn ensure_trailing_blank_row_noop_on_single_blank() {
        let mut rows = vec![KvRow::new("a", "1"), KvRow::default()];
        ensure_trailing_blank_row(&mut rows);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn update_remove_row_then_reensures_blank() {
        let mut rows = vec![KvRow::new("a", "1"), KvRow::default()];
        update(&mut rows, KvEditorMessage::RemoveRow(0));
        assert_eq!(rows.len(), 1);
        assert!(rows[0].key.is_empty());
    }

    #[test]
    fn update_toggle_and_edit() {
        let mut rows = vec![KvRow::new("a", "1")];
        update(&mut rows, KvEditorMessage::ToggleEnabled(0));
        assert!(!rows[0].enabled);
        update(&mut rows, KvEditorMessage::KeyChanged(0, "b".into()));
        assert_eq!(rows[0].key, "b");
        update(&mut rows, KvEditorMessage::ValueChanged(0, "2".into()));
        assert_eq!(rows[0].value, "2");
    }
}
