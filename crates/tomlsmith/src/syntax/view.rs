use std::fmt;

use rowan::NodeOrToken;

use crate::TextRange;

use super::kind::{SyntaxKind, TomlLanguage};

#[derive(Clone)]
pub struct SyntaxNode(rowan::SyntaxNode<TomlLanguage>);

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct SyntaxToken(rowan::SyntaxToken<TomlLanguage>);

#[derive(Clone)]
pub enum SyntaxElement {
    Node(SyntaxNode),
    Token(SyntaxToken),
}

impl SyntaxNode {
    pub(crate) fn new_root(green: rowan::GreenNode) -> Self {
        Self(rowan::SyntaxNode::new_root(green))
    }

    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.0.kind()
    }

    #[must_use]
    pub fn range(&self) -> TextRange {
        range(self.0.text_range())
    }

    #[must_use]
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }

    pub fn children(&self) -> impl Iterator<Item = Self> + '_ {
        self.0.children().map(Self)
    }

    pub fn children_with_tokens(&self) -> impl Iterator<Item = SyntaxElement> + '_ {
        self.0.children_with_tokens().map(|element| match element {
            NodeOrToken::Node(node) => SyntaxElement::Node(Self(node)),
            NodeOrToken::Token(token) => SyntaxElement::Token(SyntaxToken(token)),
        })
    }
}

impl SyntaxToken {
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.0.kind()
    }

    #[must_use]
    pub fn range(&self) -> TextRange {
        range(self.0.text_range())
    }

    #[must_use]
    pub fn text(&self) -> &str {
        self.0.text()
    }
}

impl fmt::Debug for SyntaxNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxNode")
            .field("kind", &self.kind())
            .field("range", &self.range())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for SyntaxToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxToken")
            .field("kind", &self.kind())
            .field("range", &self.range())
            .field("text", &self.text())
            .finish()
    }
}

impl fmt::Debug for SyntaxElement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Node(node) => node.fmt(formatter),
            Self::Token(token) => token.fmt(formatter),
        }
    }
}

fn range(range: rowan::TextRange) -> TextRange {
    TextRange::new(u32::from(range.start()), u32::from(range.end()))
}
