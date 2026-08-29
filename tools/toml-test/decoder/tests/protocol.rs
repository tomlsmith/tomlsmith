use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

use serde_json::json;

fn decode(arguments: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tomlsmith-test-decoder"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("decoder should start");
    child
        .stdin
        .take()
        .expect("decoder stdin should be piped")
        .write_all(input.as_bytes())
        .expect("test input should be written");
    child.wait_with_output().expect("decoder should exit")
}

#[test]
fn valid_document_is_emitted_as_toml_test_tagged_json() {
    let output = decode(&["--toml-version", "1.0"], "answer = 42\n");

    assert!(output.status.success(), "{output:?}");
    let actual: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should contain JSON");
    assert_eq!(
        actual,
        json!({"answer": {"type": "integer", "value": "42"}})
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn tables_arrays_and_scalar_types_follow_the_tagged_json_protocol() {
    let output = decode(
        &["--toml-version", "1.1"],
        concat!(
            "title = \"TOML Example\"\n",
            "enabled = true\n",
            "ratio = 1.5\n",
            "when = 1979-05-27T07:32Z\n",
            "local = 07:32\n",
            "point = { x = 1, label = \"origin\" }\n",
            "ports = [8000, 8001]\n",
            "[[products]]\n",
            "name = \"Hammer\"\n",
            "[products.details]\n",
            "weight = 1\n",
            "[[products]]\n",
            "name = \"Nail\"\n",
        ),
    );

    assert!(output.status.success(), "{output:?}");
    let actual: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should contain JSON");
    assert_eq!(
        actual,
        json!({
            "title": {"type": "string", "value": "TOML Example"},
            "enabled": {"type": "bool", "value": "true"},
            "ratio": {"type": "float", "value": "1.5"},
            "when": {"type": "datetime", "value": "1979-05-27T07:32:00Z"},
            "local": {"type": "time-local", "value": "07:32:00"},
            "point": {
                "x": {"type": "integer", "value": "1"},
                "label": {"type": "string", "value": "origin"}
            },
            "ports": [
                {"type": "integer", "value": "8000"},
                {"type": "integer", "value": "8001"}
            ],
            "products": [
                {
                    "name": {"type": "string", "value": "Hammer"},
                    "details": {"weight": {"type": "integer", "value": "1"}}
                },
                {"name": {"type": "string", "value": "Nail"}}
            ]
        })
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn invalid_document_exits_one_and_reports_the_error_on_stderr() {
    let output = decode(&["--toml-version", "1.0"], "answer =\n");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn decoder_version_matches_the_toml_test_corpus_version() {
    let source = "letter = \"\\x41\"\n";

    let toml_1_0 = decode(&["--toml-version", "1.0"], source);
    assert_eq!(toml_1_0.status.code(), Some(1));

    let toml_1_1 = decode(&["--toml-version", "1.1"], source);
    assert!(toml_1_1.status.success(), "{toml_1_1:?}");
    let actual: serde_json::Value =
        serde_json::from_slice(&toml_1_1.stdout).expect("stdout should contain JSON");
    assert_eq!(actual, json!({"letter": {"type": "string", "value": "A"}}));
}
