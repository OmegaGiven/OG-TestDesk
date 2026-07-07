use iced::advanced::text::Highlighter;
use iced::advanced::text::highlighter::Format;
use std::ops::Range;

const KEYWORDS: &[&str] = &[
    "query",
    "mutation",
    "subscription",
    "fragment",
    "on",
    "true",
    "false",
    "null",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphqlToken {
    Keyword,
    Field,
    String,
    Comment,
    Punctuation,
}

pub struct GraphqlHighlighter {
    current_line: usize,
}

impl Highlighter for GraphqlHighlighter {
    type Settings = ();
    type Highlight = GraphqlToken;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, GraphqlToken)>;

    fn new(_settings: &Self::Settings) -> Self {
        Self { current_line: 0 }
    }

    fn update(&mut self, _new_settings: &Self::Settings) {}

    fn change_line(&mut self, line: usize) {
        self.current_line = self.current_line.min(line);
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        self.current_line += 1;
        tokenize_graphql_line(line).into_iter()
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}

pub fn format_for(token: &GraphqlToken, _theme: &iced::Theme) -> Format<iced::Font> {
    let color = match token {
        GraphqlToken::Keyword => iced::Color::from_rgb8(0xff, 0x79, 0xc6),
        GraphqlToken::Field => iced::Color::from_rgb8(0x8b, 0xe9, 0xfd),
        GraphqlToken::String => iced::Color::from_rgb8(0xf1, 0xfa, 0x8c),
        GraphqlToken::Comment => iced::Color::from_rgb8(0x62, 0x72, 0xa4),
        GraphqlToken::Punctuation => iced::Color::from_rgb8(0x62, 0x72, 0xa4),
    };
    Format {
        color: Some(color),
        font: None,
    }
}

/// Line-scoped GraphQL tokenizer. Field names are any bare identifier not
/// recognized as a keyword (not schema-aware — this is a lexer, not a
/// validator, matching the level of rigor already used for SQL/JSON).
fn tokenize_graphql_line(line: &str) -> Vec<(Range<usize>, GraphqlToken)> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < len {
        let c = bytes[i] as char;

        if c == '#' {
            tokens.push((i..len, GraphqlToken::Comment));
            break;
        }

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
            tokens.push((start..i, GraphqlToken::String));
            continue;
        }

        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < len && ((bytes[i] as char).is_alphanumeric() || bytes[i] as char == '_') {
                i += 1;
            }
            let word = &line[start..i];
            let kind = if KEYWORDS.contains(&word) {
                GraphqlToken::Keyword
            } else {
                GraphqlToken::Field
            };
            tokens.push((start..i, kind));
            continue;
        }

        if matches!(c, '{' | '}' | '(' | ')' | ':' | ',' | '$' | '!' | '[' | ']') {
            tokens.push((i..i + 1, GraphqlToken::Punctuation));
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

    fn kinds(line: &str) -> Vec<(String, GraphqlToken)> {
        tokenize_graphql_line(line)
            .into_iter()
            .map(|(range, kind)| (line[range].to_string(), kind))
            .collect()
    }

    #[test]
    fn highlights_keywords() {
        let tokens = kinds("query GetUser($id: ID!) {");
        assert!(tokens.contains(&("query".to_string(), GraphqlToken::Keyword)));
    }

    #[test]
    fn highlights_field_names() {
        let tokens = kinds("{ user { id name } }");
        assert!(tokens.contains(&("user".to_string(), GraphqlToken::Field)));
        assert!(tokens.contains(&("id".to_string(), GraphqlToken::Field)));
        assert!(tokens.contains(&("name".to_string(), GraphqlToken::Field)));
    }

    #[test]
    fn highlights_string_literals() {
        let tokens = kinds(r#"user(name: "bob")"#);
        assert!(tokens.contains(&("\"bob\"".to_string(), GraphqlToken::String)));
    }

    #[test]
    fn highlights_line_comment() {
        let tokens = kinds("query { id } # a comment");
        assert!(
            tokens
                .iter()
                .any(|(text, kind)| *kind == GraphqlToken::Comment && text.starts_with('#'))
        );
    }

    #[test]
    fn highlights_punctuation() {
        let tokens = kinds("{}():,$![]");
        for ch in ["{", "}", "(", ")", ":", ",", "$", "!", "[", "]"] {
            assert!(tokens.contains(&(ch.to_string(), GraphqlToken::Punctuation)));
        }
    }

    #[test]
    fn on_and_fragment_are_keywords() {
        let tokens = kinds("fragment Fields on User { id }");
        assert!(tokens.contains(&("fragment".to_string(), GraphqlToken::Keyword)));
        assert!(tokens.contains(&("on".to_string(), GraphqlToken::Keyword)));
        assert!(tokens.contains(&("Fields".to_string(), GraphqlToken::Field)));
    }
}
