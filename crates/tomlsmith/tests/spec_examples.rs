use tomlsmith::{Document, FormatOutcome, Resolution, SemanticValue};

const SPEC_STYLE_DOCUMENT: &str = r#"title = "TOML Example"

[owner]
name = "Tom Preston-Werner"
dob = 1979-05-27T07:32:00-08:00

[database]
enabled = true
ports = [8000, 8001, 8002]
data = [["delta", "phi"], [3.14]]
temp_targets = { cpu = 79.5, case = 72.0 }

[servers.alpha]
ip = "10.0.0.1"
role = "frontend"

[[products]]
name = "Hammer"
sku = 738594937

[[products]]
name = "Nail"
sku = 284758393
color = "gray"
"#;

#[test]
fn representative_toml_spec_document_is_analyzed_without_diagnostics() {
    let document = Document::parse(SPEC_STYLE_DOCUMENT);

    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics()
    );

    let Resolution::Unique(ports) = document.semantics().resolve(["database", "ports"]) else {
        panic!("database.ports should resolve uniquely");
    };
    assert_eq!(
        ports
            .value()
            .and_then(SemanticValue::as_array)
            .map(<[_]>::len),
        Some(3)
    );

    let Resolution::Ambiguous(product_names) = document.semantics().resolve(["products", "name"])
    else {
        panic!("both array-of-table product names must remain represented");
    };
    assert_eq!(product_names.len(), 2);
}

#[test]
fn representative_document_formats_to_a_stable_result() {
    let document = Document::parse(SPEC_STYLE_DOCUMENT);
    let formatted = match document.format() {
        FormatOutcome::Unchanged => SPEC_STYLE_DOCUMENT.into(),
        FormatOutcome::Changed { text, .. } => text,
        FormatOutcome::Refused { diagnostics } => {
            panic!("valid document should be safe to format: {diagnostics:?}")
        }
    };

    assert!(matches!(
        Document::parse(formatted).format(),
        FormatOutcome::Unchanged
    ));
}
