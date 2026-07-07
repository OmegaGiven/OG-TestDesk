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
    SearchMatch,
    SearchMatchCurrent,
}

/// Search-in-editor state fed into the highlighter alongside normal SQL
/// tokenizing. `current_match` is `(line_index, byte_start_in_line)` for
/// whichever match should render as "current" (prev/next navigation).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SqlHighlighterSettings {
    pub search_term: String,
    pub current_match: Option<(usize, usize)>,
}

pub struct SqlHighlighter {
    current_line: usize,
    settings: SqlHighlighterSettings,
}

impl Highlighter for SqlHighlighter {
    type Settings = SqlHighlighterSettings;
    type Highlight = SqlToken;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, SqlToken)>;

    fn new(settings: &Self::Settings) -> Self {
        Self {
            current_line: 0,
            settings: settings.clone(),
        }
    }

    fn update(&mut self, new_settings: &Self::Settings) {
        self.settings = new_settings.clone();
    }

    fn change_line(&mut self, line: usize) {
        self.current_line = self.current_line.min(line);
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        let line_index = self.current_line;
        self.current_line += 1;

        let tokens = tokenize_sql_line(line);
        let tokens = if self.settings.search_term.is_empty() {
            tokens
        } else {
            apply_search_overlay(
                tokens,
                line,
                &self.settings.search_term,
                self.settings.current_match,
                line_index,
            )
        };
        tokens.into_iter()
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
        // iced's Highlighter Format has no background field (foreground
        // color only), so search matches are distinguished by a vivid
        // foreground color rather than a highlighted background.
        SqlToken::SearchMatch => iced::Color::from_rgb8(0xff, 0xb8, 0x6c),
        SqlToken::SearchMatchCurrent => iced::Color::from_rgb8(0xff, 0x55, 0x55),
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

/// Finds all non-overlapping, case-insensitive occurrences of `term` in
/// `line`. ASCII-lowercased for byte-position stability (see caveat on
/// non-ASCII text in module docs).
pub fn find_search_matches(line: &str, term: &str) -> Vec<Range<usize>> {
    if term.is_empty() {
        return Vec::new();
    }
    let haystack = line.to_ascii_lowercase();
    let needle = term.to_ascii_lowercase();
    let mut matches = Vec::new();
    let mut pos = 0;
    while pos < haystack.len() {
        match haystack[pos..].find(&needle) {
            Some(offset) => {
                let start = pos + offset;
                let end = start + needle.len();
                matches.push(start..end);
                pos = end.max(start + 1);
            }
            None => break,
        }
    }
    matches
}

/// Overlays search-match tokens on top of the normal SQL tokens, splitting
/// any syntax token that partially overlaps a match so the match's color
/// wins for the overlapping segment.
fn apply_search_overlay(
    base: Vec<(Range<usize>, SqlToken)>,
    line: &str,
    term: &str,
    current_match: Option<(usize, usize)>,
    line_index: usize,
) -> Vec<(Range<usize>, SqlToken)> {
    let match_ranges = find_search_matches(line, term);
    if match_ranges.is_empty() {
        return base;
    }

    let mut result = Vec::new();
    let mut base_iter = base.into_iter().peekable();

    for m in &match_ranges {
        while let Some((r, _)) = base_iter.peek() {
            if r.end <= m.start {
                result.push(base_iter.next().unwrap());
            } else {
                break;
            }
        }

        while let Some((r, kind)) = base_iter.peek().cloned() {
            if r.start >= m.end {
                break;
            }
            base_iter.next();
            if r.start < m.start {
                result.push((r.start..m.start, kind));
            }
            if r.end > m.end {
                result.push((m.end..r.end, kind));
            }
        }

        let token = if current_match == Some((line_index, m.start)) {
            SqlToken::SearchMatchCurrent
        } else {
            SqlToken::SearchMatch
        };
        result.push((m.clone(), token));
    }

    for remaining in base_iter {
        result.push(remaining);
    }

    result
}

/// Scans `{{name}}`-style variable tokens out of SQL text, returning the
/// distinct variable names in first-seen order (duplicates collapsed).
/// Malformed `{{` with no closing `}}` is ignored, not treated as a token.
pub fn scan_variable_names(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut names = Vec::new();
    let mut i = 0;
    while i + 1 < len {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            let rest = &sql[i + 2..];
            let close = rest.find("}}");
            let next_open = rest.find("{{");
            // Only treat this as a well-formed {{name}} if its closing "}}"
            // appears before any nested/unrelated "{{" — otherwise this
            // opener is malformed (e.g. `{{unterminated and {{closed}}`)
            // and we resume scanning just past it, so the *next* "{{" gets
            // its own fair chance at matching.
            if let Some(close) = close {
                if next_open.is_none_or(|next| close < next) {
                    let name = sql[i + 2..i + 2 + close].trim();
                    if !name.is_empty() && !names.iter().any(|n: &String| n == name) {
                        names.push(name.to_string());
                    }
                    i = i + 2 + close + 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    names
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableFormat {
    Raw,
    List,
    Array,
}

impl VariableFormat {
    pub const ALL: [VariableFormat; 3] = [Self::Raw, Self::List, Self::Array];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Raw => "Raw",
            Self::List => "List",
            Self::Array => "Array",
        }
    }
}

/// Splits `raw` on newlines/commas, trims, drops empties.
fn split_pieces(raw: &str) -> Vec<String> {
    raw.split(['\n', ','])
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .map(str::to_string)
        .collect()
}

fn looks_numeric(piece: &str) -> bool {
    piece.parse::<f64>().is_ok()
}

/// Transforms a raw variable input into its final substitution value
/// according to the selected format mode.
pub fn format_variable_value(raw: &str, mode: VariableFormat) -> String {
    match mode {
        VariableFormat::Raw => raw.to_string(),
        VariableFormat::List => split_pieces(raw).join(", "),
        VariableFormat::Array => {
            let pieces: Vec<String> = split_pieces(raw)
                .into_iter()
                .map(|piece| {
                    if looks_numeric(&piece) {
                        piece
                    } else {
                        format!("'{}'", piece.replace('\'', "''"))
                    }
                })
                .collect();
            format!("ARRAY[{}]", pieces.join(", "))
        }
    }
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

    #[test]
    fn scans_variable_names_in_order_without_duplicates() {
        let names = scan_variable_names("select * from t where a = {{foo}} and b = {{bar}} or c = {{foo}}");
        assert_eq!(names, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn scan_variable_names_ignores_no_variables() {
        assert!(scan_variable_names("select 1").is_empty());
    }

    #[test]
    fn scan_variable_names_ignores_unclosed_braces() {
        let names = scan_variable_names("select {{unterminated and {{closed}}");
        assert_eq!(names, vec!["closed".to_string()]);
    }

    #[test]
    fn format_variable_raw_is_passthrough() {
        assert_eq!(format_variable_value("hello world", VariableFormat::Raw), "hello world");
    }

    #[test]
    fn format_variable_list_joins_with_comma() {
        assert_eq!(
            format_variable_value("a\nb, c\n\n", VariableFormat::List),
            "a, b, c"
        );
    }

    #[test]
    fn format_variable_array_quotes_non_numeric() {
        assert_eq!(
            format_variable_value("abc\n123\ndef", VariableFormat::Array),
            "ARRAY['abc', 123, 'def']"
        );
    }

    #[test]
    fn format_variable_array_escapes_quotes() {
        assert_eq!(
            format_variable_value("O'Brien", VariableFormat::Array),
            "ARRAY['O''Brien']"
        );
    }

    #[test]
    fn find_search_matches_empty_term_returns_none() {
        assert!(find_search_matches("select * from t", "").is_empty());
    }

    #[test]
    fn find_search_matches_case_insensitive() {
        let matches = find_search_matches("SELECT select Select", "select");
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn apply_search_overlay_splits_overlapping_keyword() {
        let base = tokenize_sql_line("select * from t");
        let overlaid = apply_search_overlay(base, "select * from t", "elect", None, 0);
        let texts: Vec<(String, SqlToken)> = overlaid
            .into_iter()
            .map(|(r, k)| ("select * from t"[r].to_string(), k))
            .collect();
        assert!(texts.contains(&("elect".to_string(), SqlToken::SearchMatch)));
        assert!(texts.contains(&("s".to_string(), SqlToken::Keyword)));
    }

    #[test]
    fn apply_search_overlay_marks_current_match() {
        let base = tokenize_sql_line("select 1");
        let overlaid = apply_search_overlay(base, "select 1", "select", Some((0, 0)), 0);
        assert!(
            overlaid
                .iter()
                .any(|(r, k)| *k == SqlToken::SearchMatchCurrent && r.start == 0)
        );
    }

    #[test]
    fn apply_search_overlay_no_match_returns_base_unchanged() {
        let base = tokenize_sql_line("select 1");
        let overlaid = apply_search_overlay(base.clone(), "select 1", "zzz", None, 0);
        assert_eq!(overlaid.len(), base.len());
    }
}
