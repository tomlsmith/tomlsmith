//! Public-level anchors for degenerate value payloads under the
//! token-boundary lowering semantics: the lexer/parser structure is the
//! single source of truth, malformed payloads become `SemanticValue::Invalid`
//! carrying the trimmed source slice of their own span, and the
//! `INVALID_VALUE` diagnostic points at the first offending element. Each test names the
//! retired splitter behavior it replaces so the changelog can be derived
//! from this file.

use tomlsmith::{DiagnosticCode, Document, SemanticValue, TextRange};

/// The lowered value of the first (and only) key-value declaration.
fn value_of(source: &str) -> SemanticValue {
    let document = Document::parse(source);
    let declarations = document.semantics().declarations();
    assert_eq!(declarations.len(), 1, "{source:?}: {declarations:#?}");
    declarations[0]
        .value()
        .unwrap_or_else(|| panic!("{source:?} should declare a value"))
        .clone()
}

fn invalid(text: &str) -> SemanticValue {
    SemanticValue::Invalid(text.into())
}

fn invalid_value_range(source: &str) -> TextRange {
    let document = Document::parse(source);
    document
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code() == DiagnosticCode::INVALID_VALUE)
        .unwrap_or_else(|| panic!("{source:?} should carry INVALID_VALUE"))
        .range()
}

#[test]
fn multi_token_payloads_keep_the_trimmed_slice_as_invalid_text() {
    assert_eq!(value_of("a = 1 2\n"), invalid("1 2"));
    assert_eq!(value_of("a = \"x\" y\n"), invalid("\"x\" y"));
    assert_eq!(value_of("a = =1\n"), invalid("=1"));
}

#[test]
fn collections_with_stray_trailing_tokens_are_invalid_as_a_whole() {
    // Retired splitter: re-split the text, e.g. `[1]]` -> [Invalid("1]")].
    // Now: the payload is not one well-formed value, so the whole trimmed
    // slice is the invalid payload.
    assert_eq!(value_of("a = [1] x\n"), invalid("[1] x"));
    assert_eq!(value_of("a = [1]]\n"), invalid("[1]]"));
    assert_eq!(value_of("a = [1, 2 [3]]\n"), invalid("[1, 2 [3]]"));
}

#[test]
fn recovery_leftovers_become_invalid_elements_at_token_boundaries() {
    // Retired splitter: `[1 { , 2]` hid the comma behind the `{` and made
    // one part. Now each parsed element keeps its own token span.
    assert_eq!(
        value_of("a = [1 { , 2]\n"),
        SemanticValue::Array([invalid("1 {"), SemanticValue::Integer(2)].into())
    );
    // Retired splitter: [Invalid("[1] 2"), 3]. Now the parsed `[1]` stays an
    // array and the stray `2` is its own invalid element.
    assert_eq!(
        value_of("a = [[1] 2, 3]\n"),
        SemanticValue::Array(
            [
                SemanticValue::Array([SemanticValue::Integer(1)].into()),
                invalid("2"),
                SemanticValue::Integer(3),
            ]
            .into()
        )
    );
}

#[test]
fn unclosed_collections_lower_to_their_parsed_structure() {
    // Retired splitter: Invalid("[1, 2") / Invalid("{x = 1"). The parser
    // already diagnoses UNCLOSED_ARRAY / UNCLOSED_INLINE_TABLE; the
    // semantics now keep the parsed elements.
    assert_eq!(
        value_of("a = [1, 2\n"),
        SemanticValue::Array([SemanticValue::Integer(1), SemanticValue::Integer(2)].into())
    );
    let document = Document::parse("a = {x = 1\n");
    let declarations = document.semantics().declarations();
    let Some(SemanticValue::InlineTable(entries)) = declarations[0].value() else {
        panic!("expected an inline table: {declarations:#?}");
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].1, SemanticValue::Integer(1));
    assert!(
        document
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == DiagnosticCode::UNCLOSED_INLINE_TABLE)
    );
}

#[test]
fn unterminated_strings_end_at_their_token_boundary() {
    // The lexer ends an unterminated single-line string at the line end;
    // the swallowed `]` stays inside the element and the array stays an
    // array (the retired splitter dropped the `]` from the payload).
    assert_eq!(
        value_of("a = [\"x, 1]\n"),
        SemanticValue::Array([invalid("\"x, 1]")].into())
    );
    // A lone carriage return terminates the string token; the retired
    // splitter scanned through it and resurrected the comma split.
    assert_eq!(
        value_of("a = [\"a\rb\",\"c\"]\n"),
        SemanticValue::Array([invalid("\"a\rb\",\"c\"]")].into())
    );
}

#[test]
fn non_ascii_whitespace_no_longer_reveals_hidden_structure() {
    // U+00A0 lexes into a Bare token, so the payload is a token run, not an
    // array; the retired splitter trimmed it away and parsed `[1]`.
    assert_eq!(value_of("a = \u{a0}[1]\n"), invalid("[1]"));
    // Whitespace-only parts still produce no element.
    assert_eq!(value_of("a = [\u{a0}]\n"), SemanticValue::Array([].into()));
}

#[test]
fn comments_end_at_their_token_boundary() {
    // The comment token ends at the lone `\r`, so `2` is a real element;
    // the retired splitter kept scanning to the `\n` and swallowed it.
    assert_eq!(
        value_of("a = [1, #c\r2]\n"),
        SemanticValue::Array([SemanticValue::Integer(1), SemanticValue::Integer(2)].into())
    );
    assert_eq!(
        value_of("a = [1 #c\n, 2]\n"),
        SemanticValue::Array([SemanticValue::Integer(1), SemanticValue::Integer(2)].into())
    );
}

#[test]
fn inline_table_entries_need_their_own_equals_token() {
    // Later `=` tokens stay inside the value's span.
    assert_eq!(
        value_of("a = {x = 1 = 2}\n"),
        SemanticValue::InlineTable([(key(&["x"]), invalid("1 = 2"))].into())
    );
    // Entries without an `=` token drop out; a comment between key and `=`
    // is no longer scanned through (the retired splitter resurrected x = 1).
    assert_eq!(
        value_of("a = { x # c\n = 1 }\n"),
        SemanticValue::InlineTable([].into())
    );
    assert_eq!(
        value_of("a = { x }\n"),
        SemanticValue::InlineTable([].into())
    );
    // A dropped entry no longer hides the entries after it (the retired
    // splitter's bracket counters swallowed `A = ` entirely).
    assert_eq!(
        value_of("a = {0 [[''], A = }\n"),
        SemanticValue::InlineTable([(key(&["A"]), invalid(""))].into())
    );
}

#[test]
fn dotted_inline_keys_stay_flat_and_conflicts_are_diagnosed() {
    let document = Document::parse("a = {x.y = 1, x = 2}\n");
    let declarations = document.semantics().declarations();
    let Some(SemanticValue::InlineTable(entries)) = declarations[0].value() else {
        panic!("expected an inline table: {declarations:#?}");
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].0.dotted(), "x.y");
    assert!(
        document
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == DiagnosticCode::CONFLICTING_KEY)
    );
}

#[test]
fn depth_limited_collections_keep_their_raw_source_slice() {
    let depth = 258;
    let source = format!("a = {}0{}\n", "[".repeat(depth), "]".repeat(depth));
    let document = Document::parse(source);
    let declarations = document.semantics().declarations();
    let mut value = declarations[0].value().expect("a should have a value");
    for _ in 0..256 {
        let SemanticValue::Array(elements) = value else {
            panic!("expected 256 structured levels, got {value:#?}");
        };
        assert_eq!(elements.len(), 1);
        value = &elements[0];
    }
    // The 257th collection is flat; its payload is the raw trimmed slice.
    assert_eq!(value, &invalid("[[0]]"));
    assert!(
        document
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == DiagnosticCode::NESTING_LIMIT)
    );
}

#[test]
fn depth_limited_mismatched_soups_stay_one_invalid_element() {
    // Retired splitter: its independent saturating counters hit ground
    // after `[{]}` and the comma split the payload into two invalid parts.
    // Now the parser's flat soup node is the single source of truth.
    let payload = format!("{}[{{]}},]{}", "[".repeat(256), "]".repeat(256));
    let direct = value_of(&format!("a = {payload}\n"));
    let mut value = &direct;
    for _ in 0..255 {
        let SemanticValue::Array(elements) = value else {
            panic!("expected 255 single-element levels, got {value:#?}");
        };
        assert_eq!(elements.len(), 1);
        value = &elements[0];
    }
    let SemanticValue::Array(innermost) = value else {
        panic!("expected the 256th structured level, got {value:#?}");
    };
    assert_eq!(innermost.len(), 1);
    assert_eq!(innermost[0], invalid("[{]},]"));
}

#[test]
fn oversized_closing_quote_runs_follow_the_lexer_tokens() {
    // Six quotes close the token after five; the remainder opens a second
    // string token and the element is a multi-token run.
    assert_eq!(
        value_of("a = [\"\"\"a\"\"\"\"\"\"x\"]\n"),
        SemanticValue::Array([invalid("\"\"\"a\"\"\"\"\"\"x\"")].into())
    );
    // Up to five closing quotes stay inside the literal itself.
    assert_eq!(
        value_of("a = \"\"\"x\"\"\"\"\"\n"),
        SemanticValue::String("x\"\"".into())
    );
}

#[test]
fn statement_keys_split_on_dot_tokens_outside_strings() {
    // The unterminated string swallows the whole line into one token, so
    // the dot never splits.
    let document = Document::parse("\"a.b = 1\n");
    let declarations = document.semantics().declarations();
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].key().dotted(), "\"a.b = 1");
    assert_eq!(declarations[0].value(), Some(&invalid("")));

    // A lone carriage return ends the string token, so the dot after it is
    // a real separator (the retired splitter stayed inside the string).
    let document = Document::parse("\"a\rb.c = 1\n");
    let declarations = document.semantics().declarations();
    assert_eq!(declarations.len(), 1);
    let segments: Vec<&str> = declarations[0].key().segments().collect();
    assert_eq!(segments, ["\"a\rb", "c"]);
}

#[test]
fn the_invalid_value_diagnostic_points_at_the_first_offending_element() {
    // The whole-payload cases point at the trimmed payload span.
    let source = "a = 1 2\n";
    assert_eq!(invalid_value_range(source), TextRange::new(4, 7));

    // Element-level cases point at the offending element, not the whole
    // declaration (the retired behavior used the declaration range).
    let source = "a = [1, oops, 2]\n";
    let start = offset_of(source, "oops");
    assert_eq!(
        invalid_value_range(source),
        TextRange::new(start, start + 4)
    );

    // Nested entries point at the innermost first offender.
    let source = "a = {x = [1, {y = bad}, 3]}\n";
    let start = offset_of(source, "bad");
    assert_eq!(
        invalid_value_range(source),
        TextRange::new(start, start + 3)
    );
}

fn offset_of(source: &str, needle: &str) -> u32 {
    u32::try_from(source.find(needle).expect("needle present")).expect("offset fits in u32")
}

fn key(segments: &[&str]) -> tomlsmith::KeyPath {
    let source = format!("{} = 0\n", segments.join("."));
    let document = Document::parse(source);
    document.semantics().declarations()[0].key().clone()
}
