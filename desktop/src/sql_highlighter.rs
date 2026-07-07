use iced::advanced::text::Highlighter;
use iced::advanced::text::highlighter::Format;
use std::ops::Range;

const KEYWORDS: &[&str] = &[
    "select", "from", "where", "insert", "into", "values", "update", "set", "delete", "join",
    "inner", "left", "right", "outer", "full", "on", "as", "and", "or", "not", "null", "is",
    "in", "like", "between", "group", "by", "order", "having", "limit", "offset", "distinct",
    "union", "all", "create", "table", "alter", "drop", "index", "view", "primary", "key",
    "foreign", "references", "default", "constraint", "cascade", "returning", "with", "case",
    "when", "then", "else", "end", "exists", "count", "sum", "avg", "min", "max", "asc", "desc",
    "true", "false", "begin", "commit", "rollback", "transaction",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlToken {
    Keyword,
    String,
    Number,
    Comment,
}

pub struct SqlHighlighter {
    current_line: usize,
}

impl Highlighter for SqlHighlighter {
    type Settings = ();
    type Highlight = SqlToken;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, SqlToken)>;

    fn new(_settings: &Self::Settings) -> Self {
        Self { current_line: 0 }
    }

    fn update(&mut self, _new_settings: &Self::Settings) {}

    fn change_line(&mut self, line: usize) {
        self.current_line = self.current_line.min(line);
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        self.current_line += 1;
        tokenize_sql_line(line).into_iter()
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}

pub fn format_for(token: &SqlToken, _theme: &iced::Theme) -> Format<iced::Font> {
    let color = match token {
        SqlToken::Keyword => iced::Color::from_rgb8(0xff, 0x79, 0xc6),
        SqlToken::String => iced::Color::from_rgb8(0xf1, 0xfa, 0x8c),
        SqlToken::Number => iced::Color::from_rgb8(0xbd, 0x93, 0xf9),
        SqlToken::Comment => iced::Color::from_rgb8(0x62, 0x72, 0xa4),
    };
    Format {
        color: Some(color),
        font: None,
    }
}

/// Line-scoped SQL tokenizer. Block comments (`/* ... */`) are only
/// recognized within a single line — spanning a block comment across
/// multiple lines would need cross-line highlighter state, deferred.
fn tokenize_sql_line(line: &str) -> Vec<(Range<usize>, SqlToken)> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < len {
        let c = bytes[i] as char;

        if c == '-' && i + 1 < len && bytes[i + 1] as char == '-' {
            tokens.push((i..len, SqlToken::Comment));
            break;
        }

        if c == '/' && i + 1 < len && bytes[i + 1] as char == '*' {
            let end = line[i..].find("*/").map(|p| i + p + 2).unwrap_or(len);
            tokens.push((i..end, SqlToken::Comment));
            i = end;
            continue;
        }

        if c == '\'' {
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] as char == '\'' {
                    if i + 1 < len && bytes[i + 1] as char == '\'' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            tokens.push((start..i, SqlToken::String));
            continue;
        }

        if c.is_ascii_digit() {
            let start = i;
            while i < len && (bytes[i] as char).is_ascii_digit() || (i < len && bytes[i] as char == '.') {
                i += 1;
            }
            tokens.push((start..i, SqlToken::Number));
            continue;
        }

        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < len && ((bytes[i] as char).is_alphanumeric() || bytes[i] as char == '_') {
                i += 1;
            }
            let word = &line[start..i];
            if KEYWORDS.contains(&word.to_ascii_lowercase().as_str()) {
                tokens.push((start..i, SqlToken::Keyword));
            }
            continue;
        }

        i += 1;
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(String, SqlToken)> {
        tokenize_sql_line(line)
            .into_iter()
            .map(|(range, kind)| (line[range].to_string(), kind))
            .collect()
    }

    #[test]
    fn highlights_keywords() {
        let tokens = kinds("select * from users where id = 1");
        assert!(tokens.contains(&("select".to_string(), SqlToken::Keyword)));
        assert!(tokens.contains(&("from".to_string(), SqlToken::Keyword)));
        assert!(tokens.contains(&("where".to_string(), SqlToken::Keyword)));
    }

    #[test]
    fn highlights_strings_and_numbers() {
        let tokens = kinds("select 'hello world', 42 from t");
        assert!(tokens.contains(&("'hello world'".to_string(), SqlToken::String)));
        assert!(tokens.contains(&("42".to_string(), SqlToken::Number)));
    }

    #[test]
    fn highlights_line_comment() {
        let tokens = kinds("select 1 -- a comment");
        assert!(
            tokens
                .iter()
                .any(|(text, kind)| *kind == SqlToken::Comment && text.starts_with("--"))
        );
    }

    #[test]
    fn highlights_block_comment_single_line() {
        let tokens = kinds("select /* inline */ 1");
        assert!(tokens.contains(&("/* inline */".to_string(), SqlToken::Comment)));
    }

    #[test]
    fn does_not_flag_identifiers_as_keywords() {
        let tokens = kinds("select selection_notes from t");
        assert!(!tokens.iter().any(|(text, _)| text == "selection_notes"));
    }
}
