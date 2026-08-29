use rowan::Language;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum SyntaxKind {
    Root,
    KeyValue,
    Key,
    Value,
    Table,
    ArrayTable,
    Whitespace,
    Newline,
    Comment,
    Bare,
    BasicString,
    LiteralString,
    Equals,
    Dot,
    Comma,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Array,
    InlineTable,
    Bom,
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
