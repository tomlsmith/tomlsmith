use tomlsmith::{DiagnosticCode, Document};

#[test]
fn invalid_keys_values_and_empty_array_elements_are_diagnosed() {
    let document =
        Document::parse("= 1\nbare key = 2\nunknown = not-a-toml-value\nitems = [1,, 2]\n");
    let codes = document
        .diagnostics()
        .iter()
        .map(tomlsmith::Diagnostic::code)
        .collect::<Vec<_>>();

    assert!(codes.contains(&DiagnosticCode::MISSING_KEY));
    assert!(codes.contains(&DiagnosticCode::INVALID_BARE_KEY));
    assert!(codes.contains(&DiagnosticCode::INVALID_VALUE));
    assert!(codes.contains(&DiagnosticCode::MISSING_VALUE));
}

#[test]
fn leading_zero_decimal_integers_are_not_accepted() {
    let document = Document::parse("valid = 0\ninvalid = 01\n");

    assert!(
        document
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == DiagnosticCode::INVALID_VALUE })
    );
}
