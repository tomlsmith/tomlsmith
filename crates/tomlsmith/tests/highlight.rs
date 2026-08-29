use tomlsmith::{Document, HighlightKind};

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
fn inline_table_keys_are_highlighted_as_keys() {
    let source = "meta = { answer = 42, nested = { enabled = true } }\n";
    let document = Document::parse(source);
    let highlighted_keys = document
        .highlights()
        .iter()
        .filter(|highlight| highlight.kind() == HighlightKind::Key)
        .map(|highlight| {
            &source[highlight.range().start() as usize..highlight.range().end() as usize]
        })
        .collect::<Vec<_>>();

    assert_eq!(highlighted_keys, ["meta", "answer", "nested", "enabled"]);
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

    assert!(source_backed.contains(&(HighlightKind::Table, "products")));
    assert!(source_backed.contains(&(HighlightKind::Key, "name")));
    assert!(source_backed.contains(&(HighlightKind::Key, "price")));
    assert!(source_backed.contains(&(HighlightKind::String, "\"Hammer\"")));
    assert!(source_backed.contains(&(HighlightKind::Number, "10")));
}
