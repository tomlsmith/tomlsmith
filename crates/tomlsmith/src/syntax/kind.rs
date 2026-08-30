use rowan::Language;

/// A lossless syntax node or token category.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum SyntaxKind {
    /// The complete document node.
    Root,
    /// A key-value statement node.
    KeyValue,
    /// A dotted key node.
    Key,
    /// A scalar or container value node.
    Value,
    /// A table header node.
    Table,
    /// An array-of-tables header node.
    ArrayTable,
    /// Horizontal whitespace token.
    Whitespace,
    /// A source newline token.
    Newline,
    /// A comment token.
    Comment,
    /// An unquoted token used for bare keys and non-string values.
    Bare,
    /// A basic or multiline-basic string token.
    BasicString,
    /// A literal or multiline-literal string token.
    LiteralString,
    /// The equals-sign token.
    Equals,
    /// The dotted-key separator token.
    Dot,
    /// The container item separator token.
    Comma,
    /// A left square-bracket token.
    LeftBracket,
    /// A right square-bracket token.
    RightBracket,
    /// A left brace token.
    LeftBrace,
    /// A right brace token.
    RightBrace,
    /// An array value node.
    Array,
    /// An inline-table value node.
    InlineTable,
    /// A UTF-8 byte-order mark token at the start of the document.
    Bom,
    /// Invalid or otherwise unclassified source bytes.
    Invalid,
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum TomlLanguage {}

impl Language for TomlLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        match raw.0 {
            0 => SyntaxKind::Root,
            1 => SyntaxKind::KeyValue,
            2 => SyntaxKind::Key,
            3 => SyntaxKind::Value,
            4 => SyntaxKind::Table,
            5 => SyntaxKind::ArrayTable,
            6 => SyntaxKind::Whitespace,
            7 => SyntaxKind::Newline,
            8 => SyntaxKind::Comment,
            9 => SyntaxKind::Bare,
            10 => SyntaxKind::BasicString,
            11 => SyntaxKind::LiteralString,
            12 => SyntaxKind::Equals,
            13 => SyntaxKind::Dot,
            14 => SyntaxKind::Comma,
            15 => SyntaxKind::LeftBracket,
            16 => SyntaxKind::RightBracket,
            17 => SyntaxKind::LeftBrace,
            18 => SyntaxKind::RightBrace,
            19 => SyntaxKind::Array,
            20 => SyntaxKind::InlineTable,
            21 => SyntaxKind::Bom,
            _ => SyntaxKind::Invalid,
        }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}
