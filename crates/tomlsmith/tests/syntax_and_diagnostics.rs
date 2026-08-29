use tomlsmith::{DiagnosticCode, Document, SyntaxKind};

#[test]
fn syntax_view_is_lossless_and_hides_rowan() {
    let source = "name = \"TomlSmith\"\n# trailing trivia\n";
    let document = Document::parse(source);
    let root = document.root();

    assert_eq!(root.kind(), SyntaxKind::Root);
    assert_eq!(root.text(), source);
    assert!(
        root.children()
            .any(|child| child.kind() == SyntaxKind::KeyValue)
    );
}

#[test]
fn malformed_input_returns_a_document_with_stable_diagnostics() {
    let source = "name = \"unterminated\nnext = 1\n";

    let document = Document::parse(source);

    assert_eq!(document.text(), source);
    assert!(document.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == DiagnosticCode::UNTERMINATED_STRING && diagnostic.range().start() == 7
    }));
}

#[test]
fn table_headers_contain_a_typed_key_node() {
    let document = Document::parse("[[products.tools]]\nname = \"hammer\"\n");
    let table = document
        .root()
        .children()
        .find(|node| node.kind() == SyntaxKind::ArrayTable)
        .expect("array table node");

    assert!(table.children().any(|node| node.kind() == SyntaxKind::Key));
}
