use iced::advanced::text::Highlighter;
use iced::advanced::text::highlighter::Format;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonToken {
    Key,
    String,
    Number,
    BoolNull,
    Punctuation,
}

pub struct JsonHighlighter {
    current_line: usize,
}

impl Highlighter for JsonHighlighter {
    type Settings = ();
    type Highlight = JsonToken;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, JsonToken)>;

    fn new(_settings: &Self::Settings) -> Self {
        Self { current_line: 0 }
    }

    fn update(&mut self, _new_settings: &Self::Settings) {}

    fn change_line(&mut self, line: usize) {
        self.current_line = self.current_line.min(line);
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        self.current_line += 1;
        tokenize_json_line(line).into_iter()
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}

pub fn format_for(token: &JsonToken, _theme: &iced::Theme) -> Format<iced::Font> {
    let color = match token {
        JsonToken::Key => iced::Color::from_rgb8(0x8b, 0xe9, 0xfd),
        JsonToken::String => iced::Color::from_rgb8(0xf1, 0xfa, 0x8c),
        JsonToken::Number => iced::Color::from_rgb8(0xbd, 0x93, 0xf9),
        JsonToken::BoolNull => iced::Color::from_rgb8(0xff, 0x79, 0xc6),
        JsonToken::Punctuation => iced::Color::from_rgb8(0x62, 0x72, 0xa4),
    };
    Format {
        color: Some(color),
        font: None,
    }
}

/// Line-scoped JSON tokenizer. A quoted string is classified as a `Key`
/// if the next non-whitespace character after its closing quote is `:`,
/// otherwise it's a value `String`. Strings/keys spanning multiple lines
/// aren't tracked across `highlight_line` calls (same single-line
/// limitation documented in `sql_highlighter.rs` for block comments).
fn tokenize_json_line(line: &str) -> Vec<(Range<usize>, JsonToken)> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < len {
        let c = bytes[i] as char;

        if c == '"' {
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] as char == '\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if bytes[i] as char == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let mut lookahead = i;
            while lookahead < len && (bytes[lookahead] as char).is_whitespace() {
                lookahead += 1;
            }
            let is_key = lookahead < len && bytes[lookahead] as char == ':';
            tokens.push((start..i, if is_key { JsonToken::Key } else { JsonToken::String }));
            continue;
        }

        if c == '-' || c.is_ascii_digit() {
            let start = i;
            if c == '-' {
                i += 1;
            }
            while i < len {
                let d = bytes[i] as char;
                if d.is_ascii_digit() || d == '.' || d == 'e' || d == 'E' || d == '+' || d == '-' {
                    i += 1;
                } else {
                    break;
                }
            }
            if i > start {
                tokens.push((start..i, JsonToken::Number));
                continue;
            }
        }

        if c.is_alphabetic() {
            let start = i;
            while i < len && (bytes[i] as char).is_alphabetic() {
                i += 1;
            }
            let word = &line[start..i];
            if word == "true" || word == "false" || word == "null" {
                tokens.push((start..i, JsonToken::BoolNull));
            }
            continue;
        }

        if matches!(c, '{' | '}' | '[' | ']' | ':' | ',') {
            tokens.push((i..i + 1, JsonToken::Punctuation));
            i += 1;
            continue;
        }

        i += 1;
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(String, JsonToken)> {
        tokenize_json_line(line)
            .into_iter()
            .map(|(range, kind)| (line[range].to_string(), kind))
            .collect()
    }

    #[test]
    fn classifies_keys_and_string_values() {
        let tokens = kinds(r#"{"name": "value"}"#);
        assert!(tokens.contains(&("\"name\"".to_string(), JsonToken::Key)));
        assert!(tokens.contains(&("\"value\"".to_string(), JsonToken::String)));
    }

    #[test]
    fn key_detection_tolerates_whitespace_before_colon() {
        let tokens = kinds(r#""key"   : 1"#);
        assert!(tokens.contains(&("\"key\"".to_string(), JsonToken::Key)));
    }

    #[test]
    fn highlights_numbers_including_negative_and_exponent() {
        let tokens = kinds(r#"[-1, 2.5, 3e10]"#);
        assert!(tokens.contains(&("-1".to_string(), JsonToken::Number)));
        assert!(tokens.contains(&("2.5".to_string(), JsonToken::Number)));
        assert!(tokens.contains(&("3e10".to_string(), JsonToken::Number)));
    }

    #[test]
    fn highlights_true_false_null() {
        let tokens = kinds("[true, false, null]");
        assert!(tokens.contains(&("true".to_string(), JsonToken::BoolNull)));
        assert!(tokens.contains(&("false".to_string(), JsonToken::BoolNull)));
        assert!(tokens.contains(&("null".to_string(), JsonToken::BoolNull)));
    }

    #[test]
    fn highlights_punctuation() {
        let tokens = kinds("{}[]:,");
        for ch in ["{", "}", "[", "]", ":", ","] {
            assert!(tokens.contains(&(ch.to_string(), JsonToken::Punctuation)));
        }
    }

    #[test]
    fn escaped_quote_inside_string_does_not_end_it_early() {
        let tokens = kinds(r#""a\"b": 1"#);
        assert!(tokens.contains(&("\"a\\\"b\"".to_string(), JsonToken::Key)));
    }
}
