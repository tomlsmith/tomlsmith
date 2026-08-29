#![forbid(unsafe_code)]

use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand, ValueEnum};
use tomlsmith::{Diagnostic, DiagnosticCode, Document, FormatOutcome, Severity};

#[derive(Debug, Parser)]
#[command(name = "tomlsmith", version, about = "A unified TOML toolchain")]
struct Cli {
    /// TOML language version used for parsing and validation.
    #[arg(long, value_enum, default_value_t = TomlVersionArg::V1_1, global = true)]
    toml_version: TomlVersionArg,

    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum TomlVersionArg {
    #[value(name = "1.0")]
    V1_0,
    #[default]
    #[value(name = "1.1")]
    V1_1,
}

impl From<TomlVersionArg> for tomlsmith::TomlVersion {
    fn from(version: TomlVersionArg) -> Self {
        match version {
            TomlVersionArg::V1_0 => Self::V1_0,
            TomlVersionArg::V1_1 => Self::V1_1,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check a TOML document and report diagnostics.
    Check {
        /// Input file, or `-` for standard input.
        #[arg(default_value = "-")]
        input: PathBuf,
    },

    /// Format a TOML document.
    Fmt {
        /// Exit with status 1 instead of writing when formatting is needed.
        #[arg(long)]
        check: bool,

        /// Input file, or `-` for standard input.
        #[arg(default_value = "-")]
        input: PathBuf,
    },

    /// Parse a TOML document and emit diagnostics as JSON.
    Parse {
        /// Input file, or `-` for standard input.
        #[arg(default_value = "-")]
        input: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitStatus {
    Success,
    ContentFailure,
    OperationalFailure,
}

impl ExitStatus {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::ContentFailure => 1,
            Self::OperationalFailure => 2,
        }
    }
}

struct InvalidUtf8Input {
    source_name: String,
    start: u32,
    end: u32,
}

enum SourceRead {
    Text { source_name: String, source: String },
    InvalidUtf8(InvalidUtf8Input),
}

pub fn run<I, S>(
    arguments: I,
    stdin: &mut dyn io::Read,
    stdout: &mut dyn io::Write,
    stderr: &mut dyn io::Write,
) -> ExitStatus
where
    I: IntoIterator<Item = S>,
    S: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error) => {
            let status = if error.use_stderr() {
                let _ = write!(stderr, "{error}");
                ExitStatus::OperationalFailure
            } else {
                let _ = write!(stdout, "{error}");
                ExitStatus::Success
            };
            return status;
        }
    };

    match execute(cli, stdin, stdout, stderr) {
        Ok(status) => status,
        Err(error) => {
            let _ = writeln!(stderr, "tomlsmith: {error}");
            ExitStatus::OperationalFailure
        }
    }
}

fn execute(
    cli: Cli,
    stdin: &mut dyn io::Read,
    stdout: &mut dyn io::Write,
    stderr: &mut dyn io::Write,
) -> io::Result<ExitStatus> {
    let version = cli.toml_version.into();
    match cli.command {
        Command::Check { input } => {
            let (source_name, source) = match read_source(&input, stdin)? {
                SourceRead::Text {
                    source_name,
                    source,
                } => (source_name, source),
                SourceRead::InvalidUtf8(diagnostic) => {
                    render_invalid_utf8(stderr, &diagnostic)?;
                    return Ok(ExitStatus::ContentFailure);
                }
            };
            let document = Document::parse_as(source, version);
            render_diagnostics(stderr, &source_name, document.diagnostics())?;

            Ok(if has_errors(document.diagnostics()) {
                ExitStatus::ContentFailure
            } else {
                ExitStatus::Success
            })
        }
        Command::Fmt { check, input } => {
            let (source_name, source) = match read_source(&input, stdin)? {
                SourceRead::Text {
                    source_name,
                    source,
                } => (source_name, source),
                SourceRead::InvalidUtf8(diagnostic) => {
                    render_invalid_utf8(stderr, &diagnostic)?;
                    return Ok(ExitStatus::ContentFailure);
                }
            };
            let document = Document::parse_as(source, version);

            match document.format() {
                FormatOutcome::Unchanged => {
                    if !check && input == Path::new("-") {
                        stdout.write_all(document.text().as_bytes())?;
                    }
                    Ok(ExitStatus::Success)
                }
                FormatOutcome::Changed { text, .. } => {
                    if check {
                        Ok(ExitStatus::ContentFailure)
                    } else {
                        if input == Path::new("-") {
                            stdout.write_all(text.as_bytes())?;
                        } else {
                            std::fs::write(input, text.as_bytes())?;
                        }
                        Ok(ExitStatus::Success)
                    }
                }
                FormatOutcome::Refused { diagnostics } => {
                    render_diagnostics(stderr, &source_name, &diagnostics)?;
                    Ok(ExitStatus::ContentFailure)
                }
            }
        }
        Command::Parse { input } => {
            let source = match read_source(&input, stdin)? {
                SourceRead::Text { source, .. } => source,
                SourceRead::InvalidUtf8(diagnostic) => {
                    let output = serde_json::json!({
                        "tomlVersion": version_label(version),
                        "valid": false,
                        "diagnostics": [invalid_utf8_json(&diagnostic)],
                    });
                    serde_json::to_writer(&mut *stdout, &output).map_err(io::Error::other)?;
                    writeln!(stdout)?;
                    return Ok(ExitStatus::ContentFailure);
                }
            };
            let document = Document::parse_as(source, version);
            let diagnostics = document
                .diagnostics()
                .iter()
                .map(diagnostic_json)
                .collect::<Vec<_>>();
            let output = serde_json::json!({
                "tomlVersion": version_label(version),
                "valid": !has_errors(document.diagnostics()),
                "diagnostics": diagnostics,
            });
            serde_json::to_writer(&mut *stdout, &output).map_err(io::Error::other)?;
            writeln!(stdout)?;

            Ok(if has_errors(document.diagnostics()) {
                ExitStatus::ContentFailure
            } else {
                ExitStatus::Success
            })
        }
    }
}

const fn version_label(version: tomlsmith::TomlVersion) -> &'static str {
    match version {
        tomlsmith::TomlVersion::V1_0 => "1.0",
        tomlsmith::TomlVersion::V1_1 => "1.1",
    }
}

fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity() == Severity::Error)
}

fn diagnostic_json(diagnostic: &Diagnostic) -> serde_json::Value {
    serde_json::json!({
        "code": diagnostic.code().as_str(),
        "severity": match diagnostic.severity() {
            Severity::Error => "error",
            Severity::Warning => "warning",
        },
        "message": diagnostic.message(),
        "range": {
            "start": diagnostic.range().start(),
            "end": diagnostic.range().end(),
        },
    })
}

fn read_source(input: &Path, stdin: &mut dyn io::Read) -> io::Result<SourceRead> {
    let (source_name, bytes) = if input == Path::new("-") {
        let mut bytes = Vec::new();
        stdin.read_to_end(&mut bytes)?;
        ("stdin".to_owned(), bytes)
    } else {
        (input.display().to_string(), std::fs::read(input)?)
    };
    match String::from_utf8(bytes) {
        Ok(source) => Ok(SourceRead::Text {
            source_name,
            source,
        }),
        Err(error) => {
            let utf8_error = error.utf8_error();
            let start = utf8_error.valid_up_to();
            let end = start.saturating_add(utf8_error.error_len().unwrap_or(1));
            Ok(SourceRead::InvalidUtf8(InvalidUtf8Input {
                source_name,
                start: u32::try_from(start).unwrap_or(u32::MAX),
                end: u32::try_from(end).unwrap_or(u32::MAX),
            }))
        }
    }
}

fn invalid_utf8_json(diagnostic: &InvalidUtf8Input) -> serde_json::Value {
    serde_json::json!({
        "code": DiagnosticCode::INVALID_UTF8.as_str(),
        "severity": "error",
        "message": "TOML input must be valid UTF-8",
        "range": {
            "start": diagnostic.start,
            "end": diagnostic.end,
        },
    })
}

fn render_invalid_utf8(
    output: &mut dyn io::Write,
    diagnostic: &InvalidUtf8Input,
) -> io::Result<()> {
    writeln!(
        output,
        "{}:{}..{}: error[{}]: TOML input must be valid UTF-8",
        diagnostic.source_name,
        diagnostic.start,
        diagnostic.end,
        DiagnosticCode::INVALID_UTF8,
    )
}

fn render_diagnostics(
    output: &mut dyn io::Write,
    source_name: &str,
    diagnostics: &[Diagnostic],
) -> io::Result<()> {
    for diagnostic in diagnostics {
        let severity = match diagnostic.severity() {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        writeln!(
            output,
            "{}:{}..{}: {}[{}]: {}",
            source_name,
            diagnostic.range().start(),
            diagnostic.range().end(),
            severity,
            diagnostic.code(),
            diagnostic.message(),
        )?;
    }
    Ok(())
}
