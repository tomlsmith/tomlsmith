use std::process::Command;

#[test]
fn version_flag_identifies_the_packaged_server() {
    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith-lsp"))
        .arg("--version")
        .output()
        .expect("tomlsmith-lsp process should start");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).expect("version output should be UTF-8"),
        format!("tomlsmith-lsp {}\n", env!("CARGO_PKG_VERSION")),
    );
}

#[test]
fn help_documents_the_optional_stdio_compatibility_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith-lsp"))
        .arg("--help")
        .output()
        .expect("tomlsmith-lsp process should start");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("help output should be UTF-8");
    assert!(
        stdout.contains("Usage: tomlsmith-lsp [--stdio]"),
        "{stdout}"
    );
}

#[test]
fn unknown_arguments_are_usage_errors_instead_of_starting_the_server() {
    let output = Command::new(env!("CARGO_BIN_EXE_tomlsmith-lsp"))
        .args(["--socket", "127.0.0.1:9000"])
        .output()
        .expect("tomlsmith-lsp process should start");

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("error output should be UTF-8");
    assert!(stderr.contains("unexpected argument"), "{stderr}");
    assert!(stderr.contains("--help"), "{stderr}");
}
