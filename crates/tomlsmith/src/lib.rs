#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Lossless, error-tolerant TOML parsing, validation, semantics, highlighting, and formatting.
//!
//! [`Document`] is the main entry point. It owns an immutable source snapshot and all products
//! derived from that snapshot. Choose the TOML language version explicitly in reusable tools;
//! [`Document::parse`] is a convenience that currently selects TOML 1.1.
//!
//! ```
//! use tomlsmith::{Document, FormatOutcome, TomlVersion};
//!
//! let document = Document::parse_as("name=\"TomlSmith\"\n", TomlVersion::V1_1);
//! assert!(document.diagnostics().is_empty());
//! assert_eq!(document.semantics().declarations().len(), 1);
//!
//! match document.format() {
//!     FormatOutcome::Changed { text, .. } => assert_eq!(&*text, "name = \"TomlSmith\"\n"),
//!     outcome => panic!("expected a formatting change, got {outcome:?}"),
//! }
//! ```
//!
//! Formatting is guarded: snapshots with parse, version, or semantic errors produce
//! [`FormatOutcome::Refused`] instead of rewritten text. Syntax handles are snapshot-scoped and
//! expose `TomlSmith`'s own [`SyntaxKind`] rather than the private tree implementation.

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
