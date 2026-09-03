//! Formatter and snapshot invariants over the pinned upstream `toml-test` corpus.
//!
//! The corpus is not vendored: `tools/toml-test/run.sh` downloads the pinned
//! `toml-lang/toml-test` module and exports its `tests` directory through
//! `TOMLSMITH_TOML_TEST_CORPUS` before running this suite. Without that variable
//! the tests report themselves as skipped, so the ordinary `cargo test` gate
//! stays hermetic while the conformance job proves the whole-corpus invariants.

use std::{
    fs,
    path::{Path, PathBuf},
};

use tomlsmith::{Document, FormatOptions, FormatOutcome, Severity, TomlVersion};

mod support;

fn corpus_root() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("TOMLSMITH_TOML_TEST_CORPUS")?);
    assert!(
        root.join("valid").is_dir() && root.join("invalid").is_dir(),
        "TOMLSMITH_TOML_TEST_CORPUS must point at the toml-test `tests` directory, got {}",
        root.display()
    );
    Some(root)
}

fn toml_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).expect("readable corpus directory") {
            let path = entry.expect("readable corpus entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "toml")
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn has_errors(document: &Document) -> bool {
    document
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.severity() == Severity::Error)
}

/// Every corpus file, valid or invalid, in both language versions: formatting
/// terminates, is refused exactly when the snapshot has errors, and otherwise
/// produces a clean, idempotent result that decodes to the same semantic root.
#[test]
fn formatting_the_conformance_corpus_is_guarded_idempotent_and_semantics_preserving() {
    let Some(root) = corpus_root() else {
        eprintln!("skipping: TOMLSMITH_TOML_TEST_CORPUS is not set");
        return;
    };
    let mut checked = 0_usize;
    let mut failures = Vec::new();

    for path in toml_files(&root.join("valid"))
        .into_iter()
        .chain(toml_files(&root.join("invalid")))
    {
        let Ok(source) = fs::read_to_string(&path) else {
            // Encoding fixtures intentionally contain invalid UTF-8; the CLI
            // rejects those before a Document exists.
            continue;
        };
        for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
            checked += 1;
            let name = format!("{} [{version:?}]", path.display());
            let document = Document::parse_as(source.as_str(), version);
            let options = FormatOptions {
                target_version: version,
                ..FormatOptions::default()
            };
            let outcome = document.format_with(&options);
            if has_errors(&document) {
                if !matches!(outcome, FormatOutcome::Refused { .. }) {
                    failures.push(format!("{name}: a snapshot with errors was not refused"));
                }
                continue;
            }
            let formatted = match outcome {
                FormatOutcome::Refused { diagnostics } => {
                    failures.push(format!(
                        "{name}: a clean snapshot was refused: {diagnostics:?}"
                    ));
                    continue;
                }
                FormatOutcome::Unchanged => continue,
                FormatOutcome::Changed { text, .. } => text,
            };
            let reparsed = Document::parse_as(formatted.as_ref(), version);
            if has_errors(&reparsed) {
                failures.push(format!(
                    "{name}: formatted output has errors: {:?}",
                    reparsed.diagnostics()
                ));
                continue;
            }
            if !matches!(reparsed.format_with(&options), FormatOutcome::Unchanged) {
                failures.push(format!("{name}: formatting is not idempotent"));
            }
            if !support::semantic_roots_equal(
                reparsed.semantics().root(),
                document.semantics().root(),
            ) {
                failures.push(format!(
                    "{name}: formatting changed the decoded semantic root"
                ));
            }
        }
    }

    assert!(
        checked > 0,
        "the corpus at {} contained no TOML files",
        root.display()
    );
    assert!(
        failures.is_empty(),
        "{} corpus formatting failure(s) out of {checked} checks:\n{}",
        failures.len(),
        failures.join("\n")
    );
    eprintln!("checked {checked} corpus snapshots");
}
