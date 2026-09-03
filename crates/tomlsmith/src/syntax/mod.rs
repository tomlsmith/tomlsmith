pub(crate) mod ast;
mod kind;
pub(crate) mod lexer;
mod parser;
mod sink;
mod view;

use rowan::GreenNode;

use crate::Diagnostic;

pub use kind::SyntaxKind;
pub(crate) use kind::TomlLanguage;
pub(crate) use lexer::TokenTape;
pub use view::{SyntaxElement, SyntaxNode, SyntaxToken};

pub(crate) struct Parse {
    pub(crate) green: GreenNode,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) tokens: TokenTape,
}

pub(crate) fn parse(source: &str) -> Parse {
    let lexed = lexer::lex(source);
    let parsed = parser::parse(&lexed.tokens);
    let green = sink::finish(source, &lexed.tokens, &parsed.events);
    let mut diagnostics = lexed.diagnostics;
    diagnostics.extend(parsed.diagnostics);
    diagnostics.sort_by_key(Diagnostic::range);

    Parse {
        green,
        diagnostics,
        tokens: lexed.tokens,
    }
}

pub(crate) fn root(green: GreenNode) -> SyntaxNode {
    SyntaxNode::new_root(green)
}
