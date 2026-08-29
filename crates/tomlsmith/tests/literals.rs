use tomlsmith::{DiagnosticCode, Document, Resolution, SemanticValue, TomlVersion};

fn codes(document: &Document) -> Vec<DiagnosticCode> {
    document
        .diagnostics()
        .iter()
        .map(tomlsmith::Diagnostic::code)
        .collect()
}

#[test]
fn integer_and_float_grammar_rejects_invalid_spellings() {
    for source in [
        "value = 1__2\n",
        "value = _1\n",
        "value = 1_\n",
        "value = 1.\n",
        "value = 01.5\n",
        "value = -0x1\n",
        "value = 0x_1\n",
    ] {
        let document = Document::parse(source);
        assert!(
            codes(&document).contains(&DiagnosticCode::INVALID_VALUE),
            "{source:?} should be rejected: {:?}",
            document.diagnostics()
        );
    }
}

#[test]
fn signed_64_bit_integer_boundaries_are_lowered_exactly() {
    let document = Document::parse(
        "minimum = -9223372036854775808\nmaximum = 9223372036854775807\ntoo_low = -9223372036854775809\ntoo_high = 9223372036854775808\n",
    );

    let Resolution::Unique(minimum) = document.semantics().resolve(["minimum"]) else {
        panic!("minimum should resolve");
    };
    let Resolution::Unique(maximum) = document.semantics().resolve(["maximum"]) else {
        panic!("maximum should resolve");
    };
    assert_eq!(
        minimum.value().and_then(SemanticValue::as_integer),
        Some(i64::MIN)
    );
    assert_eq!(
        maximum.value().and_then(SemanticValue::as_integer),
        Some(i64::MAX)
    );
    assert_eq!(
        codes(&document)
            .into_iter()
            .filter(|code| *code == DiagnosticCode::INVALID_VALUE)
            .count(),
        2
    );
}

#[test]
fn date_and_time_components_are_range_checked() {
    for source in [
        "value = 2024-99-99\n",
        "value = 2023-02-29\n",
        "value = 24:00:00\n",
        "value = 12:60:00\n",
        "value = 1979-05-27T07:32:00+25:00\n",
    ] {
        let document = Document::parse(source);
        assert!(
            codes(&document).contains(&DiagnosticCode::INVALID_VALUE),
            "{source:?} should be rejected: {:?}",
            document.diagnostics()
        );
    }

    let leap_day = Document::parse("value = 2024-02-29\n");
    assert!(
        leap_day.diagnostics().is_empty(),
        "{:?}",
        leap_day.diagnostics()
    );
}

#[test]
fn optional_seconds_are_version_checked_in_datetime_forms() {
    for value in [
        "07:32",
        "1979-05-27T07:32",
        "1979-05-27T07:32Z",
        "1979-05-27 07:32-07:00",
    ] {
        let source = format!("value = {value}\n");
        let v11 = Document::parse_as(source.clone(), TomlVersion::V1_1);
        assert!(
            v11.diagnostics().is_empty(),
            "{value}: {:?}",
            v11.diagnostics()
        );

        let v10 = Document::parse_as(source, TomlVersion::V1_0);
        assert!(
            codes(&v10).contains(&DiagnosticCode::TOML_1_1_SYNTAX),
            "{value}: {:?}",
            v10.diagnostics()
        );
    }
}

#[test]
fn offset_datetime_leap_seconds_follow_rfc_3339() {
    for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
        for value in [
            "1990-12-31T23:59:60Z",
            "1990-12-31T15:59:60-08:00",
            "1991-01-01T00:59:60+01:00",
            "2016-12-31T23:59:60.5Z",
        ] {
            let document = Document::parse_as(format!("value = {value}\n"), version);
            assert!(
                document.diagnostics().is_empty(),
                "{version:?} rejected {value}: {:?}",
                document.diagnostics(),
            );
        }

        for value in [
            "1990-12-31T23:58:60Z",
            "1990-12-30T23:59:60Z",
            "1991-01-01T00:59:60Z",
            "2015-12-31T23:59:60Z",
            "2006-01-01T00:00:60-00:00",
        ] {
            let document = Document::parse_as(format!("value = {value}\n"), version);
            assert!(
                codes(&document).contains(&DiagnosticCode::INVALID_VALUE),
                "{version:?} accepted {value}: {:?}",
                document.diagnostics(),
            );
        }
    }
}

#[test]
fn local_datetime_leap_seconds_remain_unbound_wall_clock_values() {
    for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
        for value in ["12:34:60.5", "2024-01-01T12:34:60"] {
            let document = Document::parse_as(format!("value = {value}\n"), version);
            assert!(
                document.diagnostics().is_empty(),
                "{version:?} rejected {value}: {:?}",
                document.diagnostics(),
            );
        }
    }
}

#[test]
fn basic_strings_and_quoted_keys_decode_hex_and_unicode_escapes() {
    let document = Document::parse("value = \"\\u0041\\x42\\U0001F600\"\n\"\\u0061\" = 1\na = 2\n");

    let Resolution::Unique(value) = document.semantics().resolve(["value"]) else {
        panic!("value should resolve");
    };
    assert_eq!(value.value().and_then(SemanticValue::as_str), Some("AB😀"));
    assert!(codes(&document).contains(&DiagnosticCode::DUPLICATE_KEY));
    assert!(matches!(
        document.semantics().resolve(["a"]),
        Resolution::Ambiguous(_)
    ));
}

#[test]
fn invalid_unicode_scalars_and_raw_controls_are_diagnosed() {
    for source in [
        "value = \"\\uD800\"\n",
        "value = \"\\U00110000\"\n",
        "value = \"raw \u{1} control\"\n",
        "# raw \u{1} control\nvalue = 1\n",
    ] {
        let document = Document::parse(source);
        assert!(
            !document.diagnostics().is_empty(),
            "{source:?} should be diagnosed"
        );
    }
}

#[test]
fn multiline_keys_are_rejected_and_crlf_opening_newline_is_trimmed() {
    let invalid_key = Document::parse("\"\"\"key\"\"\" = 1\n");
    assert!(codes(&invalid_key).contains(&DiagnosticCode::INVALID_BARE_KEY));

    let document = Document::parse("value = \"\"\"\r\nhello\"\"\"\n");
    let Resolution::Unique(value) = document.semantics().resolve(["value"]) else {
        panic!("value should resolve");
    };
    assert_eq!(value.value().and_then(SemanticValue::as_str), Some("hello"));
}

#[test]
fn multiline_string_physical_newlines_are_normalized_to_lf() {
    let document =
        Document::parse("basic = \"\"\"\r\none\r\ntwo\"\"\"\r\nliteral = '''\r\none\r\ntwo'''\r\n");
    assert!(
        document.diagnostics().is_empty(),
        "{:?}",
        document.diagnostics()
    );

    for key in ["basic", "literal"] {
        let Resolution::Unique(value) = document.semantics().resolve([key]) else {
            panic!("{key} should resolve");
        };
        assert_eq!(
            value.value().and_then(SemanticValue::as_str),
            Some("one\ntwo"),
        );
    }
}
