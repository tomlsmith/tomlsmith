use crate::{SyntaxKind, TextRange, syntax::lexer::Token};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HighlightKind {
    Key,
    Table,
    String,
    Number,
    Boolean,
    DateTime,
    Comment,
    Punctuation,
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Highlight {
    kind: HighlightKind,
    range: TextRange,
}

impl Highlight {
    #[must_use]
    pub const fn kind(self) -> HighlightKind {
        self.kind
    }

    #[must_use]
    pub const fn range(self) -> TextRange {
        self.range
    }
}

pub(crate) fn collect(source: &str, tokens: &[Token]) -> Vec<Highlight> {
    let mut highlights = Vec::new();
    let mut in_key = true;
    let mut in_table_header = false;
    let mut bracket_depth = 0_u32;
    let mut containers = Vec::new();

    for token in tokens {
        let raw = &source[token.range.clone()];
        let kind = match token.kind {
            SyntaxKind::Newline => {
                if bracket_depth == 0 && containers.is_empty() {
                    in_key = true;
                    in_table_header = false;
                }
                None
            }
            SyntaxKind::Comment => Some(HighlightKind::Comment),
            SyntaxKind::BasicString | SyntaxKind::LiteralString => Some(if in_key {
                if in_table_header {
                    HighlightKind::Table
                } else {
                    HighlightKind::Key
                }
            } else {
                HighlightKind::String
            }),
            SyntaxKind::Bare => Some(if in_table_header {
                HighlightKind::Table
            } else if in_key {
                HighlightKind::Key
            } else {
                classify_value(raw)
            }),
            SyntaxKind::Equals => {
                in_key = false;
                Some(HighlightKind::Punctuation)
            }
            SyntaxKind::LeftBracket => {
                let starts_or_extends_table_header = in_key
                    && containers.is_empty()
                    && (bracket_depth == 0 || (in_table_header && bracket_depth == 1));
                if starts_or_extends_table_header {
                    in_table_header = true;
                } else {
                    containers.push(Container::Array);
                }
                bracket_depth += 1;
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
            highlights.push(Highlight {
                kind,
                range: TextRange::from_usize(token.range.start, token.range.end),
            });
        }
    }

    highlights
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
