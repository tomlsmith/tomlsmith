use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "tomlsmith-cli-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("temporary directory should be created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fmt_check_reports_a_changed_file_without_rewriting_it() {
    let directory = TempDirectory::new();
    let path = directory.path().join("needs-formatting.toml");
    let original = "name=\"TomlSmith\"\n";
    fs::write(&path, original).expect("fixture should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith"))
        .args(["fmt", "--check"])
        .arg(&path)
        .output()
        .expect("tomlsmith process should start");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert_eq!(
        fs::read_to_string(&path).expect("fixture should remain readable"),
        original,
        "fmt --check must not modify the input file",
    );
}

#[test]
fn check_uses_process_exit_one_for_invalid_toml() {
    let directory = TempDirectory::new();
    let path = directory.path().join("invalid.toml");
    fs::write(&path, "broken\n").expect("fixture should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith"))
        .arg("check")
        .arg(&path)
        .output()
        .expect("tomlsmith process should start");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("diagnostics should be UTF-8");
    assert!(stderr.contains("parse.missing-equals"), "{stderr:?}");
}

#[test]
fn check_uses_process_exit_one_for_invalid_utf8() {
    let directory = TempDirectory::new();
    let path = directory.path().join("invalid-utf8.toml");
    fs::write(&path, [0xff]).expect("fixture should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith"))
        .arg("check")
        .arg(&path)
        .output()
        .expect("tomlsmith process should start");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("diagnostics should be UTF-8");
    assert!(stderr.contains("parse.invalid-utf8"), "{stderr:?}");
}

#[test]
fn fmt_rewrites_a_file_when_check_is_not_requested() {
    let directory = TempDirectory::new();
    let path = directory.path().join("format-me.toml");
    fs::write(&path, "name=\"TomlSmith\"\n").expect("fixture should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith"))
        .arg("fmt")
        .arg(&path)
        .output()
        .expect("tomlsmith process should start");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        fs::read_to_string(path).expect("formatted file should be readable"),
        "name = \"TomlSmith\"\n",
    );
}

#[test]
fn parse_reads_a_file_and_emits_json() {
    let directory = TempDirectory::new();
    let path = directory.path().join("valid.toml");
    fs::write(&path, "name = \"TomlSmith\"\n").expect("fixture should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith"))
        .args(["parse", "--toml-version", "1.0"])
        .arg(&path)
        .output()
        .expect("tomlsmith process should start");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse output should be JSON");
    assert_eq!(json["tomlVersion"], "1.0");
    assert_eq!(json["valid"], true);
}

#[test]
fn unsupported_toml_version_is_a_usage_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith"))
        .args(["check", "--toml-version", "0.5"])
        .output()
        .expect("tomlsmith process should start");

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("usage errors should be UTF-8");
    assert!(stderr.contains("invalid value '0.5'"), "{stderr:?}");
}

#[test]
fn deeply_nested_dotted_keys_are_rejected_without_crashing() {
    let source = format!("{} = 1\n", vec!["level"; 5_000].join("."));
    let mut child = Command::new(env!("CARGO_BIN_EXE_tomlsmith"))
        .arg("check")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("tomlsmith process should start");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(source.as_bytes())
        .expect("fixture should be written");
    let output = child
        .wait_with_output()
        .expect("tomlsmith process should exit");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("diagnostics should be UTF-8");
    assert!(stderr.contains("parse.nesting-limit"), "{stderr:?}");
}
