use tomlsmith::{Document, HighlightKind};

#[test]
fn distinguishes_structural_and_value_shaped_keys() {
    let source = r#"[workspace]
name = "TomlSmith"
members = ["crates/tomlsmith"]
metadata = { enabled = true }

[[bin]]
path = "src/main.rs"
"#;
    let document = Document::parse(source);
    let source_backed = document
        .highlights()
        .iter()
        .map(|highlight| {
            (
                highlight.kind(),
                &source[highlight.range().start() as usize..highlight.range().end() as usize],
            )
        })
        .collect::<Vec<_>>();

    assert!(source_backed.contains(&(HighlightKind::Table, "workspace")));
    assert!(source_backed.contains(&(HighlightKind::Key, "name")));
    assert!(source_backed.contains(&(HighlightKind::ArrayKey, "members")));
    assert!(source_backed.contains(&(HighlightKind::InlineTableKey, "metadata")));
    assert!(source_backed.contains(&(HighlightKind::Key, "enabled")));
    assert!(source_backed.contains(&(HighlightKind::ArrayTable, "bin")));
    assert!(source_backed.contains(&(HighlightKind::Key, "path")));
}

#[test]
fn highlights_are_sorted_non_overlapping_and_source_backed() {
    let source = "title = \"TomlSmith\" # tool\n[package]\nport = 8000\nenabled = true\n";
    let document = Document::parse(source);
    let highlights = document.highlights();

    assert!(
        highlights
            .windows(2)
            .all(|pair| { pair[0].range().end() <= pair[1].range().start() })
    );
    assert!(
        highlights
            .iter()
            .all(|highlight| { highlight.range().end() as usize <= source.len() })
    );

    let source_backed = highlights
        .iter()
        .map(|highlight| {
            (
                highlight.kind(),
                &source[highlight.range().start() as usize..highlight.range().end() as usize],
            )
        })
        .collect::<Vec<_>>();

    assert!(source_backed.contains(&(HighlightKind::Key, "title")));
    assert!(source_backed.contains(&(HighlightKind::String, "\"TomlSmith\"")));
    assert!(source_backed.contains(&(HighlightKind::Comment, "# tool")));
    assert!(source_backed.contains(&(HighlightKind::Table, "package")));
    assert!(source_backed.contains(&(HighlightKind::Number, "8000")));
    assert!(source_backed.contains(&(HighlightKind::Boolean, "true")));
}

#[test]
fn inline_table_value_keys_are_distinct_from_nested_scalar_keys() {
    let source = "meta = { answer = 42, nested = { enabled = true } }\n";
    let document = Document::parse(source);
    let source_backed = document
        .highlights()
        .iter()
        .map(|highlight| {
            (
                highlight.kind(),
                &source[highlight.range().start() as usize..highlight.range().end() as usize],
            )
        })
        .collect::<Vec<_>>();

    assert!(source_backed.contains(&(HighlightKind::InlineTableKey, "meta")));
    assert!(source_backed.contains(&(HighlightKind::Key, "answer")));
    assert!(source_backed.contains(&(HighlightKind::InlineTableKey, "nested")));
    assert!(source_backed.contains(&(HighlightKind::Key, "enabled")));
}

#[test]
fn inline_table_members_retain_their_container_context() {
    let source =
        "criterion = { version = \"0.8.2\", nested = { enabled = true }, features = [] }\n";
    let document = Document::parse(source);
    let keys = document
        .highlights()
        .iter()
        .filter(|highlight| {
            matches!(
                highlight.kind(),
                HighlightKind::Key | HighlightKind::ArrayKey | HighlightKind::InlineTableKey
            )
        })
        .map(|highlight| {
            (
                highlight.kind(),
                &source[highlight.range().start() as usize..highlight.range().end() as usize],
                highlight.is_inline_table_member(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        keys,
        vec![
            (HighlightKind::InlineTableKey, "criterion", false),
            (HighlightKind::Key, "version", true),
            (HighlightKind::InlineTableKey, "nested", true),
            (HighlightKind::Key, "enabled", true),
            (HighlightKind::ArrayKey, "features", true),
        ]
    );
}

#[test]
fn array_table_headers_do_not_leak_into_following_assignments() {
    let source = "[[products]]\nname = \"Hammer\"\nprice = 10\n";
    let document = Document::parse(source);
    let highlights = document.highlights();
    let source_backed = highlights
        .iter()
        .map(|highlight| {
            (
                highlight.kind(),
                &source[highlight.range().start() as usize..highlight.range().end() as usize],
            )
        })
        .collect::<Vec<_>>();

    assert!(source_backed.contains(&(HighlightKind::ArrayTable, "products")));
    assert!(source_backed.contains(&(HighlightKind::Key, "name")));
    assert!(source_backed.contains(&(HighlightKind::Key, "price")));
    assert!(source_backed.contains(&(HighlightKind::String, "\"Hammer\"")));
    assert!(source_backed.contains(&(HighlightKind::Number, "10")));
}
