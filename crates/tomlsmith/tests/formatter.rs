use tomlsmith::{Document, FormatOptions, FormatOutcome, TomlVersion};

#[test]
fn formatting_is_idempotent_and_preserves_literal_spelling() {
    let source = "title=  \"keep\\u0041\"   #comment\nnumbers=[1,2,  3]\n";
    let document = Document::parse(source);

    let FormatOutcome::Changed { text, edits } = document.format() else {
        panic!("unformatted input should change");
    };

    assert_eq!(
        text.as_ref(),
        "title = \"keep\\u0041\" #comment\nnumbers = [1, 2, 3]\n"
    );
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].range().start(), 0);
    assert_eq!(edits[0].range().end() as usize, source.len());
    assert_eq!(edits[0].replacement(), text.as_ref());
    assert!(matches!(
        Document::parse(text).format(),
        FormatOutcome::Unchanged
    ));
}

#[test]
fn formatting_refuses_structurally_invalid_documents() {
    let document = Document::parse("title = \"unterminated\n");

    let FormatOutcome::Refused { diagnostics } = document.format() else {
        panic!("unsafe formatting must be refused");
    };

    assert!(!diagnostics.is_empty());
}

#[test]
fn formatting_refuses_semantically_invalid_documents() {
    for source in [
        "answer = 42\nanswer = 43\n",
        "[table]\nvalue = 1\n\n[[table]]\nvalue = 2\n",
    ] {
        assert!(matches!(
            Document::parse(source).format(),
            FormatOutcome::Refused { .. }
        ));
    }
}

#[test]
fn formatting_preserves_an_initial_utf8_bom() {
    let FormatOutcome::Changed { text, .. } = Document::parse("\u{feff}answer=42\n").format()
    else {
        panic!("spacing around equals should change");
    };
    assert_eq!(text.as_ref(), "\u{feff}answer = 42\n");
}

#[test]
fn formatting_preserves_the_complete_comment_token() {
    for (source, expected) in [
        (
            "value=1 # keep trailing comment spaces   \n",
            "value = 1 # keep trailing comment spaces   \n",
        ),
        (
            "value=1 # keep trailing comment spaces   ",
            "value = 1 # keep trailing comment spaces   ",
        ),
    ] {
        let FormatOutcome::Changed { text, .. } = Document::parse(source).format() else {
            panic!("spacing around equals should change");
        };
        assert_eq!(text.as_ref(), expected);
    }
}

#[test]
fn formatting_does_not_prefix_top_level_comments_with_a_space() {
    let source = "# project manifest\nanswer=42\n";
    let FormatOutcome::Changed { text, .. } = Document::parse(source).format() else {
        panic!("spacing around equals should change");
    };

    assert_eq!(text.as_ref(), "# project manifest\nanswer = 42\n");
    assert!(matches!(
        Document::parse(text).format(),
        FormatOutcome::Unchanged
    ));
}

#[test]
fn formatting_refuses_syntax_not_supported_by_the_target_version() {
    let document = Document::parse_as("escape=\"\\e\"\n", TomlVersion::V1_1);
    let options = FormatOptions {
        target_version: TomlVersion::V1_0,
        ..FormatOptions::default()
    };

    assert!(matches!(
        document.format_with(&options),
        FormatOutcome::Refused { .. }
    ));
}

#[test]
fn line_width_wraps_arrays_at_safe_comma_boundaries() {
    let document = Document::parse("values=[11111,22222]\n");
    let options = FormatOptions {
        line_width: 14,
        ..FormatOptions::default()
    };

    let FormatOutcome::Changed { text, .. } = document.format_with(&options) else {
        panic!("the array should be formatted and wrapped");
    };
    assert_eq!(text.as_ref(), "values = [11111,\n  22222]\n");
    assert!(matches!(
        Document::parse(text).format_with(&options),
        FormatOutcome::Unchanged
    ));
}
