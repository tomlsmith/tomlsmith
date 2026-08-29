use tomlsmith::{DiagnosticCode, Document, TomlVersion};

#[test]
fn toml_1_1_accepts_new_escapes_optional_seconds_and_multiline_inline_tables() {
    let source = "escape = \"\\e\\x41\"\ntime = 07:32\ninline = {\n  answer = 42,\n}\n";

    let document = Document::parse_as(source, TomlVersion::V1_1);

    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics()
    );
}

#[test]
fn strict_toml_1_0_reports_toml_1_1_only_syntax() {
    let source = "escape = \"\\e\\x41\"\ntime = 07:32\ninline = {\n  answer = 42,\n}\n";

    let document = Document::parse_as(source, TomlVersion::V1_0);

    assert!(
        document
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == DiagnosticCode::TOML_1_1_SYNTAX })
    );
}

#[test]
fn toml_1_0_reports_each_new_escape_once_with_the_right_reason() {
    let document = Document::parse_as("escape = \"\\e\\x41\"\n", TomlVersion::V1_0);
    let version_diagnostics = document
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.code() == DiagnosticCode::TOML_1_1_SYNTAX)
        .collect::<Vec<_>>();

    assert_eq!(version_diagnostics.len(), 2);
    assert!(
        version_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message().contains("escape"))
    );
}

#[test]
fn invalid_basic_string_escapes_are_parse_errors_in_every_version() {
    let document = Document::parse("value = \"bad \\q escape\"\n");

    assert!(
        document
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == DiagnosticCode::INVALID_ESCAPE })
    );
}
