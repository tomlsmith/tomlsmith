use tomlsmith::{ChangeError, Document, Revision, TextChange, TextRange};

#[test]
fn changes_create_a_new_immutable_revision() {
    let original = Document::parse("emoji = \"😀\"\n");
    let changed = original
        .with_changes([TextChange::edit(TextRange::new(9, 13), "TomlSmith")])
        .expect("valid UTF-8-boundary edit");

    assert_eq!(original.text(), "emoji = \"😀\"\n");
    assert_eq!(original.revision(), Revision::INITIAL);
    assert_eq!(changed.text(), "emoji = \"TomlSmith\"\n");
    assert_eq!(changed.revision(), Revision::new(1));
}

#[test]
fn edits_reject_ranges_inside_utf_8_code_points() {
    let document = Document::parse("key = \"😀\"\n");
    let error = document
        .with_changes([TextChange::edit(TextRange::new(8, 9), "x")])
        .expect_err("the range starts inside the emoji");

    assert_eq!(error, ChangeError::InvalidUtf8Boundary { offset: 8 });
}
