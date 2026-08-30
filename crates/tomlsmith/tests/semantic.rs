use tomlsmith::{DiagnosticCode, Document, Resolution};

#[test]
fn semantic_lookup_resolves_dotted_keys_inside_tables() {
    let document =
        Document::parse("[package]\nname = \"tomlsmith\"\nmetadata.port = 8000\nenabled = true\n");
    let semantic = document.semantics();

    let Resolution::Unique(name) = semantic.resolve(["package", "name"]) else {
        panic!("package.name should resolve uniquely");
    };
    assert_eq!(
        name.value().and_then(|value| value.as_str()),
        Some("tomlsmith")
    );

    let Resolution::Unique(port) = semantic.resolve(["package", "metadata", "port"]) else {
        panic!("package.metadata.port should resolve uniquely");
    };
    assert_eq!(
        port.value().and_then(tomlsmith::SemanticValue::as_integer),
        Some(8000)
    );
}

#[test]
fn semantic_lookup_uses_the_typed_header_key_range() {
    let document = Document::parse("[package]   # retained\nname = \"tomlsmith\"\n");

    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics()
    );
    let Resolution::Unique(name) = document.semantics().resolve(["package", "name"]) else {
        panic!("package.name should resolve despite trivia after the header")
    };
    assert_eq!(
        name.value().and_then(|value| value.as_str()),
        Some("tomlsmith")
    );
}

#[test]
fn duplicate_keys_remain_ambiguous_instead_of_last_write_wins() {
    let document = Document::parse("port = 8000\nport = 9000\n");

    let Resolution::Ambiguous(declarations) = document.semantics().resolve(["port"]) else {
        panic!("duplicate declarations must be represented as ambiguous");
    };

    assert_eq!(declarations.len(), 2);
    assert_eq!(
        declarations[0]
            .value()
            .and_then(tomlsmith::SemanticValue::as_integer),
        Some(8000)
    );
    assert_eq!(
        declarations[1]
            .value()
            .and_then(tomlsmith::SemanticValue::as_integer),
        Some(9000)
    );
    assert!(
        document
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == DiagnosticCode::DUPLICATE_KEY })
    );
}

#[test]
fn conflict_diagnostics_carry_the_earliest_conflicting_declaration_range() {
    let document = Document::parse("port = 8000\nport = 9000\nport = 10000\n");
    let duplicates = document
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.code() == DiagnosticCode::DUPLICATE_KEY)
        .collect::<Vec<_>>();
    assert_eq!(duplicates.len(), 2, "{:?}", document.diagnostics());
    for duplicate in duplicates {
        let related = duplicate
            .related_range()
            .expect("a duplicate key must link its first declaration");
        assert_eq!(
            related.start(),
            0,
            "both redeclarations must link the earliest declaration"
        );
    }
}

#[test]
fn related_ranges_stay_inside_the_conflicting_array_table_element() {
    // `name = "apple"` in the first element is not in conflict; the
    // duplicate must link `name = "banana"` from its own element, not the
    // identically named key of the earlier instance.
    let source = "[[fruit]]\nname = \"apple\"\n\n[[fruit]]\nname = \"banana\"\nname = \"cherry\"\n";
    let document = Document::parse(source);
    let duplicate = document
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code() == DiagnosticCode::DUPLICATE_KEY)
        .expect("the second element holds a duplicate key");
    let banana = source.find("name = \"banana\"").unwrap();
    assert_eq!(
        duplicate
            .related_range()
            .map(|range| range.start() as usize),
        Some(banana),
        "{:?}",
        document.diagnostics()
    );
}

#[test]
fn array_of_table_instances_do_not_conflict_with_each_other() {
    let document =
        Document::parse("[[products]]\nname = \"Hammer\"\n\n[[products]]\nname = \"Nail\"\n");

    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics()
    );
    let Resolution::Ambiguous(names) = document.semantics().resolve(["products", "name"]) else {
        panic!("the two array elements should both remain represented");
    };
    assert_eq!(names.len(), 2);
}

#[test]
fn super_tables_and_subtables_in_distinct_array_elements_do_not_conflict() {
    let super_table = Document::parse("[fruit.apple]\n[fruit]\n");
    assert!(
        super_table.diagnostics().is_empty(),
        "{:?}",
        super_table.diagnostics()
    );

    let separate_elements = Document::parse(
        "[[products]]\n[products.details]\nweight = 1\n\n[[products]]\n[products.details]\nweight = 2\n",
    );
    assert!(
        separate_elements.diagnostics().is_empty(),
        "{:?}",
        separate_elements.diagnostics()
    );
}

#[test]
fn scalar_and_dotted_child_conflicts_are_diagnosed() {
    let document = Document::parse("fruit = \"apple\"\nfruit.color = \"red\"\n");

    assert!(
        document
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == DiagnosticCode::CONFLICTING_KEY })
    );
}

#[test]
fn array_table_headers_conflict_with_existing_scalar_or_table_namespaces() {
    for source in [
        "fruit = []\n[[fruit]]\n",
        "[fruit]\n[[fruit]]\n",
        "[[fruit]]\n[fruit]\n",
    ] {
        let document = Document::parse(source);
        assert!(
            document
                .diagnostics()
                .iter()
                .any(|diagnostic| { diagnostic.code() == DiagnosticCode::CONFLICTING_KEY }),
            "{source:?} should conflict: {:?}",
            document.diagnostics()
        );
    }
}

#[test]
fn duplicate_and_prefix_conflicts_inside_inline_tables_are_diagnosed() {
    let duplicate = Document::parse("point = { x = 1, x = 2 }\n");
    assert!(
        duplicate
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == DiagnosticCode::DUPLICATE_KEY })
    );

    let prefix = Document::parse("point = { x = 1, x.y = 2 }\n");
    assert!(
        prefix
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == DiagnosticCode::CONFLICTING_KEY })
    );
}

#[test]
fn multiline_literals_do_not_split_collection_values() {
    let document = Document::parse("values = ['''don't, split''', 2]\n");
    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics()
    );

    let Resolution::Unique(values) = document.semantics().resolve(["values"]) else {
        panic!("values should resolve");
    };
    let Some(values) = values.value().and_then(tomlsmith::SemanticValue::as_array) else {
        panic!("values should be an array");
    };
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].as_str(), Some("don't, split"));
}

#[test]
fn ten_thousand_unique_keys_remain_indexable() {
    let mut source = String::new();
    for index in 0..10_000 {
        use std::fmt::Write as _;
        writeln!(source, "key{index} = {index}").expect("writing to a String cannot fail");
    }

    let document = Document::parse(source.as_str());
    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics()
    );
    assert!(matches!(
        document.semantics().resolve(["key9999"]),
        Resolution::Unique(_)
    ));
}
