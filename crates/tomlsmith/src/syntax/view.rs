use std::fmt;

use rowan::NodeOrToken;

use crate::TextRange;

use super::kind::{SyntaxKind, TomlLanguage};

/// A snapshot-scoped lossless syntax node.
#[derive(Clone)]
pub struct SyntaxNode(rowan::SyntaxNode<TomlLanguage>);

/// A snapshot-scoped lossless source token.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct SyntaxToken(rowan::SyntaxToken<TomlLanguage>);

/// Either a child syntax node or a child source token.
#[derive(Clone)]
pub enum SyntaxElement {
    /// A structured syntax node.
    Node(SyntaxNode),
    /// A lossless source token.
    Token(SyntaxToken),
}

impl SyntaxNode {
    pub(crate) fn new_root(green: rowan::GreenNode) -> Self {
        Self(rowan::SyntaxNode::new_root(green))
    }

    /// Returns this node's grammar category.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.0.kind()
    }

    /// Returns this node's half-open UTF-8 byte range.
    #[must_use]
    pub fn range(&self) -> TextRange {
        range(self.0.text_range())
    }

    /// Reconstructs the exact source text covered by this node.
    #[must_use]
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }

    /// Iterates direct child nodes, excluding tokens.
    pub fn children(&self) -> impl Iterator<Item = Self> + '_ {
        self.0.children().map(Self)
    }

    /// Iterates direct child nodes and tokens in source order.
    pub fn children_with_tokens(&self) -> impl Iterator<Item = SyntaxElement> + '_ {
        self.0.children_with_tokens().map(|element| match element {
            NodeOrToken::Node(node) => SyntaxElement::Node(Self(node)),
            NodeOrToken::Token(token) => SyntaxElement::Token(SyntaxToken(token)),
        })
    }
}

impl SyntaxToken {
    /// Returns this token's lexical category.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.0.kind()
    }

    /// Returns this token's half-open UTF-8 byte range.
    #[must_use]
    pub fn range(&self) -> TextRange {
        range(self.0.text_range())
    }

    /// Returns the exact source slice represented by this token.
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
