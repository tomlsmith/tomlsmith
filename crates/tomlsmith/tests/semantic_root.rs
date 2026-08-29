use tomlsmith::{DateTimeKind, Document, SemanticValue};

#[test]
fn semantic_root_materializes_tables_and_array_table_elements() {
    let document = Document::parse(
        "title = \"example\"\n[empty]\n[[products]]\nname = \"Hammer\"\n[[products]]\nname = \"Nail\"\n",
    );
    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics()
    );

    let root = document.semantics().root();
    assert_eq!(
        root.get("title").and_then(SemanticValue::as_str),
        Some("example")
    );
    assert_eq!(
        root.get("empty")
            .and_then(SemanticValue::as_table)
            .map(|table| table.entries().len()),
        Some(0),
    );

    let products = root
        .get("products")
        .and_then(SemanticValue::as_array)
        .expect("products should be an array of tables");
    assert_eq!(products.len(), 2);
    assert_eq!(
        products[0]
            .as_table()
            .and_then(|table| table.get("name"))
            .and_then(SemanticValue::as_str),
        Some("Hammer"),
    );
    assert_eq!(
        products[1]
            .as_table()
            .and_then(|table| table.get("name"))
            .and_then(SemanticValue::as_str),
        Some("Nail"),
    );
}

#[test]
fn semantic_datetime_values_expose_their_toml_types_and_canonical_values() {
    let document = Document::parse(
        "offset = 1979-05-27 07:32z\nlocal = 1979-05-27t07:32\ndate = 1979-05-27\ntime = 07:32\n",
    );
    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics()
    );

    for (key, kind, canonical) in [
        (
            "offset",
            DateTimeKind::OffsetDateTime,
            "1979-05-27T07:32:00Z",
        ),
        ("local", DateTimeKind::LocalDateTime, "1979-05-27T07:32:00"),
        ("date", DateTimeKind::LocalDate, "1979-05-27"),
        ("time", DateTimeKind::LocalTime, "07:32:00"),
    ] {
        let value = document
            .semantics()
            .root()
            .get(key)
            .and_then(SemanticValue::as_datetime)
            .unwrap_or_else(|| panic!("{key} should be a datetime value"));
        assert_eq!(value.kind(), kind);
        assert_eq!(value.canonical(), canonical);
    }
}

#[test]
fn nested_tables_follow_the_current_parent_array_element() {
    let document = Document::parse(
        "[[fruits]]\nname = \"apple\"\n[[fruits.varieties]]\nname = \"red\"\n[[fruits]]\nname = \"banana\"\n[fruits.varieties.details]\nx = 1\n",
    );
    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics(),
    );

    let fruits = document
        .semantics()
        .root()
        .get("fruits")
        .and_then(SemanticValue::as_array)
        .expect("fruits should be an array of tables");
    assert_eq!(fruits.len(), 2);
    assert_eq!(
        fruits[0]
            .as_table()
            .and_then(|fruit| fruit.get("varieties"))
            .and_then(SemanticValue::as_array)
            .and_then(|varieties| varieties.first())
            .and_then(SemanticValue::as_table)
            .and_then(|variety| variety.get("name"))
            .and_then(SemanticValue::as_str),
        Some("red"),
    );
    assert_eq!(
        fruits[1]
            .as_table()
            .and_then(|fruit| fruit.get("varieties"))
            .and_then(SemanticValue::as_table)
            .and_then(|varieties| varieties.get("details"))
            .and_then(SemanticValue::as_table)
            .and_then(|details| details.get("x"))
            .and_then(SemanticValue::as_integer),
        Some(1),
    );
}
