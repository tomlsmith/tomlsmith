use std::ops::Range;

use crate::{Diagnostic, DiagnosticCode, TextRange};

use super::SyntaxKind;

/// One lexed token, materialized on demand from a [`TokenTape`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Token {
    pub(crate) kind: SyntaxKind,
    pub(crate) range: Range<usize>,
}

/// The token stream of one source text in a compact columnar form.
///
/// Every token costs six bytes (its kind and its start offset); a token's end
/// is the next token's start or the text length, so a tape is about a fifth
/// of the size of a `Vec<Token>` and cheap enough for a snapshot to keep for
/// its editor products. Offsets are `u32` like every public range.
#[derive(Clone, Debug, Default)]
pub(crate) struct TokenTape {
    kinds: Vec<SyntaxKind>,
    starts: Vec<u32>,
    end: u32,
}

impl TokenTape {
    fn with_capacity(tokens: usize) -> Self {
        Self {
            kinds: Vec::with_capacity(tokens),
            starts: Vec::with_capacity(tokens),
            end: 0,
        }
    }

    fn push(&mut self, kind: SyntaxKind, start: usize) {
        self.kinds.push(kind);
        self.starts.push(offset(start));
    }

    pub(crate) fn len(&self) -> usize {
        self.kinds.len()
    }

    /// The kind of token `index`; panics when out of bounds like slice indexing.
    pub(crate) fn kind(&self, index: usize) -> SyntaxKind {
        self.kinds[index]
    }

    /// The byte range of token `index`; panics when out of bounds like slice indexing.
    pub(crate) fn range(&self, index: usize) -> Range<usize> {
        let start = self.starts[index];
        let end = self.starts.get(index + 1).copied().unwrap_or(self.end);
        start as usize..end as usize
    }

    pub(crate) fn get(&self, index: usize) -> Option<Token> {
        (index < self.len()).then(|| Token {
            kind: self.kind(index),
            range: self.range(index),
        })
    }

    pub(crate) fn last(&self) -> Option<Token> {
        self.get(self.len().checked_sub(1)?)
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = Token> + ExactSizeIterator + '_ {
        (0..self.len()).map(|index| Token {
            kind: self.kind(index),
            range: self.range(index),
        })
    }
}

fn offset(value: usize) -> u32 {
    u32::try_from(value).expect("source text is bounded by u32 offsets")
}

pub(crate) struct Lexed {
    pub(crate) tokens: TokenTape,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

pub(crate) fn lex(source: &str) -> Lexed {
    let bytes = source.as_bytes();
    // Real documents average roughly five bytes per token.
    let mut tokens = TokenTape::with_capacity(source.len() / 5);
    let mut diagnostics = Vec::new();
    let mut cursor = 0;

    while cursor < bytes.len() {
        let start = cursor;
        let kind = match bytes[cursor] {
            0xef if cursor == 0 && bytes.get(..3) == Some(&[0xef, 0xbb, 0xbf]) => {
                cursor += 3;
                SyntaxKind::Bom
            }
            b' ' | b'\t' => {
                cursor += 1;
                while matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
                    cursor += 1;
                }
                SyntaxKind::Whitespace
            }
            b'\n' => {
                cursor += 1;
                SyntaxKind::Newline
            }
            b'\r' if bytes.get(cursor + 1) == Some(&b'\n') => {
                cursor += 2;
                SyntaxKind::Newline
            }
            b'#' => {
                cursor += 1;
                while !matches!(bytes.get(cursor), None | Some(b'\n' | b'\r')) {
                    cursor += 1;
                }
                SyntaxKind::Comment
            }
            b'"' => lex_string(
                source,
                &mut cursor,
                b'"',
                SyntaxKind::BasicString,
                &mut diagnostics,
            ),
            b'\'' => lex_string(
                source,
                &mut cursor,
                b'\'',
                SyntaxKind::LiteralString,
                &mut diagnostics,
            ),
            b'=' => single(&mut cursor, SyntaxKind::Equals),
            b'.' => single(&mut cursor, SyntaxKind::Dot),
            b',' => single(&mut cursor, SyntaxKind::Comma),
            b'[' => single(&mut cursor, SyntaxKind::LeftBracket),
            b']' => single(&mut cursor, SyntaxKind::RightBracket),
            b'{' => single(&mut cursor, SyntaxKind::LeftBrace),
            b'}' => single(&mut cursor, SyntaxKind::RightBrace),
            _ => {
                // Byte-wise advance: every delimiter is ASCII, and UTF-8
                // continuation bytes are >= 0x80, so scanning bytes lands on
                // exactly the same (char-boundary) end position as a
                // char-wise scan.
                cursor += 1;
                while cursor < bytes.len() && !is_delimiter(bytes[cursor]) {
                    cursor += 1;
                }
                SyntaxKind::Bare
            }
        };

        tokens.push(kind, start);
    }
    tokens.end = offset(source.len());

    Lexed {
        tokens,
        diagnostics,
    }
}

const fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t'
            | b'\n'
            | b'\r'
            | b'#'
            | b'"'
            | b'\''
            | b'='
            | b'.'
            | b','
            | b'['
            | b']'
            | b'{'
            | b'}'
    )
}

const fn single(cursor: &mut usize, kind: SyntaxKind) -> SyntaxKind {
    *cursor += 1;
    kind
}

fn lex_string(
    source: &str,
    cursor: &mut usize,
    quote: u8,
    kind: SyntaxKind,
    diagnostics: &mut Vec<Diagnostic>,
) -> SyntaxKind {
    let bytes = source.as_bytes();
    let start = *cursor;
    let multiline = bytes.get(start..start + 3) == Some(&[quote, quote, quote]);
    *cursor += if multiline { 3 } else { 1 };
    let mut escaped = false;
    let mut terminated = false;

    while *cursor < bytes.len() {
        let byte = bytes[*cursor];
        if !multiline && matches!(byte, b'\n' | b'\r') {
            break;
        }

        if quote == b'"' {
            if escaped {
                escaped = false;
                *cursor += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                *cursor += 1;
                continue;
            }
        }

        if multiline && byte == quote {
            let quote_count = bytes[*cursor..]
                .iter()
                .take_while(|&&byte| byte == quote)
                .count();
            if quote_count >= 3 {
                // Up to two quote characters may immediately precede the
                // closing three-character delimiter. They are part of the
                // string value, so the complete four/five quote run belongs
                // to this token.
                *cursor += quote_count.min(5);
                terminated = true;
                break;
            }
            *cursor += quote_count;
            continue;
        }

        if !multiline && byte == quote {
            *cursor += 1;
            terminated = true;
            break;
        }
        // Byte-wise advance: the loop only compares against ASCII bytes
        // (quote, backslash, CR/LF), which never occur inside a multi-byte
        // UTF-8 sequence, so this visits the same stopping points as a
        // char-wise scan.
        *cursor += 1;
    }

    if !terminated {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::UNTERMINATED_STRING,
            "unterminated string",
            TextRange::from_usize(start, *cursor),
        ));
    }

    kind
}
