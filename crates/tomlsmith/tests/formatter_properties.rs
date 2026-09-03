//! Grammar-aware property tests for the formatter.
//!
//! `properties.rs` draws arbitrary strings, which the guarded formatter almost
//! always refuses, so it exercises refusal rather than layout. This suite
//! generates valid TOML 1.1 documents with nested arrays and inline tables,
//! comments in every position the grammar allows, multi-line strings,
//! date-times, unicode, and irregular source layout, then checks the layout
//! contracts at several widths: formatting is idempotent, the output reparses
//! without errors, and the decoded semantic root is unchanged.

use std::fmt::Write;

use proptest::prelude::*;
use tomlsmith::{Document, FormatOptions, FormatOutcome, Severity, TomlVersion};

mod support;

const SCALARS: &[&str] = &[
    "1",
    "22222",
    "-7",
    "true",
    "false",
    "3.14",
    "\"str\"",
    "\"long string value here\"",
    "'lit'",
    "\"a\\nb\"",
    "\"日本語\"",
    "\"🎉\"",
    "\"{\"",
    "\"}\"",
    "1979-05-27T07:32:00Z",
    "1979-05-27 07:32:00",
    "07:32:00",
    "\"\"\"\nml\n\"\"\"",
    "'''\nml'''",
    "\"\"\"one line\"\"\"",
];

/// Horizontal layout that is legal between any two tokens.
const SPACING: &[&str] = &["", " ", "  ", "\t"];
/// Layout that is legal inside TOML 1.1 arrays and inline tables.
const CONTAINER_LAYOUT: &[&str] = &["", " ", "\n", "\n  ", " \n\t", "\n\n"];

fn spacing() -> impl Strategy<Value = &'static str> {
    prop::sample::select(SPACING)
}

fn container_layout() -> impl Strategy<Value = &'static str> {
    prop::sample::select(CONTAINER_LAYOUT)
}

fn key(index: usize) -> impl Strategy<Value = String> {
    prop_oneof![
        Just(format!("k{index}")),
        Just(format!("\"quoted key {index}\"")),
        Just(format!("'literal{index}'")),
        Just(format!("d{index}.x")),
    ]
}

/// A comment that may follow a comma inside a container; it always ends the line.
fn container_comment() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => Just(String::new()),
        1 => Just(" # note\n".to_owned()),
        1 => Just(" # trailing spaces   \n".to_owned()),
    ]
}

fn value(depth: u32) -> impl Strategy<Value = String> {
    let scalar = prop::sample::select(SCALARS).prop_map(str::to_owned);
    scalar.prop_recursive(depth, 24, 4, |inner| {
        prop_oneof![
            (
                container_layout(),
                prop::collection::vec((inner.clone(), spacing(), container_comment()), 0..4),
                container_layout(),
                prop::bool::ANY,
            )
                .prop_map(|(open, items, close, trailing_comma)| {
                    let mut text = String::from("[");
                    text.push_str(open);
                    for (index, (item, space, comment)) in items.iter().enumerate() {
                        if index > 0 {
                            text.push(',');
                            text.push_str(comment);
                            text.push_str(space);
                        }
                        text.push_str(item);
                    }
                    if trailing_comma && !items.is_empty() {
                        text.push(',');
                    }
                    text.push_str(close);
                    text.push(']');
                    text
                }),
            (
                container_layout(),
                prop::collection::vec((inner, spacing(), spacing(), container_comment()), 0..4),
                container_layout(),
                prop::bool::ANY,
            )
                .prop_map(|(open, entries, close, trailing_comma)| {
                    let mut text = String::from("{");
                    text.push_str(open);
                    for (index, (item, before, after, comment)) in entries.iter().enumerate() {
                        if index > 0 {
                            text.push(',');
                            text.push_str(comment);
                            text.push_str(after);
                        }
                        let _ = write!(text, "e{index}{before}={after}{item}");
                    }
                    if trailing_comma && !entries.is_empty() {
                        text.push(',');
                    }
                    text.push_str(close);
                    text.push('}');
                    text
                }),
        ]
    })
}

#[derive(Clone, Debug)]
enum Line {
    KeyValue {
        key: String,
        before: &'static str,
        after: &'static str,
        value: String,
        comment: bool,
    },
    Table(usize),
    ArrayTable(usize),
    Comment(&'static str),
    Blank(u8),
}

fn line(index: usize) -> impl Strategy<Value = Line> {
    prop_oneof![
        6 => (key(index), spacing(), spacing(), value(3), prop::bool::ANY).prop_map(
            |(key, before, after, value, comment)| Line::KeyValue {
                key,
                before,
                after,
                value,
                comment,
            }
        ),
        1 => Just(Line::Table(index)),
        1 => Just(Line::ArrayTable(index)),
        1 => prop::sample::select(&["# comment", "#", "  # indented comment   "][..])
            .prop_map(Line::Comment),
        1 => (1..4_u8).prop_map(Line::Blank),
    ]
}

fn document() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::bool::ANY, 0..12).prop_flat_map(|slots| {
        let lines: Vec<_> = (0..slots.len()).map(line).collect();
        (lines, prop::bool::ANY).prop_map(|(lines, trailing_newline)| {
            let mut text = String::new();
            for entry in lines {
                match entry {
                    Line::KeyValue {
                        key,
                        before,
                        after,
                        value,
                        comment,
                    } => {
                        let _ = write!(text, "{key}{before}={after}{value}");
                        if comment {
                            text.push_str(" # tail");
                        }
                        text.push('\n');
                    }
                    Line::Table(index) => {
                        let _ = writeln!(text, "[table{index}]");
                    }
                    Line::ArrayTable(index) => {
                        let _ = writeln!(text, "[[items{index}]]");
                    }
                    Line::Comment(comment) => {
                        text.push_str(comment);
                        text.push('\n');
                    }
                    Line::Blank(count) => text.push_str(&"\n".repeat(usize::from(count))),
                }
            }
            if !trailing_newline {
                while text.ends_with('\n') {
                    text.pop();
                }
            }
            text
        })
    })
}

fn has_errors(document: &Document) -> bool {
    document
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.severity() == Severity::Error)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generated_documents_format_to_stable_semantics_preserving_layout(source in document()) {
        let document = Document::parse_as(source.as_str(), TomlVersion::V1_1);
        prop_assume!(!has_errors(&document));
        for line_width in [8_u16, 20, 100] {
            let options = FormatOptions {
                line_width,
                ..FormatOptions::default()
            };
            let formatted = match document.format_with(&options) {
                FormatOutcome::Unchanged => continue,
                FormatOutcome::Changed { text, .. } => text,
                FormatOutcome::Refused { diagnostics } => {
                    prop_assert!(false, "a clean document was refused: {diagnostics:?}");
                    unreachable!()
                }
            };
            let reparsed = Document::parse_as(formatted.as_ref(), TomlVersion::V1_1);
            prop_assert!(
                !has_errors(&reparsed),
                "width {line_width}: formatted output has errors {:?}\n--- source\n{source}\n--- output\n{formatted}",
                reparsed.diagnostics(),
            );
            prop_assert!(
                matches!(reparsed.format_with(&options), FormatOutcome::Unchanged),
                "width {line_width}: formatting is not idempotent\n--- source\n{source}\n--- output\n{formatted}",
            );
            prop_assert!(
                support::semantic_roots_equal(reparsed.semantics().root(), document.semantics().root()),
                "width {line_width}: formatting changed the decoded root\n--- source\n{source}\n--- output\n{formatted}",
            );
        }
    }
}
