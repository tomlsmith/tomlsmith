use std::sync::Arc;

use crate::{Diagnostic, Document, SyntaxKind, TextRange, TomlVersion, syntax::lexer};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum LineEnding {
    #[default]
    Preserve,
    Lf,
    CrLf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormatOptions {
    pub target_version: TomlVersion,
    pub indent_width: u8,
    pub line_width: u16,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextEdit {
    range: TextRange,
    replacement: Arc<str>,
}

impl TextEdit {
    #[must_use]
    pub fn new(range: TextRange, replacement: impl Into<Arc<str>>) -> Self {
        Self {
            range,
            replacement: replacement.into(),
        }
    }

    #[must_use]
    pub const fn range(&self) -> TextRange {
        self.range
    }

    #[must_use]
    pub fn replacement(&self) -> &str {
        &self.replacement
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FormatOutcome {
    Unchanged,
    Changed {
        text: Arc<str>,
        edits: Arc<[TextEdit]>,
    },
    Refused {
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

    let source = document.text();
    let lexed = lexer::lex(source);
    let newline = selected_newline(source, options.line_ending);
    let mut output = String::with_capacity(source.len());
    let mut line_start = true;
    let mut depth = 0_usize;
    let mut delimiters = Vec::new();

    for (index, token) in lexed.tokens.iter().enumerate() {
        let raw = &source[token.range.clone()];
        let previous = significant_before(&lexed.tokens, index);
        let next = significant_after(&lexed.tokens, index);

        match token.kind {
            SyntaxKind::Bom => output.push_str(raw),
            SyntaxKind::Whitespace => {
                if line_start || whitespace_is_layout(previous, next) {
                    continue;
                }
                if next == Some(SyntaxKind::Comment) {
                    trim_horizontal(&mut output);
                    output.push(' ');
                } else {
                    output.push_str(raw);
                }
            }
            SyntaxKind::Newline => {
                if previous != Some(SyntaxKind::Comment) {
                    trim_horizontal(&mut output);
                }
                output.push_str(newline);
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

    finish_format(source, output)
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
