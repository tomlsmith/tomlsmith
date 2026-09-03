use crate::{
    SyntaxKind, TextRange,
    syntax::lexer::{Token, TokenTape},
};

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
    inline_table_member: bool,
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

    /// Returns whether this key segment is declared inside an inline table.
    #[must_use]
    pub const fn is_inline_table_member(self) -> bool {
        self.inline_table_member
    }
}

pub(crate) fn collect(source: &str, tokens: &TokenTape) -> Vec<Highlight> {
    let mut highlights = Vec::new();
    let mut state = CollectorState::new();

    for token in tokens.iter() {
        let raw = &source[token.range.clone()];
        let inline_table_member = state.is_inline_table_member();
        resolve_pending_key_kind(
            token.kind,
            &mut state.awaiting_value,
            &mut state.pending_key_highlights,
            &mut highlights,
        );

        let kind = state.classify_token(token.kind, raw);

        if let Some(kind) = kind {
            push_highlight(
                &mut highlights,
                &mut state.pending_key_highlights,
                &token,
                kind,
                inline_table_member,
            );
        }
    }

    highlights
}

struct CollectorState {
    in_key: bool,
    header: Header,
    bracket_depth: u32,
    containers: Vec<Container>,
    pending_key_highlights: Vec<usize>,
    awaiting_value: bool,
}

impl CollectorState {
    const fn new() -> Self {
        Self {
            in_key: true,
            header: Header::None,
            bracket_depth: 0,
            containers: Vec::new(),
            pending_key_highlights: Vec::new(),
            awaiting_value: false,
        }
    }

    fn is_inline_table_member(&self) -> bool {
        self.in_key && self.containers.last() == Some(&Container::InlineTable)
    }

    fn classify_token(&mut self, token_kind: SyntaxKind, raw: &str) -> Option<HighlightKind> {
        match token_kind {
            SyntaxKind::Newline => {
                self.reset_line_state();
                None
            }
            SyntaxKind::Comment => Some(HighlightKind::Comment),
            SyntaxKind::BasicString | SyntaxKind::LiteralString => Some(if self.in_key {
                key_kind(self.header)
            } else {
                HighlightKind::String
            }),
            SyntaxKind::Bare => Some(if self.header != Header::None {
                key_kind(self.header)
            } else if self.in_key {
                HighlightKind::Key
            } else {
                classify_value(raw)
            }),
            SyntaxKind::Equals => {
                self.in_key = false;
                self.awaiting_value = true;
                Some(HighlightKind::Punctuation)
            }
            SyntaxKind::LeftBracket => {
                open_bracket(
                    self.in_key,
                    &mut self.header,
                    &mut self.bracket_depth,
                    &mut self.containers,
                );
                Some(HighlightKind::Punctuation)
            }
            SyntaxKind::RightBracket => {
                self.bracket_depth = self.bracket_depth.saturating_sub(1);
                if self.header == Header::None && self.containers.last() == Some(&Container::Array)
                {
                    self.containers.pop();
                }
                Some(HighlightKind::Punctuation)
            }
            SyntaxKind::LeftBrace => {
                self.containers.push(Container::InlineTable);
                self.in_key = true;
                Some(HighlightKind::Punctuation)
            }
            SyntaxKind::RightBrace => {
                if self.containers.last() == Some(&Container::InlineTable) {
                    self.containers.pop();
                }
                self.in_key = false;
                Some(HighlightKind::Punctuation)
            }
            SyntaxKind::Comma => {
                if self.containers.last() == Some(&Container::InlineTable) {
                    self.pending_key_highlights.clear();
                    self.in_key = true;
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
        }
    }

    fn reset_line_state(&mut self) {
        self.pending_key_highlights.clear();
        if self.bracket_depth == 0 && self.containers.is_empty() {
            self.in_key = true;
            self.header = Header::None;
        }
    }
}

fn open_bracket(
    in_key: bool,
    header: &mut Header,
    bracket_depth: &mut u32,
    containers: &mut Vec<Container>,
) {
    let starts_or_extends_table_header = in_key
        && containers.is_empty()
        && (*bracket_depth == 0 || (*header != Header::None && *bracket_depth == 1));
    if starts_or_extends_table_header {
        *header = if *bracket_depth == 1 {
            Header::ArrayTable
        } else {
            Header::Table
        };
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
    inline_table_member: bool,
) {
    let highlight_index = highlights.len();
    highlights.push(Highlight {
        kind,
        range: TextRange::from_usize(token.range.start, token.range.end),
        inline_table_member: inline_table_member && kind == HighlightKind::Key,
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

const fn key_kind(header: Header) -> HighlightKind {
    match header {
        Header::None => HighlightKind::Key,
        Header::Table => HighlightKind::Table,
        Header::ArrayTable => HighlightKind::ArrayTable,
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

#[derive(Clone, Copy, Eq, PartialEq)]
enum Header {
    None,
    Table,
    ArrayTable,
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
