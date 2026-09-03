use std::fmt;

/// A UTF-8 byte range in a TOML document.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TextRange {
    start: u32,
    end: u32,
}

impl TextRange {
    /// Creates a half-open byte range `start..end`.
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// Returns the inclusive start byte offset.
    #[must_use]
    pub const fn start(self) -> u32 {
        self.start
    }

    /// Returns the exclusive end byte offset.
    #[must_use]
    pub const fn end(self) -> u32 {
        self.end
    }

    /// Returns whether the range contains no bytes.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub(crate) fn from_usize(start: usize, end: usize) -> Self {
        Self {
            start: u32::try_from(start).unwrap_or(u32::MAX),
            end: u32::try_from(end).unwrap_or(u32::MAX),
        }
    }
}

/// A stable machine-readable diagnostic identifier.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DiagnosticCode(&'static str);

impl DiagnosticCode {
    /// A string or quoted key reaches the end of input without its closing delimiter.
    pub const UNTERMINATED_STRING: Self = Self("parse.unterminated-string");
    /// A key-value statement has no equals sign.
    pub const MISSING_EQUALS: Self = Self("parse.missing-equals");
    /// A table or array-of-tables header has no closing bracket.
    pub const UNCLOSED_TABLE_HEADER: Self = Self("parse.unclosed-table-header");
    /// An array has no closing bracket.
    pub const UNCLOSED_ARRAY: Self = Self("parse.unclosed-array");
    /// An inline table has no closing brace.
    pub const UNCLOSED_INLINE_TABLE: Self = Self("parse.unclosed-inline-table");
    /// Adjacent container values or entries are missing a comma.
    pub const MISSING_COMMA: Self = Self("parse.missing-comma");
    /// A key-value pair, array item, or inline-table entry has no value.
    pub const MISSING_VALUE: Self = Self("parse.missing-value");
    /// A statement contains tokens after its complete value or header.
    pub const TRAILING_TOKENS: Self = Self("parse.trailing-tokens");
    /// A parser-controlled nesting or key-depth limit was exceeded.
    pub const NESTING_LIMIT: Self = Self("parse.nesting-limit");
    /// A basic string contains an escape sequence not permitted by the selected TOML version.
    pub const INVALID_ESCAPE: Self = Self("parse.invalid-escape");
    /// A string contains a forbidden control character.
    pub const INVALID_CONTROL_CHARACTER: Self = Self("parse.invalid-control-character");
    /// A statement or dotted path contains an empty key segment.
    pub const MISSING_KEY: Self = Self("parse.missing-key");
    /// A bare key contains a character outside the selected TOML grammar.
    pub const INVALID_BARE_KEY: Self = Self("parse.invalid-bare-key");
    /// A value token cannot be decoded as a TOML value.
    pub const INVALID_VALUE: Self = Self("parse.invalid-value");
    /// Input supplied through a byte-oriented adapter is not valid UTF-8.
    ///
    /// The core [`crate::Document`] accepts UTF-8 Rust strings, so adapters use this code when
    /// validating their byte input before parsing.
    pub const INVALID_UTF8: Self = Self("parse.invalid-utf8");
    /// TOML 1.1-only syntax was used while TOML 1.0 was selected.
    pub const TOML_1_1_SYNTAX: Self = Self("version.toml-1.1-syntax");
    /// The same key or table was declared more than once.
    pub const DUPLICATE_KEY: Self = Self("semantic.duplicate-key");
    /// A declaration conflicts with an existing value or table path.
    pub const CONFLICTING_KEY: Self = Self("semantic.conflicting-key");

    /// Returns the stable dotted identifier used by machine-readable integrations.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// Whether a diagnostic with this code makes a snapshot unsafe to rewrite.
    ///
    /// Parse, version, and semantic problems all refuse formatting; the same
    /// predicate decides refusal and whether a speculative render is skipped,
    /// so the two can never disagree.
    pub(crate) fn refuses_formatting(self) -> bool {
        let code = self.0;
        code.starts_with("parse.") || code.starts_with("semantic.") || code.starts_with("version.")
    }
}

impl fmt::Debug for DiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("DiagnosticCode")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// The impact of a diagnostic on document validity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Severity {
    /// The document is invalid for the selected TOML version.
    Error,
    /// The document remains valid, but an integration should surface the finding.
    Warning,
}

/// A parser, version, or semantic finding tied to a UTF-8 byte range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    code: DiagnosticCode,
    severity: Severity,
    message: String,
    range: TextRange,
    related_range: Option<TextRange>,
}

impl Diagnostic {
    pub(crate) fn error(
        code: DiagnosticCode,
        message: impl Into<String>,
        range: TextRange,
    ) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            range,
            related_range: None,
        }
    }

    pub(crate) const fn with_related_range(mut self, range: TextRange) -> Self {
        self.related_range = Some(range);
        self
    }

    /// Returns the stable machine-readable code.
    #[must_use]
    pub const fn code(&self) -> DiagnosticCode {
        self.code
    }

    /// Returns whether the finding invalidates the document.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// Returns the human-readable explanation.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the primary UTF-8 byte range in the parsed snapshot.
    #[must_use]
    pub const fn range(&self) -> TextRange {
        self.range
    }

    /// The earlier declaration this diagnostic conflicts with, when the
    /// conflict scan resolved one; consumers can link a duplicate or
    /// conflicting key back to its first declaration without re-deriving
    /// the pairing.
    #[must_use]
    pub const fn related_range(&self) -> Option<TextRange> {
        self.related_range
    }
}
