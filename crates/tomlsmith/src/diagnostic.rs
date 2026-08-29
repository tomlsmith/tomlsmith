use std::fmt;

/// A UTF-8 byte range in a TOML document.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TextRange {
    start: u32,
    end: u32,
}

impl TextRange {
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn start(self) -> u32 {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> u32 {
        self.end
    }

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
    pub const UNTERMINATED_STRING: Self = Self("parse.unterminated-string");
    pub const MISSING_EQUALS: Self = Self("parse.missing-equals");
    pub const UNCLOSED_TABLE_HEADER: Self = Self("parse.unclosed-table-header");
    pub const UNCLOSED_ARRAY: Self = Self("parse.unclosed-array");
    pub const UNCLOSED_INLINE_TABLE: Self = Self("parse.unclosed-inline-table");
    pub const MISSING_COMMA: Self = Self("parse.missing-comma");
    pub const MISSING_VALUE: Self = Self("parse.missing-value");
    pub const TRAILING_TOKENS: Self = Self("parse.trailing-tokens");
    pub const NESTING_LIMIT: Self = Self("parse.nesting-limit");
    pub const INVALID_ESCAPE: Self = Self("parse.invalid-escape");
    pub const INVALID_CONTROL_CHARACTER: Self = Self("parse.invalid-control-character");
    pub const MISSING_KEY: Self = Self("parse.missing-key");
    pub const INVALID_BARE_KEY: Self = Self("parse.invalid-bare-key");
    pub const INVALID_VALUE: Self = Self("parse.invalid-value");
    pub const INVALID_UTF8: Self = Self("parse.invalid-utf8");
    pub const TOML_1_1_SYNTAX: Self = Self("version.toml-1.1-syntax");
    pub const DUPLICATE_KEY: Self = Self("semantic.duplicate-key");
    pub const CONFLICTING_KEY: Self = Self("semantic.conflicting-key");

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    code: DiagnosticCode,
    severity: Severity,
    message: String,
    range: TextRange,
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
        }
    }

    #[must_use]
    pub const fn code(&self) -> DiagnosticCode {
        self.code
    }

    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub const fn range(&self) -> TextRange {
        self.range
    }
}
