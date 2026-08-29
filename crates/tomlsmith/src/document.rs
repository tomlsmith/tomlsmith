use std::{fmt, sync::Arc};

use rowan::GreenNode;

use crate::{Diagnostic, SemanticDocument, SyntaxNode, semantic, syntax};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum TomlVersion {
    V1_0,
    #[default]
    V1_1,
}

#[derive(Clone)]
pub struct Document {
    inner: Arc<DocumentData>,
}

struct DocumentData {
    text: Arc<str>,
    version: TomlVersion,
    revision: Revision,
    #[allow(dead_code)]
    green: GreenNode,
    diagnostics: Arc<[Diagnostic]>,
    semantic: SemanticDocument,
    highlights: Arc<[crate::Highlight]>,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Revision(u64);

impl Revision {
    pub const INITIAL: Self = Self(0);

    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextChange {
    Replace(Arc<str>),
    Edit {
        range: crate::TextRange,
        insert: Arc<str>,
    },
}

impl TextChange {
    #[must_use]
    pub fn replace(text: impl Into<Arc<str>>) -> Self {
        Self::Replace(text.into())
    }

    #[must_use]
    pub fn edit(range: crate::TextRange, insert: impl Into<Arc<str>>) -> Self {
        Self::Edit {
            range,
            insert: insert.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ChangeError {
    #[error("edit range {start}..{end} is outside a document of {text_len} bytes")]
    OutOfBounds { start: u32, end: u32, text_len: u32 },
    #[error("byte offset {offset} is not a UTF-8 character boundary")]
    InvalidUtf8Boundary { offset: u32 },
    #[error("document revision overflowed")]
    RevisionOverflow,
}

impl Document {
    /// Parses a document using TOML 1.1 rules.
    #[must_use]
    pub fn parse(text: impl Into<Arc<str>>) -> Self {
        Self::parse_as(text, TomlVersion::V1_1)
    }

    /// Parses a document with an explicit TOML language version.
    #[must_use]
    pub fn parse_as(text: impl Into<Arc<str>>, version: TomlVersion) -> Self {
        Self::parse_at(text.into(), version, Revision::INITIAL)
    }

    fn parse_at(text: Arc<str>, version: TomlVersion, revision: Revision) -> Self {
        let parsed = syntax::parse(&text);
        let highlights = crate::highlight::collect(&text, &parsed.tokens);
        let validation_diagnostics =
            crate::validate::validate(&text, version, parsed.green.clone(), &parsed.tokens);
        let syntax::Parse {
            green,
            mut diagnostics,
            tokens,
        } = parsed;
        drop(tokens);
        let lowered = semantic::lower(&text, green.clone());
        diagnostics.extend(validation_diagnostics);
        diagnostics.extend(lowered.diagnostics);
        diagnostics.sort_by_key(Diagnostic::range);

        Self {
            inner: Arc::new(DocumentData {
                text,
                version,
                revision,
                green,
                diagnostics: diagnostics.into(),
                semantic: lowered.document,
                highlights: highlights.into(),
            }),
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.inner.text
    }

    #[must_use]
    pub fn version(&self) -> TomlVersion {
        self.inner.version
    }

    #[must_use]
    pub fn revision(&self) -> Revision {
        self.inner.revision
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.inner.diagnostics
    }

    #[must_use]
    pub fn semantics(&self) -> &SemanticDocument {
        &self.inner.semantic
    }

    #[must_use]
    pub fn highlights(&self) -> &[crate::Highlight] {
        &self.inner.highlights
    }

    #[must_use]
    pub fn format(&self) -> crate::FormatOutcome {
        let options = crate::FormatOptions {
            target_version: self.version(),
            ..crate::FormatOptions::default()
        };
        self.format_with(&options)
    }

    #[must_use]
    pub fn format_with(&self, options: &crate::FormatOptions) -> crate::FormatOutcome {
        crate::formatter::format(self, options)
    }

    /// Returns a snapshot-scoped syntax root without exposing Rowan types.
    #[must_use]
    pub fn root(&self) -> SyntaxNode {
        syntax::root(self.inner.green.clone())
    }

    /// Applies edits in iteration order and returns a new immutable snapshot.
    /// Each edit range is interpreted against the result of the preceding edit.
    ///
    /// # Errors
    ///
    /// Returns an error when a range is out of bounds, splits a UTF-8 code point,
    /// or the snapshot revision cannot be incremented.
    pub fn with_changes(
        &self,
        changes: impl IntoIterator<Item = TextChange>,
    ) -> Result<Self, ChangeError> {
        let mut text = self.text().to_owned();

        for change in changes {
            match change {
                TextChange::Replace(replacement) => text = replacement.to_string(),
                TextChange::Edit { range, insert } => {
                    let start = range.start() as usize;
                    let end = range.end() as usize;
                    if start > end || end > text.len() {
                        return Err(ChangeError::OutOfBounds {
                            start: range.start(),
                            end: range.end(),
                            text_len: u32::try_from(text.len()).unwrap_or(u32::MAX),
                        });
                    }
                    if !text.is_char_boundary(start) {
                        return Err(ChangeError::InvalidUtf8Boundary {
                            offset: range.start(),
                        });
                    }
                    if !text.is_char_boundary(end) {
                        return Err(ChangeError::InvalidUtf8Boundary {
                            offset: range.end(),
                        });
                    }
                    text.replace_range(start..end, &insert);
                }
            }
        }

        let revision = self
            .revision()
            .get()
            .checked_add(1)
            .map(Revision::new)
            .ok_or(ChangeError::RevisionOverflow)?;
        Ok(Self::parse_at(text.into(), self.version(), revision))
    }
}

impl fmt::Debug for Document {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Document")
            .field("version", &self.version())
            .field("revision", &self.revision())
            .field("text_len", &self.text().len())
            .field("diagnostics", &self.diagnostics())
            .finish_non_exhaustive()
    }
}
