use crate::{Diagnostic, DiagnosticCode, TextRange};

use super::{SyntaxKind, TokenTape};

pub(crate) enum Event {
    Start(SyntaxKind),
    Token(usize),
    Finish,
}

pub(crate) struct Parsed {
    pub(crate) events: Vec<Event>,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

pub(crate) fn parse(tokens: &TokenTape) -> Parsed {
    Parser {
        tokens,
        position: 0,
        collection_depth: 0,
        events: vec![Event::Start(SyntaxKind::Root)],
        diagnostics: Vec::new(),
    }
    .root()
}

struct Parser<'tokens> {
    tokens: &'tokens TokenTape,
    position: usize,
    collection_depth: usize,
    events: Vec<Event>,
    diagnostics: Vec<Diagnostic>,
}

const MAX_COLLECTION_DEPTH: usize = 256;

impl Parser<'_> {
    fn root(mut self) -> Parsed {
        while self.position < self.tokens.len() {
            match self.current() {
                SyntaxKind::Bom
                | SyntaxKind::Whitespace
                | SyntaxKind::Newline
                | SyntaxKind::Comment => self.bump(),
                SyntaxKind::LeftBracket => self.table_header(),
                _ => self.key_value(),
            }
        }
        self.events.push(Event::Finish);
        Parsed {
            events: self.events,
            diagnostics: self.diagnostics,
        }
    }

    fn key_value(&mut self) {
        self.start(SyntaxKind::KeyValue);
        self.start(SyntaxKind::Key);
        let mut has_key = false;
        while !self.at_line_end() && self.current() != SyntaxKind::Equals {
            has_key |= matches!(
                self.current(),
                SyntaxKind::Bare | SyntaxKind::BasicString | SyntaxKind::LiteralString
            );
            self.bump();
        }
        self.finish();
        if !has_key {
            self.error_here(DiagnosticCode::MISSING_KEY, "expected a key before `=`");
        }

        if self.current() == SyntaxKind::Equals {
            self.bump();
        } else {
            let offset = self.current_offset();
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::MISSING_EQUALS,
                "expected `=` after key",
                TextRange::new(offset, offset),
            ));
        }

        self.start(SyntaxKind::Value);
        while self.current() == SyntaxKind::Whitespace {
            self.bump();
        }
        if self.at_line_end() {
            self.error_here(DiagnosticCode::MISSING_VALUE, "expected a value after `=`");
        } else {
            self.value_body(ValueStop::Line);
            while self.current() == SyntaxKind::Whitespace {
                self.bump();
            }
            if !self.at_line_end() {
                self.error_here(
                    DiagnosticCode::TRAILING_TOKENS,
                    "unexpected tokens after value",
                );
                while !self.at_line_end() {
                    self.bump();
                }
            }
        }
        self.finish();
        self.finish();
    }

    fn value_body(&mut self, stop: ValueStop) {
        match self.current() {
            SyntaxKind::LeftBracket => self.array(),
            SyntaxKind::LeftBrace => self.inline_table(),
            _ => {
                while !self.is_eof() && !stop.matches(self.current()) {
                    self.bump();
                }
            }
        }
    }

    fn array(&mut self) {
        self.start(SyntaxKind::Array);
        if self.collection_depth == MAX_COLLECTION_DEPTH {
            self.error_here(
                DiagnosticCode::NESTING_LIMIT,
                "collection nesting exceeds the supported limit",
            );
            self.consume_flat_collection();
            self.finish();
            return;
        }
        self.collection_depth += 1;
        self.bump();
        loop {
            self.collection_trivia();
            if self.current() == SyntaxKind::RightBracket {
                self.bump();
                self.collection_depth -= 1;
                self.finish();
                return;
            }
            if self.is_eof() {
                self.error_here(
                    DiagnosticCode::UNCLOSED_ARRAY,
                    "array is missing a closing `]`",
                );
                self.collection_depth -= 1;
                self.finish();
                return;
            }

            if self.current() == SyntaxKind::Comma {
                self.error_here(DiagnosticCode::MISSING_VALUE, "expected an array value");
                self.bump();
                continue;
            }

            self.start(SyntaxKind::Value);
            self.value_body(ValueStop::Array);
            self.finish();
            self.collection_trivia();

            match self.current() {
                SyntaxKind::Comma => self.bump(),
                SyntaxKind::RightBracket => {}
                _ => {
                    self.error_here(
                        DiagnosticCode::MISSING_COMMA,
                        "expected `,` between array values",
                    );
                    self.recover_collection(SyntaxKind::RightBracket);
                    if self.current() == SyntaxKind::Comma {
                        self.bump();
                    }
                }
            }
        }
    }

    fn inline_table(&mut self) {
        self.start(SyntaxKind::InlineTable);
        if self.collection_depth == MAX_COLLECTION_DEPTH {
            self.error_here(
                DiagnosticCode::NESTING_LIMIT,
                "collection nesting exceeds the supported limit",
            );
            self.consume_flat_collection();
            self.finish();
            return;
        }
        self.collection_depth += 1;
        self.bump();
        loop {
            self.collection_trivia();
            if self.current() == SyntaxKind::RightBrace {
                self.bump();
                self.collection_depth -= 1;
                self.finish();
                return;
            }
            if self.is_eof() {
                self.error_here(
                    DiagnosticCode::UNCLOSED_INLINE_TABLE,
                    "inline table is missing a closing `}`",
                );
                self.collection_depth -= 1;
                self.finish();
                return;
            }

            self.inline_key_value();
            self.collection_trivia();
            match self.current() {
                SyntaxKind::Comma => self.bump(),
                SyntaxKind::RightBrace => {}
                _ => {
                    self.error_here(
                        DiagnosticCode::MISSING_COMMA,
                        "expected `,` between inline-table entries",
                    );
                    self.recover_collection(SyntaxKind::RightBrace);
                    if self.current() == SyntaxKind::Comma {
                        self.bump();
                    }
                }
            }
        }
    }

    fn inline_key_value(&mut self) {
        self.start(SyntaxKind::KeyValue);
        self.start(SyntaxKind::Key);
        let mut has_key = false;
        while !self.is_eof()
            && !matches!(
                self.current(),
                SyntaxKind::Equals
                    | SyntaxKind::Comma
                    | SyntaxKind::RightBrace
                    | SyntaxKind::Newline
                    | SyntaxKind::Comment
            )
        {
            has_key |= matches!(
                self.current(),
                SyntaxKind::Bare | SyntaxKind::BasicString | SyntaxKind::LiteralString
            );
            self.bump();
        }
        self.finish();
        if !has_key {
            self.error_here(DiagnosticCode::MISSING_KEY, "expected an inline-table key");
        }
        if self.current() == SyntaxKind::Equals {
            self.bump();
        } else {
            self.error_here(
                DiagnosticCode::MISSING_EQUALS,
                "expected `=` after inline-table key",
            );
        }
        self.start(SyntaxKind::Value);
        while self.current() == SyntaxKind::Whitespace {
            self.bump();
        }
        if ValueStop::InlineTable.matches(self.current()) || self.is_eof() {
            self.error_here(
                DiagnosticCode::MISSING_VALUE,
                "expected an inline-table value",
            );
        } else {
            self.value_body(ValueStop::InlineTable);
        }
        self.finish();
        self.finish();
    }

    fn table_header(&mut self) {
        let array_table = self.nth(1) == SyntaxKind::LeftBracket;
        let expected_closing = if array_table { 2 } else { 1 };
        self.start(if array_table {
            SyntaxKind::ArrayTable
        } else {
            SyntaxKind::Table
        });

        for _ in 0..expected_closing {
            if self.current() == SyntaxKind::LeftBracket {
                self.bump();
            }
        }
        self.start(SyntaxKind::Key);
        let mut has_key = false;
        while !self.at_line_end()
            && !(self.current() == SyntaxKind::RightBracket
                && (!array_table || self.nth(1) == SyntaxKind::RightBracket))
        {
            has_key |= matches!(
                self.current(),
                SyntaxKind::Bare | SyntaxKind::BasicString | SyntaxKind::LiteralString
            );
            self.bump();
        }
        self.finish();
        if !has_key {
            self.error_here(DiagnosticCode::MISSING_KEY, "expected a table key");
        }

        let mut closing = 0;
        while closing < expected_closing && self.current() == SyntaxKind::RightBracket {
            closing += 1;
            self.bump();
        }
        if closing == expected_closing {
            while self.current() == SyntaxKind::Whitespace {
                self.bump();
            }
            if !self.at_line_end() {
                self.error_here(
                    DiagnosticCode::TRAILING_TOKENS,
                    "unexpected tokens after table header",
                );
            }
        }
        while !self.at_line_end() {
            self.bump();
        }
        self.finish();

        if closing < expected_closing {
            let offset = self.current_offset();
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::UNCLOSED_TABLE_HEADER,
                "table header is missing a closing bracket",
                TextRange::new(offset, offset),
            ));
        }
    }

    fn at_line_end(&self) -> bool {
        self.is_eof() || matches!(self.current(), SyntaxKind::Newline | SyntaxKind::Comment)
    }

    fn is_eof(&self) -> bool {
        self.position == self.tokens.len()
    }

    fn collection_trivia(&mut self) {
        while matches!(
            self.current(),
            SyntaxKind::Whitespace | SyntaxKind::Newline | SyntaxKind::Comment
        ) && !self.is_eof()
        {
            self.bump();
        }
    }

    fn recover_collection(&mut self, closing: SyntaxKind) {
        while !self.is_eof()
            && !matches!(self.current(), SyntaxKind::Comma | SyntaxKind::Newline)
            && self.current() != closing
        {
            self.bump();
        }
    }

    fn consume_flat_collection(&mut self) {
        let mut closings = Vec::new();
        while !self.is_eof() {
            let kind = self.current();
            match kind {
                SyntaxKind::LeftBracket => closings.push(SyntaxKind::RightBracket),
                SyntaxKind::LeftBrace => closings.push(SyntaxKind::RightBrace),
                SyntaxKind::RightBracket | SyntaxKind::RightBrace
                    if closings.last() == Some(&kind) =>
                {
                    closings.pop();
                }
                _ => {}
            }
            self.bump();
            if closings.is_empty() {
                return;
            }
        }
    }

    fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    fn nth(&self, lookahead: usize) -> SyntaxKind {
        let index = self.position + lookahead;
        if index < self.tokens.len() {
            self.tokens.kind(index)
        } else {
            SyntaxKind::Invalid
        }
    }

    fn current_offset(&self) -> u32 {
        let offset = self.tokens.get(self.position).map_or_else(
            || self.tokens.last().map_or(0, |token| token.range.end),
            |token| token.range.start,
        );
        u32::try_from(offset).unwrap_or(u32::MAX)
    }

    fn error_here(&mut self, code: DiagnosticCode, message: &'static str) {
        let offset = self.current_offset();
        self.diagnostics.push(Diagnostic::error(
            code,
            message,
            TextRange::new(offset, offset),
        ));
    }

    fn start(&mut self, kind: SyntaxKind) {
        self.events.push(Event::Start(kind));
    }

    fn finish(&mut self) {
        self.events.push(Event::Finish);
    }

    fn bump(&mut self) {
        self.events.push(Event::Token(self.position));
        self.position += 1;
    }
}

#[derive(Clone, Copy)]
enum ValueStop {
    Line,
    Array,
    InlineTable,
}

impl ValueStop {
    const fn matches(self, kind: SyntaxKind) -> bool {
        match self {
            Self::Line => matches!(kind, SyntaxKind::Newline | SyntaxKind::Comment),
            Self::Array => matches!(
                kind,
                SyntaxKind::Comma
                    | SyntaxKind::RightBracket
                    | SyntaxKind::Newline
                    | SyntaxKind::Comment
            ),
            Self::InlineTable => matches!(
                kind,
                SyntaxKind::Comma
                    | SyntaxKind::RightBrace
                    | SyntaxKind::Newline
                    | SyntaxKind::Comment
            ),
        }
    }
}
