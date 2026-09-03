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
fn formatting_removes_blank_lines_between_contiguous_table_items() {
    let source = "[workspace.dependencies]\n\n\nclap={version=\"4.5\",features=[\"derive\"]}\nlsp-server=\"0.10\"\n\n\n\nlsp-types=\"0.97\"\n\n# Release binaries\n";
    let expected = "[workspace.dependencies]\nclap = { version = \"4.5\", features = [\"derive\"] }\nlsp-server = \"0.10\"\nlsp-types = \"0.97\"\n\n# Release binaries\n";

    let FormatOutcome::Changed { text, .. } = Document::parse(source).format() else {
        panic!("blank lines between contiguous table items should be removed");
    };

    assert_eq!(text.as_ref(), expected);
    assert!(matches!(
        Document::parse(text).format(),
        FormatOutcome::Unchanged
    ));
}

#[test]
fn formatting_flattens_short_multiline_inline_tables() {
    let source = "clap = {\n\n\n  version = \"4.5\", features = [\n    \"derive\"\n  ]\n}\n";
    let expected = "clap = { version = \"4.5\", features = [\"derive\"] }\n";

    let FormatOutcome::Changed { text, .. } = Document::parse(source).format() else {
        panic!("a short comment-free inline table should be flattened");
    };

    assert_eq!(text.as_ref(), expected);
    assert!(matches!(
        Document::parse(text).format(),
        FormatOutcome::Unchanged
    ));
}

#[test]
fn line_width_expands_inline_tables_consistently() {
    let source = "package = {\n  version=\"4.5\", features=[\"derive\"] }\n";
    let expected = "package = {\n  version = \"4.5\",\n  features = [\"derive\"]\n}\n";
    let options = FormatOptions {
        line_width: 32,
        ..FormatOptions::default()
    };

    let FormatOutcome::Changed { text, .. } = Document::parse(source).format_with(&options) else {
        panic!("an inline table that does not fit should be fully expanded");
    };

    assert_eq!(text.as_ref(), expected);
    assert!(matches!(
        Document::parse(text).format_with(&options),
        FormatOutcome::Unchanged
    ));
}

#[test]
fn line_width_expands_single_line_inline_tables() {
    let source = "package={version=\"4.5\",features=[\"derive\"]}\n";
    let expected = "package = {\n  version = \"4.5\",\n  features = [\"derive\"]\n}\n";
    let options = FormatOptions {
        line_width: 32,
        ..FormatOptions::default()
    };

    let FormatOutcome::Changed { text, .. } = Document::parse(source).format_with(&options) else {
        panic!("a long single-line inline table should be expanded");
    };

    assert_eq!(text.as_ref(), expected);
    assert!(matches!(
        Document::parse(text).format_with(&options),
        FormatOutcome::Unchanged
    ));
}

#[test]
fn line_width_keeps_toml_1_0_inline_tables_single_line() {
    let source = "package={version=\"4.5\",features=[\"derive\"]}\n";
    let expected = "package = { version = \"4.5\", features = [\"derive\"] }\n";
    let options = FormatOptions {
        target_version: TomlVersion::V1_0,
        line_width: 32,
        ..FormatOptions::default()
    };

    let FormatOutcome::Changed { text, .. } =
        Document::parse_as(source, TomlVersion::V1_0).format_with(&options)
    else {
        panic!("TOML 1.0 inline tables should still have canonical spacing");
    };

    assert_eq!(text.as_ref(), expected);
    assert!(matches!(
        Document::parse_as(text, TomlVersion::V1_0).format_with(&options),
        FormatOutcome::Unchanged
    ));
}

#[test]
fn formatting_keeps_commented_inline_tables_fully_multiline() {
    let source = "package = {\n  # Keep this note.\n  version=\"4.5\", features=[\"derive\"] }\n";
    let expected =
        "package = {\n  # Keep this note.\n  version = \"4.5\",\n  features = [\"derive\"]\n}\n";

    let FormatOutcome::Changed { text, .. } = Document::parse(source).format() else {
        panic!("a commented inline table should be fully expanded");
    };

    assert_eq!(text.as_ref(), expected);
    assert!(matches!(
        Document::parse(text).format(),
        FormatOutcome::Unchanged
    ));
}

#[test]
fn formatting_expands_inline_tables_with_multiline_strings() {
    let source = "package = { text = \"\"\"alpha\nbeta\"\"\", other=1 }\n";
    let expected = "package = {\n  text = \"\"\"alpha\nbeta\"\"\",\n  other = 1\n}\n";

    let FormatOutcome::Changed { text, .. } = Document::parse(source).format() else {
        panic!("an inline table containing a multiline string should be fully expanded");
    };

    assert_eq!(text.as_ref(), expected);
    assert!(matches!(
        Document::parse(text).format(),
        FormatOutcome::Unchanged
    ));
}

#[test]
fn formatting_normalizes_nested_inline_tables_in_one_pass() {
    let source = "root={nested={alpha=\"11111\",beta=\"22222\"},tail=1}\n";
    let expected = "root = {\n  nested = {\n    alpha = \"11111\",\n    beta = \"22222\"\n  },\n  tail = 1\n}\n";
    let options = FormatOptions {
        line_width: 40,
        ..FormatOptions::default()
    };

    let FormatOutcome::Changed { text, .. } = Document::parse(source).format_with(&options) else {
        panic!("nested inline tables should reach their canonical layout in one format call");
    };

    assert_eq!(text.as_ref(), expected);
    assert!(matches!(
        Document::parse(text).format_with(&options),
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
fn arrays_inside_expanded_inline_tables_wrap_at_their_real_columns() {
    // The table does not fit at width 50, so it expands; its array is then
    // measured on the expanded line, where it fits without wrapping.
    let source = "wide = [\n  { name = \"alpha\", version = \"1.0.0\", features = [\"one\", \"two\", \"three\"] },\n]\n";
    let expected = "wide = [\n  {\n    name = \"alpha\",\n    version = \"1.0.0\",\n    features = [\"one\", \"two\", \"three\"]\n  },\n]\n";
    let options = FormatOptions {
        line_width: 50,
        ..FormatOptions::default()
    };

    let FormatOutcome::Changed { text, .. } = Document::parse(source).format_with(&options) else {
        panic!("the inline table should expand");
    };

    assert_eq!(text.as_ref(), expected);
    assert!(matches!(
        Document::parse(text).format_with(&options),
        FormatOutcome::Unchanged
    ));
}

/// Runs `body` on a thread with a deliberately small stack so that any
/// recursion over token-level nesting overflows deterministically on every
/// platform instead of depending on the host's default stack size.
fn on_small_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(body)
        .expect("spawn a small-stack test thread")
        .join()
        .expect("the formatter must not overflow a small stack");
}

#[test]
fn pathological_nesting_is_refused_without_rendering() {
    // Deeper than the parser's collection limit: the document is refused, and
    // neither the guarded nor the one-shot path may recurse or render the
    // quadratic expanded layout before refusing.
    on_small_stack(|| {
        let depth = 20_000;
        let source = format!("a = {}1{}\n", "{ b = ".repeat(depth), " }".repeat(depth));
        let options = FormatOptions {
            line_width: 20,
            ..FormatOptions::default()
        };

        let document = Document::parse(source.as_str());
        assert!(matches!(
            document.format_with(&options),
            FormatOutcome::Refused { .. }
        ));
        let (_, outcome) = Document::parse_and_format_with(source, TomlVersion::V1_1, &options);
        assert!(matches!(outcome, FormatOutcome::Refused { .. }));
    });
}

#[test]
fn nesting_within_the_supported_limit_expands_iteratively() {
    on_small_stack(|| {
        let depth = 256;
        let source = format!("a = {}1{}\n", "{ b = ".repeat(depth), " }".repeat(depth));
        let options = FormatOptions {
            line_width: 20,
            ..FormatOptions::default()
        };

        let FormatOutcome::Changed { text, .. } =
            Document::parse(source.as_str()).format_with(&options)
        else {
            panic!("a deeply nested but valid table should be expanded");
        };
        assert!(text.starts_with("a = {\n  b = {\n"));
        assert!(text.ends_with("  }\n}\n"));
        assert!(matches!(
            Document::parse(text).format_with(&options),
            FormatOutcome::Unchanged
        ));
    });
}

#[test]
fn one_shot_formatting_matches_guarded_formatting() {
    let sources = [
        "a=1\n",
        "a = { b = 1, c = [1, 2] }\n",
        "a = {\n  # note\n  b = 1 }\n",
        "a = 1\na = 2\n",
        "title = \"unterminated\n",
        "value = {{{ 1 }}}\n",
    ];
    for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
        for target in [TomlVersion::V1_0, TomlVersion::V1_1] {
            let options = FormatOptions {
                target_version: target,
                line_width: 24,
                ..FormatOptions::default()
            };
            for source in sources {
                let guarded = Document::parse_as(source, version).format_with(&options);
                let (_, one_shot) = Document::parse_and_format_with(source, version, &options);
                assert_eq!(one_shot, guarded, "{source:?} {version:?} -> {target:?}");
            }
        }
    }
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
