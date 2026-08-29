#![forbid(unsafe_code)]

mod diagnostic;
mod document;
mod formatter;
mod highlight;
mod literal;
mod semantic;
mod syntax;
mod validate;

pub use diagnostic::{Diagnostic, DiagnosticCode, Severity, TextRange};
pub use document::{ChangeError, Document, Revision, TextChange, TomlVersion};
pub use formatter::{FormatOptions, FormatOutcome, LineEnding, TextEdit};
pub use highlight::{Highlight, HighlightKind};
pub use semantic::{
    DateTimeKind, DateTimeValue, Declaration, DeclarationKind, KeyPath, Resolution,
    SemanticDocument, SemanticTable, SemanticValue,
};
pub use syntax::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};
