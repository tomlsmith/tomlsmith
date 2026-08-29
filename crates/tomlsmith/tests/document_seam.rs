use static_assertions::assert_impl_all;
use tomlsmith::{Document, TomlVersion};

#[test]
fn parsing_is_lossless_and_defaults_to_toml_1_1() {
    let source = "title = \"TomlSmith\" # keep me\n";

    let document = Document::parse(source);

    assert_eq!(document.text(), source);
    assert_eq!(document.version(), TomlVersion::V1_1);
    assert!(document.diagnostics().is_empty());
}

#[test]
fn document_snapshots_are_shareable() {
    assert_impl_all!(Document: Clone, Send, Sync);
}
