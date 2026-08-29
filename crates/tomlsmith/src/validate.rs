use crate::{
    Diagnostic, DiagnosticCode, SyntaxElement, SyntaxKind, SyntaxNode, TextRange, TomlVersion,
    literal,
    semantic::MAX_KEY_DEPTH,
    syntax::{self, lexer},
};

pub(crate) fn validate(
    source: &str,
    version: TomlVersion,
    green: rowan::GreenNode,
    tokens: &[lexer::Token],
) -> Vec<Diagnostic> {
    struct InlineTableState {
        bracket_depth: u32,
        reported_multiline: bool,
    }

    let mut diagnostics = validate_raw_control_characters(source);
    let mut bracket_depth = 0_u32;
    let mut inline_tables = Vec::<InlineTableState>::new();

    for (index, token) in tokens.iter().enumerate() {
        let raw = &source[token.range.clone()];
        match token.kind {
            SyntaxKind::BasicString => {
                validate_basic_string(raw, token.range.start, version, &mut diagnostics);
            }
            SyntaxKind::LeftBracket => bracket_depth += 1,
            SyntaxKind::RightBracket => bracket_depth = bracket_depth.saturating_sub(1),
            SyntaxKind::LeftBrace => {
                inline_tables.push(InlineTableState {
                    bracket_depth,
                    reported_multiline: false,
                });
            }
            SyntaxKind::RightBrace => {
                inline_tables.pop();
            }
            SyntaxKind::Newline if version == TomlVersion::V1_0 => {
                if let Some(table) = inline_tables
                    .iter_mut()
                    .rev()
                    .find(|table| table.bracket_depth == bracket_depth)
                {
                    if !table.reported_multiline {
                        diagnostics.push(version_diagnostic(
                            token.range.start,
                            token.range.end,
                            "multiline inline tables require TOML 1.1",
                        ));
                        table.reported_multiline = true;
                    }
                }
            }
            SyntaxKind::Comma
                if version == TomlVersion::V1_0
                    && inline_tables
                        .last()
                        .is_some_and(|table| table.bracket_depth == bracket_depth)
                    && next_significant_kind(tokens, index) == Some(SyntaxKind::RightBrace) =>
            {
                diagnostics.push(version_diagnostic(
                    token.range.start,
                    token.range.end,
                    "a trailing comma in an inline table requires TOML 1.1",
                ));
            }
            _ => {}
        }
    }

    let root = syntax::root(green);
    validate_keys(&root, &mut diagnostics);
    if version == TomlVersion::V1_0 {
        validate_versioned_literals(source, &root, &mut diagnostics);
    }
    diagnostics
}

fn validate_versioned_literals(source: &str, node: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    if node.kind() == SyntaxKind::Value {
        let range = node.range();
        let raw = &source[range.start() as usize..range.end() as usize];
        let trimmed = raw.trim();
        if !trimmed.starts_with('"')
            && literal::parse(trimmed).is_some_and(|value| value.requires_toml_1_1)
        {
            let leading = raw.len() - raw.trim_start().len();
            let start = range.start() as usize + leading;
            diagnostics.push(version_diagnostic(
                start,
                start + trimmed.len(),
                "times without seconds require TOML 1.1",
            ));
        }
    }
    for child in node.children() {
        validate_versioned_literals(source, &child, diagnostics);
    }
}

fn validate_keys(node: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    if node.kind() == SyntaxKind::Key {
        let mut expects_segment = true;
        let mut invalid = false;
        let mut segment_count = 0_usize;
        for element in node.children_with_tokens() {
            let SyntaxElement::Token(token) = element else {
                continue;
            };
            match token.kind() {
                SyntaxKind::Whitespace => {}
                SyntaxKind::Bare | SyntaxKind::BasicString | SyntaxKind::LiteralString => {
                    segment_count += 1;
                    invalid |= !expects_segment;
                    if token.kind() == SyntaxKind::Bare {
                        invalid |= !token.text().chars().all(|character| {
                            character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                        });
                    } else {
                        invalid |=
                            token.text().starts_with("\"\"\"") || token.text().starts_with("'''");
                    }
                    expects_segment = false;
                }
                SyntaxKind::Dot => {
                    invalid |= expects_segment;
                    expects_segment = true;
                }
                _ => invalid = true,
            }
        }
        invalid |= expects_segment && !node.range().is_empty();
        if invalid {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::INVALID_BARE_KEY,
                "invalid key syntax",
                node.range(),
            ));
        }
        if segment_count > MAX_KEY_DEPTH {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::NESTING_LIMIT,
                format!("key nesting exceeds the supported limit of {MAX_KEY_DEPTH}"),
                node.range(),
            ));
        }
    }
    for child in node.children() {
        validate_keys(&child, diagnostics);
    }
}

fn validate_basic_string(
    raw: &str,
    source_start: usize,
    version: TomlVersion,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let bytes = raw.as_bytes();
    let delimiter = if bytes.starts_with(b"\"\"\"") { 3 } else { 1 };
    let closing = if delimiter == 3 { "\"\"\"" } else { "\"" };
    let content_end = if raw.len() >= delimiter * 2 && raw.ends_with(closing) {
        raw.len() - delimiter
    } else {
        raw.len()
    };
    let mut cursor = delimiter;

    while cursor < content_end {
        if bytes[cursor] != b'\\' {
            cursor += raw[cursor..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        let escape_start = cursor;
        cursor += 1;
        if delimiter == 3 {
            if let Some(after_fold) = consume_multiline_fold(bytes, cursor, content_end) {
                cursor = after_fold;
                continue;
            }
        }
        let Some(escape) = raw[cursor..content_end].chars().next() else {
            invalid_escape(source_start, escape_start, cursor, diagnostics);
            break;
        };
        cursor += escape.len_utf8();
        match escape {
            'b' | 't' | 'n' | 'f' | 'r' | '"' | '\\' => {}
            'e' => {
                if version == TomlVersion::V1_0 {
                    diagnostics.push(version_diagnostic(
                        source_start + escape_start,
                        source_start + cursor,
                        "the `\\e` escape requires TOML 1.1",
                    ));
                }
            }
            'x' => {
                if !consume_hex(raw, &mut cursor, 2, content_end) {
                    invalid_escape(source_start, escape_start, cursor, diagnostics);
                } else if version == TomlVersion::V1_0 {
                    diagnostics.push(version_diagnostic(
                        source_start + escape_start,
                        source_start + cursor,
                        "the `\\xHH` escape requires TOML 1.1",
                    ));
                }
            }
            'u' => {
                let digits_start = cursor;
                if !consume_unicode_scalar(raw, &mut cursor, digits_start, 4, content_end) {
                    invalid_escape(source_start, escape_start, cursor, diagnostics);
                }
            }
            'U' => {
                let digits_start = cursor;
                if !consume_unicode_scalar(raw, &mut cursor, digits_start, 8, content_end) {
                    invalid_escape(source_start, escape_start, cursor, diagnostics);
                }
            }
            _ => invalid_escape(source_start, escape_start, cursor, diagnostics),
        }
    }
}

fn consume_multiline_fold(bytes: &[u8], mut cursor: usize, content_end: usize) -> Option<usize> {
    while cursor < content_end && matches!(bytes[cursor], b' ' | b'\t') {
        cursor += 1;
    }
    match bytes.get(cursor..content_end)? {
        [b'\n', ..] => cursor += 1,
        [b'\r', b'\n', ..] => cursor += 2,
        _ => return None,
    }
    while cursor < content_end {
        match bytes[cursor] {
            b' ' | b'\t' | b'\n' => cursor += 1,
            b'\r' if bytes.get(cursor + 1) == Some(&b'\n') && cursor + 1 < content_end => {
                cursor += 2;
            }
            _ => break,
        }
    }
    Some(cursor)
}

fn consume_unicode_scalar(
    raw: &str,
    cursor: &mut usize,
    digits_start: usize,
    count: usize,
    content_end: usize,
) -> bool {
    if !consume_hex(raw, cursor, count, content_end) {
        return false;
    }
    u32::from_str_radix(&raw[digits_start..*cursor], 16)
        .ok()
        .and_then(char::from_u32)
        .is_some()
}

fn consume_hex(raw: &str, cursor: &mut usize, count: usize, content_end: usize) -> bool {
    let mut valid = true;
    for _ in 0..count {
        let Some(character) = raw[*cursor..content_end].chars().next() else {
            return false;
        };
        *cursor += character.len_utf8();
        valid &= character.is_ascii_hexdigit();
    }
    valid
}

fn invalid_escape(
    source_start: usize,
    escape_start: usize,
    escape_end: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(Diagnostic::error(
        DiagnosticCode::INVALID_ESCAPE,
        "invalid basic-string escape",
        TextRange::from_usize(source_start + escape_start, source_start + escape_end),
    ));
}

fn version_diagnostic(start: usize, end: usize, message: &'static str) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::TOML_1_1_SYNTAX,
        message,
        TextRange::from_usize(start, end),
    )
}

fn next_significant_kind(tokens: &[lexer::Token], index: usize) -> Option<SyntaxKind> {
    tokens[index + 1..]
        .iter()
        .find(|token| {
            !matches!(
                token.kind,
                SyntaxKind::Whitespace | SyntaxKind::Newline | SyntaxKind::Comment
            )
        })
        .map(|token| token.kind)
}

fn validate_raw_control_characters(source: &str) -> Vec<Diagnostic> {
    source
        .char_indices()
        .filter_map(|(offset, character)| {
            let invalid = matches!(character, '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{7f}');
            invalid.then(|| {
                Diagnostic::error(
                    DiagnosticCode::INVALID_CONTROL_CHARACTER,
                    "raw control character is not allowed in TOML",
                    TextRange::from_usize(offset, offset + character.len_utf8()),
                )
            })
        })
        .collect()
}
