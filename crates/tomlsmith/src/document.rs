use std::{fmt, sync::Arc};

use rowan::GreenNode;

use crate::{Diagnostic, SemanticDocument, SyntaxNode, semantic, syntax};

/// The published TOML language specification used to parse and validate a snapshot.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum TomlVersion {
    /// TOML 1.0.0.
    V1_0,
    /// TOML 1.1.0.
    #[default]
    V1_1,
}

/// An immutable TOML source snapshot and its derived syntax, semantics, diagnostics, and spans.
///
/// Clones share all snapshot data. Applying [`TextChange`] values creates another independently
/// queryable snapshot rather than mutating the original.
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

/// A monotonically increasing identifier for snapshots derived through [`Document::with_changes`].
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Revision(u64);

impl Revision {
    /// The revision assigned to a freshly parsed snapshot.
    pub const INITIAL: Self = Self(0);

    /// Creates a revision from its numeric representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A source update applied to an immutable [`Document`] snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextChange {
    /// Replaces the complete document text.
    Replace(Arc<str>),
    /// Replaces a UTF-8 byte range with new text.
    Edit {
        /// The half-open byte range in the result of the preceding change.
        range: crate::TextRange,
        /// Text inserted in place of `range`.
        insert: Arc<str>,
    },
}

impl TextChange {
    /// Creates a complete-document replacement.
    #[must_use]
    pub fn replace(text: impl Into<Arc<str>>) -> Self {
        Self::Replace(text.into())
    }

    /// Creates a range edit whose offsets are UTF-8 bytes.
    #[must_use]
    pub fn edit(range: crate::TextRange, insert: impl Into<Arc<str>>) -> Self {
        Self::Edit {
            range,
            insert: insert.into(),
        }
    }
}

/// Why a source update could not produce a new snapshot.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ChangeError {
    /// The edit range is reversed or extends beyond the current text.
    #[error("edit range {start}..{end} is outside a document of {text_len} bytes")]
    OutOfBounds {
        /// Requested start offset.
        start: u32,
        /// Requested exclusive end offset.
        end: u32,
        /// Current document length in bytes.
        text_len: u32,
    },
    /// An edit offset splits a multi-byte UTF-8 scalar value.
    #[error("byte offset {offset} is not a UTF-8 character boundary")]
    InvalidUtf8Boundary {
        /// The invalid byte offset.
        offset: u32,
    },
    /// Incrementing the previous snapshot revision would exceed `u64`.
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
        Self::parse_pipeline(text, version, revision, None).0
    }

    /// Parses `text` under `version` and immediately formats the snapshot
    /// with `options`, producing exactly what `parse_as` followed by
    /// `format_with` would. One-shot pipelines get the formatted text's
    /// purely lexical construction overlapped with semantic analysis.
    #[must_use]
    pub fn parse_and_format_with(
        text: impl Into<Arc<str>>,
        version: TomlVersion,
        options: &crate::FormatOptions,
    ) -> (Self, crate::FormatOutcome) {
        let text = text.into();
        if options.target_version != version {
            // A version mismatch makes the formatter's safety check re-parse
            // under the target version, so there is no overlap to win here.
            let document = Self::parse_at(text, version, Revision::INITIAL);
            let outcome = document.format_with(options);
            return (document, outcome);
        }
        let (document, formatted) =
            Self::parse_pipeline(text, version, Revision::INITIAL, Some(options));
        let outcome = match formatted {
            Some(output) => crate::formatter::finish_prebuilt(&document, options, output),
            None => document.format_with(options),
        };
        (document, outcome)
    }

    /// Shared parse pipeline. When `format_options` is given, the formatted
    /// text is additionally produced on the side-analysis thread, so a
    /// one-shot parse-then-format pipeline pays no extra wall time for it.
    fn parse_pipeline(
        text: Arc<str>,
        version: TomlVersion,
        revision: Revision,
        format_options: Option<&crate::FormatOptions>,
    ) -> (Self, Option<String>) {
        let syntax::Parse {
            green,
            mut diagnostics,
            tokens,
        } = syntax::parse(&text);
        // Highlight collection, validation, and the optional lexical
        // formatting pass only need the source text, token stream, and green
        // tree. Native targets overlap them with semantic lowering. WebAssembly
        // targets run the same pure operations sequentially because
        // `wasm32-unknown-unknown` has no native thread-spawn capability.
        #[cfg(target_family = "wasm")]
        let (lowered, (highlights, validation_diagnostics, formatted)) = {
            let lowered = semantic::lower(&text, &green);
            let highlights = crate::highlight::collect(&text, &tokens);
            let validation_diagnostics = crate::validate::validate(&text, version, &green, &tokens);
            let formatted =
                format_options.map(|options| crate::formatter::build_text(&text, options));
            (lowered, (highlights, validation_diagnostics, formatted))
        };
        #[cfg(not(target_family = "wasm"))]
        let (lowered, (highlights, validation_diagnostics, formatted)) =
            std::thread::scope(|scope| {
                let side = scope.spawn(|| {
                    let highlights = crate::highlight::collect(&text, &tokens);
                    let validation_diagnostics =
                        crate::validate::validate(&text, version, &green, &tokens);
                    let formatted =
                        format_options.map(|options| crate::formatter::build_text(&text, options));
                    (highlights, validation_diagnostics, formatted)
                });
                let lowered = semantic::lower(&text, &green);
                let side = match side.join() {
                    Ok(side) => side,
                    Err(panic) => std::panic::resume_unwind(panic),
                };
                (lowered, side)
            });
        drop(tokens);
        diagnostics.extend(validation_diagnostics);
        diagnostics.extend(lowered.diagnostics);
        diagnostics.sort_by_key(Diagnostic::range);

        let document = Self {
            inner: Arc::new(DocumentData {
                text,
                version,
                revision,
                green,
                diagnostics: diagnostics.into(),
                semantic: lowered.document,
                highlights: highlights.into(),
            }),
        };
        (document, formatted)
    }

    /// Returns the exact source text owned by this snapshot.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.inner.text
    }

    /// Returns the TOML language version selected for this snapshot.
    #[must_use]
    pub fn version(&self) -> TomlVersion {
        self.inner.version
    }

    /// Returns the snapshot revision.
    #[must_use]
    pub fn revision(&self) -> Revision {
        self.inner.revision
    }

    /// Returns parser, version, and semantic diagnostics sorted by primary range.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.inner.diagnostics
    }

    /// Returns declarations and lazily materialized decoded values for this snapshot.
    #[must_use]
    pub fn semantics(&self) -> &SemanticDocument {
        &self.inner.semantic
    }

    /// Returns source-backed semantic highlight spans.
    #[must_use]
    pub fn highlights(&self) -> &[crate::Highlight] {
        &self.inner.highlights
    }

    /// Formats with default layout options and this snapshot's TOML version.
    ///
    /// Invalid snapshots are refused rather than rewritten.
    #[must_use]
    pub fn format(&self) -> crate::FormatOutcome {
        let options = crate::FormatOptions {
            target_version: self.version(),
            ..crate::FormatOptions::default()
        };
        self.format_with(&options)
    }

    /// Formats with explicit layout and target-version options.
    ///
    /// If `options.target_version` differs from [`Self::version`], safety diagnostics are
    /// recomputed under the target version before any change is returned.
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
