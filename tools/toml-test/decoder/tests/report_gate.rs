use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_REPORT: AtomicU64 = AtomicU64::new(0);

struct ReportFile(PathBuf);

impl ReportFile {
    fn new(contents: &str) -> Self {
        let sequence = NEXT_REPORT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "tomlsmith-test-report-{}-{sequence}.json",
            std::process::id()
        ));
        fs::write(&path, contents).expect("fixture report should be written");
        Self(path)
    }
}

impl Drop for ReportFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[test]
fn report_gate_rejects_nonzero_failures_even_if_the_runner_exited_successfully() {
    let report = ReportFile::new(
        r#"{
            "version": "toml-test v2.2.0",
            "toml": "1.1.0",
            "passed_valid": 213,
            "failed_valid": 1,
            "passed_invalid": 467,
            "failed_invalid": 0,
            "passed_encoder": 0,
            "failed_encoder": 0,
            "skipped": 0
        }"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith-test-report"))
        .arg(&report.0)
        .output()
        .expect("report gate should start");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed_valid=1"));
}

#[test]
fn report_gate_requires_zero_invalid_encoder_and_skipped_counts() {
    for field in [
        "failed_invalid",
        "passed_encoder",
        "failed_encoder",
        "skipped",
    ] {
        let mut report = serde_json::json!({
            "version": "toml-test v2.2.0",
            "toml": "1.1.0",
            "passed_valid": 214,
            "failed_valid": 0,
            "passed_invalid": 467,
            "failed_invalid": 0,
            "passed_encoder": 0,
            "failed_encoder": 0,
            "skipped": 0
        });
        report[field] = serde_json::json!(1);
        let report = ReportFile::new(&report.to_string());

        let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith-test-report"))
            .arg(&report.0)
            .output()
            .expect("report gate should start");

        assert_eq!(output.status.code(), Some(1), "{field}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(&format!("{field}=1")),
            "{field}: {output:?}"
        );
    }
}

#[test]
fn report_gate_accepts_an_unskipped_zero_failure_report() {
    let report = ReportFile::new(
        r#"{
            "version": "toml-test v2.2.0",
            "toml": "1.0.0",
            "passed_valid": 205,
            "failed_valid": 0,
            "passed_invalid": 474,
            "failed_invalid": 0,
            "passed_encoder": 0,
            "failed_encoder": 0,
            "skipped": 0
        }"#,
    );
    let partner = ReportFile::new(
        r#"{
            "version": "toml-test v2.2.0",
            "toml": "1.1.0",
            "passed_valid": 214,
            "failed_valid": 0,
            "passed_invalid": 467,
            "failed_invalid": 0,
            "passed_encoder": 0,
            "failed_encoder": 0,
            "skipped": 0
        }"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith-test-report"))
        .args([&report.0, &partner.0])
        .output()
        .expect("report gate should start");

    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("failed=0, skipped=0"));
    assert!(output.stderr.is_empty());
}

#[test]
fn report_gate_rejects_a_zero_case_false_positive() {
    let report = ReportFile::new(
        r#"{
            "version": "toml-test v2.2.0",
            "toml": "1.0.0",
            "passed_valid": 0,
            "failed_valid": 0,
            "passed_invalid": 0,
            "failed_invalid": 0,
            "passed_encoder": 0,
            "failed_encoder": 0,
            "skipped": 0
        }"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith-test-report"))
        .arg(&report.0)
        .output()
        .expect("report gate should start");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("expected 205 valid and 474 invalid"));
}

#[test]
fn report_gate_rejects_an_unexpected_toml_version() {
    let report = ReportFile::new(
        r#"{
            "version": "toml-test v2.2.0",
            "toml": "1.2.0",
            "passed_valid": 205,
            "failed_valid": 0,
            "passed_invalid": 474,
            "failed_invalid": 0,
            "passed_encoder": 0,
            "failed_encoder": 0,
            "skipped": 0
        }"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith-test-report"))
        .arg(&report.0)
        .output()
        .expect("report gate should start");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected TOML version"));
}

#[test]
fn report_gate_rejects_incomplete_passing_counts() {
    let report = ReportFile::new(
        r#"{
            "version": "toml-test v2.2.0",
            "toml": "1.1.0",
            "passed_valid": 213,
            "failed_valid": 0,
            "passed_invalid": 467,
            "failed_invalid": 0,
            "passed_encoder": 0,
            "failed_encoder": 0,
            "skipped": 0
        }"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith-test-report"))
        .arg(&report.0)
        .output()
        .expect("report gate should start");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("expected 214 valid and 467 invalid"));
}

#[test]
fn report_gate_requires_exactly_one_report_for_each_corpus_version() {
    let report = ReportFile::new(
        r#"{
            "version": "toml-test v2.2.0",
            "toml": "1.0.0",
            "passed_valid": 205,
            "failed_valid": 0,
            "passed_invalid": 474,
            "failed_invalid": 0,
            "passed_encoder": 0,
            "failed_encoder": 0,
            "skipped": 0
        }"#,
    );

    let missing = Command::new(env!("CARGO_BIN_EXE_tomlsmith-test-report"))
        .arg(&report.0)
        .output()
        .expect("report gate should start");
    assert_eq!(missing.status.code(), Some(1), "{missing:?}");
    assert!(String::from_utf8_lossy(&missing.stderr).contains("missing TOML 1.1.0 report"));

    let duplicate = Command::new(env!("CARGO_BIN_EXE_tomlsmith-test-report"))
        .args([&report.0, &report.0])
        .output()
        .expect("report gate should start");
    assert_eq!(duplicate.status.code(), Some(1), "{duplicate:?}");
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("duplicate TOML 1.0.0 report"));
}
