use std::sync::Arc;

use crate::{Diagnostic, Document, SyntaxKind, TextRange, TomlVersion, syntax::lexer};

/// How formatted output chooses newline bytes.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum LineEnding {
    /// Use the first newline style found in the source, falling back to LF.
    #[default]
    Preserve,
    /// Write line feed (`\n`) newlines.
    Lf,
    /// Write carriage-return and line-feed (`\r\n`) newlines.
    CrLf,
}

/// Layout and language-version settings for guarded formatting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormatOptions {
    /// The TOML version that the formatted output must satisfy.
    pub target_version: TomlVersion,
    /// Spaces written for each nested array or inline-table indentation level.
    pub indent_width: u8,
    /// Preferred maximum line width used when deciding whether container items should wrap.
    pub line_width: u16,
    /// Newline style written by the formatter.
    pub line_ending: LineEnding,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            target_version: TomlVersion::V1_1,
            indent_width: 2,
            line_width: 100,
            line_ending: LineEnding::Preserve,
        }
    }
}

/// A replacement over the original snapshot's UTF-8 byte offsets.
///
/// Formatter edits currently describe the complete replacement required to produce the returned
/// text; they are not a minimal-edit contract. Editor adapters may derive smaller protocol edits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextEdit {
    range: TextRange,
    replacement: Arc<str>,
}

impl TextEdit {
    /// Creates a replacement edit.
    #[must_use]
    pub fn new(range: TextRange, replacement: impl Into<Arc<str>>) -> Self {
        Self {
            range,
            replacement: replacement.into(),
        }
    }

    /// Returns the half-open source byte range replaced by this edit.
    #[must_use]
    pub const fn range(&self) -> TextRange {
        self.range
    }

    /// Returns the replacement text.
    #[must_use]
    pub fn replacement(&self) -> &str {
        &self.replacement
    }
}

/// The result of guarded full-document formatting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FormatOutcome {
    /// The source already matches the selected layout.
    Unchanged,
    /// Formatting is safe and changes the source.
    Changed {
        /// Complete formatted document text.
        text: Arc<str>,
        /// Replacements that transform the original snapshot into `text`.
        edits: Arc<[TextEdit]>,
    },
    /// Formatting was not attempted because the source is unsafe to rewrite.
    Refused {
        /// Parse, version, or semantic errors responsible for the refusal.
        diagnostics: Arc<[Diagnostic]>,
    },
}

pub(crate) fn format(document: &Document, options: &FormatOptions) -> FormatOutcome {
    let unsafe_diagnostics = unsafe_diagnostics(document, options.target_version);
    if !unsafe_diagnostics.is_empty() {
        return FormatOutcome::Refused {
            diagnostics: unsafe_diagnostics.into(),
        };
    }

    finish_format(
        document.text(),
        build_text(document.text(), document.tape(), options),
    )
}

/// Turns already-built formatted text into the outcome `format` would have
/// produced, applying the same refusal rules.
pub(crate) fn finish_prebuilt(
    document: &Document,
    options: &FormatOptions,
    output: String,
) -> FormatOutcome {
    let unsafe_diagnostics = unsafe_diagnostics(document, options.target_version);
    if !unsafe_diagnostics.is_empty() {
        return FormatOutcome::Refused {
            diagnostics: unsafe_diagnostics.into(),
        };
    }
    finish_format(document.text(), output)
}

/// The formatting pass over a token tape of `source`.
///
/// Formatting is three linear passes with no recursion and no re-lexing of
/// produced text: a reverse pass records each token's next significant and
/// content neighbours, a forward pass computes the canonical flat width and
/// comment/multiline facts of every TOML 1.1 inline table in opening order,
/// and one forward render pass writes the output while a delimiter-frame
/// stack carries the layout mode chosen for each inline table at its opening
/// brace. Time is bounded by the input tokens plus the produced bytes;
/// transient memory is a few bytes per token plus one entry per inline table.
pub(crate) fn build_text(
    source: &str,
    tokens: &lexer::TokenTape,
    options: &FormatOptions,
) -> String {
    let lookahead = TokenLookahead::new(tokens);
    let plan = (options.target_version != TomlVersion::V1_0 && lookahead.has_inline_table)
        .then(|| InlineTablePlan::analyze(source, tokens, &lookahead));
    Renderer::new(source, tokens, options, &lookahead, plan.as_ref()).render()
}

/// Next-neighbour facts for every token, built in one reverse pass so the
/// renderer never rescans the tape forwards.
#[derive(Debug)]
struct TokenLookahead {
    /// The kind of the next token that is not a BOM or horizontal whitespace.
    significant: Vec<Option<SyntaxKind>>,
    /// The kind of the next token that is also not a newline.
    content: Vec<Option<SyntaxKind>>,
    has_inline_table: bool,
}

impl TokenLookahead {
    fn new(tokens: &lexer::TokenTape) -> Self {
        let mut significant = vec![None; tokens.len()];
        let mut content = vec![None; tokens.len()];
        let mut next_significant = None;
        let mut next_content = None;
        let mut has_inline_table = false;
        for (index, token) in tokens.iter().enumerate().rev() {
            significant[index] = next_significant;
            content[index] = next_content;
            if !matches!(token.kind, SyntaxKind::Bom | SyntaxKind::Whitespace) {
                next_significant = Some(token.kind);
            }
            if !matches!(
                token.kind,
                SyntaxKind::Bom | SyntaxKind::Whitespace | SyntaxKind::Newline
            ) {
                next_content = Some(token.kind);
            }
            has_inline_table |= token.kind == SyntaxKind::LeftBrace;
        }
        Self {
            significant,
            content,
            has_inline_table,
        }
    }

    fn significant_after(&self, index: usize) -> Option<SyntaxKind> {
        self.significant[index]
    }

    fn content_after(&self, index: usize) -> Option<SyntaxKind> {
        self.content[index]
    }
}

/// Facts about one TOML 1.1 inline table that decide its layout mode.
#[derive(Clone, Copy, Debug, Default)]
struct InlineTableFacts {
    /// Width of the table rendered on one line with canonical spacing.
    flat_width: usize,
    has_comment: bool,
    has_multiline_string: bool,
    /// Whether a matching closing brace exists; unmatched braces keep lexical layout.
    closed: bool,
}

/// Facts for every inline table in opening order, computed by one forward
/// pass with a stack of open tables so memory is proportional to the number
/// of tables rather than to the token count.
#[derive(Debug)]
struct InlineTablePlan {
    tables: Vec<InlineTableFacts>,
}

impl InlineTablePlan {
    fn analyze(source: &str, tokens: &lexer::TokenTape, lookahead: &TokenLookahead) -> Self {
        let mut tables = Vec::<InlineTableFacts>::new();
        let mut open = Vec::<usize>::new();
        let mut previous_content = None;
        let mut previous_was_layout = false;
        for (index, token) in tokens.iter().enumerate() {
            let raw = &source[token.range.clone()];
            if token.kind == SyntaxKind::LeftBrace {
                open.push(tables.len());
                tables.push(InlineTableFacts::default());
            }
            if let Some(&id) = open.last() {
                let facts = &mut tables[id];
                facts.flat_width = facts.flat_width.saturating_add(flat_token_width(
                    raw,
                    token.kind,
                    previous_content,
                    lookahead.content_after(index),
                    previous_was_layout,
                ));
                match token.kind {
                    SyntaxKind::Comment => facts.has_comment = true,
                    SyntaxKind::BasicString | SyntaxKind::LiteralString
                        if raw.contains(['\n', '\r']) =>
                    {
                        facts.has_multiline_string = true;
                    }
                    _ => {}
                }
            }
            if token.kind == SyntaxKind::RightBrace {
                if let Some(id) = open.pop() {
                    tables[id].closed = true;
                    let child = tables[id];
                    if let Some(&parent) = open.last() {
                        let parent = &mut tables[parent];
                        parent.flat_width = parent.flat_width.saturating_add(child.flat_width);
                        parent.has_comment |= child.has_comment;
                        parent.has_multiline_string |= child.has_multiline_string;
                    }
                }
            }
            if !matches!(
                token.kind,
                SyntaxKind::Bom | SyntaxKind::Whitespace | SyntaxKind::Newline
            ) {
                previous_content = Some(token.kind);
            }
            previous_was_layout =
                matches!(token.kind, SyntaxKind::Whitespace | SyntaxKind::Newline);
        }
        Self { tables }
    }
}

/// The width one token contributes when its inline table is written flat.
fn flat_token_width(
    raw: &str,
    kind: SyntaxKind,
    previous: Option<SyntaxKind>,
    next: Option<SyntaxKind>,
    previous_was_layout: bool,
) -> usize {
    match kind {
        SyntaxKind::Bom => 0,
        SyntaxKind::Whitespace | SyntaxKind::Newline => usize::from(
            !previous_was_layout
                && previous.is_some()
                && next.is_some()
                && !whitespace_is_layout(previous, next),
        ),
        SyntaxKind::Equals => 3,
        SyntaxKind::Comma => {
            1 + usize::from(!matches!(
                next,
                None | Some(
                    SyntaxKind::Comment | SyntaxKind::RightBracket | SyntaxKind::RightBrace
                )
            ))
        }
        SyntaxKind::Dot | SyntaxKind::LeftBracket | SyntaxKind::RightBracket => 1,
        SyntaxKind::LeftBrace => {
            1 + usize::from(!matches!(
                next,
                None | Some(SyntaxKind::Comment | SyntaxKind::RightBrace)
            ))
        }
        SyntaxKind::RightBrace => 1 + usize::from(previous != Some(SyntaxKind::LeftBrace)),
        _ => raw.chars().count(),
    }
}

/// The layout chosen for an inline table when its opening brace is written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TableMode {
    /// TOML 1.0 targets and unmatched braces keep the ordinary lexical rules.
    Lexical,
    /// The table fits on the current line and is written with canonical spacing.
    Flat,
    /// One entry per line, indented one level deeper than the table.
    Expanded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Frame {
    Array,
    Table(TableMode),
}

struct Renderer<'a> {
    source: &'a str,
    tokens: &'a lexer::TokenTape,
    options: &'a FormatOptions,
    lookahead: &'a TokenLookahead,
    plan: Option<&'a InlineTablePlan>,
    newline: &'static str,
    output: String,
    column: usize,
    line_start: bool,
    consecutive_newlines: u8,
    /// Open delimiters, innermost last; its length is the indentation depth.
    frames: Vec<Frame>,
    /// Modes of the open inline tables, innermost last.
    table_modes: Vec<TableMode>,
    /// Source layout after an expanded table's brace, comma, or newline is
    /// replaced by the expanded layout until the next content token.
    skip_layout: bool,
    previous_significant: Option<SyntaxKind>,
    previous_content: Option<SyntaxKind>,
    previous_was_layout: bool,
    /// Index of the next inline table in `plan`, in opening order.
    next_table: usize,
}

impl<'a> Renderer<'a> {
    fn new(
        source: &'a str,
        tokens: &'a lexer::TokenTape,
        options: &'a FormatOptions,
        lookahead: &'a TokenLookahead,
        plan: Option<&'a InlineTablePlan>,
    ) -> Self {
        Self {
            source,
            tokens,
            options,
            lookahead,
            plan,
            newline: selected_newline(source, options.line_ending),
            output: String::with_capacity(source.len()),
            column: 0,
            line_start: true,
            consecutive_newlines: 0,
            frames: Vec::new(),
            table_modes: Vec::new(),
            skip_layout: false,
            previous_significant: None,
            previous_content: None,
            previous_was_layout: false,
            next_table: 0,
        }
    }

    fn render(mut self) -> String {
        for index in 0..self.tokens.len() {
            let kind = self.tokens.kind(index);
            let is_layout = matches!(kind, SyntaxKind::Whitespace | SyntaxKind::Newline);
            if !is_layout {
                // A content token ends any layout run that an expanded brace,
                // comma, or newline asked to skip; the token's own rule may
                // start a new one.
                self.consecutive_newlines = 0;
                self.skip_layout = false;
            }
            match self.table_modes.last() {
                Some(TableMode::Flat) => self.render_flat(index, kind),
                Some(TableMode::Expanded) => self.render_expanded(index, kind),
                Some(TableMode::Lexical) | None => self.render_lexical(index, kind),
            }
            self.previous_was_layout = is_layout;
            if !matches!(kind, SyntaxKind::Bom | SyntaxKind::Whitespace) {
                self.previous_significant = Some(kind);
            }
            if !is_layout && kind != SyntaxKind::Bom {
                self.previous_content = Some(kind);
            }
        }
        if self
            .tokens
            .last()
            .is_none_or(|token| token.kind != SyntaxKind::Comment)
        {
            self.trim_horizontal();
        }
        self.output
    }

    /// Top-level, array, and TOML 1.0 inline-table layout.
    fn render_lexical(&mut self, index: usize, kind: SyntaxKind) {
        match kind {
            SyntaxKind::Bom => self.push_raw(index),
            SyntaxKind::Whitespace => self.write_whitespace(index),
            SyntaxKind::Newline => self.write_newline(index),
            SyntaxKind::Comment => self.write_comment(index),
            SyntaxKind::Equals => self.write_equals(),
            SyntaxKind::Comma => self.write_comma(index),
            SyntaxKind::Dot => self.write_dot(),
            SyntaxKind::LeftBracket => self.open_array(index),
            SyntaxKind::LeftBrace => self.open_table(index),
            SyntaxKind::RightBracket => self.close_array(index),
            SyntaxKind::RightBrace => self.close_table(index),
            _ => {
                self.indent_if_needed();
                self.push_raw(index);
            }
        }
    }

    /// Inside a flat inline table every layout token collapses to canonical
    /// spacing and nothing wraps: the table was measured to fit.
    fn render_flat(&mut self, index: usize, kind: SyntaxKind) {
        match kind {
            SyntaxKind::Bom => {}
            SyntaxKind::Whitespace | SyntaxKind::Newline => {
                let next = self.lookahead.content_after(index);
                if !self.previous_was_layout
                    && self.previous_content.is_some()
                    && next.is_some()
                    && !whitespace_is_layout(self.previous_content, next)
                {
                    self.push(" ");
                }
            }
            SyntaxKind::Equals => self.push(" = "),
            SyntaxKind::Comma => {
                self.trim_horizontal();
                self.push(",");
                if !matches!(
                    self.lookahead.content_after(index),
                    None | Some(
                        SyntaxKind::Comment | SyntaxKind::RightBracket | SyntaxKind::RightBrace
                    )
                ) {
                    self.push(" ");
                }
            }
            SyntaxKind::Dot => self.write_dot(),
            SyntaxKind::LeftBracket => {
                self.frames.push(Frame::Array);
                self.push_raw(index);
            }
            SyntaxKind::LeftBrace => self.open_table(index),
            SyntaxKind::RightBracket => {
                if self.frames.last() == Some(&Frame::Array) {
                    self.frames.pop();
                }
                self.trim_horizontal();
                self.push_raw(index);
            }
            SyntaxKind::RightBrace => self.close_table(index),
            _ => self.push_raw(index),
        }
    }

    /// Inside an expanded inline table: entries at brace level are broken
    /// one per line, blank lines collapse, and nested arrays follow the
    /// lexical rules at their real columns.
    fn render_expanded(&mut self, index: usize, kind: SyntaxKind) {
        let at_brace_level = matches!(self.frames.last(), Some(Frame::Table(_)));
        match kind {
            SyntaxKind::Whitespace | SyntaxKind::Newline if self.skip_layout => {}
            SyntaxKind::Newline => {
                if self.previous_significant != Some(SyntaxKind::Comment) {
                    self.trim_horizontal();
                }
                if !self.output.ends_with('\n') {
                    self.push(self.newline);
                }
                self.line_start = true;
                self.skip_layout = at_brace_level;
            }
            SyntaxKind::Comma if at_brace_level => {
                // A comma-first entry (after a comment line) keeps the
                // table's item indentation instead of landing at column 0.
                self.trim_horizontal();
                self.indent_if_needed();
                self.push(",");
                if self.lookahead.significant_after(index) != Some(SyntaxKind::Comment) {
                    self.push(self.newline);
                    self.line_start = true;
                    self.skip_layout = true;
                }
            }
            _ => self.render_lexical(index, kind),
        }
    }

    fn write_whitespace(&mut self, index: usize) {
        // Whitespace before a comment is layout; write_comment adds the one
        // canonical separating space when the comment follows content.
        if !self.line_start
            && !whitespace_is_layout(
                self.previous_significant,
                self.lookahead.significant_after(index),
            )
        {
            self.push_raw(index);
        }
    }

    fn write_newline(&mut self, index: usize) {
        if self.previous_significant != Some(SyntaxKind::Comment) {
            self.trim_horizontal();
        }
        let newline_limit = if preserves_blank_line(
            self.previous_content,
            self.lookahead.content_after(index),
            self.frames.len(),
        ) {
            2
        } else {
            1
        };
        if self.consecutive_newlines < newline_limit {
            self.push(self.newline);
        }
        self.consecutive_newlines = self.consecutive_newlines.saturating_add(1);
        self.line_start = true;
    }

    fn write_comment(&mut self, index: usize) {
        let starts_line = self.line_start;
        self.indent_if_needed();
        if !starts_line && !self.output.ends_with(['\n', '\r', ' ', '\t']) {
            self.push(" ");
        }
        self.push_raw(index);
    }

    fn write_equals(&mut self) {
        self.indent_if_needed();
        self.trim_horizontal();
        self.push(" = ");
    }

    fn write_dot(&mut self) {
        self.trim_horizontal();
        self.push(".");
    }

    fn write_comma(&mut self, index: usize) {
        self.trim_horizontal();
        self.push(",");
        let next = self.lookahead.significant_after(index);
        let next_stays_on_line = !matches!(
            next,
            None | Some(
                SyntaxKind::Newline
                    | SyntaxKind::Comment
                    | SyntaxKind::RightBracket
                    | SyntaxKind::RightBrace
            )
        );
        let line_width = usize::from(self.options.line_width);
        let wrap_array = self.frames.last() == Some(&Frame::Array)
            && next_stays_on_line
            && (self.column >= line_width
                || (next == Some(SyntaxKind::LeftBrace) && self.next_table_needs_fresh_line()));
        if wrap_array {
            self.push(self.newline);
            self.line_start = true;
        } else if next_stays_on_line {
            self.push(" ");
        }
    }

    /// An array element that is an inline table keeps its flat layout when it
    /// fits on a fresh line, rather than being pushed past the width on the
    /// current line and expanded there.
    fn next_table_needs_fresh_line(&self) -> bool {
        let Some(facts) = self
            .plan
            .and_then(|plan| plan.tables.get(self.next_table))
            .filter(|facts| facts.closed && !facts.has_comment && !facts.has_multiline_string)
        else {
            return false;
        };
        let line_width = usize::from(self.options.line_width);
        let fresh_column = self.frames.len() * usize::from(self.options.indent_width);
        self.column + 1 + facts.flat_width > line_width
            && fresh_column + facts.flat_width <= line_width
    }

    fn open_array(&mut self, index: usize) {
        self.indent_if_needed();
        self.push_raw(index);
        self.frames.push(Frame::Array);
    }

    fn close_array(&mut self, index: usize) {
        if self.frames.last() == Some(&Frame::Array) {
            self.frames.pop();
        }
        self.trim_horizontal();
        self.indent_if_needed();
        self.push_raw(index);
    }

    /// Chooses the table's mode from its measured facts and the real output
    /// column, then writes its opening brace in that mode.
    fn open_table(&mut self, index: usize) {
        let mode = self.choose_table_mode();
        match mode {
            TableMode::Lexical => {
                self.indent_if_needed();
                self.push_raw(index);
                if !matches!(
                    self.lookahead.significant_after(index),
                    None | Some(SyntaxKind::Newline | SyntaxKind::Comment | SyntaxKind::RightBrace)
                ) {
                    self.push(" ");
                }
            }
            TableMode::Flat => {
                self.indent_if_needed();
                self.push_raw(index);
                if !matches!(
                    self.lookahead.content_after(index),
                    None | Some(SyntaxKind::Comment | SyntaxKind::RightBrace)
                ) {
                    self.push(" ");
                }
            }
            TableMode::Expanded => {
                self.indent_if_needed();
                self.push_raw(index);
                self.push(self.newline);
                self.line_start = true;
                self.skip_layout = true;
            }
        }
        self.frames.push(Frame::Table(mode));
        self.table_modes.push(mode);
    }

    fn choose_table_mode(&mut self) -> TableMode {
        let Some(plan) = self.plan else {
            return TableMode::Lexical;
        };
        let facts = plan.tables.get(self.next_table).copied();
        self.next_table += 1;
        let Some(facts) = facts.filter(|facts| facts.closed) else {
            return TableMode::Lexical;
        };
        if self.table_modes.last() == Some(&TableMode::Flat) {
            return TableMode::Flat;
        }
        if facts.has_comment || facts.has_multiline_string {
            return TableMode::Expanded;
        }
        // The column already includes any indentation written for this line.
        let column = if self.line_start {
            self.frames.len() * usize::from(self.options.indent_width)
        } else {
            self.column
        };
        if column.saturating_add(facts.flat_width) <= usize::from(self.options.line_width) {
            TableMode::Flat
        } else {
            TableMode::Expanded
        }
    }

    fn close_table(&mut self, index: usize) {
        let mode = match self.frames.last() {
            Some(Frame::Table(mode)) => {
                let mode = *mode;
                self.frames.pop();
                self.table_modes.pop();
                mode
            }
            _ => TableMode::Lexical,
        };
        match mode {
            TableMode::Lexical => {
                self.trim_horizontal();
                if !self.line_start && !self.output.ends_with('{') {
                    self.push(" ");
                }
                self.indent_if_needed();
                self.push_raw(index);
            }
            TableMode::Flat => {
                self.trim_horizontal();
                if self.previous_content != Some(SyntaxKind::LeftBrace) {
                    self.push(" ");
                }
                self.push_raw(index);
            }
            TableMode::Expanded => {
                self.trim_horizontal();
                if !self.output.ends_with('\n') {
                    self.push(self.newline);
                }
                self.line_start = true;
                self.indent_if_needed();
                self.push_raw(index);
            }
        }
    }

    fn indent_if_needed(&mut self) {
        if self.line_start {
            let columns = self.frames.len() * usize::from(self.options.indent_width);
            self.output.extend(std::iter::repeat_n(' ', columns));
            self.column += columns;
            self.line_start = false;
        }
    }

    fn push_raw(&mut self, index: usize) {
        let kind = self.tokens.kind(index);
        let raw = &self.source[self.tokens.range(index)];
        self.output.push_str(raw);
        match kind {
            SyntaxKind::Newline => self.column = 0,
            // Only string tokens can span lines; every other token advances
            // the column by its own character count.
            SyntaxKind::BasicString | SyntaxKind::LiteralString => {
                self.column = match raw.rsplit_once('\n') {
                    Some((_, tail)) => tail.chars().count(),
                    None => self.column + raw.chars().count(),
                };
            }
            _ => self.column += raw.chars().count(),
        }
    }

    fn push(&mut self, text: &str) {
        self.output.push_str(text);
        self.column = match text.rsplit_once('\n') {
            Some((_, tail)) => tail.chars().count(),
            None => self.column + text.chars().count(),
        };
    }

    fn trim_horizontal(&mut self) {
        let trimmed = self.output.trim_end_matches([' ', '\t']).len();
        let removed = self.output.len() - trimmed;
        self.output.truncate(trimmed);
        self.column = self.column.saturating_sub(removed);
    }
}

fn unsafe_diagnostics(document: &Document, target_version: TomlVersion) -> Vec<Diagnostic> {
    let target_document = (target_version != document.version())
        .then(|| Document::parse_as(document.text(), target_version));
    target_document
        .as_ref()
        .unwrap_or(document)
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.code().refuses_formatting())
        .cloned()
        .collect()
}

fn finish_format(source: &str, output: String) -> FormatOutcome {
    if output == source {
        return FormatOutcome::Unchanged;
    }
    let text: Arc<str> = output.into();
    let edit = TextEdit::new(TextRange::from_usize(0, source.len()), text.clone());
    FormatOutcome::Changed {
        text,
        edits: vec![edit].into(),
    }
}

fn selected_newline(source: &str, option: LineEnding) -> &'static str {
    match option {
        LineEnding::CrLf => "\r\n",
        LineEnding::Preserve if source.contains("\r\n") => "\r\n",
        LineEnding::Lf | LineEnding::Preserve => "\n",
    }
}

fn preserves_blank_line(
    previous: Option<SyntaxKind>,
    next: Option<SyntaxKind>,
    depth: usize,
) -> bool {
    previous == Some(SyntaxKind::Comment)
        || next == Some(SyntaxKind::Comment)
        || (depth == 0 && next == Some(SyntaxKind::LeftBracket))
}

fn whitespace_is_layout(previous: Option<SyntaxKind>, next: Option<SyntaxKind>) -> bool {
    matches!(
        previous,
        Some(
            SyntaxKind::Equals
                | SyntaxKind::Comma
                | SyntaxKind::Dot
                | SyntaxKind::LeftBracket
                | SyntaxKind::LeftBrace
        )
    ) || matches!(
        next,
        Some(
            SyntaxKind::Equals
                | SyntaxKind::Comma
                | SyntaxKind::Dot
                | SyntaxKind::RightBracket
                | SyntaxKind::RightBrace
                | SyntaxKind::Comment
        )
    )
}
