use tomlsmith::{Document, Resolution, SemanticValue, SyntaxKind, SyntaxNode};

#[test]
fn multiline_arrays_and_inline_tables_have_structured_cst_and_semantics() {
    let source = "values = [\n  1,\n  # retained\n  2,\n]\nmeta = { a = 1, b = \"two\", }\n";
    let document = Document::parse(source);

    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics()
    );
    assert!(contains_kind(&document.root(), SyntaxKind::Array));
    assert!(contains_kind(&document.root(), SyntaxKind::InlineTable));

    let Resolution::Unique(values) = document.semantics().resolve(["values"]) else {
        panic!("values should resolve uniquely");
    };
    let Some(array) = values.value().and_then(SemanticValue::as_array) else {
        panic!("values should be an array");
    };
    assert_eq!(array.len(), 2);
    assert_eq!(array[0].as_integer(), Some(1));
    assert_eq!(array[1].as_integer(), Some(2));

    let Resolution::Unique(meta) = document.semantics().resolve(["meta"]) else {
        panic!("meta should resolve uniquely");
    };
    assert_eq!(
        meta.value()
            .and_then(SemanticValue::as_inline_table)
            .map(<[_]>::len),
        Some(2)
    );
}

fn contains_kind(node: &SyntaxNode, expected: SyntaxKind) -> bool {
    node.kind() == expected || node.children().any(|child| contains_kind(&child, expected))
}
