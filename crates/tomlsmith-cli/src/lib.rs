#![forbid(unsafe_code)]

use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand, ValueEnum};
use tomlsmith::{
    Diagnostic, DiagnosticCode, Document, FormatOptions, FormatOutcome, LineEnding, Severity,
};

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

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum LineEndingArg {
    #[default]
    Preserve,
    Lf,
    Crlf,
}

impl From<LineEndingArg> for LineEnding {
    fn from(line_ending: LineEndingArg) -> Self {
        match line_ending {
            LineEndingArg::Preserve => Self::Preserve,
            LineEndingArg::Lf => Self::Lf,
            LineEndingArg::Crlf => Self::CrLf,
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

        /// Number of spaces per indentation level.
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..))]
        indent_width: Option<u8>,

        /// Line width that triggers wrapping inside arrays.
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
        line_width: Option<u16>,

        /// Line-ending policy for the formatted output.
        #[arg(long, value_enum, default_value_t = LineEndingArg::Preserve)]
        line_ending: LineEndingArg,

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
        Command::Fmt {
            check,
            indent_width,
            line_width,
            line_ending,
            input,
        } => {
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
            let mut options = FormatOptions {
                target_version: version,
                line_ending: line_ending.into(),
                ..FormatOptions::default()
            };
            if let Some(indent_width) = indent_width {
                options.indent_width = indent_width;
            }
            if let Some(line_width) = line_width {
                options.line_width = line_width;
            }
            let (document, outcome) = Document::parse_and_format_with(source, version, &options);
            render_format_outcome(
                &document,
                outcome,
                check,
                &input,
                &source_name,
                stdout,
                stderr,
            )
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

fn render_format_outcome(
    document: &Document,
    outcome: FormatOutcome,
    check: bool,
    input: &Path,
    source_name: &str,
    stdout: &mut dyn io::Write,
    stderr: &mut dyn io::Write,
) -> io::Result<ExitStatus> {
    match outcome {
        FormatOutcome::Unchanged => {
            if !check && input == Path::new("-") {
                stdout.write_all(document.text().as_bytes())?;
            }
            Ok(ExitStatus::Success)
        }
        FormatOutcome::Changed { text, .. } => {
            if check {
                writeln!(stderr, "would reformat {source_name}")?;
                Ok(ExitStatus::ContentFailure)
            } else {
                if input == Path::new("-") {
                    stdout.write_all(text.as_bytes())?;
                } else {
                    write_file_atomically(input, text.as_bytes())?;
                }
                Ok(ExitStatus::Success)
            }
        }
        FormatOutcome::Refused { diagnostics } => {
            render_diagnostics(stderr, source_name, &diagnostics)?;
            Ok(ExitStatus::ContentFailure)
        }
    }
}

/// Replaces `input` through a same-directory temporary file and rename so
/// an interrupted write can never leave a truncated document behind.
fn write_file_atomically(input: &Path, contents: &[u8]) -> io::Result<()> {
    let directory = input
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let file_name = input
        .file_name()
        .ok_or_else(|| io::Error::other("cannot format a path without a file name"))?;
    let mut temporary_name = file_name.to_owned();
    temporary_name.push(format!(".tomlsmith-{}.tmp", std::process::id()));
    let temporary_path = directory.map_or_else(
        || PathBuf::from(&temporary_name),
        |parent| parent.join(&temporary_name),
    );

    let result = (|| {
        std::fs::write(&temporary_path, contents)?;
        if let Ok(metadata) = std::fs::metadata(input) {
            std::fs::set_permissions(&temporary_path, metadata.permissions())?;
        }
        std::fs::rename(&temporary_path, input)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary_path);
    }
    result
}

fn read_source(input: &Path, stdin: &mut dyn io::Read) -> io::Result<SourceRead> {
    let (source_name, bytes) = if input == Path::new("-") {
        // Pre-size the buffer so `read_to_end` on a pipe does not spend the
        // cold-start budget growing a fresh Vec while the writer refills it.
        let mut bytes = Vec::with_capacity(256 * 1024);
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
