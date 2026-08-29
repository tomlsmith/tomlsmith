use std::io::Cursor;

use tomlsmith_cli::{ExitStatus, run};

#[test]
fn check_reports_invalid_stdin_and_returns_content_failure() {
    let mut stdin = Cursor::new(b"broken\n".to_vec());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let status = run(
        ["tomlsmith", "check", "-"],
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(status, ExitStatus::ContentFailure);
    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).expect("diagnostics should be UTF-8");
    assert!(stderr.contains("parse.missing-equals"), "{stderr:?}");
}

#[test]
fn invalid_utf8_is_a_content_diagnostic_at_the_cli_boundary() {
    let mut stdin = Cursor::new(vec![b'a', b' ', b'=', b' ', 0xff, b'\n']);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let status = run(["tomlsmith", "check"], &mut stdin, &mut stdout, &mut stderr);

    assert_eq!(status, ExitStatus::ContentFailure);
    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).expect("diagnostics should be UTF-8");
    assert!(stderr.contains("parse.invalid-utf8"), "{stderr:?}");
    assert!(stderr.contains("4..5"), "{stderr:?}");

    let mut stdin = Cursor::new(vec![0xff]);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let status = run(["tomlsmith", "parse"], &mut stdin, &mut stdout, &mut stderr);
    assert_eq!(status, ExitStatus::ContentFailure);
    assert!(stderr.is_empty());
    let output: serde_json::Value =
        serde_json::from_slice(&stdout).expect("invalid UTF-8 parse output should be JSON");
    assert_eq!(output["valid"], false);
    assert_eq!(output["diagnostics"][0]["code"], "parse.invalid-utf8");
    assert_eq!(output["diagnostics"][0]["range"]["start"], 0);
    assert_eq!(output["diagnostics"][0]["range"]["end"], 1);
}

#[test]
fn check_defaults_to_stdin_and_is_quiet_for_valid_toml() {
    let mut stdin = Cursor::new(b"name = \"TomlSmith\"\n".to_vec());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let status = run(["tomlsmith", "check"], &mut stdin, &mut stdout, &mut stderr);

    assert_eq!(status, ExitStatus::Success);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

#[test]
fn parse_emits_json_and_honors_explicit_toml_version() {
    let mut stdin = Cursor::new(b"name = \"TomlSmith\"\n".to_vec());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let status = run(
        ["tomlsmith", "parse", "--toml-version", "1.0"],
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(status, ExitStatus::Success);
    assert!(stderr.is_empty());
    let output: serde_json::Value =
        serde_json::from_slice(&stdout).expect("parse output should be JSON");
    assert_eq!(output["tomlVersion"], "1.0");
    assert_eq!(output["valid"], true);
    assert_eq!(output["diagnostics"], serde_json::json!([]));
}

#[test]
fn parse_keeps_json_on_stdout_when_the_document_is_invalid() {
    let mut stdin = Cursor::new(b"broken\n".to_vec());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let status = run(["tomlsmith", "parse"], &mut stdin, &mut stdout, &mut stderr);

    assert_eq!(status, ExitStatus::ContentFailure);
    assert!(stderr.is_empty());
    let output: serde_json::Value =
        serde_json::from_slice(&stdout).expect("invalid parse output should still be JSON");
    assert_eq!(output["tomlVersion"], "1.1");
    assert_eq!(output["valid"], false);
    let missing_equals = output["diagnostics"]
        .as_array()
        .expect("diagnostics should be an array")
        .iter()
        .find(|diagnostic| diagnostic["code"] == "parse.missing-equals")
        .expect("the stable missing-equals diagnostic should be present");
    assert_eq!(missing_equals["severity"], "error");
    assert_eq!(missing_equals["range"]["start"], 6);
    assert_eq!(missing_equals["range"]["end"], 6);
}

#[test]
fn missing_input_is_an_operational_failure() {
    let mut stdin = Cursor::new(Vec::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let status = run(
        [
            "tomlsmith",
            "check",
            "/a/path/that/does/not/exist/tomlsmith.toml",
        ],
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(status, ExitStatus::OperationalFailure);
    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).expect("errors should be UTF-8");
    assert!(stderr.starts_with("tomlsmith: "), "{stderr:?}");
}

#[test]
fn fmt_formats_stdin_to_stdout() {
    let mut stdin = Cursor::new(b"name=\"TomlSmith\"\n".to_vec());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let status = run(["tomlsmith", "fmt"], &mut stdin, &mut stdout, &mut stderr);

    assert_eq!(status, ExitStatus::Success);
    assert_eq!(stdout, b"name = \"TomlSmith\"\n");
    assert!(stderr.is_empty());
}

#[test]
fn fmt_refuses_invalid_stdin_without_emitting_rewritten_text() {
    let mut stdin = Cursor::new(b"broken\n".to_vec());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let status = run(["tomlsmith", "fmt"], &mut stdin, &mut stdout, &mut stderr);

    assert_eq!(status, ExitStatus::ContentFailure);
    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).expect("diagnostics should be UTF-8");
    assert!(stderr.contains("parse.missing-equals"), "{stderr:?}");
}
