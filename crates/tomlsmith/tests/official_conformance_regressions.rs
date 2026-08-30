use tomlsmith::{DiagnosticCode, Document, Resolution, SemanticValue, TomlVersion};

fn assert_conflicting_in_both_versions(source: &str) {
    for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
        let document = Document::parse_as(source, version);
        assert!(
            document
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code() == DiagnosticCode::CONFLICTING_KEY),
            "{version:?} accepted an invalid table namespace: {source:?}; diagnostics: {:?}",
            document.diagnostics(),
        );
    }
}

// Regression cases are taken from toml-lang/toml-test v2.2.0. They exercise
// observable Document diagnostics rather than the private namespace index.
#[test]
fn dotted_keys_cannot_extend_an_array_of_tables_from_its_parent() {
    assert_conflicting_in_both_versions("[[tab.arr]]\n[tab]\narr.val1=1\n");
    assert_conflicting_in_both_versions("[[a.b]]\n\n[a]\nb.y = 2\n");
}

#[test]
fn dotted_keys_cannot_extend_a_deeper_explicit_table_from_its_parent() {
    for source in [
        "[a.b.c]\nz = 9\n\n[a]\nb.c.t = \"invalid injection\"\n",
        "[a.b.c.d]\nz = 9\n\n[a]\nb.c.d.k.t = \"invalid injection\"\n",
        "[a.b.c]\nz = 9\n\n[[unrelated]]\nx = 123\n\n[a]\nb.c.t = \"invalid injection\"\n",
    ] {
        assert_conflicting_in_both_versions(source);
    }
}

#[test]
fn an_implicitly_created_table_cannot_later_become_an_array_of_tables() {
    assert_conflicting_in_both_versions(
        "[[albums.songs]]\nname = \"Glory Days\"\n\n[[albums]]\nname = \"Born in the USA\"\n",
    );
}

#[test]
fn arrays_allow_newlines_and_comments_after_values() {
    for source in [
        "value = [\n  { name = \"one\" }\n]\n",
        "value = [\n  1 # comment\n  , 2\n]\n",
        "value = [[\n  1\n], [\n  2\n]]\n",
    ] {
        let document = Document::parse(source);
        assert!(
            document.diagnostics().is_empty(),
            "{source:?}: {:?}",
            document.diagnostics(),
        );
    }
}

#[test]
fn toml_1_1_inline_tables_allow_newlines_after_values() {
    let source = "value = {\n  first = 1\n  , second = 2\n}\n";
    let document = Document::parse_as(source, TomlVersion::V1_1);
    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics()
    );
}

#[test]
fn toml_1_0_inline_tables_allow_newlines_inside_array_values() {
    let source = "value = { array = [\n  1,\n  2,\n] }\n";
    let document = Document::parse_as(source, TomlVersion::V1_0);
    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics()
    );
}

#[test]
fn multiline_basic_string_folds_whitespace_before_a_newline() {
    let document = Document::parse("value = \"\"\"\nheeee\ngeeee\\  \n\n\n\"\"\"\n");
    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics()
    );
    let Resolution::Unique(value) = document.semantics().resolve(["value"]) else {
        panic!("value should resolve");
    };
    assert_eq!(
        value.value().and_then(SemanticValue::as_str),
        Some("heeee\ngeeee")
    );
}

#[test]
fn an_implicit_parent_table_can_be_explicitly_declared_later() {
    for source in [
        "[a.b.c]\nanswer = 42\n\n[a]\nbetter = 43\n",
        "[x.y.z.w]\na = 1\n[x]\nc = 3\n",
    ] {
        let document = Document::parse(source);
        assert!(
            document.diagnostics().is_empty(),
            "{source:?}: {:?}",
            document.diagnostics(),
        );
    }
}

#[test]
fn dotted_keys_can_extend_the_current_table_context() {
    let source = "[fruit]\napple.color = \"red\"\napple.taste.sweet = true\n\n[fruit.apple.texture]\nsmooth = true\n";
    for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
        let document = Document::parse_as(source, version);
        assert!(
            document.diagnostics().is_empty(),
            "{version:?}: {:?}",
            document.diagnostics(),
        );
    }
}

#[test]
fn one_utf8_bom_is_allowed_only_at_the_start_of_the_document() {
    for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
        let source = "\u{feff}# optional UTF-8 BOM\na = 1\n";
        let document = Document::parse_as(source, version);
        assert!(
            document.diagnostics().is_empty(),
            "{version:?}: {:?}",
            document.diagnostics(),
        );
        assert_eq!(document.root().text(), source);
        let value = document
            .semantics()
            .root()
            .get("a")
            .and_then(SemanticValue::as_integer);
        assert_eq!(value, Some(1));

        for invalid in ["\u{feff}\u{feff}a = 1\n", "a = \u{feff}1\n"] {
            assert!(
                !Document::parse_as(invalid, version)
                    .diagnostics()
                    .is_empty(),
                "{version:?} accepted {invalid:?}",
            );
        }
    }
}

#[test]
fn implicit_tables_are_scoped_to_their_parent_array_element() {
    let valid = "[[a]]\n[a.b.c]\nx = 1\n[[a]]\n[[a.b]]\ny = 2\n";
    for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
        let document = Document::parse_as(valid, version);
        assert!(
            document.diagnostics().is_empty(),
            "{version:?}: {:?}",
            document.diagnostics(),
        );
    }

    let invalid =
        "[[a]]\n[[a.b.c]]\nx = 1\n[a.b]\ny = 2\n[[a]]\n[[a.b.c]]\nx = 3\n[[a.b]]\ny = 4\n";
    assert_conflicting_in_both_versions(invalid);
}

#[test]
fn a_lone_carriage_return_is_rejected_in_every_position() {
    // The pinned toml-test suite only covers a lone CR at the start of a
    // line; these forms exercise the value-trailing positions it misses.
    for invalid in [
        "a = 1\r",
        "a = 1\r\r\n",
        "a = 1 \r\r\n",
        "s = \"x\"\r",
        "a = 1\r# comment\n",
        "# comment\rb = 2\n",
        "m = \"\"\"line\rline\"\"\"\n",
    ] {
        for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
            let document = Document::parse_as(invalid, version);
            assert!(
                document.diagnostics().iter().any(
                    |diagnostic| diagnostic.code() == DiagnosticCode::INVALID_CONTROL_CHARACTER
                ),
                "{version:?} accepted a lone carriage return: {invalid:?}; diagnostics: {:?}",
                document.diagnostics(),
            );
        }
    }
}

#[test]
fn carriage_returns_inside_crlf_newlines_remain_valid() {
    for valid in [
        "a = 1\r\n",
        "a = 1\r\nb = 2\r\n",
        "m = \"\"\"line\r\nline\"\"\"\r\n",
    ] {
        for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
            let document = Document::parse_as(valid, version);
            assert!(
                document.diagnostics().is_empty(),
                "{version:?} rejected CRLF input {valid:?}: {:?}",
                document.diagnostics(),
            );
        }
    }
}
