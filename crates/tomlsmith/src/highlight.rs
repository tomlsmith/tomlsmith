use crate::{SyntaxKind, TextRange, syntax::lexer::Token};

/// A source-level semantic classification suitable for editor highlighting.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HighlightKind {
    /// A bare or quoted key segment in a key-value declaration.
    Key,
    /// A bare or quoted key segment whose value is an array.
    ArrayKey,
    /// A bare or quoted key segment whose value is an inline table.
    InlineTableKey,
    /// A key segment in a table header.
    Table,
    /// A key segment in an array-of-tables header.
    ArrayTable,
    /// A string value.
    String,
    /// An integer or floating-point value.
    Number,
    /// A boolean value.
    Boolean,
    /// An offset/local date, time, or date-time value.
    DateTime,
    /// A comment including its leading hash.
    Comment,
    /// TOML structural punctuation.
    Punctuation,
    /// A value-shaped token that cannot be classified as valid TOML.
    Invalid,
}

/// A semantic highlight category attached to a UTF-8 source byte range.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Highlight {
    kind: HighlightKind,
    range: TextRange,
}

impl Highlight {
    /// Returns the semantic category.
    #[must_use]
    pub const fn kind(self) -> HighlightKind {
        self.kind
    }

    /// Returns the classified source byte range.
    #[must_use]
    pub const fn range(self) -> TextRange {
        self.range
    }
}

pub(crate) fn collect(source: &str, tokens: &[Token]) -> Vec<Highlight> {
    let mut highlights = Vec::new();
    let mut in_key = true;
    let mut in_table_header = false;
    let mut in_array_table_header = false;
    let mut bracket_depth = 0_u32;
    let mut containers = Vec::new();
    let mut pending_key_highlights = Vec::new();
    let mut awaiting_value = false;

    for token in tokens {
        let raw = &source[token.range.clone()];
        resolve_pending_key_kind(
            token.kind,
            &mut awaiting_value,
            &mut pending_key_highlights,
            &mut highlights,
        );

        let kind = match token.kind {
            SyntaxKind::Newline => {
                pending_key_highlights.clear();
                if bracket_depth == 0 && containers.is_empty() {
                    in_key = true;
                    in_table_header = false;
                    in_array_table_header = false;
                }
                None
            }
            SyntaxKind::Comment => Some(HighlightKind::Comment),
            SyntaxKind::BasicString | SyntaxKind::LiteralString => Some(if in_key {
                key_kind(in_table_header, in_array_table_header)
            } else {
                HighlightKind::String
            }),
            SyntaxKind::Bare => Some(if in_table_header {
                key_kind(in_table_header, in_array_table_header)
            } else if in_key {
                HighlightKind::Key
            } else {
                classify_value(raw)
            }),
            SyntaxKind::Equals => {
                in_key = false;
                awaiting_value = true;
                Some(HighlightKind::Punctuation)
            }
            SyntaxKind::LeftBracket => {
                open_bracket(
                    in_key,
                    &mut in_table_header,
                    &mut in_array_table_header,
                    &mut bracket_depth,
                    &mut containers,
                );
                Some(HighlightKind::Punctuation)
            }
            SyntaxKind::RightBracket => {
                bracket_depth = bracket_depth.saturating_sub(1);
                if !in_table_header && containers.last() == Some(&Container::Array) {
                    containers.pop();
                }
                Some(HighlightKind::Punctuation)
            }
            SyntaxKind::LeftBrace => {
                containers.push(Container::InlineTable);
                in_key = true;
                Some(HighlightKind::Punctuation)
            }
            SyntaxKind::RightBrace => {
                if containers.last() == Some(&Container::InlineTable) {
                    containers.pop();
                }
                in_key = false;
                Some(HighlightKind::Punctuation)
            }
            SyntaxKind::Comma => {
                if containers.last() == Some(&Container::InlineTable) {
                    pending_key_highlights.clear();
                    in_key = true;
                }
                Some(HighlightKind::Punctuation)
            }
            SyntaxKind::Dot => Some(HighlightKind::Punctuation),
            SyntaxKind::Invalid => Some(HighlightKind::Invalid),
            SyntaxKind::Bom
            | SyntaxKind::Whitespace
            | SyntaxKind::Root
            | SyntaxKind::KeyValue
            | SyntaxKind::Key
            | SyntaxKind::Value
            | SyntaxKind::Table
            | SyntaxKind::ArrayTable
            | SyntaxKind::Array
            | SyntaxKind::InlineTable => None,
        };

        if let Some(kind) = kind {
            push_highlight(&mut highlights, &mut pending_key_highlights, token, kind);
        }
    }

    highlights
}

fn open_bracket(
    in_key: bool,
    in_table_header: &mut bool,
    in_array_table_header: &mut bool,
    bracket_depth: &mut u32,
    containers: &mut Vec<Container>,
) {
    let starts_or_extends_table_header = in_key
        && containers.is_empty()
        && (*bracket_depth == 0 || (*in_table_header && *bracket_depth == 1));
    if starts_or_extends_table_header {
        *in_table_header = true;
        *in_array_table_header = *bracket_depth == 1;
    } else {
        containers.push(Container::Array);
    }
    *bracket_depth += 1;
}

fn push_highlight(
    highlights: &mut Vec<Highlight>,
    pending_key_highlights: &mut Vec<usize>,
    token: &Token,
    kind: HighlightKind,
) {
    let highlight_index = highlights.len();
    highlights.push(Highlight {
        kind,
        range: TextRange::from_usize(token.range.start, token.range.end),
    });
    if kind == HighlightKind::Key {
        pending_key_highlights.push(highlight_index);
    }
}

fn resolve_pending_key_kind(
    token_kind: SyntaxKind,
    awaiting_value: &mut bool,
    key_indices: &mut Vec<usize>,
    highlights: &mut [Highlight],
) {
    if !*awaiting_value || token_kind == SyntaxKind::Whitespace {
        return;
    }
    let replacement = match token_kind {
        SyntaxKind::LeftBracket => Some(HighlightKind::ArrayKey),
        SyntaxKind::LeftBrace => Some(HighlightKind::InlineTableKey),
        _ => None,
    };
    if let Some(kind) = replacement {
        reclassify_keys(highlights, key_indices, kind);
    }
    key_indices.clear();
    *awaiting_value = false;
}

const fn key_kind(in_table_header: bool, in_array_table_header: bool) -> HighlightKind {
    if !in_table_header {
        HighlightKind::Key
    } else if in_array_table_header {
        HighlightKind::ArrayTable
    } else {
        HighlightKind::Table
    }
}

fn reclassify_keys(highlights: &mut [Highlight], key_indices: &[usize], kind: HighlightKind) {
    for &index in key_indices {
        highlights[index].kind = kind;
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Container {
    Array,
    InlineTable,
}

fn classify_value(raw: &str) -> HighlightKind {
    if matches!(raw, "true" | "false") {
        HighlightKind::Boolean
    } else if looks_like_datetime(raw) {
        HighlightKind::DateTime
    } else if looks_like_number(raw) {
        HighlightKind::Number
    } else {
        HighlightKind::Invalid
    }
}

fn looks_like_datetime(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    (bytes.len() >= 10 && bytes.get(4) == Some(&b'-') && bytes.get(7) == Some(&b'-'))
        || (bytes.len() >= 5 && bytes.get(2) == Some(&b':'))
}

fn looks_like_number(raw: &str) -> bool {
    let normalized: std::borrow::Cow<'_, str> = if raw.contains('_') {
        raw.replace('_', "").into()
    } else {
        raw.into()
    };
    let normalized: &str = &normalized;
    if matches!(
        normalized,
        "inf" | "+inf" | "-inf" | "nan" | "+nan" | "-nan"
    ) {
        return true;
    }
    let unsigned = normalized.strip_prefix(['+', '-']).unwrap_or(normalized);
    if let Some(digits) = unsigned.strip_prefix("0x") {
        return !digits.is_empty()
            && digits
                .chars()
                .all(|character| character.is_ascii_hexdigit());
    }
    if let Some(digits) = unsigned.strip_prefix("0o") {
        return !digits.is_empty()
            && digits
                .chars()
                .all(|character| matches!(character, '0'..='7'));
    }
    if let Some(digits) = unsigned.strip_prefix("0b") {
        return !digits.is_empty()
            && digits
                .chars()
                .all(|character| matches!(character, '0' | '1'));
    }
    normalized.parse::<i64>().is_ok() || normalized.parse::<f64>().is_ok()
}
