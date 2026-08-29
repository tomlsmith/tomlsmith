use proptest::prelude::*;
use tomlsmith::{Document, SemanticValue, TextChange, TextRange};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_utf_8_is_lossless_and_terminates(source in any::<String>()) {
        let document = Document::parse(source.clone());

        prop_assert_eq!(document.text(), source.as_str());
        prop_assert_eq!(document.root().text(), source);
    }

    #[test]
    fn snapshot_edits_match_a_clean_reparse(
        prefix in any::<String>(),
        removed in any::<String>(),
        inserted in any::<String>(),
        suffix in any::<String>(),
    ) {
        let source = format!("{prefix}{removed}{suffix}");
        let start = prefix.len();
        let end = start + removed.len();
        let original = Document::parse(source);
        let incremental = original
            .with_changes([TextChange::edit(
                TextRange::new(
                    u32::try_from(start).expect("generated source fits in u32"),
                    u32::try_from(end).expect("generated source fits in u32"),
                ),
                inserted.as_str(),
            )])
            .expect("generated edits use UTF-8 boundaries");
        let rebuilt = Document::parse(format!("{prefix}{inserted}{suffix}"));

        prop_assert_eq!(incremental.text(), rebuilt.text());
        prop_assert_eq!(incremental.root().text(), rebuilt.root().text());
        prop_assert_eq!(incremental.diagnostics(), rebuilt.diagnostics());
        prop_assert_eq!(incremental.semantics().declarations(), rebuilt.semantics().declarations());
        prop_assert_eq!(incremental.highlights(), rebuilt.highlights());
    }
}

#[test]
fn excessive_collection_nesting_is_bounded_and_lossless() {
    const DEPTH: usize = 300;
    let source = format!("value = {}0{}\n", "[".repeat(DEPTH), "]".repeat(DEPTH));

    let document = Document::parse(source.clone());

    assert_eq!(document.text(), source);
    assert_eq!(document.root().text(), source);
    assert!(
        document
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == tomlsmith::DiagnosticCode::NESTING_LIMIT })
    );
}

#[test]
fn dotted_key_nesting_has_a_documented_safe_limit() {
    let supported_path = vec!["level"; 100].join(".");
    for supported in [
        format!("{supported_path} = 1\n"),
        format!("[{supported_path}]\nvalue = 1\n"),
        format!("[[{supported_path}]]\nvalue = 1\n"),
        format!("value = {{ {supported_path} = 1 }}\n"),
    ] {
        let document = Document::parse(supported);
        assert!(
            document.diagnostics().is_empty(),
            "{:?}",
            document.diagnostics(),
        );
    }

    let excessive_path = vec!["level"; 257].join(".");
    for excessive in [
        format!("{excessive_path} = 1\n"),
        format!("[{excessive_path}]\nvalue = 1\n"),
        format!("[[{excessive_path}]]\nvalue = 1\n"),
        format!("value = {{ {excessive_path} = 1 }}\n"),
    ] {
        let document = Document::parse(excessive.clone());
        assert_eq!(document.root().text(), excessive);
        assert!(
            document.diagnostics().iter().any(|diagnostic| {
                diagnostic.code() == tomlsmith::DiagnosticCode::NESTING_LIMIT
            })
        );
    }

    let header = vec!["table"; 200].join(".");
    let relative_key = vec!["key"; 57].join(".");
    let combined = Document::parse(format!("[{header}]\n{relative_key} = 1\n"));
    assert!(
        combined
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == tomlsmith::DiagnosticCode::NESTING_LIMIT })
    );
}

#[test]
fn an_excessive_table_header_does_not_pollute_the_following_scope() {
    let excessive_path = vec!["level"; 257].join(".");
    let source = format!("[{excessive_path}]\nleaked = true\n\n[safe]\nvalue = 1\n");
    let document = Document::parse(source);
    let root = document.semantics().root();

    assert!(root.get("leaked").is_none());
    assert_eq!(
        root.get("safe")
            .and_then(SemanticValue::as_table)
            .and_then(|table| table.get("value"))
            .and_then(SemanticValue::as_integer),
        Some(1),
    );
}
