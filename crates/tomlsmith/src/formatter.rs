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

    finish_format(document.text(), build_text(document.text(), options))
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

/// The purely lexical formatting pass: produces the formatted text for
/// `source` without consulting any parse result.
pub(crate) fn build_text(source: &str, options: &FormatOptions) -> String {
    let mut output = build_lexical_text(source, options);
    if !output.contains('{') {
        return output;
    }
    let pass_limit = output.bytes().filter(|byte| *byte == b'{').count();
    for _ in 0..pass_limit {
        let Some(normalized) = normalize_inline_table_layout(&output, options) else {
            break;
        };
        let next = build_lexical_text(&normalized, options);
        if next == output {
            break;
        }
        output = next;
    }
    output
}

fn build_lexical_text(source: &str, options: &FormatOptions) -> String {
    let lexed = lexer::lex(source);
    let newline = selected_newline(source, options.line_ending);
    let mut output = String::with_capacity(source.len());
    let mut line_start = true;
    let mut consecutive_newlines = 0_u8;
    let mut depth = 0_usize;
    let mut delimiters = Vec::new();

    for (index, token) in lexed.tokens.iter().enumerate() {
        let raw = &source[token.range.clone()];
        let previous = significant_before(&lexed.tokens, index);
        let next = significant_after(&lexed.tokens, index);
        if !matches!(token.kind, SyntaxKind::Whitespace | SyntaxKind::Newline) {
            consecutive_newlines = 0;
        }

        match token.kind {
            SyntaxKind::Bom => output.push_str(raw),
            SyntaxKind::Whitespace => {
                // Whitespace before a comment is always layout (see
                // whitespace_is_layout); the Comment arm below writes the
                // single separating space instead.
                if line_start || whitespace_is_layout(previous, next) {
                    continue;
                }
                output.push_str(raw);
            }
            SyntaxKind::Newline => {
                if previous != Some(SyntaxKind::Comment) {
                    trim_horizontal(&mut output);
                }
                let newline_limit = if preserves_blank_line(&lexed.tokens, index, depth) {
                    2
                } else {
                    1
                };
                if consecutive_newlines < newline_limit {
                    output.push_str(newline);
                }
                consecutive_newlines = consecutive_newlines.saturating_add(1);
                line_start = true;
            }
            SyntaxKind::Comment => {
                let starts_line = line_start;
                indent_if_needed(&mut output, &mut line_start, depth, options.indent_width);
                if !starts_line && !output.ends_with(['\n', '\r', ' ', '\t']) {
                    output.push(' ');
                }
                output.push_str(raw);
                line_start = false;
            }
            SyntaxKind::Equals => {
                indent_if_needed(&mut output, &mut line_start, depth, options.indent_width);
                trim_horizontal(&mut output);
                output.push_str(" = ");
            }
            SyntaxKind::Comma => write_comma(
                &mut output,
                &mut line_start,
                &delimiters,
                next,
                newline,
                options.line_width,
            ),
            SyntaxKind::Dot => {
                trim_horizontal(&mut output);
                output.push('.');
            }
            SyntaxKind::LeftBracket | SyntaxKind::LeftBrace => {
                indent_if_needed(&mut output, &mut line_start, depth, options.indent_width);
                output.push_str(raw);
                if token.kind == SyntaxKind::LeftBrace
                    && !matches!(
                        next,
                        None | Some(
                            SyntaxKind::Newline | SyntaxKind::Comment | SyntaxKind::RightBrace
                        )
                    )
                {
                    output.push(' ');
                }
                delimiters.push(token.kind);
                depth += 1;
            }
            SyntaxKind::RightBracket | SyntaxKind::RightBrace => close_delimiter(
                &mut output,
                &mut line_start,
                &mut depth,
                &mut delimiters,
                token.kind,
                raw,
                options.indent_width,
            ),
            _ => {
                indent_if_needed(&mut output, &mut line_start, depth, options.indent_width);
                output.push_str(raw);
            }
        }
    }
    if !matches!(lexed.tokens.last(), Some(token) if token.kind == SyntaxKind::Comment) {
        trim_horizontal(&mut output);
    }

    output
}

fn normalize_inline_table_layout(source: &str, options: &FormatOptions) -> Option<String> {
    if options.target_version == TomlVersion::V1_0 {
        return None;
    }
    let lexed = lexer::lex(source);
    let mut replacements = Vec::new();
    for (opening, closing) in inline_table_pairs(&lexed.tokens) {
        let tokens = &lexed.tokens[opening..=closing];
        let has_newline = tokens.iter().any(|token| token.kind == SyntaxKind::Newline);
        let has_comment = tokens.iter().any(|token| token.kind == SyntaxKind::Comment);
        let has_multiline_string = tokens.iter().any(|token| {
            matches!(
                token.kind,
                SyntaxKind::BasicString | SyntaxKind::LiteralString
            ) && source[token.range.clone()].contains(['\n', '\r'])
        });
        let start = lexed.tokens[opening].range.start;
        let end = lexed.tokens[closing].range.end;
        let base_indent = " ".repeat(
            delimiter_depth_before(&lexed.tokens, opening) * usize::from(options.indent_width),
        );
        if replacements
            .iter()
            .any(|(selected_start, selected_end, _)| start < *selected_end && *selected_start < end)
        {
            continue;
        }
        if has_multiline_string {
            let replacement = expand_inline_table(
                &source[start..end],
                selected_newline(source, options.line_ending),
                &base_indent,
                options.indent_width,
            );
            if replacement != source[start..end] {
                replacements.push((start, end, replacement));
            }
            continue;
        }
        if has_comment {
            if !has_newline {
                continue;
            }
            let replacement = expand_inline_table(
                &source[start..end],
                selected_newline(source, options.line_ending),
                &base_indent,
                options.indent_width,
            );
            if replacement != source[start..end] {
                replacements.push((start, end, replacement));
            }
            continue;
        }
        if !has_newline && current_line_width(&source[..end]) <= usize::from(options.line_width) {
            continue;
        }

        let mut flat_source = String::with_capacity(end - start);
        for token in tokens {
            if matches!(token.kind, SyntaxKind::Whitespace | SyntaxKind::Newline) {
                if !flat_source.ends_with(' ') {
                    flat_source.push(' ');
                }
            } else {
                flat_source.push_str(&source[token.range.clone()]);
            }
        }
        let mut flat_options = options.clone();
        flat_options.line_width = u16::MAX;
        let candidate = build_lexical_text(&flat_source, &flat_options);
        let projected_width = current_line_width(&source[..start]) + candidate.chars().count();
        if !candidate.contains(['\n', '\r']) {
            let replacement = if projected_width <= usize::from(options.line_width) {
                candidate
            } else {
                expand_inline_table(
                    &source[start..end],
                    selected_newline(source, options.line_ending),
                    &base_indent,
                    options.indent_width,
                )
            };
            if replacement != source[start..end] {
                replacements.push((start, end, replacement));
            }
        }
    }

    if replacements.is_empty() {
        return None;
    }
    replacements.sort_by_key(|(start, _, _)| *start);
    let mut output = source.to_owned();
    for (start, end, replacement) in replacements.into_iter().rev() {
        output.replace_range(start..end, &replacement);
    }
    Some(output)
}

fn inline_table_pairs(tokens: &[lexer::Token]) -> Vec<(usize, usize)> {
    let mut openings = Vec::new();
    let mut pairs = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            SyntaxKind::LeftBrace => openings.push(index),
            SyntaxKind::RightBrace => {
                if let Some(opening) = openings.pop() {
                    pairs.push((opening, index));
                }
            }
            _ => {}
        }
    }
    pairs.sort_by(|left, right| {
        left.1
            .saturating_sub(left.0)
            .cmp(&right.1.saturating_sub(right.0))
            .then_with(|| left.0.cmp(&right.0))
    });
    pairs
}

fn expand_inline_table(source: &str, newline: &str, base_indent: &str, indent_width: u8) -> String {
    let lexed = lexer::lex(source);
    let mut output = String::with_capacity(source.len());
    let mut item_indent = base_indent.to_owned();
    item_indent.extend(std::iter::repeat_n(' ', usize::from(indent_width)));
    let mut brace_depth = 0_usize;
    let mut bracket_depth = 0_usize;
    let mut skip_outer_layout = false;
    for (index, token) in lexed.tokens.iter().enumerate() {
        let raw = &source[token.range.clone()];
        if skip_outer_layout && matches!(token.kind, SyntaxKind::Whitespace | SyntaxKind::Newline) {
            continue;
        }
        if !matches!(token.kind, SyntaxKind::Whitespace | SyntaxKind::Newline) {
            skip_outer_layout = false;
        }
        match token.kind {
            SyntaxKind::LeftBrace => {
                brace_depth += 1;
                output.push_str(raw);
                if brace_depth == 1 {
                    output.push_str(newline);
                    output.push_str(&item_indent);
                    skip_outer_layout = true;
                }
            }
            SyntaxKind::RightBrace => {
                if brace_depth == 1 {
                    trim_horizontal(&mut output);
                    if !output.ends_with(['\n', '\r']) {
                        output.push_str(newline);
                    }
                    output.push_str(base_indent);
                }
                output.push_str(raw);
                brace_depth = brace_depth.saturating_sub(1);
            }
            SyntaxKind::LeftBracket => {
                bracket_depth += 1;
                output.push_str(raw);
            }
            SyntaxKind::RightBracket => {
                bracket_depth = bracket_depth.saturating_sub(1);
                output.push_str(raw);
            }
            SyntaxKind::Comma if brace_depth == 1 && bracket_depth == 0 => {
                trim_horizontal(&mut output);
                output.push(',');
                if significant_after(&lexed.tokens, index) != Some(SyntaxKind::Comment) {
                    output.push_str(newline);
                    output.push_str(&item_indent);
                    skip_outer_layout = true;
                }
            }
            SyntaxKind::Newline => {
                trim_horizontal(&mut output);
                if !output.ends_with('\n') {
                    output.push_str(newline);
                }
                if brace_depth == 1
                    && bracket_depth == 0
                    && content_after(&lexed.tokens, index) != Some(SyntaxKind::RightBrace)
                {
                    output.push_str(&item_indent);
                    skip_outer_layout = true;
                }
            }
            _ => output.push_str(raw),
        }
    }
    output
}

fn delimiter_depth_before(tokens: &[lexer::Token], index: usize) -> usize {
    tokens[..index]
        .iter()
        .fold(0_usize, |depth, token| match token.kind {
            SyntaxKind::LeftBracket | SyntaxKind::LeftBrace => depth + 1,
            SyntaxKind::RightBracket | SyntaxKind::RightBrace => depth.saturating_sub(1),
            _ => depth,
        })
}

fn unsafe_diagnostics(document: &Document, target_version: TomlVersion) -> Vec<Diagnostic> {
    let target_document = (target_version != document.version())
        .then(|| Document::parse_as(document.text(), target_version));
    target_document
        .as_ref()
        .unwrap_or(document)
        .diagnostics()
        .iter()
        .filter(|diagnostic| {
            let code = diagnostic.code().as_str();
            code.starts_with("parse.")
                || code.starts_with("semantic.")
                || code.starts_with("version.")
        })
        .cloned()
        .collect()
}

fn write_comma(
    output: &mut String,
    line_start: &mut bool,
    delimiters: &[SyntaxKind],
    next: Option<SyntaxKind>,
    newline: &str,
    line_width: u16,
) {
    trim_horizontal(output);
    output.push(',');
    let next_stays_on_line = !matches!(
        next,
        None | Some(
            SyntaxKind::Newline
                | SyntaxKind::Comment
                | SyntaxKind::RightBracket
                | SyntaxKind::RightBrace
        )
    );
    let wrap_array = delimiters.last() == Some(&SyntaxKind::LeftBracket)
        && next_stays_on_line
        && current_line_width(output) >= usize::from(line_width);
    if wrap_array {
        output.push_str(newline);
        *line_start = true;
    } else if next_stays_on_line {
        output.push(' ');
    }
}

fn close_delimiter(
    output: &mut String,
    line_start: &mut bool,
    depth: &mut usize,
    delimiters: &mut Vec<SyntaxKind>,
    closing: SyntaxKind,
    raw: &str,
    indent_width: u8,
) {
    *depth = depth.saturating_sub(1);
    let expected = match closing {
        SyntaxKind::RightBracket => SyntaxKind::LeftBracket,
        SyntaxKind::RightBrace => SyntaxKind::LeftBrace,
        _ => unreachable!(),
    };
    if delimiters.last() == Some(&expected) {
        delimiters.pop();
    }
    trim_horizontal(output);
    if closing == SyntaxKind::RightBrace && !*line_start && !output.ends_with('{') {
        output.push(' ');
    }
    indent_if_needed(output, line_start, *depth, indent_width);
    output.push_str(raw);
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

fn significant_before(tokens: &[lexer::Token], index: usize) -> Option<SyntaxKind> {
    tokens[..index]
        .iter()
        .rev()
        .find(|token| !matches!(token.kind, SyntaxKind::Bom | SyntaxKind::Whitespace))
        .map(|token| token.kind)
}

fn significant_after(tokens: &[lexer::Token], index: usize) -> Option<SyntaxKind> {
    tokens[index + 1..]
        .iter()
        .find(|token| !matches!(token.kind, SyntaxKind::Bom | SyntaxKind::Whitespace))
        .map(|token| token.kind)
}

fn preserves_blank_line(tokens: &[lexer::Token], index: usize, depth: usize) -> bool {
    let previous = content_before(tokens, index);
    let next = content_after(tokens, index);
    previous == Some(SyntaxKind::Comment)
        || next == Some(SyntaxKind::Comment)
        || (depth == 0 && next == Some(SyntaxKind::LeftBracket))
}

fn content_before(tokens: &[lexer::Token], index: usize) -> Option<SyntaxKind> {
    tokens[..index]
        .iter()
        .rev()
        .find(|token| {
            !matches!(
                token.kind,
                SyntaxKind::Bom | SyntaxKind::Whitespace | SyntaxKind::Newline
            )
        })
        .map(|token| token.kind)
}

fn content_after(tokens: &[lexer::Token], index: usize) -> Option<SyntaxKind> {
    tokens[index + 1..]
        .iter()
        .find(|token| {
            !matches!(
                token.kind,
                SyntaxKind::Bom | SyntaxKind::Whitespace | SyntaxKind::Newline
            )
        })
        .map(|token| token.kind)
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

fn indent_if_needed(output: &mut String, line_start: &mut bool, depth: usize, width: u8) {
    if *line_start {
        output.extend(std::iter::repeat_n(' ', depth * usize::from(width)));
        *line_start = false;
    }
}

fn trim_horizontal(output: &mut String) {
    output.truncate(output.trim_end_matches([' ', '\t']).len());
}

fn current_line_width(output: &str) -> usize {
    output
        .rsplit_once('\n')
        .map_or(output, |(_, line)| line)
        .chars()
        .count()
}
