//! Data-driven formatter snapshots.
//!
//! Every `formatter_snapshots/<case>.toml` is formatted with the options in the
//! optional `formatter_snapshots/<case>.options` file and compared byte-for-byte
//! with `formatter_snapshots/<case>.expected.toml`. The expected text must also
//! be a fixed point of the formatter, reparse without errors under the target
//! version, and decode to the same semantic root as the input. Regenerate the
//! expected files deliberately with `TOMLSMITH_UPDATE_SNAPSHOTS=1` and review the
//! diff: any change here is a user-visible formatter behavior change.

use std::{
    fs,
    path::{Path, PathBuf},
};

use tomlsmith::{Document, FormatOptions, FormatOutcome, LineEnding, Severity, TomlVersion};

mod support;

#[derive(Debug)]
struct Case {
    name: String,
    input_path: PathBuf,
    expected_path: PathBuf,
    options: FormatOptions,
    expect_refused: bool,
}

fn snapshot_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("formatter_snapshots")
}

fn parse_options(name: &str, text: &str) -> (FormatOptions, bool) {
    let mut options = FormatOptions::default();
    let mut expect_refused = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            panic!("{name}.options: expected `key = value`, found {line:?}");
        };
        let value = value.trim();
        match key.trim() {
            "target_version" => {
                options.target_version = match value {
                    "1.0" => TomlVersion::V1_0,
                    "1.1" => TomlVersion::V1_1,
                    other => panic!("{name}.options: unknown target_version {other:?}"),
                }
            }
            "indent_width" => {
                options.indent_width = value
                    .parse()
                    .unwrap_or_else(|error| panic!("{name}.options: indent_width: {error}"));
            }
            "line_width" => {
                options.line_width = value
                    .parse()
                    .unwrap_or_else(|error| panic!("{name}.options: line_width: {error}"));
            }
            "line_ending" => {
                options.line_ending = match value {
                    "preserve" => LineEnding::Preserve,
                    "lf" => LineEnding::Lf,
                    "crlf" => LineEnding::CrLf,
                    other => panic!("{name}.options: unknown line_ending {other:?}"),
                }
            }
            "expect_refused" => {
                expect_refused = match value {
                    "true" => true,
                    "false" => false,
                    other => {
                        panic!("{name}.options: expect_refused must be true/false, found {other:?}")
                    }
                }
            }
            other => panic!("{name}.options: unknown option {other:?}"),
        }
    }
    (options, expect_refused)
}

fn cases() -> Vec<Case> {
    let directory = snapshot_directory();
    let mut cases = Vec::new();
    for entry in fs::read_dir(&directory).expect("snapshot directory must exist") {
        let path = entry.expect("readable snapshot entry").path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(name) = file_name.strip_suffix(".toml") else {
            continue;
        };
        if name.ends_with(".expected") {
            continue;
        }
        let name = name.to_owned();
        let options_path = directory.join(format!("{name}.options"));
        let (options, expect_refused) = match fs::read_to_string(&options_path) {
            Ok(text) => parse_options(&name, &text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (FormatOptions::default(), false)
            }
            Err(error) => panic!("failed to read {}: {error}", options_path.display()),
        };
        cases.push(Case {
            expected_path: directory.join(format!("{name}.expected.toml")),
            input_path: path,
            name,
            options,
            expect_refused,
        });
    }
    cases.sort_by(|left, right| left.name.cmp(&right.name));
    assert!(
        !cases.is_empty(),
        "no snapshot cases found in {}",
        directory.display()
    );
    cases
}

fn has_errors(document: &Document) -> bool {
    document
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.severity() == Severity::Error)
}

#[test]
fn formatter_snapshots_are_stable_fixed_points() {
    let update = std::env::var_os("TOMLSMITH_UPDATE_SNAPSHOTS").is_some();
    let mut failures = Vec::new();

    for case in cases() {
        let name = &case.name;
        let input = fs::read_to_string(&case.input_path)
            .unwrap_or_else(|error| panic!("{name}: failed to read input: {error}"));
        // The snapshot's TOML version follows the target version so version
        // boundaries are exercised through the formatter's refusal path.
        let document = Document::parse_as(input.as_str(), case.options.target_version);
        let outcome = document.format_with(&case.options);

        if case.expect_refused {
            if !matches!(outcome, FormatOutcome::Refused { .. }) {
                failures.push(format!(
                    "{name}: expected the formatter to refuse, got {outcome:?}"
                ));
            }
            continue;
        }

        let formatted: String = match outcome {
            FormatOutcome::Unchanged => input.clone(),
            FormatOutcome::Changed { text, .. } => text.to_string(),
            FormatOutcome::Refused { diagnostics } => {
                failures.push(format!(
                    "{name}: formatter refused a snapshot input: {diagnostics:?}"
                ));
                continue;
            }
        };

        if update {
            fs::write(&case.expected_path, formatted.as_bytes())
                .unwrap_or_else(|error| panic!("{name}: failed to write snapshot: {error}"));
        }
        match fs::read(&case.expected_path) {
            Ok(expected) if expected == formatted.as_bytes() => {}
            Ok(expected) => failures.push(format!(
                "{name}: formatted output differs from {}\n--- expected\n{}\n--- actual\n{}",
                case.expected_path.display(),
                String::from_utf8_lossy(&expected),
                formatted,
            )),
            Err(error) => failures.push(format!(
                "{name}: missing snapshot {} ({error}); run with TOMLSMITH_UPDATE_SNAPSHOTS=1",
                case.expected_path.display()
            )),
        }

        let reparsed = Document::parse_as(formatted.as_str(), case.options.target_version);
        if has_errors(&reparsed) {
            failures.push(format!(
                "{name}: formatted output does not parse cleanly: {:?}",
                reparsed.diagnostics()
            ));
        }
        if !matches!(
            reparsed.format_with(&case.options),
            FormatOutcome::Unchanged
        ) {
            failures.push(format!("{name}: formatting is not idempotent"));
        }
        if !support::semantic_roots_equal(reparsed.semantics().root(), document.semantics().root())
        {
            failures.push(format!(
                "{name}: formatting changed the decoded semantic root"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} formatter snapshot failure(s):\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
